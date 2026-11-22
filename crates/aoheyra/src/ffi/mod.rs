//! The UniFFI bridge surface for the Android application.
//!
//! Every exported item is validated before it reaches application state and
//! returns [`FfiError`] on failure. Kotlin implements the three foreign
//! traits; all of them return a `Result` so a failed or unexpected callback
//! becomes a typed error instead of a panic across the FFI boundary.
//!
//! Radio arguments follow aoNetwork's native contract (NET-03): a beacon and
//! a peer key are 16 bytes, one write is at most 514 bytes, and link id 0 is
//! reserved. Invalid arguments are rejected here, before Kotlin sees them.

use std::collections::VecDeque;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Longest data-directory string accepted at the FFI boundary.
const MAX_DATA_DIR_LEN: usize = 4096;
/// Number of events held before the bridge reports overflow.
const EVENT_QUEUE_CAPACITY: usize = 64;
/// Longest callback error string kept in a typed error.
const MAX_ERROR_REASON_CHARS: usize = 256;
/// Upper bound on the database key a vault callback may return (APP-04).
const MAX_DATABASE_KEY_LEN: usize = 64;
const BEACON_LEN: usize = 16;
const PEER_KEY_LEN: usize = 16;
const MAX_WRITE_LEN: usize = 514;
const PONG: &str = "pong";

/// An event originated by Rust and delivered to the Kotlin listener.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum AppEvent {
    /// Reply emitted by [`AoHeyra::ping`].
    Pong {
        /// Opaque payload for the skeleton; later events carry typed app data.
        payload: String,
    },
}

/// Advertising duty cycle from aoNetwork's native contract (NET-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AdvertiseMode {
    /// `ADVERTISE_MODE_LOW_POWER`.
    LowPower,
    /// `ADVERTISE_MODE_BALANCED`.
    Balanced,
    /// `ADVERTISE_MODE_LOW_LATENCY`.
    LowLatency,
}

/// Scanning duty cycle from aoNetwork's native contract (NET-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ScanMode {
    /// `SCAN_MODE_LOW_POWER`.
    LowPower,
    /// `SCAN_MODE_BALANCED`.
    Balanced,
    /// `SCAN_MODE_LOW_LATENCY`.
    LowLatency,
}

/// Every error that can cross the UniFFI boundary.
///
/// UniFFI renders this enum as a Kotlin sealed exception type, so Kotlin can
/// catch each case deliberately instead of matching on error strings.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum FfiError {
    /// The supplied data directory cannot identify an app data location.
    InvalidDataDirectory {
        /// Why the value was rejected, without including the supplied path.
        reason: String,
    },
    /// A radio callback argument violated aoNetwork's native contract.
    InvalidArgument {
        /// Why the value was rejected, without echoing attacker-controlled data.
        reason: String,
    },
    /// The bounded event queue is full; the event was rejected, not dropped.
    EventQueueFull,
    /// A Kotlin callback returned an error, threw, or panicked on the Rust side.
    CallbackFailed {
        /// Bounded description from the callback.
        reason: String,
    },
    /// The object was closed; late callbacks and calls are ignored.
    Closed,
}

impl fmt::Display for FfiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDataDirectory { reason } => {
                write!(f, "invalid data directory: {reason}")
            }
            Self::InvalidArgument { reason } => write!(f, "invalid argument: {reason}"),
            Self::EventQueueFull => f.write_str("event queue is full"),
            Self::CallbackFailed { reason } => write!(f, "callback failed: {reason}"),
            Self::Closed => f.write_str("object is closed"),
        }
    }
}

impl std::error::Error for FfiError {}

impl From<uniffi::UnexpectedUniFFICallbackError> for FfiError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::CallbackFailed {
            reason: bounded_reason(error.reason),
        }
    }
}

fn bounded_reason(reason: impl Into<String>) -> String {
    reason.into().chars().take(MAX_ERROR_REASON_CHARS).collect()
}

/// Turns a panic in a Rust-implemented callback into a typed error.
///
/// UniFFI already catches panics in exported Rust methods and unexpected
/// Kotlin exceptions in foreign callbacks; this guards Rust implementations
/// used in tests and in later in-process services for the same reason.
fn guard_callback<T>(callback: impl FnOnce() -> Result<T, FfiError>) -> Result<T, FfiError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)).unwrap_or_else(|_| {
        Err(FfiError::CallbackFailed {
            reason: "callback panicked".to_owned(),
        })
    })
}

