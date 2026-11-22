//! Local public-channel service (APP-14).
//!
//! aoMesh owns the signed channel envelope, id derivation and relay budgets
//! (MSH-23). This service owns only app concerns: joined names, a bounded
//! arrival-ordered history, duplicate removal and per-origin spam controls.
//! It consumes the opaque body of `MeshOutput::ChannelDeliver`; it never
//! interprets message text.
//!
//! Origins are the shared aoIdents `NodeId` values carried by verified mesh
//! packet headers, so no second identity type is introduced.

use ao_idents::NodeId;
use ao_mesh::packet::MAX_PAYLOAD;
use ao_mesh::{
    CHANNEL_HEADER_LEN, ChannelEnvelope, ChannelError, ChannelId, MAX_CHANNEL_NAME_LEN,
    MAX_CHANNEL_SUBSCRIPTIONS, MessageId, derive_channel_id,
};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;

/// Default local history cap.
pub const DEFAULT_MAX_HISTORY: usize = 256;
/// Default maximum number of muted origins.
pub const DEFAULT_MAX_MUTES: usize = 64;
/// Default maximum number of rate-limit states.
pub const DEFAULT_MAX_SENDERS: usize = 256;
/// Default rate-limit window.
pub const DEFAULT_WINDOW_MS: u64 = 60_000;
/// Default messages accepted per origin in one window.
pub const DEFAULT_PER_ORIGIN_LIMIT: u32 = 30;

/// Typed errors for local channel policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelsError {
    /// A bound was zero or above the mesh hard cap.
    InvalidConfig,
    /// aoMesh refused a name or an envelope operation.
    Mesh(ChannelError),
    /// A delivered envelope body plus its header would exceed the mesh cap.
    PayloadTooLarge {
        /// Offered body length.
        len: usize,
        /// Body cap.
        max: usize,
    },
    /// The mute list is full.
    MutesFull {
        /// Configured maximum.
        limit: usize,
    },
    /// A new origin could not get a rate-limit slot.
    SendersFull {
        /// Configured maximum.
        limit: usize,
    },
    /// A message arrived for a channel this node has not joined.
    NotSubscribed(ChannelId),
}

impl fmt::Display for ChannelsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig => write!(f, "invalid channels config"),
            Self::Mesh(error) => write!(f, "mesh channel rule: {error}"),
            Self::PayloadTooLarge { len, max } => {
                write!(f, "channel body {len} exceeds body cap {max}")
            }
            Self::MutesFull { limit } => write!(f, "channel mute list is full at {limit}"),
            Self::SendersFull { limit } => write!(f, "channel sender table is full at {limit}"),
            Self::NotSubscribed(id) => write!(f, "channel {id} is not subscribed locally"),
        }
    }
}

impl std::error::Error for ChannelsError {}

/// Bounds and spam policy for the local channel service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelsConfig {
    /// Maximum history entries across all channels.
    pub max_history: usize,
    /// Maximum simultaneously muted origins.
    pub max_mutes: usize,
    /// Maximum origins with rate-limit state.
    pub max_senders: usize,
    /// Sliding window length for per-origin rate limiting.
    pub window_ms: u64,
    /// Messages one origin may send in a window.
    pub per_origin_limit: u32,
    /// Local subscription cap, bounded by aoMesh.
    pub max_subscriptions: usize,
    /// Channel name cap, bounded by aoMesh.
    pub max_name_len: usize,
}

impl ChannelsConfig {
    /// Validates a config without silent clamping.
    ///
    /// Returns [`ChannelsError::InvalidConfig`] for zero bounds, a zero
    /// window or limit, or values beyond aoMesh's hard caps.
    pub fn new(
        max_history: usize,
        max_mutes: usize,
        max_senders: usize,
        window_ms: u64,
        per_origin_limit: u32,
        max_subscriptions: usize,
        max_name_len: usize,
    ) -> Result<Self, ChannelsError> {
        if max_history == 0
            || max_mutes == 0
            || max_senders == 0
            || window_ms == 0
            || per_origin_limit == 0
            || max_subscriptions == 0
            || max_subscriptions > MAX_CHANNEL_SUBSCRIPTIONS
            || max_name_len == 0
            || max_name_len > MAX_CHANNEL_NAME_LEN
        {
            return Err(ChannelsError::InvalidConfig);
        }
        Ok(Self {
            max_history,
            max_mutes,
            max_senders,
            window_ms,
            per_origin_limit,
            max_subscriptions,
            max_name_len,
        })
    }
}

impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            max_history: DEFAULT_MAX_HISTORY,
            max_mutes: DEFAULT_MAX_MUTES,
            max_senders: DEFAULT_MAX_SENDERS,
            window_ms: DEFAULT_WINDOW_MS,
            per_origin_limit: DEFAULT_PER_ORIGIN_LIMIT,
            max_subscriptions: MAX_CHANNEL_SUBSCRIPTIONS.min(32),
            max_name_len: MAX_CHANNEL_NAME_LEN,
        }
    }
}

/// One accepted channel message in arrival order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelMessage {
    /// Local arrival sequence, monotonically increasing.
    pub sequence: u64,
    /// Local arrival time.
    pub at_ms: u64,
    /// Channel the message belongs to.
    pub channel_id: ChannelId,
    /// Verified origin from the signed packet header.
    pub origin: NodeId,
    /// Content address used for duplicate removal.
    pub msg_id: MessageId,
    /// Opaque application body; never interpreted here.
    pub payload: Vec<u8>,
}

/// Result of processing one delivered channel message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestOutcome {
    /// Accepted and appended to local history.
    Accepted(ChannelMessage),
    /// Already delivered locally; not appended twice.
    Duplicate,
    /// The origin is muted; nothing is stored.
    Muted,
    /// The origin exceeded its window; nothing is stored.
    RateLimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SenderState {
    window_start_ms: u64,
    count: u32,
}

/// Bounded, deterministic local channel state.
#[derive(Debug)]
pub struct ChannelService {
    config: ChannelsConfig,
    names: BTreeMap<ChannelId, Vec<u8>>,
    clock_ms: u64,
    history: VecDeque<ChannelMessage>,
    seen: BTreeMap<MessageId, u64>,
    muted: BTreeMap<NodeId, u64>,
    senders: BTreeMap<NodeId, SenderState>,
    sequence: u64,
}

impl ChannelService {
    /// Creates an empty service.
    pub fn new(config: ChannelsConfig) -> Self {
        Self {
            config,
            names: BTreeMap::new(),
            clock_ms: 0,
            history: VecDeque::new(),
            seen: BTreeMap::new(),
            muted: BTreeMap::new(),
            senders: BTreeMap::new(),
            sequence: 0,
        }
    }

    /// Joins a channel by name, returning its derived id.
    ///
    /// Rejoining the same name is idempotent. The id comes from aoMesh's
    /// public derivation, so it matches the wire envelope exactly.
    pub fn join(&mut self, name: &[u8]) -> Result<ChannelId, ChannelsError> {
        self.check_name(name)?;
        let id = derive_channel_id(name).map_err(ChannelsError::Mesh)?;
        if let Some(bound) = self.names.get(&id) {
            if bound.as_slice() != name {
                return Err(ChannelsError::Mesh(ChannelError::IdCollision { id }));
            }
            return Ok(id);
        }
        if self.names.len() >= self.config.max_subscriptions {
            return Err(ChannelsError::Mesh(ChannelError::SubscriptionsFull {
                limit: self.config.max_subscriptions,
            }));
        }
        self.names.insert(id, name.to_vec());
        Ok(id)
    }

    /// Leaves a channel, dropping its local history and dedupe entries.
    /// Unknown names are a no-op after the name is validated.
    pub fn leave(&mut self, name: &[u8]) -> Result<(), ChannelsError> {
        self.check_name(name)?;
        let id = derive_channel_id(name).map_err(ChannelsError::Mesh)?;
        if let Some(bound) = self.names.get(&id)
            && bound.as_slice() != name
        {
            return Err(ChannelsError::Mesh(ChannelError::IdCollision { id }));
        }
        self.names.remove(&id);
        let dropped: Vec<MessageId> = self
            .history
            .iter()
            .filter(|message| message.channel_id == id)
            .map(|message| message.msg_id)
            .collect();
        self.history.retain(|message| message.channel_id != id);
        for msg_id in dropped {
            self.seen.remove(&msg_id);
        }
        Ok(())
    }

    /// Whether the channel id is subscribed locally.
    #[must_use]
    pub fn is_subscribed(&self, id: ChannelId) -> bool {
        self.names.contains_key(&id)
    }

    /// Local history for one channel in arrival order.
    #[must_use]
    pub fn history(&self, id: ChannelId) -> Vec<&ChannelMessage> {
        self.history
            .iter()
            .filter(|message| message.channel_id == id)
            .collect()
    }

