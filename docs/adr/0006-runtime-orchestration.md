# ADR 0006: Sans-IO runtime owns lifecycle, store and event fan-out

Status: accepted (APP-08, M2).

## Context

The UniFFI bridge, network facade, mesh engine, identity, store and settings
exist as separate pieces. Android's foreground service needs one object that
starts, pauses, resumes and stops them in a defined order, drains bounded host
events and shuts down cleanly when the service is destroyed. Two placements
were considered:

1. Let each service (messaging, contacts, location) build its own engine and
   network handles and share them behind a global lock.
2. Put one `Runtime` object in the Rust core. It owns the adapter, identity,
   message store and validated settings; services submit opaque bytes and
   receive typed events.

Option 1 makes lifecycle order and shutdown ambiguous, invites duplicate
dedupe state, and lets a late radio callback reach a stopped service. Option 2
matches ADR 0002's single UniFFI-facing owner and ADR 0005's single
runtime-owned aoNetwork object.

For async execution, a Tokio task inside the core would make the engine
non-deterministic and tie tests to a runtime. A sans-IO state machine with an
optional current-thread executor behind the `runtime-tokio` feature keeps the
core testable with injected time and RNG while still giving Android a timer and
wakeup driver.

For store ownership, leaving sync requests and receipt purges to each app
service would create a second lifecycle owner and duplicate quarantine rules.
Giving the runtime a generic `MessageStore` keeps store policy in aoMesh and
persistence in the app's backend.

## Decision

`aoheyra::runtime::Runtime<R, T, S>` owns a `NetworkAdapter<R, T>` (aoMesh
engine plus aoNetwork), an `Identity`, a validated `RuntimeConfig` and one
`S: MessageStore`. Its `start`, `pause`, `resume` and `stop` methods are
idempotent. `step` polls one transport event, applies store feedback (sync
items, sync requests and receipt purges), then returns typed host events.
Time and RNG are inputs; no core method reads a clock, spawns a task or
interprets an application payload.

An `EventQueue` with a validated capacity fans events out to the host. When it
is full it returns `EventQueueError::Full` with the offending event; it never
drops an event silently. Store feedback is bounded to a fixed number of nested
passes and reports `RuntimeError::StoreCascade` rather than recursing without a
caller tick.

The optional `runtime-tokio` feature exposes `block_on_current_thread` for the
Android service thread. It is a host convenience, not part of the sans-IO
core.

## Consequences

- Two simulated runtimes can exchange a sealed opaque Direct payload through
  aoNetwork, aoMesh and the wired store with a fixed seed and injected time.
- A receipt purges the stored copy in the same step as delivery; the host sees
  a `Purge` notification after the store was updated.
- A host that needs a different persistence backend implements aoMesh's
  `MessageStore` and injects it; aoheyra never opens a database itself.
- App payloads stay opaque here. Messaging, contacts and location services own
  their schemas and call `submit_raw` with a `PacketClass` and sealed bytes.
- `Runtime::new` takes the store explicitly. Existing callers must provide one;
  `MemoryStore` is the in-memory reference used by tests.