fn require_len(bytes: &[u8], expected: usize, label: &'static str) -> Result<(), FfiError> {
    if bytes.len() == expected {
        return Ok(());
    }
    Err(FfiError::InvalidArgument {
        reason: format!("{label} must be exactly {expected} bytes"),
    })
}

fn require_max_len(bytes: &[u8], limit: usize, label: &'static str) -> Result<(), FfiError> {
    if bytes.len() <= limit {
        return Ok(());
    }
    Err(FfiError::InvalidArgument {
        reason: format!("{label} must be at most {limit} bytes"),
    })
}

fn require_link(link: u64) -> Result<(), FfiError> {
    if link == 0 {
        return Err(FfiError::InvalidArgument {
            reason: "link id 0 is reserved".to_owned(),
        });
    }
    Ok(())
}

/// Rust-to-Kotlin event listener.
///
/// Implementations must return immediately and post any UI work to their own
/// dispatcher. A returned error is surfaced to the Rust caller as
/// [`FfiError::CallbackFailed`]; an unexpected Kotlin exception is converted
/// by the same `From` implementation.
#[uniffi::export(with_foreign)]
pub trait EventListener: Send + Sync {
    /// Delivers one Rust-originated event.
    fn on_event(&self, event: AppEvent) -> Result<(), FfiError>;
}

impl fmt::Debug for dyn EventListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EventListener")
    }
}

/// Kotlin-backed secret vault used later by storage (APP-04, APP-06).
///
/// The database key is never logged or copied into application state by this
/// skeleton; the callback is held for the runtime to consume.
#[uniffi::export(with_foreign)]
pub trait SecretVault: Send + Sync {
    /// Returns the app database key from the Android Keystore.
    fn database_key(&self) -> Result<Vec<u8>, FfiError>;
}

impl fmt::Debug for dyn SecretVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretVault")
    }
}

/// Kotlin BLE shim, matching aoNetwork's `NativeSink` (NET-03).
///
/// Every method must return immediately and post radio work to the single BLE
/// thread. Byte lengths and link ids are validated by [`AoHeyra`] before a
/// method is called.
#[uniffi::export(with_foreign)]
pub trait NativeSink: Send + Sync {
    /// Starts or replaces advertising with `beacon`, or stops it with `None`.
    fn set_advertising(&self, beacon: Option<Vec<u8>>, mode: AdvertiseMode)
    -> Result<(), FfiError>;
    /// Starts or changes scanning, or stops it with `None`.
    fn set_scanning(&self, mode: Option<ScanMode>) -> Result<(), FfiError>;
    /// Opens `link` to `peer` as central.
    fn connect(&self, link: u64, peer: Vec<u8>) -> Result<(), FfiError>;
    /// Sends `bytes` as one ATT value on `link`.
    fn write(&self, link: u64, bytes: Vec<u8>) -> Result<(), FfiError>;
    /// Tears `link` down.
    fn disconnect(&self, link: u64) -> Result<(), FfiError>;
}

impl fmt::Debug for dyn NativeSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeSink")
    }
}

#[derive(Debug)]
struct EventQueue {
    listener: Arc<dyn EventListener>,
    pending: Mutex<VecDeque<AppEvent>>,
    closed: AtomicBool,
}

impl EventQueue {
    fn new(listener: Arc<dyn EventListener>) -> Self {
        Self {
            listener,
            pending: Mutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
        }
    }

    fn enqueue(&self, event: AppEvent) -> Result<(), FfiError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(FfiError::Closed);
        }
        let mut pending = lock(&self.pending);
        if pending.len() >= EVENT_QUEUE_CAPACITY {
            return Err(FfiError::EventQueueFull);
        }
        pending.push_back(event);
        Ok(())
    }

    fn emit(&self, event: AppEvent) -> Result<(), FfiError> {
        self.enqueue(event)?;
        self.drain()
    }

    fn drain(&self) -> Result<(), FfiError> {
        loop {
            if self.closed.load(Ordering::Acquire) {
                return Err(FfiError::Closed);
            }
            let event = lock(&self.pending).pop_front();
            let Some(event) = event else {
                return Ok(());
            };
            guard_callback(|| self.listener.on_event(event))?;
        }
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let _discarded = std::mem::take(&mut *lock(&self.pending));
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug)]
struct Inner {
    data_dir: PathBuf,
    events: EventQueue,
    vault: Arc<dyn SecretVault>,
    sink: Arc<dyn NativeSink>,
}

