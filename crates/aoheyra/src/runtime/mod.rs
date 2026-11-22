//! Runtime orchestration: one owner for the mesh engine, the network facade
//! and the local identity.
//!
//! The runtime is sans-IO. [`Runtime::step`] takes an injected time and a
//! caller-owned [`std::task::Context`], polls one transport event and returns
//! typed events for the host. Nothing here reads a clock, draws randomness,
//! spawns a task or interprets an application payload: aoMesh sees opaque
//! bytes and an [`ao_mesh::packet::PacketClass`], and aoIdents owns the keys
//! and the seal. The wired [`ao_mesh::store::MessageStore`] absorbs sync
//! items, answers sync requests and applies
//! receipt purges before the host sees the remaining events.
//!
//! Event fan-out is a separate bounded [`EventQueue`]. A full queue returns
//! [`EventQueueError::Full`] instead of dropping an event; the caller must
//! drain it before emitting more. The optional `runtime-tokio` feature
//! supplies a current-thread executor for the Android service thread, while
//! the core stays independent of any async runtime.

use std::collections::VecDeque;
use std::future::Future;
use std::task::{Context, Poll};
use std::time::Duration;

use ao_idents::{IdentError, Identity, PublicIdentity};
use ao_mesh::adapter::{AdapterEvent, NetworkAdapter};
use ao_mesh::store::{MemoryStore, MessageStore, StoreError, StoreMetadata, StoredMessage};
use ao_mesh::{
    Header, MeshError, MeshHello, MeshInput, PacketClass, SHORT_ID_BYTES, StoreSummary, SyncOffer,
    derive_msg_id,
};
use ao_network::{
    AppHello, BeaconFlags, DisconnectReason, FrameError, LinkId, LocalTag, SendError, Transport,
};
use rand_core::RngCore;

/// Default number of host events held before fan-out reports overflow.
pub const DEFAULT_EVENT_CAPACITY: usize = 64;
/// Default Direct hop limit for an opaque submission.
pub const DEFAULT_TTL: u8 = 8;
/// Default lifetime of an opaque submission, in seconds.
pub const DEFAULT_LIFETIME_S: u64 = 3_600;
/// Maximum nested store/sync passes for one network event.
const MAX_STORE_EVENT_PASSES: usize = 8;

/// Validated bounds and defaults owned by the app runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeConfig {
    event_capacity: usize,
    ttl: u8,
    lifetime_s: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            event_capacity: DEFAULT_EVENT_CAPACITY,
            ttl: DEFAULT_TTL,
            lifetime_s: DEFAULT_LIFETIME_S,
        }
    }
}

impl RuntimeConfig {
    /// Builds a config, rejecting values that would break bounded operation.
    ///
    /// # Errors
    ///
    /// [`ConfigError::EventCapacity`] for zero or an unreasonably large queue,
    /// [`ConfigError::Ttl`] for a zero hop limit and
    /// [`ConfigError::Lifetime`] for a zero lifetime.
    pub fn new(event_capacity: usize, ttl: u8, lifetime_s: u64) -> Result<Self, ConfigError> {
        if event_capacity == 0 || event_capacity > 1024 {
            return Err(ConfigError::EventCapacity {
                value: event_capacity,
            });
        }
        if ttl == 0 {
            return Err(ConfigError::Ttl);
        }
        if lifetime_s == 0 {
            return Err(ConfigError::Lifetime);
        }
        Ok(Self {
            event_capacity,
            ttl,
            lifetime_s,
        })
    }

    /// Maximum number of events held by [`EventQueue`].
    #[must_use]
    pub const fn event_capacity(&self) -> usize {
        self.event_capacity
    }

    /// Hop limit stamped on an opaque submission.
    #[must_use]
    pub const fn ttl(&self) -> u8 {
        self.ttl
    }

    /// Lifetime stamped on an opaque submission.
    #[must_use]
    pub const fn lifetime_s(&self) -> u64 {
        self.lifetime_s
    }
}

/// A rejected runtime setting. Values are reported, never clamped silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// Event queue capacity outside 1..=1024.
    EventCapacity {
        /// Rejected capacity.
        value: usize,
    },
    /// A zero hop limit would stop a packet before its first relay.
    Ttl,
    /// A zero lifetime would expire a packet before delivery.
    Lifetime,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EventCapacity { value } => {
                write!(f, "event capacity {value} outside 1..=1024")
            }
            Self::Ttl => f.write_str("packet TTL must be positive"),
            Self::Lifetime => f.write_str("packet lifetime must be positive"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Idempotent lifecycle state of the single runtime object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// No hello or beacon is advertised and no events are produced.
    Stopped,
    /// Discovery and event polling are active.
    Running,
    /// Links are kept, discovery is quiet and no new events are polled.
    Paused,
}

