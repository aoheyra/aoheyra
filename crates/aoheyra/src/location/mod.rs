//! Location sharing service: bounded sessions, recipient sealing and privacy.
//!
//! The service is sans-IO. It receives an injected RNG, stores bounded session
//! state and returns prepared outgoing packets plus events. It never reads a
//! clock, opens a socket or learns an application payload outside
//! [`crate::payload`].
//!
//! APP-07 supplies the location payload codec. That codec applies the privacy
//! policy before encoding: `Approx100m` rounds coordinates onto a roughly
//! 100 m grid and `Off` omits coordinates entirely. The location service
//! builds those values at the share boundary, so a precise input coordinate is
//! never encoded for a non-precise session. Sealed recipient updates use
//! aoIdents `SealedMessage`; no second encryption scheme is introduced here.

use std::collections::BTreeMap;
use std::fmt;

use ao_idents::{IdentError, Identity, PublicIdentity, SealedMessage, compute_node_id};
use ao_mesh::packet::PacketClass;
use ao_mesh::{Header, MessageId, derive_msg_id};
use rand_core06::{CryptoRng, RngCore};

use crate::payload::{AppPayload, LocationPrecision, LocationUpdate, PayloadError};

/// Default hop limit for a location packet.
pub const DEFAULT_TTL: u8 = 8;
/// Maximum number of concurrent local share sessions.
pub const MAX_SESSIONS: usize = 32;
/// Maximum number of chosen contacts per session.
pub const MAX_RECIPIENTS: usize = 16;
/// Maximum duration of one share session, in milliseconds (24 hours).
pub const MAX_DURATION_MS: u64 = 86_400_000;
/// Maximum interval between planned publishes, in milliseconds (1 hour).
pub const MAX_INTERVAL_MS: u64 = 3_600_000;
/// Hard cap on a sealed location update on the wire.
pub const MAX_SEALED_BYTES: usize = 1024;
/// Hard cap on aggregated peer positions kept across all sessions.
pub const MAX_PEER_POSITIONS: usize = 512;

const SHARE_VERSION: u8 = 1;
const SHARE_KIND_LOCATION: u8 = 1;
const SHARE_KIND_STOP: u8 = 2;
const SHARE_FIXED_LEN: usize = 1 + 1 + 16 + 8 + 8 + 8 + 1;

/// Opaque identifier for one share session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId([u8; 16]);

impl SessionId {
    /// Wraps 16 random bytes as a session identifier.
    #[must_use]
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Returns the identifier bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// The node identifier used for a chosen contact.
pub type RecipientId = [u8; 16];

/// One raw device location sample supplied by the host.
///
/// The service applies the session's precision policy when it builds an
/// outgoing update. Keeping the raw sample on the device is allowed; only
/// prepared outgoing payloads are privacy-relevant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocationSample {
    /// Unix timestamp in whole seconds.
    pub timestamp_s: u32,
    /// Latitude in 1e-6 degrees.
    pub latitude_e6: i32,
    /// Longitude in 1e-6 degrees.
    pub longitude_e6: i32,
    /// Reported accuracy in metres.
    pub accuracy_m: u16,
    /// Heading in tenths of a degree, if available.
    pub heading_tenths: Option<u16>,
}

/// Validated policy for one location share session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    /// Chosen contacts. Each receives a sealed update at session start and
    /// on explicit stop or expiry.
    pub recipients: Vec<PublicIdentity>,
    /// Wall-clock start time supplied by the host, in milliseconds.
    pub started_at_ms: u64,
    /// How long the session remains active, in milliseconds.
    pub duration_ms: u64,
    /// Interval between planned local publishes, in milliseconds.
    pub interval_ms: u64,
    /// Precision applied to every outgoing update.
    pub precision: LocationPrecision,
    /// Initial device sample, replaced by [`LocationService::update_sample`].
    pub initial: LocationSample,
}

/// Whether a share payload carries a position or ends a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareKind {
    /// A regular position update.
    Location,
    /// A revocation notice sent when sharing stops or expires.
    Stop,
}

/// A prepared outgoing location packet.
///
/// A sealed recipient update has a `recipient` and a `sealed` envelope. A
/// local telemetry publish has neither, because its payload has already been
/// reduced to the session's chosen precision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingLocation {
    session_id: SessionId,
    recipient: Option<RecipientId>,
    sequence: u64,
    kind: ShareKind,
    header: Header,
    sealed: Option<SealedMessage>,
    location: Option<LocationUpdate>,
}

impl OutgoingLocation {
    /// Session this packet belongs to.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Intended contact for a sealed update, or `None` for a local publish.
    #[must_use]
    pub const fn recipient(&self) -> Option<RecipientId> {
        self.recipient
    }

    /// Monotonic sequence inside the session.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Whether this packet carries a position or a stop notice.
    #[must_use]
    pub const fn kind(&self) -> ShareKind {
        self.kind
    }

    /// aoMesh header to submit with the payload.
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// Sealed envelope, present only for a chosen-contact update.
    #[must_use]
    pub const fn sealed(&self) -> Option<&SealedMessage> {
        self.sealed.as_ref()
    }

    /// Precision-limited position, absent from a stop notice.
    #[must_use]
    pub const fn location(&self) -> Option<LocationUpdate> {
        self.location
    }

    /// Message id carried by the prepared header.
    #[must_use]
    pub const fn message_id(&self) -> MessageId {
        self.header.msg_id
    }
}

