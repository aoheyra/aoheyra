//! Direct messaging service: compose, seal, submit plans, inbox and status.
//!
//! The service is sans-IO. It owns opaque plaintext bodies and the state
//! machine, but never reads a clock, draws its own randomness, opens a socket
//! or interprets the application schema. A host takes prepared
//! [`Submission`]s, hands them to the mesh runtime and reports the resulting
//! carried, receipt, failure or delivery events back here.
//!
//! APP-07's typed payload schema is not merged yet, so bodies stay opaque
//! bytes. The sealed envelope is built directly on aoIdents and aoMesh's
//! header-bound message id; no second encryption or routing policy is
//! introduced here.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use ao_idents::{IdentError, Identity, PublicIdentity, SealedMessage, compute_node_id};
use ao_mesh::{Header, MessageId, PacketClass, derive_msg_id};
use rand_core06::{CryptoRng, RngCore};

/// Default Direct hop limit for an outgoing message.
pub const DEFAULT_TTL: u8 = 8;
/// Default lifetime of an outgoing message, in seconds.
pub const DEFAULT_LIFETIME_S: u64 = 3_600;
/// Default cap on one opaque body, in bytes.
pub const DEFAULT_MAX_BODY_BYTES: usize = 8 * 1024;

fn node_id(public: &PublicIdentity) -> [u8; 16] {
    *compute_node_id(public.signing_pk(), public.agreement_pk()).as_bytes()
}

/// Validated bounds for one messaging service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessagingConfig {
    ttl: u8,
    lifetime_s: u64,
    max_body_bytes: usize,
    max_outbound: usize,
    max_inbox: usize,
    max_conversations: usize,
    max_retries: u8,
}

impl Default for MessagingConfig {
    fn default() -> Self {
        Self {
            ttl: DEFAULT_TTL,
            lifetime_s: DEFAULT_LIFETIME_S,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            max_outbound: 512,
            max_inbox: 2048,
            max_conversations: 256,
            max_retries: 3,
        }
    }
}

impl MessagingConfig {
    /// Builds a config, rejecting out-of-range values rather than clamping.
    ///
    /// # Errors
    ///
    /// A [`ConfigError`] naming the rejected field and value.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ttl: u8,
        lifetime_s: u64,
        max_body_bytes: usize,
        max_outbound: usize,
        max_inbox: usize,
        max_conversations: usize,
        max_retries: u8,
    ) -> Result<Self, ConfigError> {
        if ttl == 0 {
            return Err(ConfigError::Ttl { value: ttl });
        }
        if lifetime_s == 0 {
            return Err(ConfigError::Lifetime { value: lifetime_s });
        }
        if max_body_bytes == 0 || max_body_bytes > 32 * 1024 {
            return Err(ConfigError::BodyBytes {
                value: max_body_bytes,
            });
        }
        if max_outbound == 0 || max_outbound > 4096 {
            return Err(ConfigError::Outbound {
                value: max_outbound,
            });
        }
        if max_inbox == 0 || max_inbox > 16_384 {
            return Err(ConfigError::Inbox { value: max_inbox });
        }
        if max_conversations == 0 || max_conversations > 1024 {
            return Err(ConfigError::Conversations {
                value: max_conversations,
            });
        }
        if max_retries > 8 {
            return Err(ConfigError::Retries { value: max_retries });
        }
        Ok(Self {
            ttl,
            lifetime_s,
            max_body_bytes,
            max_outbound,
            max_inbox,
            max_conversations,
            max_retries,
        })
    }

    /// Direct hop limit stamped on a submission.
    #[must_use]
    pub const fn ttl(&self) -> u8 {
        self.ttl
    }

    /// Message lifetime in seconds.
    #[must_use]
    pub const fn lifetime_s(&self) -> u64 {
        self.lifetime_s
    }

    /// Largest accepted opaque body.
    #[must_use]
    pub const fn max_body_bytes(&self) -> usize {
        self.max_body_bytes
    }

    /// Maximum live outbound records.
    #[must_use]
    pub const fn max_outbound(&self) -> usize {
        self.max_outbound
    }

    /// Maximum live inbox records.
    #[must_use]
    pub const fn max_inbox(&self) -> usize {
        self.max_inbox
    }

    /// Maximum tracked conversations.
    #[must_use]
    pub const fn max_conversations(&self) -> usize {
        self.max_conversations
    }

    /// Maximum retry attempts per composed message.
    #[must_use]
    pub const fn max_retries(&self) -> u8 {
        self.max_retries
    }
}