/// Why a local link refused an outgoing queue entry, without returning the
/// payload to the event listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendFailure {
    /// The per-link send queue was full.
    QueueFull,
    /// The link was closing or closed.
    LinkClosed,
    /// The payload exceeded the peer's frame limit.
    TooLarge,
}

/// A host-visible result of one runtime step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    /// An authenticated, opened application payload reached this node.
    Delivered {
        /// Packet header as received; `payload_len` counts the sealed bytes.
        header: Header,
        /// Opaque opened bytes, owned by the application.
        payload: Vec<u8>,
    },
    /// A delivery receipt authenticated by aoMesh reached this node.
    Receipt {
        /// Acknowledged message id.
        id: [u8; 16],
        /// Sender named by the receipt.
        origin: [u8; 16],
        /// Destination that issued the receipt.
        by: [u8; 16],
    },
    /// A link refused an outgoing packet.
    SendFailed {
        /// Link that refused it.
        link: LinkId,
        /// Typed refusal, with no payload attached.
        failure: SendFailure,
    },
    /// An authenticated receipt purged this content address from the wired
    /// store; emitted for diagnostics after the store was updated.
    Purge {
        /// Content address to purge.
        id: ao_mesh::MessageId,
    },
}

/// Bounded fan-out queue for host events.
#[derive(Debug)]
pub struct EventQueue {
    capacity: usize,
    pending: VecDeque<RuntimeEvent>,
}

impl EventQueue {
    /// Creates an empty queue with the validated capacity.
    #[must_use]
    pub const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            pending: VecDeque::new(),
        }
    }

    /// Offers one event, rejecting the newest when the queue is full.
    ///
    /// # Errors
    ///
    /// [`EventQueueError::Full`] leaves the supplied event with the caller;
    /// nothing is dropped silently.
    pub fn try_push(&mut self, event: RuntimeEvent) -> Result<(), EventQueueError> {
        if self.pending.len() >= self.capacity {
            return Err(EventQueueError::Full);
        }
        self.pending.push_back(event);
        Ok(())
    }

    /// Offers a batch in order. On overflow the first event that does not fit
    /// is returned to the caller together with everything after it.
    ///
    /// # Errors
    ///
    /// [`EventQueueError::Full`] and the unconsumed suffix.
    pub fn try_extend(
        &mut self,
        events: Vec<RuntimeEvent>,
    ) -> Result<(), (EventQueueError, Vec<RuntimeEvent>)> {
        let room = self.capacity.saturating_sub(self.pending.len());
        if events.len() > room {
            let mut tail = events;
            let head = tail.drain(..room).collect::<Vec<_>>();
            self.pending.extend(head);
            return Err((EventQueueError::Full, tail));
        }
        self.pending.extend(events);
        Ok(())
    }

    /// Removes and returns all pending events in insertion order.
    pub fn drain(&mut self) -> Vec<RuntimeEvent> {
        self.pending.drain(..).collect()
    }

    /// Number of events currently waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Whether no event is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// A bounded queue refused a new event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventQueueError {
    /// The queue is at capacity; drain it before emitting more.
    Full,
}

impl std::fmt::Display for EventQueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("runtime event queue is full")
    }
}

impl std::error::Error for EventQueueError {}