/// An immutable snapshot of a live session for diagnostics and UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionView {
    session_id: SessionId,
    started_at_ms: u64,
    ends_at_ms: u64,
    interval_ms: u64,
    recipient_count: usize,
    next_sequence: u64,
    precision: LocationPrecision,
}

impl SessionView {
    /// Session identifier.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Host-supplied start time in milliseconds.
    #[must_use]
    pub const fn started_at_ms(&self) -> u64 {
        self.started_at_ms
    }

    /// Absolute expiry time in milliseconds.
    #[must_use]
    pub const fn ends_at_ms(&self) -> u64 {
        self.ends_at_ms
    }

    /// Planned publish interval in milliseconds.
    #[must_use]
    pub const fn interval_ms(&self) -> u64 {
        self.interval_ms
    }

    /// Number of chosen contacts.
    #[must_use]
    pub const fn recipient_count(&self) -> usize {
        self.recipient_count
    }

    /// Next sequence number to publish.
    #[must_use]
    pub const fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// Precision applied to outgoing updates.
    #[must_use]
    pub const fn precision(&self) -> LocationPrecision {
        self.precision
    }
}

/// A sealed position update received from a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedLocationUpdate {
    session_id: SessionId,
    sender: RecipientId,
    recipient: RecipientId,
    sequence: u64,
    message_id: MessageId,
    envelope: SealedMessage,
}

impl ReceivedLocationUpdate {
    /// Wraps the fields extracted from an incoming aoMesh packet header and
    /// payload.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        sender: RecipientId,
        recipient: RecipientId,
        sequence: u64,
        message_id: MessageId,
        envelope: SealedMessage,
    ) -> Self {
        Self {
            session_id,
            sender,
            recipient,
            sequence,
            message_id,
            envelope,
        }
    }

    /// Session named by the update.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Authenticated sender claimed by the packet header.
    #[must_use]
    pub const fn sender(&self) -> RecipientId {
        self.sender
    }

    /// Recipient named by the packet header.
    #[must_use]
    pub const fn recipient(&self) -> RecipientId {
        self.recipient
    }

    /// Sequence claimed by the packet header.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Message id from the packet header, used as the seal's associated data.
    #[must_use]
    pub const fn message_id(&self) -> MessageId {
        self.message_id
    }

    /// Sealed envelope from the packet payload.
    #[must_use]
    pub const fn envelope(&self) -> &SealedMessage {
        &self.envelope
    }
}

/// Why an incoming update was ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreReason {
    /// The update claimed the local sharer's identity.
    OwnerUpdate,
    /// The sequence was already seen or went backwards.
    Replayed,
    /// The envelope did not authenticate to this recipient.
    InvalidEnvelope,
    /// The plaintext did not match the session policy.
    InvalidPayload,
}

/// Output event from the location service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocationEvent {
    /// A session was created.
    SessionStarted {
        /// New session identifier.
        session_id: SessionId,
        /// Host-supplied start time.
        started_at_ms: u64,
        /// Absolute expiry time.
        ends_at_ms: u64,
    },
    /// A sealed update or stop notice is ready for one chosen contact.
    RecipientUpdate {
        /// Prepared packet.
        outgoing: OutgoingLocation,
    },
    /// A precision-limited local publish is due.
    PublishDue {
        /// Prepared packet.
        outgoing: OutgoingLocation,
    },
    /// The host stopped the session; publishing stopped immediately.
    SessionStopped {
        /// Stopped session.
        session_id: SessionId,
    },
    /// The session reached its expiry time and was removed.
    SessionExpired {
        /// Expired session.
        session_id: SessionId,
    },
    /// A peer position was authenticated and aggregated.
    PeerLocated {
        /// Session the update belongs to.
        session_id: SessionId,
        /// Authenticated sender.
        sender: RecipientId,
        /// Update sequence.
        sequence: u64,
        /// Precision-limited position.
        location: LocationUpdate,
    },
    /// A peer revoked its session.
    PeerStopped {
        /// Session the notice belongs to.
        session_id: SessionId,
        /// Authenticated sender.
        sender: RecipientId,
        /// Update sequence.
        sequence: u64,
    },
    /// An untrusted incoming update was dropped.
    UpdateIgnored {
        /// Session named by the update.
        session_id: SessionId,
        /// Sender named by the update.
        sender: RecipientId,
        /// Typed reason for the drop.
        reason: IgnoreReason,
    },
}

/// Typed failure from configuration or local state management.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocationError {
    /// A public field was empty, oversized or inconsistent.
    InvalidConfig(&'static str),
    /// The session id is not live.
    UnknownSession(SessionId),
    /// The session cap is full.
    TooManySessions,
    /// An integer computation overflowed.
    Overflow,
    /// The APP-07 payload codec rejected a value.
    Payload(PayloadError),
    /// aoIdents rejected a seal or an open operation.
    Ident(IdentError),
}

impl fmt::Display for LocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(field) => write!(f, "invalid location config: {field}"),
            Self::UnknownSession(id) => write!(f, "unknown location session {id}"),
            Self::TooManySessions => f.write_str("too many live location sessions"),
            Self::Overflow => f.write_str("location time arithmetic overflowed"),
            Self::Payload(error) => write!(f, "location payload error: {error}"),
            Self::Ident(error) => write!(f, "location seal error: {error}"),
        }
    }
}

impl std::error::Error for LocationError {}

impl From<PayloadError> for LocationError {
    fn from(error: PayloadError) -> Self {
        Self::Payload(error)
    }
}

