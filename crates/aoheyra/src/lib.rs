//! This crate is the only FFI surface in the project. APP-03 exposes the
//! UniFFI [`AoHeyra`] object, its typed errors and the three callback
//! interfaces the Kotlin app implements. The public Rust API is deliberately
//! the same surface: it exists so Rust tests can exercise the bridge without
//! a JVM.
//!
//! Application schema, persistence and orchestration are added by APP-04 to
//! APP-16, never by reimplementing sibling-crate behaviour.
//!
//! The messaging module owns direct-message state, sealing plans and inbox
//! bookkeeping; it stays independent of the runtime and never interprets an
//! application payload. APP-03's UniFFI object calls into these services.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Direct-message state machine, sealing plans and bounded inbox (APP-09).
pub mod messaging;

pub use messaging::{
    Conversation, ConversationDirection, ConversationEntry, DEFAULT_MAX_BODY_BYTES, FailureReason,
    InboxMessage, MessageStatus, MessagingConfig, MessagingError, MessagingEvent, MessagingService,
    OutboundMessage, Submission,
};
// Runtime and messaging both have a `ConfigError` and TTL/lifetime defaults.
// Qualify the messaging ones at this surface; the module paths remain canonical.
pub use messaging::{
    ConfigError as MessagingConfigError, DEFAULT_LIFETIME_S as MESSAGING_DEFAULT_LIFETIME_S,
    DEFAULT_TTL as MESSAGING_DEFAULT_TTL,
};

/// Versioned binary schema for text, location, profile and receipt payloads
/// (APP-07).
pub mod payload;

/// Sans-IO runtime that owns the mesh engine, network facade, identity and
/// injected message store (APP-08).
pub mod runtime;

#[cfg(feature = "runtime-tokio")]
pub use runtime::block_on_current_thread;
pub use runtime::{
    ConfigError, DEFAULT_EVENT_CAPACITY, DEFAULT_LIFETIME_S, DEFAULT_TTL, EventQueue,
    EventQueueError, LifecycleState, Runtime, RuntimeConfig, RuntimeError, RuntimeEvent,
    SendFailure,
};

mod ffi;

pub use ffi::{
    AdvertiseMode, AoHeyra, AppEvent, EventListener, FfiError, NativeSink, ScanMode, SecretVault,
};

uniffi::setup_scaffolding!();

/// Encrypted SQLite message store and schema migration support (APP-04).
pub mod storage;

/// Crate version from `Cargo.toml`, used by diagnostics and release checks.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Release-tag matrix and its manifest check (APP-18).
pub mod release;

#[cfg(test)]
mod tests {
    use super::*;
    use ao_idents::{Identity, SealedMessage};
    use rand08::{SeedableRng, rngs::StdRng};

    fn identities() -> (Identity, Identity) {
        (Identity::from_seed([1; 32]), Identity::from_seed([2; 32]))
    }

    fn services() -> (MessagingService, MessagingService, Identity, Identity) {
        let (alice, bob) = identities();
        let alice_id = *alice.node_id().as_bytes();
        let bob_id = *bob.node_id().as_bytes();
        (
            MessagingService::new(alice_id, MessagingConfig::default()),
            MessagingService::new(bob_id, MessagingConfig::default()),
            alice,
            bob,
        )
    }