/// A typed failure from one runtime operation.
#[derive(Debug)]
pub enum RuntimeError {
    /// The supplied runtime config was invalid.
    Config(ConfigError),
    /// The operation needs a running runtime.
    NotRunning,
    /// A mesh submission or tick was refused.
    Mesh(MeshError),
    /// The signed mesh introduction could not be built.
    Identity(IdentError),
    /// The introduction did not fit aoNetwork's opaque blob.
    Hello(FrameError),
    /// The wired message store refused a sync item or summary operation.
    Store(StoreError),
    /// Store feedback kept producing nested mesh work beyond the bound.
    StoreCascade,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(error) => write!(f, "invalid runtime config: {error}"),
            Self::NotRunning => f.write_str("runtime is not running"),
            Self::Mesh(error) => write!(f, "mesh refused the operation: {error}"),
            Self::Identity(error) => write!(f, "identity refused the operation: {error}"),
            Self::Hello(error) => write!(f, "mesh introduction was rejected: {error}"),
            Self::Store(error) => write!(f, "message store refused the operation: {error}"),
            Self::StoreCascade => f.write_str("store feedback exceeded its pass limit"),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<ConfigError> for RuntimeError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl<R: RngCore, T: Transport, S: MessageStore> std::fmt::Debug for Runtime<R, T, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Identity material, store contents and transport state are redacted.
        f.debug_struct("Runtime")
            .field("state", &self.state)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// The single owner of one node's runtime state.
///
/// `R` draws seeded relay randomness for aoMesh; `T` is the radio transport
/// chosen by the host (`SimTransport` in tests, `NativeTransport` on Android);
/// `S` is the host's message store. Identity key material is owned here and
/// borrowed for every engine call; neither the engine nor the facade retains
/// it. The store is owned here so sync requests, sync items and receipts are
/// answered without a second lifecycle owner.
pub struct Runtime<R: RngCore, T: Transport, S: MessageStore = MemoryStore> {
    adapter: NetworkAdapter<R, T>,
    identity: Identity,
    config: RuntimeConfig,
    state: LifecycleState,
    card_hash: [u8; 8],
    store_digest: [u8; 4],
    store: S,
}

impl<R: RngCore, T: Transport, S: MessageStore> Runtime<R, T, S> {
    /// Takes ownership of an already constructed adapter, identity and store.
    #[must_use]
    pub fn new(
        config: RuntimeConfig,
        identity: Identity,
        card_hash: [u8; 8],
        adapter: NetworkAdapter<R, T>,
        store: S,
    ) -> Self {
        Self {
            adapter,
            identity,
            config,
            state: LifecycleState::Stopped,
            card_hash,
            store_digest: [0; 4],
            store,
        }
    }

    /// Current lifecycle state.
    #[must_use]
    pub const fn state(&self) -> LifecycleState {
        self.state
    }

    /// Validated app settings.
    #[must_use]
    pub const fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Earliest absolute network deadline; the host arranges the wakeup.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Duration> {
        self.adapter.next_deadline()
    }

    /// Read-only mesh and network handles for diagnostics.
    #[must_use]
    pub fn adapter(&self) -> &NetworkAdapter<R, T> {
        &self.adapter
    }

    /// Read-only handle to the wired message store.
    #[must_use]
    pub const fn store(&self) -> &S {
        &self.store
    }

    /// Starts or restarts discovery and link formation. Idempotent while
    /// running. `now_ms` is caller-supplied local time in milliseconds.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Identity`] or [`RuntimeError::Hello`] if the signed
    /// introduction cannot be built, [`RuntimeError::Mesh`] if the initial
    /// store summary or tick is refused.
    pub fn start(&mut self, now: Duration, now_ms: u64) -> Result<Vec<RuntimeEvent>, RuntimeError> {
        if self.state == LifecycleState::Running {
            return Ok(Vec::new());
        }
        let ids = self.store.summary(now_ms);
        self.store_digest = StoreSummary::from_ids(&ids).digest();
        let hello = MeshHello::sign(&self.identity, self.card_hash, self.store_digest)
            .map_err(RuntimeError::Identity)?;
        let app_hello = AppHello::new(hello.encode().to_vec()).map_err(RuntimeError::Hello)?;
        self.adapter.network_mut().set_local_hello(
            LocalTag::new(*self.identity.node_id().as_bytes()),
            app_hello,
        );
        let mut hint = [0u8; 8];
        hint.copy_from_slice(&self.identity.node_id().as_bytes()[..8]);
        self.adapter.network_mut().set_beacon(
            hint,
            self.store_digest,
            BeaconFlags {
                accepting: true,
                relay_willing: true,
                low_battery: false,
            },
        );
        self.adapter.network_mut().set_idle(false);
        self.state = LifecycleState::Running;
        let summary_events = {
            let adapter = &mut self.adapter;
            adapter
                .handle(now, now_ms, MeshInput::StoreSummary { ids: &ids })
                .map_err(RuntimeError::Mesh)?
        };
        let mut events = self.process_events(now, now_ms, summary_events, 0)?;
        let tick_events = Self::feed(
            &mut self.adapter,
            now,
            now_ms,
            MeshInput::Tick {
                identity: &self.identity,
                card_hash: self.card_hash,
            },
        )?;
        events.extend(self.process_events(now, now_ms, tick_events, 0)?);
        Ok(events)
    }