/// A rejected messaging setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// Zero hop limit.
    Ttl {
        /// Rejected value.
        value: u8,
    },
    /// Zero lifetime.
    Lifetime {
        /// Rejected value.
        value: u64,
    },
    /// Body cap outside 1..=32768.
    BodyBytes {
        /// Rejected value.
        value: usize,
    },
    /// Outbound cap outside 1..=4096.
    Outbound {
        /// Rejected value.
        value: usize,
    },
    /// Inbox cap outside 1..=16384.
    Inbox {
        /// Rejected value.
        value: usize,
    },
    /// Conversation cap outside 1..=1024.
    Conversations {
        /// Rejected value.
        value: usize,
    },
    /// Retry cap above eight.
    Retries {
        /// Rejected value.
        value: u8,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ttl { value } => write!(f, "TTL {value} must be positive"),
            Self::Lifetime { value } => write!(f, "lifetime {value} must be positive"),
            Self::BodyBytes { value } => write!(f, "body cap {value} outside 1..=32768"),
            Self::Outbound { value } => write!(f, "outbound cap {value} outside 1..=4096"),
            Self::Inbox { value } => write!(f, "inbox cap {value} outside 1..=16384"),
            Self::Conversations { value } => {
                write!(f, "conversation cap {value} outside 1..=1024")
            }
            Self::Retries { value } => write!(f, "retry cap {value} above eight"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Why an outgoing submission failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    /// The mesh refused the prepared submission.
    SubmitRejected,
    /// The retry budget was spent.
    RetryExhausted,
}

/// State of one outgoing message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageStatus {
    /// Sealed and waiting for the host to submit it.
    Queued,
    /// Accepted by the mesh and on air or in the carried store.
    Carried {
        /// Local time when acceptance was reported.
        since_ms: u64,
    },
    /// A signed receipt reached the sender.
    Delivered {
        /// Local time when the receipt was accepted.
        at_ms: u64,
    },
    /// Submission or retries failed.
    Failed {
        /// Local time of the failure.
        at_ms: u64,
        /// Typed cause.
        reason: FailureReason,
    },
    /// The lifetime elapsed without a receipt.
    Expired {
        /// Local time of expiry.
        at_ms: u64,
    },
}

impl MessageStatus {
    /// Whether no further transition can follow without a retry.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Delivered { .. } | Self::Failed { .. } | Self::Expired { .. }
        )
    }
}

/// Which side wrote a conversation entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationDirection {
    /// The local user composed the message.
    Outbound,
    /// The peer sent the message.
    Inbound,
}

/// One bounded entry in a conversation transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversationEntry {
    /// Message id, shared with the outbound or inbox record.
    pub id: MessageId,
    /// Who wrote it.
    pub direction: ConversationDirection,
    /// Local time attached to the entry.
    pub at_ms: u64,
    /// Whether an inbound entry is still unread.
    pub unread: bool,
}

/// Bounded transcript and unread count for one peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    peer: [u8; 16],
    entries: VecDeque<ConversationEntry>,
    unread: usize,
}

impl Conversation {
    /// Peer this conversation belongs to.
    #[must_use]
    pub const fn peer(&self) -> &[u8; 16] {
        &self.peer
    }

    /// Newest-first view of the bounded transcript.
    pub fn entries(&self) -> impl Iterator<Item = &ConversationEntry> {
        self.entries.iter().rev()
    }

    /// Number of unread inbound entries.
    #[must_use]
    pub const fn unread(&self) -> usize {
        self.unread
    }
}

/// A received, authenticated opaque payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxMessage {
    /// Signed mesh message id.
    pub id: MessageId,
    /// Authenticated sender from the packet header.
    pub peer: [u8; 16],
    /// Opened plaintext; the app schema decodes it later.
    pub payload: Vec<u8>,
    /// Local receipt time.
    pub received_at_ms: u64,
    /// Local expiry time derived from the header lifetime.
    pub expires_at_ms: u64,
}

