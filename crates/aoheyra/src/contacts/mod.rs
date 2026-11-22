//! Contacts service: QR onboarding, trust, key-change alerts and safety numbers.
//!
//! The service is sans-IO and storage-agnostic. aoIdents owns the contact
//! logic and the `ContactStore` seam; this module owns the app-level sequence:
//! a verified card is stored, a changed key raises an alert, and every send
//! path must consult [`ContactsService::can_send`] before a decision is made.
//! It never reads a clock, draws randomness or opens a database.
//!
//! APP-06 (SQLite `ContactStore` and the secret-vault bridge) is not merged
//! yet, so the service is generic over `ContactStore` and tests use aoIdents'
//! in-memory implementation. No contact logic is duplicated here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ao_idents::{
    Contact, ContactBook, ContactStore, ContactUpdate, IdentError, Identity, IdentityCard, NodeId,
    RotationStatement, Trust, compute_node_id, safety_number as compute_safety_number,
};

/// A pending key-change decision, keyed by the identity that was claimed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingKeyChange {
    /// Identity whose keys were offered in the presentation.
    offered: NodeId,
    /// Injected time of the first alert, in milliseconds.
    observed_at_ms: u64,
}

/// A user-visible contact state change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContactsEvent {
    /// A verified card was stored for the first time.
    ContactAdded {
        /// Identity that was added.
        node_id: NodeId,
        /// What the contact book did with the card.
        update: ContactUpdate,
    },
    /// A known card was presented again with no changes.
    ContactUnchanged {
        /// Identity that was presented.
        node_id: NodeId,
    },
    /// A claimed identity presented different keys. Sending is now blocked.
    KeyChangeAlert {
        /// Identity that was claimed by the presentation.
        node_id: NodeId,
        /// Identity derived from the offered keys.
        offered_node_id: NodeId,
        /// Injected time of the alert, in milliseconds.
        observed_at_ms: u64,
    },
    /// The user resolved a key-change alert.
    KeyChangeResolved {
        /// Identity that was claimed by the presentation.
        node_id: NodeId,
        /// Decision the user made.
        decision: KeyChangeDecision,
        /// New identity when a signed rotation was accepted.
        successor: Option<NodeId>,
        /// Injected time of the decision, in milliseconds.
        at_ms: u64,
    },
    /// The contact was verified after a human comparison.
    Verified {
        /// Identity that was verified.
        node_id: NodeId,
        /// Injected time of the change, in milliseconds.
        at_ms: u64,
    },
    /// The contact was blocked.
    Blocked {
        /// Identity that was blocked.
        node_id: NodeId,
        /// Injected time of the change, in milliseconds.
        at_ms: u64,
    },
    /// The local petname was set or cleared.
    Renamed {
        /// Identity that was renamed.
        node_id: NodeId,
        /// Injected time of the change, in milliseconds.
        at_ms: u64,
    },
}

/// The user's answer to a key-change alert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyChangeDecision {
    /// Follow a signed rotation statement to the successor identity.
    AcceptRotation,
    /// Keep refusing the claimed identity.
    Block,
}

/// An eight-byte safety number suitable for out-of-band comparison.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SafetyNumber([u8; 8]);

impl SafetyNumber {
    /// Raw bytes of the number.
    #[must_use]
    pub const fn bytes(&self) -> &[u8; 8] {
        &self.0
    }

    /// Uppercase hexadecimal text in four groups of four digits.
    #[must_use]
    pub fn text(&self) -> String {
        format!("{self}")
    }
}

impl fmt::Display for SafetyNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02X}{:02X} {:02X}{:02X} {:02X}{:02X} {:02X}{:02X}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5], self.0[6], self.0[7],
        )
    }
}

impl fmt::Debug for SafetyNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SafetyNumber({self})")
    }
}

/// A typed contacts failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContactsError {
    /// The QR string was malformed or its card signature failed.
    Qr(IdentError),
    /// The contact book refused a store operation.
    Book(IdentError),
    /// No contact is stored under this identity.
    UnknownContact(NodeId),
    /// A mutating operation cannot proceed while a key decision is pending.
    PendingKeyChange(NodeId),
    /// No key-change alert is pending for this identity.
    NoPendingKeyChange(NodeId),
    /// A rotation did not chain the claimed identity to the offered one.
    KeyChangeMismatch {
        /// Identity expected by the pending alert.
        expected: NodeId,
        /// Identity named by the rotation statement.
        actual: NodeId,
    },
    /// Sending is blocked until the user resolves a key-change alert.
    SendBlockedPendingKeyDecision(NodeId),
    /// The contact was deliberately blocked.
    ContactBlocked(NodeId),
    /// The bounded pending-alert table is full.
    Capacity,
    /// The supplied identity is not this service's local identity.
    WrongLocalIdentity {
        /// Local identity the service was built for.
        expected: NodeId,
        /// Identity that was supplied.
        actual: NodeId,
    },
}

