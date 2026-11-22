//! Rust-side evidence for APP-03's "Done when" line: `ping()` returns and a
//! Rust-originated event reaches the listener through the same public surface
//! the generated Kotlin bindings call.

use std::fmt;
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};

use aoheyra::{
    AdvertiseMode, AoHeyra, AppEvent, EventListener, FfiError, NativeSink, ScanMode, SecretVault,
};

#[derive(Debug, Default)]
struct RecordingListener {
    events: Mutex<Vec<AppEvent>>,
}

impl EventListener for RecordingListener {
    fn on_event(&self, event: AppEvent) -> Result<(), FfiError> {
        lock(&self.events).push(event);
        Ok(())
    }
}

struct FailingListener;

impl EventListener for FailingListener {
    fn on_event(&self, _event: AppEvent) -> Result<(), FfiError> {
        Err(FfiError::CallbackFailed {
            reason: "listener unavailable".to_owned(),
        })
    }
}

impl fmt::Debug for FailingListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FailingListener")
    }
}

#[derive(Debug)]
struct FixedVault {
    key: Vec<u8>,
}

impl SecretVault for FixedVault {
    fn database_key(&self) -> Result<Vec<u8>, FfiError> {
        Ok(self.key.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SinkCall {
    Advertising(Option<Vec<u8>>, AdvertiseMode),
    Scanning(Option<ScanMode>),
    Connect(u64, Vec<u8>),
    Write(u64, Vec<u8>),
    Disconnect(u64),
}

#[derive(Debug, Default)]
struct RecordingSink {
    calls: Mutex<Vec<SinkCall>>,
}

impl NativeSink for RecordingSink {
    fn set_advertising(
        &self,
        beacon: Option<Vec<u8>>,
        mode: AdvertiseMode,
    ) -> Result<(), FfiError> {
        lock(&self.calls).push(SinkCall::Advertising(beacon, mode));
        Ok(())
    }

    fn set_scanning(&self, mode: Option<ScanMode>) -> Result<(), FfiError> {
        lock(&self.calls).push(SinkCall::Scanning(mode));
        Ok(())
    }

    fn connect(&self, link: u64, peer: Vec<u8>) -> Result<(), FfiError> {
        lock(&self.calls).push(SinkCall::Connect(link, peer));
        Ok(())
    }

    fn write(&self, link: u64, bytes: Vec<u8>) -> Result<(), FfiError> {
        lock(&self.calls).push(SinkCall::Write(link, bytes));
        Ok(())
    }

    fn disconnect(&self, link: u64) -> Result<(), FfiError> {
        lock(&self.calls).push(SinkCall::Disconnect(link));
        Ok(())
    }
}

#[derive(Debug)]
struct FailingSink;

impl NativeSink for FailingSink {
    fn set_advertising(
        &self,
        _beacon: Option<Vec<u8>>,
        _mode: AdvertiseMode,
    ) -> Result<(), FfiError> {
        Ok(())
    }

    fn set_scanning(&self, _mode: Option<ScanMode>) -> Result<(), FfiError> {
        Ok(())
    }

    fn connect(&self, _link: u64, _peer: Vec<u8>) -> Result<(), FfiError> {
        Err(FfiError::CallbackFailed {
            reason: "radio unavailable".to_owned(),
        })
    }

    fn write(&self, _link: u64, _bytes: Vec<u8>) -> Result<(), FfiError> {
        Ok(())
    }

    fn disconnect(&self, _link: u64) -> Result<(), FfiError> {
        Ok(())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn test_data_dir() -> String {
    std::env::temp_dir().to_string_lossy().into_owned()
}

fn new_app_with(
    listener: Arc<dyn EventListener>,
    vault: Arc<dyn SecretVault>,
    sink: Arc<dyn NativeSink>,
) -> Arc<AoHeyra> {
    AoHeyra::new(test_data_dir(), listener, vault, sink)
        .unwrap_or_else(|error| panic!("constructor failed: {error}"))
}

fn new_app(listener: Arc<dyn EventListener>) -> Arc<AoHeyra> {
    new_app_with(
        listener,
        Arc::new(FixedVault { key: vec![1, 2, 3] }),
        Arc::new(RecordingSink::default()),
    )
}

#[test]
fn ping_delivers_a_rust_originated_event_to_the_listener() {
    let listener = Arc::new(RecordingListener::default());
    let app = new_app(listener.clone());

    assert_eq!(block_on(app.ping()), Ok("pong".to_owned()));

    let events = lock(&listener.events);
    assert_eq!(
        events.as_slice(),
        &[AppEvent::Pong {
            payload: "pong".to_owned(),
        }]
    );
}

#[test]
fn listener_failure_is_a_typed_error_not_a_panic() {
    let app = new_app(Arc::new(FailingListener));

    assert_eq!(
        block_on(app.ping()),
        Err(FfiError::CallbackFailed {
            reason: "listener unavailable".to_owned(),
        })
    );
}

#[test]
fn late_calls_after_shutdown_are_rejected() {
    let listener = Arc::new(RecordingListener::default());
    let app = new_app(listener.clone());

    app.shutdown();
    assert_eq!(block_on(app.ping()), Err(FfiError::Closed));
    assert!(lock(&listener.events).is_empty());
}

#[test]
fn data_directory_round_trips_through_the_object() {
    let app = new_app(Arc::new(RecordingListener::default()));
    assert_eq!(app.data_dir(), test_data_dir());
}

#[test]
fn sink_calls_are_validated_and_forwarded_in_order() {
    let sink = Arc::new(RecordingSink::default());
    let app = new_app_with(
        Arc::new(RecordingListener::default()),
        Arc::new(FixedVault { key: vec![1; 32] }),
        Arc::clone(&sink) as Arc<dyn NativeSink>,
    );

    assert_eq!(
        block_on(app.set_advertising(Some(vec![7; 16]), AdvertiseMode::Balanced)),
        Ok(())
    );
    assert_eq!(block_on(app.set_scanning(Some(ScanMode::LowPower))), Ok(()));
    assert_eq!(block_on(app.connect(9, vec![8; 16])), Ok(()));
    assert_eq!(block_on(app.write(9, vec![5; 514])), Ok(()));
    assert_eq!(block_on(app.disconnect(9)), Ok(()));

    assert_eq!(
        lock(&sink.calls).as_slice(),
        &[
            SinkCall::Advertising(Some(vec![7; 16]), AdvertiseMode::Balanced),
            SinkCall::Scanning(Some(ScanMode::LowPower)),
            SinkCall::Connect(9, vec![8; 16]),
            SinkCall::Write(9, vec![5; 514]),
            SinkCall::Disconnect(9),
        ]
    );
}

#[test]
fn native_contract_violations_are_rejected_before_the_sink_is_called() {
    let sink = Arc::new(RecordingSink::default());
    let app = new_app_with(
        Arc::new(RecordingListener::default()),
        Arc::new(FixedVault { key: vec![1; 32] }),
        Arc::clone(&sink) as Arc<dyn NativeSink>,
    );

    assert_eq!(
        block_on(app.set_advertising(Some(vec![7; 15]), AdvertiseMode::Balanced)),
        Err(FfiError::InvalidArgument {
            reason: "beacon must be exactly 16 bytes".to_owned(),
        })
    );
    assert_eq!(
        block_on(app.connect(0, vec![8; 16])),
        Err(FfiError::InvalidArgument {
            reason: "link id 0 is reserved".to_owned(),
        })
    );
    assert_eq!(
        block_on(app.connect(9, vec![8; 15])),
        Err(FfiError::InvalidArgument {
            reason: "peer key must be exactly 16 bytes".to_owned(),
        })
    );
    assert_eq!(
        block_on(app.write(9, vec![5; 515])),
        Err(FfiError::InvalidArgument {
            reason: "write must be at most 514 bytes".to_owned(),
        })
    );
    assert!(lock(&sink.calls).is_empty());
}

#[test]
fn sink_failure_is_a_typed_error_not_a_panic() {
    let app = new_app_with(
        Arc::new(RecordingListener::default()),
        Arc::new(FixedVault { key: vec![1; 32] }),
        Arc::new(FailingSink),
    );

    assert_eq!(
        block_on(app.connect(9, vec![8; 16])),
        Err(FfiError::CallbackFailed {
            reason: "radio unavailable".to_owned(),
        })
    );
}

#[test]
fn secret_vault_key_is_checked_without_being_exposed() {
    let app = new_app(Arc::new(RecordingListener::default()));
    assert_eq!(block_on(app.check_secret_vault()), Ok(()));

    let bad_vault = new_app_with(
        Arc::new(RecordingListener::default()),
        Arc::new(FixedVault { key: vec![1; 65] }),
        Arc::new(RecordingSink::default()),
    );
    assert_eq!(
        block_on(bad_vault.check_secret_vault()),
        Err(FfiError::CallbackFailed {
            reason: "database key has an invalid length".to_owned(),
        })
    );
}