/// The single object Kotlin creates to reach the Rust application.
#[derive(Debug, uniffi::Object)]
pub struct AoHeyra {
    inner: Arc<Inner>,
}

#[uniffi::export]
impl AoHeyra {
    /// Creates the bridge for an app-owned data directory and three callbacks.
    #[uniffi::constructor]
    pub fn new(
        data_dir: String,
        listener: Arc<dyn EventListener>,
        vault: Arc<dyn SecretVault>,
        sink: Arc<dyn NativeSink>,
    ) -> Result<Arc<Self>, FfiError> {
        validate_data_dir(&data_dir)?;
        Ok(Arc::new(Self {
            inner: Arc::new(Inner {
                data_dir: PathBuf::from(data_dir),
                events: EventQueue::new(listener),
                vault,
                sink,
            }),
        }))
    }

    /// Returns the validated absolute data directory as a string.
    pub fn data_dir(&self) -> String {
        self.inner.data_dir.to_string_lossy().into_owned()
    }

    /// Checks the bridge and emits one Rust-originated event.
    ///
    /// This is `async` so Kotlin calls it as a suspend function and does not
    /// block the Android main thread. The event is delivered in order through
    /// the bounded queue; a failed listener is reported as a typed error.
    pub async fn ping(&self) -> Result<String, FfiError> {
        self.inner.events.emit(AppEvent::Pong {
            payload: PONG.to_owned(),
        })?;
        Ok(PONG.to_owned())
    }

    /// Asks the Kotlin vault for its key and discards it.
    ///
    /// This verifies the callback is wired and returns a bounded key length
    /// without ever exposing key bytes across the boundary again.
    pub async fn check_secret_vault(&self) -> Result<(), FfiError> {
        let key = guard_callback(|| self.inner.vault.database_key())?;
        if key.is_empty() || key.len() > MAX_DATABASE_KEY_LEN {
            return Err(FfiError::CallbackFailed {
                reason: "database key has an invalid length".to_owned(),
            });
        }
        Ok(())
    }

    /// Starts or replaces advertising, or stops it with `None` (NET-03).
    pub async fn set_advertising(
        &self,
        beacon: Option<Vec<u8>>,
        mode: AdvertiseMode,
    ) -> Result<(), FfiError> {
        if let Some(beacon) = beacon.as_deref() {
            require_len(beacon, BEACON_LEN, "beacon")?;
        }
        guard_callback(|| self.inner.sink.set_advertising(beacon, mode))
    }

    /// Starts or changes scanning, or stops it with `None` (NET-03).
    pub async fn set_scanning(&self, mode: Option<ScanMode>) -> Result<(), FfiError> {
        guard_callback(|| self.inner.sink.set_scanning(mode))
    }

    /// Opens an outbound link to a 16-byte peer key (NET-03).
    pub async fn connect(&self, link: u64, peer: Vec<u8>) -> Result<(), FfiError> {
        require_link(link)?;
        require_len(&peer, PEER_KEY_LEN, "peer key")?;
        guard_callback(|| self.inner.sink.connect(link, peer))
    }

    /// Sends one ATT value of at most 514 bytes (NET-03).
    pub async fn write(&self, link: u64, bytes: Vec<u8>) -> Result<(), FfiError> {
        require_link(link)?;
        require_max_len(&bytes, MAX_WRITE_LEN, "write")?;
        guard_callback(|| self.inner.sink.write(link, bytes))
    }

    /// Tears a link down (NET-03).
    pub async fn disconnect(&self, link: u64) -> Result<(), FfiError> {
        require_link(link)?;
        guard_callback(|| self.inner.sink.disconnect(link))
    }

    /// Closes the bridge and discards undelivered events.
    ///
    /// Idempotent. Calls and callbacks after close are ignored with
    /// [`FfiError::Closed`]; this is the late-callback rule from ADR 0002.
    pub fn close(&self) {
        self.inner.events.close();
    }
}