    #[test]
    fn message_moves_through_every_status_and_appears_once() {
        let (mut alice_service, mut bob_service, alice, bob) = services();
        let bob_public = bob.public().clone();
        let bob_id = *bob.node_id().as_bytes();
        let mut rng = StdRng::seed_from_u64(42);
        let body = b"an opaque message body";

        let (id, queued) = alice_service
            .compose(&alice, &bob_public, body, 1_000, &mut rng)
            .expect("compose succeeds");
        assert!(queued.contains(&MessagingEvent::StatusChanged {
            id,
            peer: bob_id,
            status: MessageStatus::Queued,
        }));
        assert_eq!(
            alice_service.message(&id).map(|message| message.status),
            Some(MessageStatus::Queued)
        );

        let submissions = alice_service.take_submissions();
        assert_eq!(submissions.len(), 1);
        let submission = &submissions[0];
        alice_service
            .mark_carried(&id, 1_010)
            .expect("carried transition");
        assert_eq!(
            alice_service.message(&id).map(|message| message.status),
            Some(MessageStatus::Carried { since_ms: 1_010 })
        );

        let sealed = SealedMessage::decode(&submission.payload).expect("valid seal");
        let opened = sealed
            .open(&bob, submission.binding.as_bytes())
            .expect("bob opens");
        assert_eq!(opened.plaintext(), body);
        let received = bob_service
            .on_delivery(&submission.header, opened.plaintext(), 1_020)
            .expect("delivery accepted");
        assert!(received.contains(&MessagingEvent::Received {
            id,
            peer: *alice.node_id().as_bytes(),
            payload: body.to_vec(),
        }));
        let duplicate = bob_service
            .on_delivery(&submission.header, opened.plaintext(), 1_021)
            .expect("duplicate suppressed");
        assert!(duplicate.is_empty());
        assert_eq!(bob_service.inbox(alice.node_id().as_bytes()).len(), 1);

        let delivered = alice_service
            .on_receipt(&id, 1_030)
            .expect("receipt accepted");
        assert!(delivered.contains(&MessagingEvent::StatusChanged {
            id,
            peer: bob_id,
            status: MessageStatus::Delivered { at_ms: 1_030 },
        }));
        let alice_conversations = alice_service.conversations();
        assert_eq!(alice_conversations.len(), 1);
        assert_eq!(alice_conversations[0].peer(), &bob_id);
        let bob_conversations = bob_service.conversations();
        assert_eq!(bob_conversations.len(), 1);
        assert_eq!(bob_conversations[0].unread(), 1);
        assert_eq!(bob_service.mark_read(alice.node_id().as_bytes()).len(), 1);
        assert_eq!(bob_conversations[0].unread(), 1);
        assert_eq!(
            bob_service
                .conversation(alice.node_id().as_bytes())
                .unwrap()
                .unread(),
            0
        );

        let (failed_id, _) = alice_service
            .compose(&alice, &bob_public, b"failed", 1_040, &mut rng)
            .expect("second compose");
        alice_service
            .mark_failed(&failed_id, 1_050, FailureReason::SubmitRejected)
            .expect("failure recorded");
        assert_eq!(
            alice_service
                .message(&failed_id)
                .map(|message| message.status),
            Some(MessageStatus::Failed {
                at_ms: 1_050,
                reason: FailureReason::SubmitRejected,
            })
        );
        let (retried_id, _) = alice_service
            .retry(&failed_id, &alice, 1_060, &mut rng)
            .expect("retry seals a new id");
        assert_ne!(retried_id, failed_id);
        assert_eq!(
            alice_service
                .message(&retried_id)
                .map(|message| message.status),
            Some(MessageStatus::Queued)
        );
        let expired = alice_service.tick(1_060 + 3_600_000);
        assert!(expired.contains(&MessagingEvent::StatusChanged {
            id: retried_id,
            peer: bob_id,
            status: MessageStatus::Expired {
                at_ms: 1_060 + 3_600_000,
            },
        }));
        assert_eq!(
            alice_service
                .cancel(&failed_id)
                .expect("cancel succeeds")
                .len(),
            1
        );
        assert!(matches!(
            alice_service.cancel(&id),
            Err(MessagingError::CannotCancelDelivered(cancelled)) if cancelled == id
        ));
    }

    #[test]
    fn config_rejects_out_of_range_values() {
        assert_eq!(
            MessagingConfig::new(0, 1, 1, 1, 1, 1, 0),
            Err(MessagingConfigError::Ttl { value: 0 })
        );
        assert_eq!(
            MessagingConfig::new(8, 0, 1, 1, 1, 1, 0),
            Err(MessagingConfigError::Lifetime { value: 0 })
        );
        assert_eq!(
            MessagingConfig::new(8, 1, 32 * 1024 + 1, 1, 1, 1, 0),
            Err(MessagingConfigError::BodyBytes {
                value: 32 * 1024 + 1
            })
        );
        assert!(MessagingConfig::new(8, 1, 1, 1, 1, 1, 0).is_ok());
    }

    #[test]
    fn delivery_for_another_node_is_rejected() {
        let (_, mut bob_service, alice, bob) = services();
        let bob_public = bob.public().clone();
        let mut rng = StdRng::seed_from_u64(7);
        let (id, _) = bob_service
            .compose(&bob, alice.public(), b"outbound", 100, &mut rng)
            .expect("compose");
        let submission = bob_service.take_submissions().remove(0);
        assert_eq!(submission.id, id);
        let mut wrong_header = submission.header;
        wrong_header.dst = Some(*alice.node_id().as_bytes());
        assert_eq!(
            bob_service.on_delivery(&wrong_header, b"x", 100),
            Err(MessagingError::Misdirected)
        );
        let _ = bob_public;
    }
}