    /// Suspends discovery without dropping live links. Idempotent.
    pub fn pause(&mut self) {
        self.adapter.network_mut().set_idle(true);
        self.state = LifecycleState::Paused;
    }

    /// Resumes discovery after a pause. Idempotent while running.
    pub fn resume(&mut self) {
        if self.state == LifecycleState::Paused {
            self.adapter.network_mut().set_idle(false);
            self.state = LifecycleState::Running;
        }
    }

    /// Closes every up link and stops event production. Idempotent.
    pub fn stop(&mut self, now: Duration) {
        if self.state == LifecycleState::Stopped {
            return;
        }
        let links = self
            .adapter
            .network()
            .links()
            .into_iter()
            .map(|info| info.link)
            .collect::<Vec<_>>();
        for link in links {
            self.adapter
                .network_mut()
                .disconnect(now, link, DisconnectReason::Closed);
        }
        self.adapter.network_mut().set_idle(true);
        self.state = LifecycleState::Stopped;
    }

    /// Polls one network event into the mesh engine and maps host actions.
    /// `Pending` means no progress was made at this timestamp.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::NotRunning`] while stopped or paused, and
    /// [`RuntimeError::Mesh`] when the engine refuses an input.
    pub fn step(
        &mut self,
        cx: &mut Context<'_>,
        now: Duration,
        now_ms: u64,
    ) -> Result<Vec<RuntimeEvent>, RuntimeError> {
        if self.state != LifecycleState::Running {
            return Err(RuntimeError::NotRunning);
        }
        match self
            .adapter
            .poll_event(cx, now, now_ms, &self.identity)
            .map_err(RuntimeError::Mesh)?
        {
            Poll::Pending => Ok(Vec::new()),
            Poll::Ready(events) => self.process_events(now, now_ms, events, 0),
        }
    }

    /// Runs time-driven mesh work at `now_ms`. The host owns the timer.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::NotRunning`] unless running and
    /// [`RuntimeError::Mesh`] if the engine refuses the tick.
    pub fn tick(&mut self, now: Duration, now_ms: u64) -> Result<Vec<RuntimeEvent>, RuntimeError> {
        if self.state != LifecycleState::Running {
            return Err(RuntimeError::NotRunning);
        }
        let expired = self.store.expire(now_ms);
        let tick_events = Self::feed(
            &mut self.adapter,
            now,
            now_ms,
            MeshInput::Tick {
                identity: &self.identity,
                card_hash: self.card_hash,
            },
        )?;
        let mut events = self.process_events(now, now_ms, tick_events, 0)?;
        if !expired.is_empty() {
            let summary_events = self.refresh_store_summary(now, now_ms)?;
            events.extend(self.process_events(now, now_ms, summary_events, 0)?);
        }
        Ok(events)
    }

    /// Submits caller-owned opaque bytes without interpreting them.
    ///
    /// The caller supplies the already sealed payload and packet class; this
    /// method only builds the routing header and hands both to aoMesh.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::NotRunning`] unless running and
    /// [`RuntimeError::Mesh`] if the engine refuses the submission.
    pub fn submit_raw(
        &mut self,
        now: Duration,
        now_ms: u64,
        class: PacketClass,
        dst: Option<[u8; 16]>,
        payload: &[u8],
    ) -> Result<Vec<RuntimeEvent>, RuntimeError> {
        if self.state != LifecycleState::Running {
            return Err(RuntimeError::NotRunning);
        }
        let header = Header {
            class,
            dst,
            msg_id: [0; 16].into(),
            origin: *self.identity.node_id().as_bytes(),
            created_ms: now_ms,
            lifetime_s: self.config.lifetime_s(),
            payload_len: payload.len() as u64,
            ttl: self.config.ttl(),
            hops: 0,
        };
        let events = Self::feed(
            &mut self.adapter,
            now,
            now_ms,
            MeshInput::Submit {
                identity: &self.identity,
                header,
                payload,
                via: None,
            },
        )?;
        self.process_events(now, now_ms, events, 0)
    }