fn validate_data_dir(data_dir: &str) -> Result<(), FfiError> {
    if data_dir.is_empty() {
        return Err(FfiError::InvalidDataDirectory {
            reason: "must not be empty".to_owned(),
        });
    }
    if data_dir.len() > MAX_DATA_DIR_LEN {
        return Err(FfiError::InvalidDataDirectory {
            reason: "path is too long".to_owned(),
        });
    }
    if data_dir.as_bytes().contains(&0) {
        return Err(FfiError::InvalidDataDirectory {
            reason: "path contains a NUL byte".to_owned(),
        });
    }
    if !Path::new(data_dir).is_absolute() {
        return Err(FfiError::InvalidDataDirectory {
            reason: "path must be absolute".to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct TestListener {
        calls: Mutex<Vec<AppEvent>>,
    }

    impl EventListener for TestListener {
        fn on_event(&self, event: AppEvent) -> Result<(), FfiError> {
            lock(&self.calls).push(event);
            Ok(())
        }
    }

    #[test]
    fn event_queue_capacity_is_enforced_and_close_discards_pending() {
        let listener = Arc::new(TestListener::default());
        let queue = EventQueue::new(Arc::clone(&listener) as Arc<dyn EventListener>);
        for index in 0..EVENT_QUEUE_CAPACITY {
            queue
                .enqueue(AppEvent::Pong {
                    payload: index.to_string(),
                })
                .expect("event fits in the bounded queue");
        }
        assert_eq!(
            queue.enqueue(AppEvent::Pong {
                payload: "overflow".to_owned(),
            }),
            Err(FfiError::EventQueueFull)
        );
        queue.close();
        assert_eq!(lock(&listener.calls).len(), 0);
        assert_eq!(
            queue.enqueue(AppEvent::Pong {
                payload: "late".to_owned(),
            }),
            Err(FfiError::Closed)
        );
    }

    #[test]
    fn callback_error_is_returned_to_the_caller() {
        struct FailingListener;
        impl EventListener for FailingListener {
            fn on_event(&self, _event: AppEvent) -> Result<(), FfiError> {
                Err(FfiError::CallbackFailed {
                    reason: "vault unavailable".to_owned(),
                })
            }
        }
        let queue = EventQueue::new(Arc::new(FailingListener));
        assert_eq!(
            queue.emit(AppEvent::Pong {
                payload: PONG.to_owned(),
            }),
            Err(FfiError::CallbackFailed {
                reason: "vault unavailable".to_owned(),
            })
        );
    }

    #[test]
    fn listener_panic_is_converted_to_a_typed_error() {
        struct PanickingListener;
        impl EventListener for PanickingListener {
            fn on_event(&self, _event: AppEvent) -> Result<(), FfiError> {
                panic!("test listener panic");
            }
        }
        let queue = EventQueue::new(Arc::new(PanickingListener));
        assert_eq!(
            queue.emit(AppEvent::Pong {
                payload: PONG.to_owned(),
            }),
            Err(FfiError::CallbackFailed {
                reason: "callback panicked".to_owned(),
            })
        );
    }

    #[test]
    fn data_directory_is_validated_without_disclosing_the_path() {
        assert_eq!(
            validate_data_dir(""),
            Err(FfiError::InvalidDataDirectory {
                reason: "must not be empty".to_owned(),
            })
        );
        assert_eq!(
            validate_data_dir("relative/path"),
            Err(FfiError::InvalidDataDirectory {
                reason: "path must be absolute".to_owned(),
            })
        );
        assert_eq!(
            validate_data_dir("/tmp/app\0data"),
            Err(FfiError::InvalidDataDirectory {
                reason: "path contains a NUL byte".to_owned(),
            })
        );
        assert!(validate_data_dir("/tmp/aoheyra").is_ok());
    }

    #[test]
    fn unexpected_callback_error_is_bounded_and_typed() {
        let error = FfiError::from(uniffi::UnexpectedUniFFICallbackError::new("x".repeat(1000)));
        match error {
            FfiError::CallbackFailed { reason } => assert_eq!(reason.chars().count(), 256),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