impl fmt::Display for ContactsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Qr(error) => write!(f, "invalid identity QR: {error}"),
            Self::Book(error) => write!(f, "contact book refused the operation: {error}"),
            Self::UnknownContact(node_id) => write!(f, "unknown contact {node_id:?}"),
            Self::PendingKeyChange(node_id) => {
                write!(f, "contact {node_id:?} has a pending key-change decision")
            }
            Self::NoPendingKeyChange(node_id) => {
                write!(f, "contact {node_id:?} has no pending key-change decision")
            }
            Self::KeyChangeMismatch { expected, actual } => {
                write!(f, "rotation chains {actual:?}, alert expected {expected:?}")
            }
            Self::SendBlockedPendingKeyDecision(node_id) => {
                write!(
                    f,
                    "sending to {node_id:?} is blocked pending a key decision"
                )
            }
            Self::ContactBlocked(node_id) => write!(f, "contact {node_id:?} is blocked"),
            Self::Capacity => f.write_str("pending key-change table is at capacity"),
            Self::WrongLocalIdentity { expected, actual } => {
                write!(f, "identity {actual:?} is not local identity {expected:?}")
            }
        }
    }
}

impl std::error::Error for ContactsError {}

/// App-level contacts service over aoIdents' contact-book seam.
pub struct ContactsService<S: ContactStore> {
    book: ContactBook<S>,
    local_id: NodeId,
    known: BTreeSet<NodeId>,
    pending: BTreeMap<NodeId, PendingKeyChange>,
}

impl<S: ContactStore> ContactsService<S> {
    /// Wraps a contact store for one local identity.
    #[must_use]
    pub fn new(local_id: NodeId, store: S) -> Self {
        Self {
            book: ContactBook::new(store),
            local_id,
            known: BTreeSet::new(),
            pending: BTreeMap::new(),
        }
    }

    /// Adds a contact from a verified `aoheyra:id:` QR string.
    ///
    /// # Errors
    ///
    /// [`ContactsError::Qr`] for a malformed or forged QR and
    /// [`ContactsError::Book`] if the store refuses the card.
    pub fn add_from_qr(
        &mut self,
        qr: &str,
        now_ms: u64,
    ) -> Result<(NodeId, Vec<ContactsEvent>), ContactsError> {
        let card = IdentityCard::from_qr_string(qr).map_err(ContactsError::Qr)?;
        let node_id = compute_node_id(card.signing_pk(), card.agreement_pk());
        let update = self
            .book
            .observe(node_id, &card, now_ms)
            .map_err(ContactsError::Book)?;
        if update == ContactUpdate::FirstUse {
            self.known.insert(node_id);
        }
        let event = match update {
            ContactUpdate::FirstUse => ContactsEvent::ContactAdded { node_id, update },
            ContactUpdate::Unchanged => ContactsEvent::ContactUnchanged { node_id },
        };
        Ok((node_id, vec![event]))
    }

    /// Observes a card presented for a specific claimed identity.
    ///
    /// A presentation path such as presence carries both a claimed node id and
    /// a card. When they disagree, aoIdents reports `KeyChanged`; this service
    /// turns that into an alert, leaves the stored contact untouched and blocks
    /// sending until the user accepts a signed rotation or blocks the contact.
    /// Repeating the same presentation does not raise a second alert.
    ///
    /// # Errors
    ///
    /// [`ContactsError::Book`] for any other book failure and
    /// [`ContactsError::Capacity`] when too many decisions are pending.
    pub fn observe_claimed_card(
        &mut self,
        claimed: NodeId,
        card: &IdentityCard,
        now_ms: u64,
    ) -> Result<Vec<ContactsEvent>, ContactsError> {
        let offered = compute_node_id(card.signing_pk(), card.agreement_pk());
        match self.book.observe(claimed, card, now_ms) {
            Ok(ContactUpdate::FirstUse) => {
                self.known.insert(claimed);
                Ok(vec![ContactsEvent::ContactAdded {
                    node_id: claimed,
                    update: ContactUpdate::FirstUse,
                }])
            }
            Ok(ContactUpdate::Unchanged) => {
                Ok(vec![ContactsEvent::ContactUnchanged { node_id: claimed }])
            }
            Err(IdentError::KeyChanged(node_id)) => {
                if self.pending.contains_key(&node_id) {
                    return Ok(Vec::new());
                }
                if self.pending.len() >= ao_idents::MAX_CONTACTS {
                    return Err(ContactsError::Capacity);
                }
                self.pending.insert(
                    node_id,
                    PendingKeyChange {
                        offered,
                        observed_at_ms: now_ms,
                    },
                );
                Ok(vec![ContactsEvent::KeyChangeAlert {
                    node_id,
                    offered_node_id: offered,
                    observed_at_ms: now_ms,
                }])
            }
            Err(error) => Err(ContactsError::Book(error)),
        }
    }