    /// Mutes one origin for `duration_ms`. Re-muting refreshes the deadline.
    ///
    /// Returns [`ChannelsError::MutesFull`] before evicting somebody else's
    /// mute: capacity is explicit rather than silently losing user policy.
    pub fn mute(
        &mut self,
        now_ms: u64,
        origin: NodeId,
        duration_ms: u64,
    ) -> Result<(), ChannelsError> {
        let now = self.effective_now(now_ms);
        self.prune_mutes(now);
        if duration_ms == 0 {
            self.muted.remove(&origin);
            return Ok(());
        }
        let until = now.saturating_add(duration_ms);
        if !self.muted.contains_key(&origin) && self.muted.len() == self.config.max_mutes {
            return Err(ChannelsError::MutesFull {
                limit: self.config.max_mutes,
            });
        }
        self.muted.insert(origin, until);
        Ok(())
    }

    /// Unmutes one origin.
    pub fn unmute(&mut self, origin: NodeId) {
        self.muted.remove(&origin);
    }

    /// Whether an origin is currently muted at `now_ms`.
    pub fn is_muted(&mut self, now_ms: u64, origin: NodeId) -> bool {
        let now = self.effective_now(now_ms);
        self.prune_mutes(now);
        self.muted.contains_key(&origin)
    }

    /// Processes one verified `ChannelDeliver` body.
    ///
    /// The caller supplies the local time, verified origin bytes, content
    /// address and opaque envelope body. Ordering is by arrival, not by any
    /// remote timestamp, so a skewed sender clock cannot reorder history.
    /// Duplicate removal happens before spam accounting: a replay cannot
    /// consume an origin's allowance.
    pub fn receive(
        &mut self,
        now_ms: u64,
        channel_id: ChannelId,
        origin: NodeId,
        msg_id: MessageId,
        body: &[u8],
    ) -> Result<IngestOutcome, ChannelsError> {
        if !self.names.contains_key(&channel_id) {
            return Err(ChannelsError::NotSubscribed(channel_id));
        }
        let max_body = MAX_PAYLOAD.saturating_sub(CHANNEL_HEADER_LEN);
        if body.len() > max_body {
            return Err(ChannelsError::PayloadTooLarge {
                len: body.len(),
                max: max_body,
            });
        }
        if self.seen.contains_key(&msg_id) {
            return Ok(IngestOutcome::Duplicate);
        }
        let now = self.effective_now(now_ms);
        self.prune_mutes(now);
        if self.muted.contains_key(&origin) {
            return Ok(IngestOutcome::Muted);
        }
        if !self.allow_origin(now, origin)? {
            return Ok(IngestOutcome::RateLimited);
        }

        let sequence = self.next_sequence();
        let message = ChannelMessage {
            sequence,
            at_ms: now,
            channel_id,
            origin,
            msg_id,
            payload: body.to_vec(),
        };
        if self.history.len() == self.config.max_history
            && let Some(removed) = self.history.pop_front()
        {
            self.seen.remove(&removed.msg_id);
        }
        self.history.push_back(message.clone());
        self.seen.insert(msg_id, sequence);
        Ok(IngestOutcome::Accepted(message))
    }