/// One outgoing message and its full local state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMessage {
    /// Signed mesh message id, stable across retries only until retry.
    pub id: MessageId,
    /// Destination peer.
    pub peer: [u8; 16],
    /// Opaque plaintext body.
    pub body: Vec<u8>,
    /// Local creation time.
    pub created_ms: u64,
    /// Local expiry time.
    pub expires_ms: u64,
    /// Current state.
    pub status: MessageStatus,
    /// Number of seal attempts made, including the first.
    pub attempts: u8,
}

/// A prepared submission for the mesh runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    /// Message id the runtime's signed header must carry.
    pub id: MessageId,
    /// Header-bound id used as the seal's associated data.
    pub binding: MessageId,
    /// Exactly the header to pass to aoMesh.
    pub header: Header,
    /// Sealed opaque bytes.
    pub payload: Vec<u8>,
}

/// Host-visible messaging change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessagingEvent {
    /// An outgoing message changed state.
    StatusChanged {
        /// Message id.
        id: MessageId,
        /// Destination peer.
        peer: [u8; 16],
        /// New state.
        status: MessageStatus,
    },
    /// An authenticated payload entered the inbox.
    Received {
        /// Message id.
        id: MessageId,
        /// Sender peer.
        peer: [u8; 16],
        /// Opaque payload.
        payload: Vec<u8>,
    },
    /// Unread count or transcript changed for a conversation.
    ConversationChanged {
        /// Peer whose conversation changed.
        peer: [u8; 16],
        /// Current unread count.
        unread: usize,
    },
    /// A queued or failed message was cancelled.
    Canceled {
        /// Message id removed.
        id: MessageId,
        /// Peer it was addressed to.
        peer: [u8; 16],
    },
}

/// A typed messaging failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessagingError {
    /// The supplied config was invalid.
    Config(ConfigError),
    /// No message with this id is live.
    NotFound(MessageId),
    /// The body exceeded the configured cap.
    BodyTooLarge {
        /// Offered body length.
        value: usize,
        /// Configured cap.
        max: usize,
    },
    /// The state machine does not allow this transition.
    WrongState {
        /// Message id.
        id: MessageId,
        /// State found.
        actual: MessageStatus,
    },
    /// A delivered message cannot be cancelled.
    CannotCancelDelivered(MessageId),
    /// No retry budget remains.
    RetryExhausted(MessageId),
    /// The bounded outbound or conversation table is full.
    Capacity,
    /// A delivery candidate was addressed elsewhere or used another class.
    Misdirected,
    /// Sealing failed in aoIdents.
    Identity(IdentError),
}

impl fmt::Display for MessagingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(f, "invalid messaging config: {error}"),
            Self::NotFound(id) => write!(f, "unknown message {id:?}"),
            Self::BodyTooLarge { value, max } => {
                write!(f, "body length {value} exceeds cap {max}")
            }
            Self::WrongState { id, actual } => {
                write!(f, "message {id:?} is in state {actual:?}")
            }
            Self::CannotCancelDelivered(id) => {
                write!(f, "delivered message {id:?} cannot be cancelled")
            }
            Self::RetryExhausted(id) => write!(f, "message {id:?} has no retries left"),
            Self::Capacity => f.write_str("messaging table is at capacity"),
            Self::Misdirected => f.write_str("delivery was not a Direct packet for this node"),
            Self::Identity(error) => write!(f, "seal refused the message: {error}"),
        }
    }
}

impl std::error::Error for MessagingError {}

impl From<ConfigError> for MessagingError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<IdentError> for MessagingError {
    fn from(error: IdentError) -> Self {
        Self::Identity(error)
    }
}

struct OutboundRecord {
    message: OutboundMessage,
    recipient: PublicIdentity,
    header: Header,
    binding: MessageId,
    wire: Vec<u8>,
}

/// The sans-IO owner of one node's messaging state.
pub struct MessagingService {
    local_id: [u8; 16],
    config: MessagingConfig,
    outbound: BTreeMap<MessageId, OutboundRecord>,
    outbox: VecDeque<MessageId>,
    inbox: BTreeMap<MessageId, InboxMessage>,
    seen_inbox: BTreeMap<MessageId, u64>,
    conversations: BTreeMap<[u8; 16], Conversation>,
}