    /// Derives the header-bound seal id for a raw Direct submission.
    ///
    /// aoMesh binds the sealed payload to `derive_msg_id(header, &[])`, as
    /// documented in its delivery pipeline. Callers compose against this
    /// value before calling [`Self::submit_raw`].
    #[must_use]
    pub fn seal_binding(
        &self,
        class: PacketClass,
        dst: Option<[u8; 16]>,
        now_ms: u64,
        payload_len: usize,
    ) -> ao_mesh::MessageId {
        let header = Header {
            class,
            dst,
            msg_id: [0; 16].into(),
            origin: *self.identity.node_id().as_bytes(),
            created_ms: now_ms,
            lifetime_s: self.config.lifetime_s(),
            payload_len: payload_len as u64,
            ttl: self.config.ttl(),
            hops: 0,
        };
        derive_msg_id(&header, &[])
    }

    /// The recipient identity this runtime opens Direct payloads with.
    #[must_use]
    pub fn public_identity(&self) -> PublicIdentity {
        self.identity.public().clone()
    }

    fn feed(
        adapter: &mut NetworkAdapter<R, T>,
        now: Duration,
        now_ms: u64,
        input: MeshInput<'_>,
    ) -> Result<Vec<AdapterEvent>, RuntimeError> {
        adapter
            .handle(now, now_ms, input)
            .map_err(RuntimeError::Mesh)
    }

    /// Re-announces the store after an insertion, eviction or receipt purge.
    fn refresh_store_summary(
        &mut self,
        now: Duration,
        now_ms: u64,
    ) -> Result<Vec<AdapterEvent>, RuntimeError> {
        let ids = self.store.summary(now_ms);
        self.store_digest = StoreSummary::from_ids(&ids).digest();
        let adapter = &mut self.adapter;
        adapter
            .handle(now, now_ms, MeshInput::StoreSummary { ids: &ids })
            .map_err(RuntimeError::Mesh)
    }

    /// Applies the store seam before fanning host events out. Nested engine
    /// work is bounded so a sync exchange cannot recurse without a caller tick.
    fn process_events(
        &mut self,
        now: Duration,
        now_ms: u64,
        events: Vec<AdapterEvent>,
        depth: usize,
    ) -> Result<Vec<RuntimeEvent>, RuntimeError> {
        if depth > MAX_STORE_EVENT_PASSES {
            return Err(RuntimeError::StoreCascade);
        }
        let mut host = Vec::new();
        let mut store_changed = false;
        for event in events {
            match event {
                AdapterEvent::SyncNeed { link, prefixes } => {
                    let nested = {
                        let store = &self.store;
                        let ids = store.summary(now_ms);
                        // aoMesh returns owned rows from `get`, so keep the
                        // rows alive while the borrowed wire slices are handed
                        // to the adapter in one call.
                        let entries = prefixes
                            .iter()
                            .filter_map(|prefix| {
                                let id = ids
                                    .iter()
                                    .find(|id| id.as_bytes()[..SHORT_ID_BYTES] == prefix[..])?;
                                let message = store.get(now_ms, id)?;
                                Some((*prefix, message))
                            })
                            .collect::<Vec<_>>();
                        let offers = entries
                            .iter()
                            .map(|(prefix, message)| SyncOffer {
                                prefix: *prefix,
                                wire: message.wire(),
                                copies_left: message.metadata().copies_left,
                            })
                            .collect::<Vec<_>>();
                        if offers.is_empty() {
                            Vec::new()
                        } else {
                            let adapter = &mut self.adapter;
                            adapter
                                .handle(
                                    now,
                                    now_ms,
                                    MeshInput::SyncItems {
                                        link,
                                        items: &offers,
                                    },
                                )
                                .map_err(RuntimeError::Mesh)?
                        }
                    };
                    host.extend(self.process_events(now, now_ms, nested, depth + 1)?);
                }
                AdapterEvent::SyncItem {
                    wire, copies_left, ..
                } => {
                    let metadata = StoreMetadata {
                        copies_left: copies_left.max(1),
                        first_seen: now_ms,
                        received_at: now_ms,
                        priority: 0,
                        received_from: None,
                        age_ms: 0,
                    };
                    let message =
                        StoredMessage::new(&wire, metadata).map_err(RuntimeError::Store)?;
                    self.store
                        .put(now_ms, message)
                        .map_err(RuntimeError::Store)?;
                    store_changed = true;
                }
                AdapterEvent::Purge { id } => {
                    self.store.evict(&id);
                    store_changed = true;
                    host.push(RuntimeEvent::Purge { id });
                }
                other => {
                    if let Some(event) = Self::map_event(other) {
                        host.push(event);
                    }
                }
            }
        }
        if store_changed {
            let nested = self.refresh_store_summary(now, now_ms)?;
            host.extend(self.process_events(now, now_ms, nested, depth + 1)?);
        }
        Ok(host)
    }