    /// Checks whether a message may be sent to a contact.
    ///
    /// This is the send gate: a pending key decision or a blocked contact is a
    /// typed refusal, never a silent drop by the caller.
    ///
    /// # Errors
    ///
    /// [`ContactsError::SendBlockedPendingKeyDecision`],
    /// [`ContactsError::ContactBlocked`] or [`ContactsError::UnknownContact`].
    pub fn can_send(&self, node_id: NodeId) -> Result<(), ContactsError> {
        if self.pending.contains_key(&node_id) {
            return Err(ContactsError::SendBlockedPendingKeyDecision(node_id));
        }
        let contact = self
            .book
            .get(node_id)
            .ok_or(ContactsError::UnknownContact(node_id))?;
        if contact.trust() == Trust::Blocked {
            return Err(ContactsError::ContactBlocked(node_id));
        }
        Ok(())
    }

    /// Accepts a verified rotation statement as the user's key decision.
    ///
    /// The statement must chain the claimed identity to the exact identity
    /// offered by the alert; a different successor is rejected before the book
    /// is changed.
    ///
    /// # Errors
    ///
    /// [`ContactsError::NoPendingKeyChange`], [`ContactsError::KeyChangeMismatch`]
    /// or [`ContactsError::Book`].
    pub fn accept_rotation(
        &mut self,
        node_id: NodeId,
        statement: &RotationStatement,
        now_ms: u64,
    ) -> Result<Vec<ContactsEvent>, ContactsError> {
        let pending = *self
            .pending
            .get(&node_id)
            .ok_or(ContactsError::NoPendingKeyChange(node_id))?;
        if statement.old_node_id() != node_id {
            return Err(ContactsError::KeyChangeMismatch {
                expected: node_id,
                actual: statement.old_node_id(),
            });
        }
        let successor = statement.new_node_id();
        if successor != pending.offered {
            return Err(ContactsError::KeyChangeMismatch {
                expected: pending.offered,
                actual: successor,
            });
        }
        let rotated = self.book.rotate(statement).map_err(ContactsError::Book)?;
        self.known.remove(&node_id);
        self.known.insert(rotated);
        self.pending.remove(&node_id);
        Ok(vec![ContactsEvent::KeyChangeResolved {
            node_id,
            decision: KeyChangeDecision::AcceptRotation,
            successor: Some(rotated),
            at_ms: now_ms,
        }])
    }

    /// Blocks the claimed identity as the user's key decision.
    ///
    /// # Errors
    ///
    /// [`ContactsError::NoPendingKeyChange`] when no alert is pending or
    /// [`ContactsError::Book`] for a store failure.
    pub fn block_key_change(
        &mut self,
        node_id: NodeId,
        now_ms: u64,
    ) -> Result<Vec<ContactsEvent>, ContactsError> {
        if self.pending.remove(&node_id).is_none() {
            return Err(ContactsError::NoPendingKeyChange(node_id));
        }
        if self.book.get(node_id).is_some() {
            self.book
                .set_trust(node_id, Trust::Blocked)
                .map_err(ContactsError::Book)?;
        }
        Ok(vec![ContactsEvent::KeyChangeResolved {
            node_id,
            decision: KeyChangeDecision::Block,
            successor: None,
            at_ms: now_ms,
        }])
    }

    /// Marks a contact verified after an out-of-band safety-number comparison.
    ///
    /// # Errors
    ///
    /// [`ContactsError::PendingKeyChange`] or [`ContactsError::Book`].
    pub fn verify(
        &mut self,
        node_id: NodeId,
        now_ms: u64,
    ) -> Result<Vec<ContactsEvent>, ContactsError> {
        self.reject_pending(node_id)?;
        self.book
            .set_trust(node_id, Trust::Verified)
            .map_err(ContactsError::Book)?;
        Ok(vec![ContactsEvent::Verified {
            node_id,
            at_ms: now_ms,
        }])
    }

