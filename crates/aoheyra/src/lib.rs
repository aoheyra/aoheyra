//! Rust core for the aoheyra Android application.
//!
//! This crate is the only FFI surface in the project. APP-03 exposes the
//! UniFFI [`AoHeyra`] object, its typed errors and the three callback
//! interfaces the Kotlin app implements. The public Rust API is deliberately
//! the same surface: it exists so Rust tests can exercise the bridge without
//! a JVM.
//!
//! Application schema, persistence and orchestration are added by APP-04 to
//! APP-16, never by reimplementing sibling-crate behaviour.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

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
    use super::VERSION;

    #[test]
    fn version_is_the_workspace_version() {
        assert_eq!(VERSION, "0.1.0");
    }
}