    fn map_event(event: AdapterEvent) -> Option<RuntimeEvent> {
        match event {
            AdapterEvent::Deliver { header, payload } => {
                Some(RuntimeEvent::Delivered { header, payload })
            }
            AdapterEvent::Delivered { id, origin, by } => Some(RuntimeEvent::Receipt {
                id: *id.as_bytes(),
                origin,
                by,
            }),
            AdapterEvent::SendFailed { link, error } => {
                let failure = match error {
                    SendError::QueueFull(_) => SendFailure::QueueFull,
                    SendError::LinkClosed(_) => SendFailure::LinkClosed,
                    SendError::TooLarge(_) => SendFailure::TooLarge,
                    _ => return None,
                };
                Some(RuntimeEvent::SendFailed { link, failure })
            }
            // Card, directory, channel and telemetry actions belong to their
            // owning services; the orchestrator does not guess their semantics.
            _ => None,
        }
    }
}

#[cfg(feature = "runtime-tokio")]
mod tokio_driver {
    use super::*;

    /// Runs one future on a dedicated current-thread Tokio runtime.
    ///
    /// Kotlin's foreground service owns the thread and calls this once per
    /// service start; the runtime handles timers and wakeups while the
    /// orchestrator itself stays sans-IO. No clock or RNG is read here.
    ///
    /// # Errors
    ///
    /// Returns the executor build error if the platform refuses a runtime.
    pub fn block_on_current_thread<F: Future>(future: F) -> Result<F::Output, std::io::Error> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()?;
        Ok(runtime.block_on(future))
    }
}

#[cfg(feature = "runtime-tokio")]
pub use tokio_driver::block_on_current_thread;

#[cfg(test)]
mod tests {
    use super::*;
    use ao_idents::SealedMessage;
    use ao_mesh::store::{MemoryStore, StoreLimits};
    use ao_mesh::{MeshConfig, MeshEngine};
    use ao_network::{Network, NetworkConfig, SimConfig, SimNode, SimWorld};
    use rand::SeedableRng;
    use rand08::{SeedableRng as SeedableRng08, rngs::StdRng as SealRng};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::task::{Context, Waker};

    /// Records store calls so the full-stack test can prove the store seam is
    /// wired even when a receipt immediately purges the delivered copy.
    #[derive(Debug)]
    struct CountingStore {
        inner: MemoryStore,
        puts: Rc<Cell<usize>>,
        evictions: Rc<Cell<usize>>,
    }

    impl CountingStore {
        fn new(puts: Rc<Cell<usize>>, evictions: Rc<Cell<usize>>) -> Self {
            Self {
                inner: MemoryStore::new(StoreLimits::default()),
                puts,
                evictions,
            }
        }
    }

    impl MessageStore for CountingStore {
        fn put(
            &mut self,
            now: u64,
            entry: StoredMessage,
        ) -> Result<Vec<ao_mesh::MessageId>, StoreError> {
            let result = self.inner.put(now, entry);
            if result.is_ok() {
                self.puts.set(self.puts.get() + 1);
            }
            result
        }

        fn get(&self, now: u64, id: &ao_mesh::MessageId) -> Option<StoredMessage> {
            self.inner.get(now, id)
        }

        fn by_destination(
            &self,
            now: u64,
            dst: &ao_mesh::store::NodeId,
        ) -> Vec<ao_mesh::MessageId> {
            self.inner.by_destination(now, dst)
        }

        fn summary(&self, now: u64) -> Vec<ao_mesh::MessageId> {
            self.inner.summary(now)
        }

        fn expire(&mut self, now: u64) -> Vec<ao_mesh::MessageId> {
            self.inner.expire(now)
        }

        fn evict(&mut self, id: &ao_mesh::MessageId) -> Option<StoredMessage> {
            let removed = self.inner.evict(id);
            if removed.is_some() {
                self.evictions.set(self.evictions.get() + 1);
            }
            removed
        }
    }

    fn runtime_with_store<S: MessageStore>(
        world: &SimWorld,
        seed: u64,
        identity: Identity,
        store: S,
    ) -> Runtime<rand::rngs::StdRng, ao_network::SimTransport, S> {
        let radio = world.add_node(SimNode::default()).expect("valid sim node");
        let network = Network::new(NetworkConfig::default(), radio);
        let engine = MeshEngine::with_config(
            rand::rngs::StdRng::seed_from_u64(seed),
            MeshConfig::default(),
        );
        Runtime::new(
            RuntimeConfig::default(),
            identity,
            [0; 8],
            NetworkAdapter::new(engine, network),
            store,
        )
    }