impl MessagingService {
    /// Creates an empty service for `local_id`.
    #[must_use]
    pub fn new(local_id: [u8; 16], config: MessagingConfig) -> Self {
        Self {
            local_id,
            config,
            outbound: BTreeMap::new(),
            outbox: VecDeque::new(),
            inbox: BTreeMap::new(),
            seen_inbox: BTreeMap::new(),
            conversations: BTreeMap::new(),
        }
    }

    /// Validated settings in force.
    #[must_use]
    pub const fn config(&self) -> &MessagingConfig {
        &self.config
    }

    /// Live outbound record, if any.
    #[must_use]
    pub fn message(&self, id: &MessageId) -> Option<&OutboundMessage> {
        self.outbound.get(id).map(|record| &record.message)
    }

    /// Live received messages for a peer, oldest first.
    #[must_use]
    pub fn inbox(&self, peer: &[u8; 16]) -> Vec<InboxMessage> {
        let mut items = self
            .inbox
            .values()
            .filter(|item| item.peer == *peer)
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by_key(|item| (item.received_at_ms, item.id));
        items
    }

    /// Bounded conversation summaries, ordered by peer id.
    #[must_use]
    pub fn conversations(&self) -> Vec<Conversation> {
        self.conversations.values().cloned().collect()
    }

    /// One conversation, if the peer has one.
    #[must_use]
    pub fn conversation(&self, peer: &[u8; 16]) -> Option<&Conversation> {
        self.conversations.get(peer)
    }

    /// Number of prepared submissions waiting for the host.
    #[must_use]
    pub fn pending_submissions(&self) -> usize {
        self.outbox.len()
    }

    /// Composes and seals one opaque body, leaving it queued.
    ///
    /// # Errors
    ///
    /// [`MessagingError::BodyTooLarge`] if the body exceeds the configured
    /// cap, [`MessagingError::Capacity`] if the bounded tables are full, and
    /// [`MessagingError::Identity`] if aoIdents refuses the seal.
    pub fn compose<R: RngCore + CryptoRng>(
        &mut self,
        identity: &Identity,
        recipient: &PublicIdentity,
        body: &[u8],
        now_ms: u64,
        rng: &mut R,
    ) -> Result<(MessageId, Vec<MessagingEvent>), MessagingError> {
        if body.len() > self.config.max_body_bytes() {
            return Err(MessagingError::BodyTooLarge {
                value: body.len(),
                max: self.config.max_body_bytes(),
            });
        }
        self.reserve_outbound(now_ms)?;
        let (header, binding, wire) =
            self.prepare(identity, recipient, body, now_ms, rng, self.config.ttl())?;
        let id = header.msg_id;
        let record = OutboundRecord {
            message: OutboundMessage {
                id,
                peer: node_id(recipient),
                body: body.to_vec(),
                created_ms: now_ms,
                expires_ms: now_ms.saturating_add(self.config.lifetime_s().saturating_mul(1000)),
                status: MessageStatus::Queued,
                attempts: 1,
            },
            recipient: recipient.clone(),
            header,
            binding,
            wire,
        };
        self.outbound.insert(id, record);
        self.outbox.push_back(id);
        let mut events = vec![MessagingEvent::StatusChanged {
            id,
            peer: node_id(recipient),
            status: MessageStatus::Queued,
        }];
        events.extend(self.add_entry(
            node_id(recipient),
            ConversationEntry {
                id,
                direction: ConversationDirection::Outbound,
                at_ms: now_ms,
                unread: false,
            },
        )?);
        Ok((id, events))
    }