impl From<IdentError> for LocationError {
    fn from(error: IdentError) -> Self {
        Self::Ident(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SharePayload {
    kind: ShareKind,
    session_id: SessionId,
    sequence: u64,
    started_at_ms: u64,
    ends_at_ms: u64,
    location: Option<LocationUpdate>,
}

impl SharePayload {
    fn encode(&self) -> Result<Vec<u8>, LocationError> {
        let location_bytes = match self.location {
            Some(location) => AppPayload::Location(location).encode()?,
            None => Vec::new(),
        };
        if location_bytes.len() > u8::MAX as usize {
            return Err(LocationError::Payload(PayloadError::TooLarge {
                len: location_bytes.len(),
                max: u8::MAX as usize,
            }));
        }
        let mut out = Vec::with_capacity(SHARE_FIXED_LEN + location_bytes.len());
        out.push(SHARE_VERSION);
        out.push(match self.kind {
            ShareKind::Location => SHARE_KIND_LOCATION,
            ShareKind::Stop => SHARE_KIND_STOP,
        });
        out.extend_from_slice(self.session_id.as_bytes());
        out.extend_from_slice(&self.sequence.to_be_bytes());
        out.extend_from_slice(&self.started_at_ms.to_be_bytes());
        out.extend_from_slice(&self.ends_at_ms.to_be_bytes());
        out.push(location_bytes.len() as u8);
        out.extend_from_slice(&location_bytes);
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> Result<Self, LocationError> {
        if bytes.len() < SHARE_FIXED_LEN {
            return Err(LocationError::Payload(PayloadError::Truncated {
                need: SHARE_FIXED_LEN,
                have: bytes.len(),
            }));
        }
        if bytes[0] != SHARE_VERSION {
            return Err(LocationError::Payload(PayloadError::UnsupportedVersion(
                bytes[0],
            )));
        }
        let kind = match bytes[1] {
            SHARE_KIND_LOCATION => ShareKind::Location,
            SHARE_KIND_STOP => ShareKind::Stop,
            other => return Err(LocationError::Payload(PayloadError::UnknownKind(other))),
        };
        let session_bytes: [u8; 16] = bytes[2..18].try_into().map_err(|_| {
            LocationError::Payload(PayloadError::Truncated {
                need: 18,
                have: bytes.len(),
            })
        })?;
        let sequence = u64::from_be_bytes(bytes[18..26].try_into().map_err(|_| {
            LocationError::Payload(PayloadError::Truncated {
                need: 26,
                have: bytes.len(),
            })
        })?);
        let started_at_ms = u64::from_be_bytes(bytes[26..34].try_into().map_err(|_| {
            LocationError::Payload(PayloadError::Truncated {
                need: 34,
                have: bytes.len(),
            })
        })?);
        let ends_at_ms = u64::from_be_bytes(bytes[34..42].try_into().map_err(|_| {
            LocationError::Payload(PayloadError::Truncated {
                need: 42,
                have: bytes.len(),
            })
        })?);
        let location_len = usize::from(bytes[42]);
        if bytes.len() != SHARE_FIXED_LEN + location_len {
            return Err(LocationError::Payload(PayloadError::Trailing {
                expected: SHARE_FIXED_LEN + location_len,
                actual: bytes.len(),
            }));
        }
        let location = if location_len == 0 {
            None
        } else {
            match AppPayload::decode(&bytes[43..])? {
                AppPayload::Location(location) => Some(location),
                _ => {
                    return Err(LocationError::Payload(PayloadError::InvalidFlags(bytes[1])));
                }
            }
        };
        if (kind == ShareKind::Stop) != location.is_none() {
            return Err(LocationError::Payload(PayloadError::InvalidFlags(bytes[1])));
        }
        Ok(Self {
            kind,
            session_id: SessionId::new(session_bytes),
            sequence,
            started_at_ms,
            ends_at_ms,
            location,
        })
    }
}

#[derive(Debug, Clone)]
struct SessionState {
    config: SessionConfig,
    ends_at_ms: u64,
    next_sequence: u64,
    last_sample: LocationUpdate,
}

/// Sans-IO owner of all live location share sessions.
pub struct LocationService<R> {
    owner: Identity,
    rng: R,
    sessions: BTreeMap<SessionId, SessionState>,
    peer_positions: BTreeMap<(SessionId, RecipientId), LocationUpdate>,
    peer_sequence: BTreeMap<(SessionId, RecipientId), u64>,
}

impl<R: RngCore + CryptoRng> LocationService<R> {
    /// Creates an empty service for `owner`.
    #[must_use]
    pub fn new(owner: Identity, rng: R) -> Self {
        Self {
            owner,
            rng,
            sessions: BTreeMap::new(),
            peer_positions: BTreeMap::new(),
            peer_sequence: BTreeMap::new(),
        }
    }

    /// Returns the host identity used to seal recipient updates.
    #[must_use]
    pub const fn owner(&self) -> &Identity {
        &self.owner
    }

    /// Starts a validated session and returns its id plus initial events.
    ///
    /// # Errors
    ///
    /// A [`LocationError`] for invalid config, session-cap pressure, time
    /// overflow or a sealing failure.
    pub fn start_session(
        &mut self,
        config: SessionConfig,
    ) -> Result<(SessionId, Vec<LocationEvent>), LocationError> {
        validate_config(&config)?;
        let ends_at_ms = config
            .started_at_ms
            .checked_add(config.duration_ms)
            .ok_or(LocationError::Overflow)?;
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(LocationError::TooManySessions);
        }
        let mut id_bytes = [0u8; 16];
        self.rng.fill_bytes(&mut id_bytes);
        let session_id = SessionId::new(id_bytes);
        let sample = make_update(config.precision, &config.initial)?;
        let state = SessionState {
            config: config.clone(),
            ends_at_ms,
            next_sequence: 1,
            last_sample: sample,
        };
        let share = SharePayload {
            kind: ShareKind::Location,
            session_id,
            sequence: 0,
            started_at_ms: config.started_at_ms,
            ends_at_ms,
            location: Some(sample),
        };
        let mut events = vec![LocationEvent::SessionStarted {
            session_id,
            started_at_ms: config.started_at_ms,
            ends_at_ms,
        }];
        for recipient in &config.recipients {
            events.push(LocationEvent::RecipientUpdate {
                outgoing: self.prepare_sealed(&state, recipient, &share)?,
            });
        }
        self.sessions.insert(session_id, state);
        Ok((session_id, events))
    }

    /// Replaces the latest sample for a live session.
    ///
    /// # Errors
    ///
    /// [`LocationError::UnknownSession`] if the id is not live, or a typed
    /// payload error if the sample is invalid for the session precision.
    pub fn update_sample(
        &mut self,
        session_id: SessionId,
        sample: LocationSample,
    ) -> Result<(), LocationError> {
        let state = self
            .sessions
            .get_mut(&session_id)
            .ok_or(LocationError::UnknownSession(session_id))?;
        state.last_sample = make_update(state.config.precision, &sample)?;
        Ok(())
    }

    /// Stops a session immediately and returns sealed stop notices.
    ///
    /// Removing the state before returning means no later
    /// [`LocationService::next_publish`] call can produce another publish for
    /// this session.
    ///
    /// # Errors
    ///
    /// [`LocationError::UnknownSession`] if the id is not live.
    pub fn stop_session(
        &mut self,
        session_id: SessionId,
        now_ms: u64,
    ) -> Result<Vec<LocationEvent>, LocationError> {
        let state = self
            .sessions
            .remove(&session_id)
            .ok_or(LocationError::UnknownSession(session_id))?;
        let share = SharePayload {
            kind: ShareKind::Stop,
            session_id,
            sequence: state.next_sequence,
            started_at_ms: state.config.started_at_ms,
            ends_at_ms: now_ms.max(state.config.started_at_ms),
            location: None,
        };
        let mut events = vec![LocationEvent::SessionStopped { session_id }];
        for recipient in &state.config.recipients {
            events.push(LocationEvent::RecipientUpdate {
                outgoing: self.prepare_sealed(&state, recipient, &share)?,
            });
        }
        self.peer_positions.retain(|(id, _), _| *id != session_id);
        Ok(events)
    }

    /// Returns the next due local publish without consuming it.
    ///
    /// # Errors
    ///
    /// A typed payload error if the stored sample cannot be encoded.
    pub fn next_publish(&mut self, now_ms: u64) -> Result<Option<OutgoingLocation>, LocationError> {
        let mut due = None;
        for (session_id, state) in &self.sessions {
            if now_ms >= state.ends_at_ms {
                continue;
            }
            let Some(sequence) = state.next_sequence.checked_mul(state.config.interval_ms) else {
                continue;
            };
            let Some(at_ms) = state.config.started_at_ms.checked_add(sequence) else {
                continue;
            };
            if at_ms <= now_ms && due.is_none_or(|(current, _)| (at_ms, *session_id) < current) {
                due = Some(((at_ms, *session_id), (*session_id, state.next_sequence)));
            }
        }
        let Some((_, (session_id, sequence))) = due else {
            return Ok(None);
        };
        let state = self.sessions[&session_id].clone();
        let share = SharePayload {
            kind: ShareKind::Location,
            session_id,
            sequence,
            started_at_ms: state.config.started_at_ms,
            ends_at_ms: state.ends_at_ms,
            location: Some(state.last_sample),
        };
        self.prepare_local_publish(&share).map(Some)
    }

    /// Advances the local sequence after a host submits a publish plan.
    ///
    /// # Errors
    ///
    /// [`LocationError::UnknownSession`] if the session was stopped or expired.
    pub fn mark_published(&mut self, outgoing: &OutgoingLocation) -> Result<(), LocationError> {
        let state = self
            .sessions
            .get_mut(&outgoing.session_id())
            .ok_or(LocationError::UnknownSession(outgoing.session_id()))?;
        if outgoing.recipient().is_some() {
            return Ok(());
        }
        if state.next_sequence <= outgoing.sequence() {
            state.next_sequence = outgoing.sequence().saturating_add(1);
        }
        Ok(())
    }

    /// Returns a snapshot of every live session.
    pub fn sessions(&self) -> impl Iterator<Item = SessionView> + '_ {
        self.sessions.iter().map(|(session_id, state)| SessionView {
            session_id: *session_id,
            started_at_ms: state.config.started_at_ms,
            ends_at_ms: state.ends_at_ms,
            interval_ms: state.config.interval_ms,
            recipient_count: state.config.recipients.len(),
            next_sequence: state.next_sequence,
            precision: state.config.precision,
        })
    }

    /// Whether a session is still live at `now_ms`.
    #[must_use]
    pub fn is_active(&self, session_id: SessionId, now_ms: u64) -> bool {
        self.sessions
            .get(&session_id)
            .is_some_and(|state| now_ms < state.ends_at_ms)
    }

    /// Returns one aggregated peer position per sender for a session.
    pub fn peer_positions(
        &self,
        session_id: SessionId,
    ) -> impl Iterator<Item = (RecipientId, LocationUpdate)> + '_ {
        self.peer_positions
            .iter()
            .filter_map(move |((id, sender), location)| {
                (*id == session_id).then_some((*sender, *location))
            })
    }

    /// Authenticates and aggregates one incoming sealed update.
    ///
    /// Invalid, replayed and unauthorized updates are dropped as typed
    /// [`LocationEvent::UpdateIgnored`] events rather than errors, so one bad
    /// packet cannot abort a host input loop.
    ///
    /// # Errors
    ///
    /// [`LocationError::Payload`] only when local state is internally invalid.
    pub fn receive(
        &mut self,
        now_ms: u64,
        local: &Identity,
        update: ReceivedLocationUpdate,
    ) -> Result<Vec<LocationEvent>, LocationError> {
        let mut events = self.reap_expired(now_ms)?;
        let owner_id = *self.owner.node_id().as_bytes();
        let local_id = *local.node_id().as_bytes();
        let envelope_len = update.envelope().encode().len();
        let peer_key = (update.session_id(), update.sender());
        let reason = if update.sender() == owner_id {
            Some(IgnoreReason::OwnerUpdate)
        } else if local_id != update.recipient() || envelope_len > MAX_SEALED_BYTES {
            Some(IgnoreReason::InvalidEnvelope)
        } else if self
            .peer_sequence
            .get(&peer_key)
            .is_some_and(|last| update.sequence() <= *last)
        {
            Some(IgnoreReason::Replayed)
        } else {
            None
        };
        if let Some(reason) = reason {
            events.push(LocationEvent::UpdateIgnored {
                session_id: update.session_id(),
                sender: update.sender(),
                reason,
            });
            return Ok(events);
        }

        let opened = match update
            .envelope()
            .open(local, update.message_id().as_bytes())
        {
            Ok(opened) => opened,
            Err(_) => {
                events.push(LocationEvent::UpdateIgnored {
                    session_id: update.session_id(),
                    sender: update.sender(),
                    reason: IgnoreReason::InvalidEnvelope,
                });
                return Ok(events);
            }
        };
        let opened_node =
            *compute_node_id(opened.sender().signing_pk(), opened.sender().agreement_pk())
                .as_bytes();
        if opened_node != update.sender() {
            events.push(LocationEvent::UpdateIgnored {
                session_id: update.session_id(),
                sender: update.sender(),
                reason: IgnoreReason::InvalidEnvelope,
            });
            return Ok(events);
        }
        let share = match SharePayload::decode(opened.plaintext()) {
            Ok(share) => share,
            Err(_) => {
                events.push(LocationEvent::UpdateIgnored {
                    session_id: update.session_id(),
                    sender: update.sender(),
                    reason: IgnoreReason::InvalidPayload,
                });
                return Ok(events);
            }
        };
        let valid = share.session_id == update.session_id() && share.sequence == update.sequence();
        if !valid {
            events.push(LocationEvent::UpdateIgnored {
                session_id: update.session_id(),
                sender: update.sender(),
                reason: IgnoreReason::InvalidPayload,
            });
            return Ok(events);
        }
        self.peer_sequence.insert(peer_key, update.sequence());
        match share.kind {
            ShareKind::Location => {
                let location =
                    share
                        .location
                        .ok_or(LocationError::Payload(PayloadError::InvalidFlags(
                            SHARE_KIND_LOCATION,
                        )))?;
                self.peer_positions
                    .insert((update.session_id(), update.sender()), location);
                self.trim_peer_positions();
                events.push(LocationEvent::PeerLocated {
                    session_id: update.session_id(),
                    sender: update.sender(),
                    sequence: update.sequence(),
                    location,
                });
            }
            ShareKind::Stop => {
                self.peer_positions
                    .remove(&(update.session_id(), update.sender()));
                events.push(LocationEvent::PeerStopped {
                    session_id: update.session_id(),
                    sender: update.sender(),
                    sequence: update.sequence(),
                });
            }
        }
        Ok(events)
    }

    fn reap_expired(&mut self, now_ms: u64) -> Result<Vec<LocationEvent>, LocationError> {
        let expired: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, state)| now_ms >= state.ends_at_ms)
            .map(|(id, _)| *id)
            .collect();
        let mut events = Vec::new();
        for session_id in expired {
            let state = self
                .sessions
                .remove(&session_id)
                .ok_or(LocationError::UnknownSession(session_id))?;
            let share = SharePayload {
                kind: ShareKind::Stop,
                session_id,
                sequence: state.next_sequence,
                started_at_ms: state.config.started_at_ms,
                ends_at_ms: state.ends_at_ms,
                location: None,
            };
            for recipient in &state.config.recipients {
                events.push(LocationEvent::RecipientUpdate {
                    outgoing: self.prepare_sealed(&state, recipient, &share)?,
                });
            }
            self.peer_positions.retain(|(id, _), _| *id != session_id);
            events.push(LocationEvent::SessionExpired { session_id });
        }
        Ok(events)
    }

    fn prepare_sealed(
        &mut self,
        state: &SessionState,
        recipient: &PublicIdentity,
        share: &SharePayload,
    ) -> Result<OutgoingLocation, LocationError> {
        let recipient_id = recipient_id(recipient);
        let plaintext = share.encode()?;
        let message_id = sealed_message_id(
            &self.owner,
            recipient_id,
            share.session_id,
            share.sequence,
            state.config.started_at_ms,
            state.config.duration_ms,
        );
        let sealed = SealedMessage::seal(
            &self.owner,
            recipient,
            message_id.as_bytes(),
            &plaintext,
            &mut self.rng,
        )?;
        let mut header = location_header(
            PacketClass::Direct,
            Some(recipient_id),
            *self.owner.node_id().as_bytes(),
            share,
            sealed.encode().len(),
        );
        header.msg_id = message_id;
        Ok(OutgoingLocation {
            session_id: share.session_id,
            recipient: Some(recipient_id),
            sequence: share.sequence,
            kind: share.kind,
            header,
            sealed: Some(sealed),
            location: share.location,
        })
    }

    fn prepare_local_publish(
        &mut self,
        share: &SharePayload,
    ) -> Result<OutgoingLocation, LocationError> {
        let location = share
            .location
            .ok_or(LocationError::Payload(PayloadError::InvalidFlags(
                SHARE_KIND_LOCATION,
            )))?;
        let plaintext = AppPayload::Location(location).encode()?;
        let mut header = location_header(
            PacketClass::Telemetry,
            None,
            *self.owner.node_id().as_bytes(),
            share,
            plaintext.len(),
        );
        header.msg_id = derive_msg_id(&header, &plaintext);
        Ok(OutgoingLocation {
            session_id: share.session_id,
            recipient: None,
            sequence: share.sequence,
            kind: share.kind,
            header,
            sealed: None,
            location: Some(location),
        })
    }

    fn trim_peer_positions(&mut self) {
        if self.peer_positions.len() <= MAX_PEER_POSITIONS {
            return;
        }
        let oldest = self
            .peer_positions
            .iter()
            .min_by_key(|((session_id, sender), location)| {
                (location.timestamp_s(), *sender, *session_id)
            })
            .map(|(key, _)| *key);
        if let Some(key) = oldest {
            self.peer_positions.remove(&key);
        }
    }
}