    fn runtime(
        world: &SimWorld,
        seed: u64,
        identity: Identity,
    ) -> Runtime<rand::rngs::StdRng, ao_network::SimTransport> {
        runtime_with_store(
            world,
            seed,
            identity,
            MemoryStore::new(StoreLimits::default()),
        )
    }

    /// Drives both runtimes until `predicate` accepts an event or the paused
    /// world runs out of steps. All time comes from `SimWorld::advance`.
    fn drive_until<S: MessageStore>(
        world: &SimWorld,
        runtimes: &mut [Runtime<rand::rngs::StdRng, ao_network::SimTransport, S>],
        steps: usize,
        mut predicate: impl FnMut(&RuntimeEvent) -> bool,
    ) -> Vec<RuntimeEvent> {
        let mut cx = Context::from_waker(Waker::noop());
        let mut found = Vec::new();
        for _ in 0..steps {
            world.advance(Duration::from_millis(50));
            for runtime in runtimes.iter_mut() {
                match runtime.step(&mut cx, world.now(), world.now().as_millis() as u64) {
                    Ok(events) => {
                        for event in events {
                            if predicate(&event) {
                                found.push(event);
                            }
                        }
                    }
                    Err(RuntimeError::NotRunning) => {}
                    Err(error) => panic!("runtime step failed: {error}"),
                }
            }
            if !found.is_empty() {
                break;
            }
        }
        found
    }

    /// Advances the world until both nodes have an authenticated link.
    /// A locally submitted Direct packet is sprayed only to known links, so
    /// the host must wait for discovery before sending transient traffic.
    fn drive_until_linked<S: MessageStore>(
        world: &SimWorld,
        runtimes: &mut [Runtime<rand::rngs::StdRng, ao_network::SimTransport, S>],
        steps: usize,
    ) -> bool {
        let mut cx = Context::from_waker(Waker::noop());
        for _ in 0..steps {
            world.advance(Duration::from_millis(50));
            for runtime in runtimes.iter_mut() {
                runtime
                    .step(&mut cx, world.now(), world.now().as_millis() as u64)
                    .expect("linked runtime step succeeds");
            }
            if runtimes
                .iter()
                .all(|runtime| !runtime.adapter().network().links().is_empty())
            {
                return true;
            }
        }
        false
    }

    /// Seals a raw payload under the binding aoMesh expects for Direct
    /// traffic; the runtime itself stays unaware of the seal.
    fn seal(
        sender: &Identity,
        recipient: &PublicIdentity,
        dst: [u8; 16],
        now_ms: u64,
        plaintext: &[u8],
        rng: &mut SealRng,
    ) -> Vec<u8> {
        let probe =
            SealedMessage::seal(sender, recipient, &[0; 16], plaintext, rng).expect("probe seal");
        let header = Header {
            class: PacketClass::Direct,
            dst: Some(dst),
            msg_id: [0; 16].into(),
            origin: *sender.node_id().as_bytes(),
            created_ms: now_ms,
            lifetime_s: DEFAULT_LIFETIME_S,
            payload_len: probe.encode().len() as u64,
            ttl: DEFAULT_TTL,
            hops: 0,
        };
        let binding = derive_msg_id(&header, &[]);
        SealedMessage::seal(sender, recipient, binding.as_bytes(), plaintext, rng)
            .expect("real seal")
            .encode()
    }