    /// Removes and returns prepared submissions in compose order.
    #[must_use]
    pub fn take_submissions(&mut self) -> Vec<Submission> {
        let ids = self.outbox.drain(..).collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| {
                let record = self.outbound.get(&id)?;
                Some(Submission {
                    id,
                    binding: record.binding,
                    header: record.header,
                    payload: record.wire.clone(),
                })
            })
            .collect()
    }

    /// Records that the mesh accepted a prepared submission.
    ///
    /// # Errors
    ///
    /// [`MessagingError::NotFound`] for an unknown id and
    /// [`MessagingError::WrongState`] unless the message is queued.
    pub fn mark_carried(
        &mut self,
        id: &MessageId,
        now_ms: u64,
    ) -> Result<Vec<MessagingEvent>, MessagingError> {
        self.set_status(
            id,
            now_ms,
            MessageStatus::Carried { since_ms: now_ms },
            |status| matches!(status, MessageStatus::Queued),
        )
    }

    /// Records that the mesh refused a submission.
    ///
    /// # Errors
    ///
    /// [`MessagingError::NotFound`] for an unknown id and
    /// [`MessagingError::WrongState`] for a terminal or delivered message.
    pub fn mark_failed(
        &mut self,
        id: &MessageId,
        now_ms: u64,
        reason: FailureReason,
    ) -> Result<Vec<MessagingEvent>, MessagingError> {
        self.set_status(
            id,
            now_ms,
            MessageStatus::Failed {
                at_ms: now_ms,
                reason,
            },
            |status| {
                matches!(
                    status,
                    MessageStatus::Queued | MessageStatus::Carried { .. }
                )
            },
        )
    }

    /// Accepts a signed delivery receipt from the destination.
    ///
    /// # Errors
    ///
    /// [`MessagingError::NotFound`] for an unknown id and
    /// [`MessagingError::WrongState`] unless the message is queued or carried.
    pub fn on_receipt(
        &mut self,
        id: &MessageId,
        now_ms: u64,
    ) -> Result<Vec<MessagingEvent>, MessagingError> {
        self.set_status(
            id,
            now_ms,
            MessageStatus::Delivered { at_ms: now_ms },
            |status| {
                matches!(
                    status,
                    MessageStatus::Queued | MessageStatus::Carried { .. }
                )
            },
        )
    }

    /// Accepts one opened delivery from the mesh and suppresses duplicates.
    ///
    /// # Errors
    ///
    /// [`MessagingError::Misdirected`] for a non-Direct packet or one addressed
    /// to another node, [`MessagingError::Capacity`] if a new conversation or
    /// bounded table is full.
    pub fn on_delivery(
        &mut self,
        header: &Header,
        payload: &[u8],
        now_ms: u64,
    ) -> Result<Vec<MessagingEvent>, MessagingError> {
        if header.class != PacketClass::Direct || header.dst != Some(self.local_id) {
            return Err(MessagingError::Misdirected);
        }
        let id = header.msg_id;
        if self.inbox.contains_key(&id) || self.seen_inbox.contains_key(&id) {
            return Ok(Vec::new());
        }
        self.reserve_inbox(now_ms)?;
        let peer = header.origin;
        let expires_at_ms = now_ms.saturating_add(header.lifetime_s.saturating_mul(1000));
        let item = InboxMessage {
            id,
            peer,
            payload: payload.to_vec(),
            received_at_ms: now_ms,
            expires_at_ms,
        };
        self.inbox.insert(id, item);
        self.note_seen(id, now_ms);
        let mut events = vec![MessagingEvent::Received {
            id,
            peer,
            payload: payload.to_vec(),
        }];
        events.extend(self.add_entry(
            peer,
            ConversationEntry {
                id,
                direction: ConversationDirection::Inbound,
                at_ms: now_ms,
                unread: true,
            },
        )?);
        Ok(events)
    }

    /// Advances expiry for outbound and inbox records.
    #[must_use]
    pub fn tick(&mut self, now_ms: u64) -> Vec<MessagingEvent> {
        let mut events = Vec::new();
        let expired = self
            .outbound
            .values()
            .filter(|record| {
                !record.message.status.is_terminal() && now_ms >= record.message.expires_ms
            })
            .map(|record| record.message.id)
            .collect::<Vec<_>>();
        for id in expired {
            if let Ok(changes) = self.set_status(
                &id,
                now_ms,
                MessageStatus::Expired { at_ms: now_ms },
                |_| true,
            ) {
                events.extend(changes);
            }
        }
        let stale = self
            .inbox
            .values()
            .filter(|item| now_ms >= item.expires_at_ms)
            .map(|item| (item.id, item.peer))
            .collect::<Vec<_>>();
        for (id, peer) in stale {
            self.inbox.remove(&id);
            self.remove_entry(peer, id);
        }
        events
    }

    /// Clears the unread count for a peer.
    #[must_use]
    pub fn mark_read(&mut self, peer: &[u8; 16]) -> Vec<MessagingEvent> {
        let Some(conversation) = self.conversations.get_mut(peer) else {
            return Vec::new();
        };
        if conversation.unread == 0 {
            return Vec::new();
        }
        conversation.unread = 0;
        for entry in &mut conversation.entries {
            entry.unread = false;
        }
        vec![MessagingEvent::ConversationChanged {
            peer: *peer,
            unread: 0,
        }]
    }

    /// Re-seals a failed or expired message with a fresh id and created time.
    ///
    /// # Errors
    ///
    /// [`MessagingError::NotFound`] for an unknown id,
    /// [`MessagingError::WrongState`] unless the message is failed or expired,
    /// [`MessagingError::RetryExhausted`] when the budget is spent, and
    /// [`MessagingError::Identity`] if aoIdents refuses the new seal.
    pub fn retry<R: RngCore + CryptoRng>(
        &mut self,
        id: &MessageId,
        identity: &Identity,
        now_ms: u64,
        rng: &mut R,
    ) -> Result<(MessageId, Vec<MessagingEvent>), MessagingError> {
        let (body, recipient, attempts) = {
            let record = self.outbound.get(id).ok_or(MessagingError::NotFound(*id))?;
            if !matches!(
                record.message.status,
                MessageStatus::Failed { .. } | MessageStatus::Expired { .. }
            ) {
                return Err(MessagingError::WrongState {
                    id: *id,
                    actual: record.message.status,
                });
            }
            if record.message.attempts >= self.config.max_retries() {
                return Err(MessagingError::RetryExhausted(*id));
            }
            (
                record.message.body.clone(),
                record.recipient.clone(),
                record.message.attempts,
            )
        };
        self.reserve_outbound(now_ms)?;
        let (header, binding, wire) =
            self.prepare(identity, &recipient, &body, now_ms, rng, self.config.ttl())?;
        let new_id = header.msg_id;
        let peer_id = node_id(&recipient);
        self.outbound.insert(
            new_id,
            OutboundRecord {
                message: OutboundMessage {
                    id: new_id,
                    peer: peer_id,
                    body,
                    created_ms: now_ms,
                    expires_ms: now_ms
                        .saturating_add(self.config.lifetime_s().saturating_mul(1000)),
                    status: MessageStatus::Queued,
                    attempts: attempts.saturating_add(1),
                },
                recipient,
                header,
                binding,
                wire,
            },
        );
        self.outbox.push_back(new_id);
        let mut events = vec![MessagingEvent::StatusChanged {
            id: new_id,
            peer: peer_id,
            status: MessageStatus::Queued,
        }];
        events.extend(self.add_entry(
            peer_id,
            ConversationEntry {
                id: new_id,
                direction: ConversationDirection::Outbound,
                at_ms: now_ms,
                unread: false,
            },
        )?);
        Ok((new_id, events))
    }

    /// Removes a non-delivered message and its queued submission.
    ///
    /// # Errors
    ///
    /// [`MessagingError::NotFound`] for an unknown id and
    /// [`MessagingError::CannotCancelDelivered`] for a delivered message.
    pub fn cancel(&mut self, id: &MessageId) -> Result<Vec<MessagingEvent>, MessagingError> {
        let record = self.outbound.get(id).ok_or(MessagingError::NotFound(*id))?;
        if matches!(record.message.status, MessageStatus::Delivered { .. }) {
            return Err(MessagingError::CannotCancelDelivered(*id));
        }
        let peer = record.message.peer;
        self.outbound.remove(id);
        self.outbox.retain(|queued| queued != id);
        self.remove_entry(peer, *id);
        Ok(vec![MessagingEvent::Canceled { id: *id, peer }])
    }

    fn prepare<R: RngCore + CryptoRng>(
        &self,
        identity: &Identity,
        recipient: &PublicIdentity,
        body: &[u8],
        created_ms: u64,
        rng: &mut R,
        ttl: u8,
    ) -> Result<(Header, MessageId, Vec<u8>), MessagingError> {
        let dst = node_id(recipient);
        let probe = SealedMessage::seal(identity, recipient, &[0; 16], body, rng)?;
        let mut header = Header {
            class: PacketClass::Direct,
            dst: Some(dst),
            msg_id: [0; 16].into(),
            origin: *identity.node_id().as_bytes(),
            created_ms,
            lifetime_s: self.config.lifetime_s(),
            payload_len: probe.encode().len() as u64,
            ttl,
            hops: 0,
        };
        let binding = derive_msg_id(&header, &[]);
        let wire =
            SealedMessage::seal(identity, recipient, binding.as_bytes(), body, rng)?.encode();
        header.msg_id = derive_msg_id(&header, &wire);
        Ok((header, binding, wire))
    }

    fn reserve_outbound(&mut self, now_ms: u64) -> Result<(), MessagingError> {
        if self.outbound.len() < self.config.max_outbound() {
            return Ok(());
        }
        let victim = self
            .outbound
            .values()
            .filter(|record| record.message.status.is_terminal())
            .min_by_key(|record| (record.message.created_ms, record.message.id))
            .map(|record| record.message.id);
        if let Some(id) = victim {
            let peer = self.outbound[&id].message.peer;
            self.outbound.remove(&id);
            self.outbox.retain(|queued| *queued != id);
            self.remove_entry(peer, id);
            return Ok(());
        }
        let _ = now_ms;
        Err(MessagingError::Capacity)
    }

    fn reserve_inbox(&mut self, now_ms: u64) -> Result<(), MessagingError> {
        if self.inbox.len() < self.config.max_inbox() {
            return Ok(());
        }
        let victim = self
            .inbox
            .values()
            .min_by_key(|item| (item.received_at_ms, item.id))
            .map(|item| item.id);
        if let Some(id) = victim {
            self.inbox.remove(&id);
        }
        while self.seen_inbox.len() >= self.config.max_inbox() {
            let victim = self
                .seen_inbox
                .iter()
                .min_by_key(|(id, at)| (**at, **id))
                .map(|(id, _)| *id);
            match victim {
                Some(id) => {
                    self.seen_inbox.remove(&id);
                }
                None => break,
            }
        }
        let _ = now_ms;
        Ok(())
    }

    fn note_seen(&mut self, id: MessageId, now_ms: u64) {
        self.seen_inbox.insert(id, now_ms);
        while self.seen_inbox.len() > self.config.max_inbox() {
            let victim = self
                .seen_inbox
                .iter()
                .min_by_key(|(id, at)| (**at, **id))
                .map(|(id, _)| *id);
            match victim {
                Some(id) => {
                    self.seen_inbox.remove(&id);
                }
                None => break,
            }
        }
    }

    fn add_entry(
        &mut self,
        peer: [u8; 16],
        entry: ConversationEntry,
    ) -> Result<Vec<MessagingEvent>, MessagingError> {
        if !self.conversations.contains_key(&peer)
            && self.conversations.len() >= self.config.max_conversations()
        {
            return Err(MessagingError::Capacity);
        }
        let conversation = self
            .conversations
            .entry(peer)
            .or_insert_with(|| Conversation {
                peer,
                entries: VecDeque::new(),
                unread: 0,
            });
        if conversation.entries.len() >= 64 {
            let removed = conversation.entries.pop_front();
            if removed.as_ref().is_some_and(|entry| entry.unread) {
                conversation.unread = conversation.unread.saturating_sub(1);
            }
        }
        if entry.unread {
            conversation.unread = conversation.unread.saturating_add(1);
        }
        conversation.entries.push_back(entry);
        Ok(vec![MessagingEvent::ConversationChanged {
            peer,
            unread: conversation.unread,
        }])
    }

    fn remove_entry(&mut self, peer: [u8; 16], id: MessageId) {
        if let Some(conversation) = self.conversations.get_mut(&peer) {
            let index = conversation.entries.iter().position(|entry| entry.id == id);
            let removed = index.and_then(|index| conversation.entries.remove(index));
            if removed.as_ref().is_some_and(|entry| entry.unread) {
                conversation.unread = conversation.unread.saturating_sub(1);
            }
            if conversation.entries.is_empty() {
                self.conversations.remove(&peer);
            }
        }
    }

    fn set_status(
        &mut self,
        id: &MessageId,
        now_ms: u64,
        status: MessageStatus,
        allowed: impl Fn(MessageStatus) -> bool,
    ) -> Result<Vec<MessagingEvent>, MessagingError> {
        let record = self
            .outbound
            .get_mut(id)
            .ok_or(MessagingError::NotFound(*id))?;
        if !allowed(record.message.status) {
            return Err(MessagingError::WrongState {
                id: *id,
                actual: record.message.status,
            });
        }
        record.message.status = status;
        let _ = now_ms;
        Ok(vec![MessagingEvent::StatusChanged {
            id: *id,
            peer: record.message.peer,
            status,
        }])
    }
}
