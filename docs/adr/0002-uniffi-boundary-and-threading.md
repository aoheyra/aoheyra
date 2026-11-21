# ADR 0002: UniFFI boundary and threading model

Status: accepted (APP-01, M0).

## Context

Kotlin UI, the foreground service and the BLE shim need to call Rust and receive
events from it. A hand-written JNI surface would be large and easy to get
wrong; exposing Rust objects directly to the main thread would risk stalls and
crashes across the FFI boundary.

Options considered:

1. Hand-written JNI with manual exception and lifetime mapping.
2. UniFFI with one `AoHeyra` object, typed errors, callback interfaces and
   generated Kotlin bindings built by a Gradle task.

Option 1 gives full control but duplicates work UniFFI already does and makes
callback safety harder to audit. Option 2 gives one reviewable boundary and
bindings that are regenerated rather than hand-edited.

## Decision

UniFFI is the only FFI surface in the project; sibling crates stay pure Rust.
Kotlin creates one `AoHeyra` object with a data directory and callbacks for
events, secrets and BLE `NativeSink`. Methods called from Kotlin return
quickly and run on the Rust runtime; callbacks into Kotlin return immediately
and forward work in order. Event fan-out uses bounded queues with an explicit
overflow behaviour. No long work runs on the Android main thread and Kotlin
never blocks the Rust runtime.

## Consequences

- Generated Kotlin bindings are build output and are never committed.
- Every FFI entry point validates inputs, returns typed errors and never
  panics across the boundary (APP-03).
- Threading and callback ordering need tests in APP-03 and the BLE shim issues.