    #[test]
    fn two_instances_exchange_a_raw_payload_through_the_full_stack() {
        let world = SimWorld::new(SimConfig::default());
        let alice = Identity::from_seed([1; 32]);
        let bob = Identity::from_seed([2; 32]);
        let bob_public = bob.public().clone();
        let bob_id = *bob.node_id().as_bytes();
        let puts = Rc::new(Cell::new(0));
        let evictions = Rc::new(Cell::new(0));
        let mut runtimes = vec![
            runtime_with_store(
                &world,
                11,
                Identity::from_seed([1; 32]),
                CountingStore::new(Rc::clone(&puts), Rc::clone(&evictions)),
            ),
            runtime_with_store(
                &world,
                22,
                bob,
                CountingStore::new(Rc::clone(&puts), Rc::clone(&evictions)),
            ),
        ];
        world.set_in_range(
            runtimes[0].adapter().network().transport().node(),
            runtimes[1].adapter().network().transport().node(),
            true,
        );
        let started_at = world.now();
        for runtime in &mut runtimes {
            runtime
                .start(started_at, started_at.as_millis() as u64)
                .expect("start succeeds");
        }
        assert!(
            drive_until_linked(&world, &mut runtimes, 400),
            "discovery should establish a link"
        );
        let now = world.now();
        let now_ms = now.as_millis() as u64;
        let mut rng = SealRng::seed_from_u64(7);
        let raw = b"a raw payload from alice";
        let payload = seal(&alice, &bob_public, bob_id, now_ms, raw, &mut rng);
        runtimes[0]
            .submit_raw(now, now_ms, PacketClass::Direct, Some(bob_id), &payload)
            .expect("submission accepted");

        let delivered = drive_until(&world, &mut runtimes, 200, |event| {
            matches!(event, RuntimeEvent::Delivered { .. })
        });
        assert_eq!(delivered.len(), 1, "one authenticated delivery");
        match &delivered[0] {
            RuntimeEvent::Delivered { payload, .. } => assert_eq!(payload.as_slice(), raw),
            other => panic!("unexpected event: {other:?}"),
        }
        assert_eq!(puts.get(), 1, "the authenticated packet entered the store");
        assert_eq!(evictions.get(), 1, "the receipt purged the stored copy");
        assert!(
            runtimes[1]
                .store()
                .summary(world.now().as_millis() as u64)
                .is_empty(),
            "no stale copy remains after the receipt"
        );
        for runtime in &mut runtimes {
            runtime.stop(world.now());
        }
    }

    #[test]
    fn lifecycle_is_idempotent_and_stop_ignores_late_work() {
        let world = SimWorld::new(SimConfig::default());
        let identity = Identity::from_seed([3; 32]);
        let mut runtime = runtime(&world, 33, identity);
        assert_eq!(runtime.state(), LifecycleState::Stopped);
        let now = world.now();
        runtime.start(now, 0).expect("start");
        runtime.start(now, 0).expect("second start is a no-op");
        assert_eq!(runtime.state(), LifecycleState::Running);
        runtime.pause();
        runtime.pause();
        assert_eq!(runtime.state(), LifecycleState::Paused);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(
            runtime.step(&mut cx, now, 0),
            Err(RuntimeError::NotRunning)
        ));
        runtime.resume();
        runtime.resume();
        assert_eq!(runtime.state(), LifecycleState::Running);
        runtime.stop(now);
        runtime.stop(now);
        assert_eq!(runtime.state(), LifecycleState::Stopped);
    }

    #[test]
    fn event_queue_rejects_overflow_without_dropping() {
        let mut queue = EventQueue::new(2);
        let first = RuntimeEvent::Purge { id: [1; 16].into() };
        queue.try_push(first.clone()).expect("first fits");
        queue.try_push(first.clone()).expect("second fits");
        assert_eq!(queue.try_push(first.clone()), Err(EventQueueError::Full));
        let extra = RuntimeEvent::Purge { id: [2; 16].into() };
        let (error, tail) = queue
            .try_extend(vec![extra.clone()])
            .expect_err("batch overflows");
        assert_eq!(error, EventQueueError::Full);
        assert_eq!(tail, vec![extra]);
        assert_eq!(queue.drain().len(), 2);
        assert!(queue.is_empty());
    }

    #[test]
    fn config_rejects_out_of_range_values() {
        assert!(matches!(
            RuntimeConfig::new(0, 8, 100),
            Err(ConfigError::EventCapacity { value: 0 })
        ));
        assert!(matches!(
            RuntimeConfig::new(8, 0, 100),
            Err(ConfigError::Ttl)
        ));
        assert!(matches!(
            RuntimeConfig::new(8, 8, 0),
            Err(ConfigError::Lifetime)
        ));
        let config = RuntimeConfig::new(8, 8, 100).expect("valid config");
        assert_eq!(config.event_capacity(), 8);
        assert_eq!(config.ttl(), 8);
        assert_eq!(config.lifetime_s(), 100);
    }

    #[cfg(feature = "runtime-tokio")]
    #[test]
    fn current_thread_executor_runs_a_future() {
        let value = block_on_current_thread(async { 21 + 21 }).expect("executor builds");
        assert_eq!(value, 42);
    }
}