    /// Blocks a contact outside a key-change alert.
    ///
    /// # Errors
    ///
    /// [`ContactsError::UnknownContact`] or [`ContactsError::Book`].
    pub fn block(
        &mut self,
        node_id: NodeId,
        now_ms: u64,
    ) -> Result<Vec<ContactsEvent>, ContactsError> {
        self.pending.remove(&node_id);
        self.book
            .set_trust(node_id, Trust::Blocked)
            .map_err(ContactsError::Book)?;
        Ok(vec![ContactsEvent::Blocked {
            node_id,
            at_ms: now_ms,
        }])
    }

    /// Sets or clears the local petname for a contact.
    ///
    /// # Errors
    ///
    /// [`ContactsError::PendingKeyChange`] or [`ContactsError::Book`].
    pub fn rename(
        &mut self,
        node_id: NodeId,
        petname: Option<&str>,
        now_ms: u64,
    ) -> Result<Vec<ContactsEvent>, ContactsError> {
        self.reject_pending(node_id)?;
        self.book
            .set_petname(node_id, petname)
            .map_err(ContactsError::Book)?;
        Ok(vec![ContactsEvent::Renamed {
            node_id,
            at_ms: now_ms,
        }])
    }

    /// Returns all persisted contacts in deterministic id order.
    #[must_use]
    pub fn contacts(&self) -> Vec<Contact> {
        self.known
            .iter()
            .filter_map(|node_id| self.book.get(*node_id))
            .collect()
    }

    /// Returns one contact, if it is known.
    #[must_use]
    pub fn contact(&self, node_id: NodeId) -> Option<Contact> {
        self.book.get(node_id)
    }

    /// Returns the identities waiting for a key decision.
    #[must_use]
    pub fn pending_key_changes(&self) -> Vec<KeyChangeAlert> {
        self.pending
            .iter()
            .map(|(node_id, pending)| KeyChangeAlert {
                node_id: *node_id,
                offered_node_id: pending.offered,
                observed_at_ms: pending.observed_at_ms,
            })
            .collect()
    }

    /// Computes the safety number a human compares with this contact.
    ///
    /// # Errors
    ///
    /// [`ContactsError::WrongLocalIdentity`] or [`ContactsError::UnknownContact`].
    pub fn safety_number(
        &self,
        identity: &Identity,
        node_id: NodeId,
    ) -> Result<SafetyNumber, ContactsError> {
        self.check_local(identity)?;
        let contact = self
            .book
            .get(node_id)
            .ok_or(ContactsError::UnknownContact(node_id))?;
        Ok(SafetyNumber(compute_safety_number(
            (
                identity.public().signing_pk(),
                identity.public().agreement_pk(),
            ),
            (contact.signing_pk(), contact.agreement_pk()),
        )))
    }

    /// Exports this service owner's identity as a verifiable QR string.
    ///
    /// # Errors
    ///
    /// [`ContactsError::WrongLocalIdentity`] or [`ContactsError::Book`] when
    /// the display name is too long or signing fails.
    pub fn export_own_card(
        &self,
        identity: &Identity,
        display_name: &[u8],
        created_at_ms: u64,
    ) -> Result<String, ContactsError> {
        self.check_local(identity)?;
        let card = identity
            .card(display_name, created_at_ms)
            .map_err(ContactsError::Book)?;
        Ok(card.to_qr_string())
    }

    fn reject_pending(&self, node_id: NodeId) -> Result<(), ContactsError> {
        if self.pending.contains_key(&node_id) {
            return Err(ContactsError::PendingKeyChange(node_id));
        }
        Ok(())
    }

    fn check_local(&self, identity: &Identity) -> Result<(), ContactsError> {
        let actual = identity.node_id();
        if actual != self.local_id {
            return Err(ContactsError::WrongLocalIdentity {
                expected: self.local_id,
                actual,
            });
        }
        Ok(())
    }
}

/// A key-change alert retained for the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyChangeAlert {
    /// Identity that was claimed by the presentation.
    pub node_id: NodeId,
    /// Identity derived from the offered keys.
    pub offered_node_id: NodeId,
    /// Injected time of the first alert, in milliseconds.
    pub observed_at_ms: u64,
}