fn validate_config(config: &SessionConfig) -> Result<(), LocationError> {
    if config.recipients.is_empty() || config.recipients.len() > MAX_RECIPIENTS {
        return Err(LocationError::InvalidConfig("recipients"));
    }
    if config.duration_ms == 0 || config.duration_ms > MAX_DURATION_MS {
        return Err(LocationError::InvalidConfig("duration_ms"));
    }
    if config.interval_ms == 0 || config.interval_ms > MAX_INTERVAL_MS {
        return Err(LocationError::InvalidConfig("interval_ms"));
    }
    if config.interval_ms > config.duration_ms {
        return Err(LocationError::InvalidConfig(
            "interval_ms exceeds duration_ms",
        ));
    }
    let _ = make_update(config.precision, &config.initial)?;
    Ok(())
}

fn make_update(
    precision: LocationPrecision,
    sample: &LocationSample,
) -> Result<LocationUpdate, LocationError> {
    Ok(LocationUpdate::new(
        precision,
        sample.timestamp_s,
        sample.latitude_e6,
        sample.longitude_e6,
        sample.accuracy_m,
        sample.heading_tenths,
    )?)
}

fn recipient_id(public: &PublicIdentity) -> RecipientId {
    *compute_node_id(public.signing_pk(), public.agreement_pk()).as_bytes()
}