    /// Wraps an opaque body in the real aoMesh channel envelope.
    ///
    /// Encoding remains owned by aoMesh; this is a convenience for callers
    /// and tests, not a second wire format.
    pub fn envelope(
        channel_id: ChannelId,
        body: &[u8],
    ) -> Result<ChannelEnvelope<'_>, ChannelsError> {
        ChannelEnvelope::new(channel_id, body).map_err(ChannelsError::Mesh)
    }

    fn check_name(&self, name: &[u8]) -> Result<(), ChannelsError> {
        if name.is_empty() {
            return Err(ChannelsError::Mesh(ChannelError::EmptyName));
        }
        if name.len() > self.config.max_name_len {
            return Err(ChannelsError::Mesh(ChannelError::NameTooLong {
                len: name.len(),
                max: self.config.max_name_len,
            }));
        }
        Ok(())
    }

    fn allow_origin(&mut self, now: u64, origin: NodeId) -> Result<bool, ChannelsError> {
        self.prune_senders(now);
        if let Some(state) = self.senders.get_mut(&origin) {
            if state.count >= self.config.per_origin_limit {
                return Ok(false);
            }
            state.count = state.count.saturating_add(1);
            return Ok(true);
        }
        if self.senders.len() == self.config.max_senders {
            return Err(ChannelsError::SendersFull {
                limit: self.config.max_senders,
            });
        }
        self.senders.insert(
            origin,
            SenderState {
                window_start_ms: now,
                count: 1,
            },
        );
        Ok(true)
    }

    fn prune_senders(&mut self, now: u64) {
        let expired: Vec<NodeId> = self
            .senders
            .iter()
            .filter(|(_, state)| now.saturating_sub(state.window_start_ms) >= self.config.window_ms)
            .map(|(origin, _)| *origin)
            .collect();
        for origin in expired {
            self.senders.remove(&origin);
        }
    }

    fn prune_mutes(&mut self, now: u64) {
        let expired: Vec<NodeId> = self
            .muted
            .iter()
            .filter(|(_, until)| **until <= now)
            .map(|(origin, _)| *origin)
            .collect();
        for origin in expired {
            self.muted.remove(&origin);
        }
    }

    fn effective_now(&mut self, now_ms: u64) -> u64 {
        self.clock_ms = self.clock_ms.max(now_ms);
        self.clock_ms
    }

    fn next_sequence(&mut self) -> u64 {
        let sequence = self.sequence;
        self.sequence = self.sequence.saturating_add(1);
        sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(byte: u8) -> NodeId {
        NodeId::from_bytes([byte; 16])
    }

    fn id(byte: u8) -> MessageId {
        MessageId::from_bytes([byte; 16])
    }

    fn service() -> ChannelService {
        let config = ChannelsConfig::new(4, 2, 8, 100, 2, 4, 16).expect("valid config");
        ChannelService::new(config)
    }

    #[test]
    fn config_rejects_bounds_outside_mesh_caps() {
        assert_eq!(
            ChannelsConfig::new(0, 1, 1, 1, 1, 1, 1),
            Err(ChannelsError::InvalidConfig)
        );
        assert_eq!(
            ChannelsConfig::new(1, 1, 1, 1, 1, 1, MAX_CHANNEL_NAME_LEN + 1),
            Err(ChannelsError::InvalidConfig)
        );
        assert_eq!(
            ChannelsConfig::new(1, 1, 1, 1, 1, MAX_CHANNEL_SUBSCRIPTIONS + 1, 1),
            Err(ChannelsError::InvalidConfig)
        );
        assert!(ChannelsConfig::new(1, 1, 1, 1, 1, 1, 1).is_ok());
    }

    #[test]
    fn joins_use_the_mesh_id_and_leave_drops_history() {
        let mut service = service();
        let channel = service.join(b"town-square").expect("name is valid");
        assert_eq!(
            channel,
            derive_channel_id(b"town-square").expect("valid name")
        );
        assert!(service.is_subscribed(channel));
        let envelope = ChannelService::envelope(channel, b"hello").expect("body fits");
        let wire = envelope.encode();
        let decoded = ChannelEnvelope::decode(&wire).expect("envelope round-trip");
        assert_eq!(decoded.channel_id, channel);
        assert_eq!(decoded.payload, b"hello");
        assert_eq!(wire.len(), CHANNEL_HEADER_LEN + b"hello".len());

        service
            .receive(1, channel, origin(1), id(1), decoded.payload)
            .expect("subscribed");
        assert_eq!(service.history(channel).len(), 1);
        service.leave(b"town-square").expect("known subscription");
        assert!(!service.is_subscribed(channel));
        assert!(service.history(channel).is_empty());

        let rejoined = service.join(b"town-square").expect("rejoin");
        assert_eq!(rejoined, channel);
        assert!(matches!(
            service.receive(2, rejoined, origin(2), id(1), b"x"),
            Ok(IngestOutcome::Accepted(_))
        ));
    }

    #[test]
    fn config_name_bound_is_enforced_before_derivation() {
        let mut service = service();
        assert_eq!(
            service.join(b"12345678901234567"),
            Err(ChannelsError::Mesh(ChannelError::NameTooLong {
                len: 17,
                max: 16,
            }))
        );
    }

    #[test]
    fn history_is_arrival_ordered_duplicates_removed_and_capped() {
        let mut service = service();
        let channel = service.join(b"orders").expect("name is valid");
        let other = service.join(b"other").expect("name is valid");
        for (at, sender, msg) in [(1u64, 1u8, 1u8), (2, 2, 2), (3, 3, 3), (4, 4, 4)] {
            service
                .receive(at, channel, origin(sender), id(msg), &[msg])
                .expect("accepted");
        }
        service
            .receive(5, other, origin(5), id(9), b"other")
            .expect("accepted");

        let history = service.history(channel);
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].payload, vec![2]);
        assert_eq!(history[1].payload, vec![3]);
        assert_eq!(history[2].payload, vec![4]);
        assert!(history.iter().map(|m| m.sequence).eq(1..4));

        let duplicate = service
            .receive(6, channel, origin(4), id(4), &[4])
            .expect("duplicate is a typed outcome");
        assert_eq!(duplicate, IngestOutcome::Duplicate);
        assert_eq!(service.history(channel).len(), 3);
    }

    #[test]
    fn duplicate_does_not_consume_rate_allowance() {
        let mut service = service();
        let channel = service.join(b"duplicates").expect("name is valid");
        assert!(matches!(
            service.receive(1, channel, origin(1), id(1), b"a"),
            Ok(IngestOutcome::Accepted(_))
        ));
        assert_eq!(
            service.receive(2, channel, origin(1), id(1), b"a"),
            Ok(IngestOutcome::Duplicate)
        );
        assert!(matches!(
            service.receive(3, channel, origin(1), id(2), b"b"),
            Ok(IngestOutcome::Accepted(_))
        ));
        assert_eq!(
            service.receive(4, channel, origin(1), id(3), b"c"),
            Ok(IngestOutcome::RateLimited)
        );
    }

    #[test]
    fn mutes_and_rate_limits_are_bounded_and_typed() {
        let mut service = service();
        let channel = service.join(b"limited").expect("name is valid");
        service.mute(10, origin(7), 50).expect("mute list has room");
        assert!(service.is_muted(10, origin(7)));
        assert_eq!(
            service.receive(11, channel, origin(7), id(1), b"x"),
            Ok(IngestOutcome::Muted)
        );
        assert!(!service.is_muted(61, origin(7)));

        assert!(matches!(
            service.receive(61, channel, origin(1), id(2), b"a"),
            Ok(IngestOutcome::Accepted(_))
        ));
        assert!(matches!(
            service.receive(62, channel, origin(1), id(3), b"b"),
            Ok(IngestOutcome::Accepted(_))
        ));
        assert_eq!(
            service.receive(63, channel, origin(1), id(4), b"c"),
            Ok(IngestOutcome::RateLimited)
        );
        assert!(matches!(
            service.receive(200, channel, origin(1), id(5), b"d"),
            Ok(IngestOutcome::Accepted(_))
        ));

        let absent = derive_channel_id(b"absent").expect("valid name");
        assert_eq!(
            service.receive(201, absent, origin(9), id(6), b"e"),
            Err(ChannelsError::NotSubscribed(absent))
        );
    }

    #[test]
    fn oversized_body_is_refused_before_copying() {
        let mut service = service();
        let channel = service.join(b"large").expect("name is valid");
        let body = vec![0u8; MAX_PAYLOAD];
        assert_eq!(
            service.receive(1, channel, origin(1), id(1), &body),
            Err(ChannelsError::PayloadTooLarge {
                len: MAX_PAYLOAD,
                max: MAX_PAYLOAD - CHANNEL_HEADER_LEN,
            })
        );
    }

    #[test]
    fn mute_capacity_is_explicit() {
        let mut service = service();
        service.mute(1, origin(1), 100).expect("first mute");
        service.mute(1, origin(2), 100).expect("second mute");
        assert_eq!(
            service.mute(1, origin(3), 100),
            Err(ChannelsError::MutesFull { limit: 2 })
        );
        service.mute(1, origin(1), u64::MAX).expect("no overflow");
        assert!(service.is_muted(u64::MAX - 1, origin(1)));
        assert!(!service.is_muted(u64::MAX, origin(1)));
    }

    #[test]
    fn sender_capacity_is_explicit() {
        let config = ChannelsConfig::new(4, 2, 1, 100, 1, 4, 16).expect("valid config");
        let mut service = ChannelService::new(config);
        let channel = service.join(b"full").expect("name is valid");
        assert!(matches!(
            service.receive(1, channel, origin(1), id(1), b"a"),
            Ok(IngestOutcome::Accepted(_))
        ));
        assert_eq!(
            service.receive(1, channel, origin(2), id(2), b"b"),
            Err(ChannelsError::SendersFull { limit: 1 })
        );
        assert!(matches!(
            service.receive(200, channel, origin(2), id(3), b"c"),
            Ok(IngestOutcome::Accepted(_))
        ));
    }

    #[test]
    fn history_sequence_advances_by_one_per_accepted_message() {
        let mut service = service();
        let channel = service.join(b"sequence").expect("name is valid");
        for (index, msg_id) in [id(1), id(2), id(3)].into_iter().enumerate() {
            let outcome = service
                .receive(
                    index as u64 + 1,
                    channel,
                    origin(u8::try_from(index).expect("three origins") + 1),
                    msg_id,
                    b"body",
                )
                .expect("body fits");
            let IngestOutcome::Accepted(message) = outcome else {
                panic!("unique messages must be accepted");
            };
            assert_eq!(message.sequence, index as u64);
        }
    }
}