fn location_header(
    class: PacketClass,
    dst: Option<RecipientId>,
    origin: RecipientId,
    share: &SharePayload,
    payload_len: usize,
) -> Header {
    let created_ms = share.location.map_or(share.ends_at_ms, |location| {
        u64::from(location.timestamp_s()).saturating_mul(1000)
    });
    Header {
        class,
        dst,
        msg_id: [0u8; 16].into(),
        origin,
        created_ms,
        lifetime_s: share.ends_at_ms.saturating_sub(share.started_at_ms) / 1000,
        payload_len: payload_len as u64,
        ttl: DEFAULT_TTL,
        hops: 0,
    }
}

fn sealed_message_id(
    owner: &Identity,
    recipient: RecipientId,
    session_id: SessionId,
    sequence: u64,
    started_at_ms: u64,
    duration_ms: u64,
) -> MessageId {
    let mut bind = Vec::with_capacity(16 + 8 + 16);
    bind.extend_from_slice(session_id.as_bytes());
    bind.extend_from_slice(&sequence.to_be_bytes());
    bind.extend_from_slice(&started_at_ms.to_be_bytes());
    let header = Header {
        class: PacketClass::Direct,
        dst: Some(recipient),
        msg_id: [0u8; 16].into(),
        origin: *owner.node_id().as_bytes(),
        created_ms: started_at_ms,
        lifetime_s: duration_ms / 1000,
        payload_len: 0,
        ttl: DEFAULT_TTL,
        hops: 0,
    };
    let _ = &bind;
    derive_msg_id(&header, &bind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::{APPROX_100M_GRID_E6, OFF_ENCODED_LEN};
    use rand08::SeedableRng;
    use rand08::rngs::StdRng;

    const ALICE_SEED: [u8; 32] = [1; 32];
    const BOB_SEED: [u8; 32] = [2; 32];
    const CAROL_SEED: [u8; 32] = [3; 32];
    const NOW: u64 = 1_700_000_000_000;
    const LAT: i32 = 28_613_942;
    const LON: i32 = 77_209_831;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(42)
    }

    fn sample() -> LocationSample {
        LocationSample {
            timestamp_s: 1_700_000_000,
            latitude_e6: LAT,
            longitude_e6: LON,
            accuracy_m: 12,
            heading_tenths: Some(900),
        }
    }

    fn config(recipient: &Identity, precision: LocationPrecision) -> SessionConfig {
        SessionConfig {
            recipients: vec![recipient.public().clone()],
            started_at_ms: NOW,
            duration_ms: 60_000,
            interval_ms: 10_000,
            precision,
            initial: sample(),
        }
    }

    fn precision_bits(payload: &[u8]) -> LocationPrecision {
        let AppPayload::Location(location) = AppPayload::decode(payload).expect("location payload")
        else {
            panic!("expected location payload");
        };
        location.precision()
    }

    fn outgoing_payload(outgoing: &OutgoingLocation, recipient: &Identity) -> Vec<u8> {
        match outgoing.sealed() {
            Some(sealed) => {
                let opened = sealed
                    .open(recipient, outgoing.header().msg_id.as_bytes())
                    .expect("seal opens to recipient");
                let share = SharePayload::decode(opened.plaintext()).expect("share payload");
                AppPayload::Location(share.location.expect("location in update"))
                    .encode()
                    .expect("encoded location")
            }
            None => AppPayload::Location(outgoing.location().expect("published location"))
                .encode()
                .expect("encoded location"),
        }
    }

    fn start_service(
        precision: LocationPrecision,
    ) -> (
        Identity,
        Identity,
        LocationService<StdRng>,
        SessionId,
        Vec<LocationEvent>,
    ) {
        let owner = Identity::from_seed(ALICE_SEED);
        let recipient = Identity::from_seed(BOB_SEED);
        let mut service = LocationService::new(Identity::from_seed(ALICE_SEED), rng());
        let (session_id, events) = service
            .start_session(config(&recipient, precision))
            .expect("session starts");
        (owner, recipient, service, session_id, events)
    }

    #[test]
    fn precise_coordinates_are_only_encoded_for_precise_sessions() {
        for precision in [
            LocationPrecision::Off,
            LocationPrecision::Approx100m,
            LocationPrecision::Precise,
        ] {
            let (_, recipient, mut service, session_id, start_events) = start_service(precision);
            let mut outgoing = Vec::new();
            for event in start_events {
                if let LocationEvent::RecipientUpdate { outgoing: item } = event {
                    outgoing.push(outgoing_payload(&item, &recipient));
                }
            }
            let publish = service
                .next_publish(NOW + 10_000)
                .expect("publish plan")
                .expect("publish due at interval");
            assert_eq!(publish.session_id(), session_id);
            outgoing.push(outgoing_payload(&publish, &recipient));

            let precise_bytes = AppPayload::Location(
                LocationUpdate::new(
                    LocationPrecision::Precise,
                    sample().timestamp_s,
                    LAT,
                    LON,
                    sample().accuracy_m,
                    sample().heading_tenths,
                )
                .expect("precise reference"),
            )
            .encode()
            .expect("precise reference bytes");

            for payload in &outgoing {
                assert_eq!(precision_bits(payload), precision);
                match precision {
                    LocationPrecision::Off => {
                        assert_eq!(payload.len(), OFF_ENCODED_LEN);
                        assert!(
                            !payload
                                .windows(4)
                                .any(|window| window == &precise_bytes[6..10])
                        );
                    }
                    LocationPrecision::Approx100m => {
                        let expected = LocationUpdate::new(
                            LocationPrecision::Approx100m,
                            sample().timestamp_s,
                            LAT,
                            LON,
                            sample().accuracy_m,
                            sample().heading_tenths,
                        )
                        .expect("approximate reference");
                        let AppPayload::Location(location) =
                            AppPayload::decode(payload).expect("decode")
                        else {
                            panic!("expected location");
                        };
                        assert_eq!(location, expected);
                        assert_eq!(
                            location
                                .latitude_e6()
                                .expect("latitude")
                                .rem_euclid(APPROX_100M_GRID_E6),
                            0
                        );
                        assert_eq!(
                            location
                                .longitude_e6()
                                .expect("longitude")
                                .rem_euclid(APPROX_100M_GRID_E6),
                            0
                        );
                        assert_ne!(payload, &precise_bytes);
                    }
                    LocationPrecision::Precise => {
                        assert_eq!(payload, &precise_bytes);
                    }
                }
            }
        }
    }

    #[test]
    fn session_publishes_once_per_interval_and_stops_immediately() {
        let (_, recipient, mut service, session_id, _) =
            start_service(LocationPrecision::Approx100m);
        let first = service
            .next_publish(NOW + 10_000)
            .expect("first plan")
            .expect("first plan exists");
        assert_eq!(first.sequence(), 1);
        service.mark_published(&first).expect("sequence advances");
        assert!(
            service
                .next_publish(NOW + 10_001)
                .expect("no duplicate")
                .is_none()
        );

        let second = service
            .next_publish(NOW + 20_000)
            .expect("second plan")
            .expect("second plan exists");
        assert_eq!(second.sequence(), 2);
        service.mark_published(&second).expect("sequence advances");

        let events = service
            .stop_session(session_id, NOW + 20_000)
            .expect("stop succeeds");
        assert!(matches!(
            events.first(),
            Some(LocationEvent::SessionStopped { .. })
        ));
        assert!(events.iter().any(|event| matches!(
            event,
            LocationEvent::RecipientUpdate { outgoing } if outgoing.kind() == ShareKind::Stop
        )));
        assert!(!service.is_active(session_id, NOW + 20_000));
        assert!(
            service
                .next_publish(NOW + 30_000)
                .expect("no publish")
                .is_none()
        );
        let _ = recipient;
    }

    #[test]
    fn session_expires_and_notifies_recipients() {
        let (_, recipient, mut service, session_id, _) = start_service(LocationPrecision::Off);
        let events = service.reap_expired(NOW + 60_001).expect("expiry reaped");
        assert!(events.iter().any(|event| matches!(
            event,
            LocationEvent::SessionExpired { session_id: id } if *id == session_id
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            LocationEvent::RecipientUpdate { outgoing } if outgoing.kind() == ShareKind::Stop
        )));
        assert!(
            service
                .next_publish(NOW + 70_000)
                .expect("no publish")
                .is_none()
        );
        let _ = recipient;
    }

    #[test]
    fn receive_authenticates_aggregates_and_rejects_replay() {
        let (owner, recipient, _, session_id, start_events) =
            start_service(LocationPrecision::Precise);
        let outgoing = start_events
            .iter()
            .find_map(|event| match event {
                LocationEvent::RecipientUpdate { outgoing } => Some(outgoing.clone()),
                _ => None,
            })
            .expect("initial sealed update");
        let mut receiver = LocationService::new(Identity::from_seed(BOB_SEED), rng());
        let update = ReceivedLocationUpdate::new(
            session_id,
            *owner.node_id().as_bytes(),
            *recipient.node_id().as_bytes(),
            outgoing.sequence(),
            outgoing.message_id(),
            outgoing.sealed().expect("sealed").clone(),
        );
        let events = receiver
            .receive(NOW + 1, &recipient, update.clone())
            .expect("receive update");
        assert!(events.iter().any(|event| matches!(
            event,
            LocationEvent::PeerLocated { location, .. }
                if location.latitude_e6() == Some(LAT)
        )));
        let positions: Vec<_> = receiver.peer_positions(session_id).collect();
        assert_eq!(positions.len(), 1);

        let replay = receiver
            .receive(NOW + 2, &recipient, update)
            .expect("replay handled");
        assert!(replay.iter().any(|event| matches!(
            event,
            LocationEvent::UpdateIgnored {
                reason: IgnoreReason::Replayed,
                ..
            }
        )));
        assert_eq!(receiver.peer_positions(session_id).count(), 1);
    }

    #[test]
    fn receive_rejects_wrong_sender_and_tampered_message_id() {
        let (owner, recipient, mut service, _, start_events) =
            start_service(LocationPrecision::Precise);
        let outgoing = start_events
            .iter()
            .find_map(|event| match event {
                LocationEvent::RecipientUpdate { outgoing } => Some(outgoing.clone()),
                _ => None,
            })
            .expect("initial sealed update");
        let stranger = Identity::from_seed(CAROL_SEED);
        let mut receiver = LocationService::new(Identity::from_seed(BOB_SEED), rng());

        let stranger_update = ReceivedLocationUpdate::new(
            outgoing.session_id(),
            *stranger.node_id().as_bytes(),
            *recipient.node_id().as_bytes(),
            outgoing.sequence(),
            outgoing.message_id(),
            outgoing.sealed().expect("sealed").clone(),
        );
        let events = service
            .receive(NOW + 1, &recipient, stranger_update)
            .expect("wrong sender handled");
        assert!(events.iter().any(|event| matches!(
            event,
            LocationEvent::UpdateIgnored {
                reason: IgnoreReason::InvalidEnvelope,
                ..
            }
        )));

        let tampered = ReceivedLocationUpdate::new(
            outgoing.session_id(),
            *owner.node_id().as_bytes(),
            *recipient.node_id().as_bytes(),
            outgoing.sequence(),
            [0u8; 16].into(),
            outgoing.sealed().expect("sealed").clone(),
        );
        let events = receiver
            .receive(NOW + 2, &recipient, tampered)
            .expect("tampered id handled");
        assert!(events.iter().any(|event| matches!(
            event,
            LocationEvent::UpdateIgnored {
                reason: IgnoreReason::InvalidEnvelope,
                ..
            }
        )));
    }

    #[test]
    fn invalid_config_is_rejected_with_a_field_name() {
        let recipient = Identity::from_seed(BOB_SEED);
        let mut config = config(&recipient, LocationPrecision::Precise);
        config.interval_ms = 0;
        assert_eq!(
            LocationService::new(Identity::from_seed(ALICE_SEED), rng()).start_session(config),
            Err(LocationError::InvalidConfig("interval_ms"))
        );
    }

    #[test]
    fn share_payload_round_trips_and_rejects_truncation() {
        let share = SharePayload {
            kind: ShareKind::Location,
            session_id: SessionId::new([7; 16]),
            sequence: 12,
            started_at_ms: NOW,
            ends_at_ms: NOW + 60_000,
            location: Some(sample_location()),
        };
        let bytes = share.encode().expect("encode share");
        assert_eq!(SharePayload::decode(&bytes).expect("decode share"), share);
        assert!(SharePayload::decode(&bytes[..SHARE_FIXED_LEN - 1]).is_err());
    }

    fn sample_location() -> LocationUpdate {
        LocationUpdate::new(
            LocationPrecision::Precise,
            sample().timestamp_s,
            LAT,
            LON,
            sample().accuracy_m,
            sample().heading_tenths,
        )
        .expect("sample location")
    }
}
