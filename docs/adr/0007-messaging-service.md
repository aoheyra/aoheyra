# ADR 0007: Messaging service prepares opaque submissions and owns status state

Status: accepted (APP-09, M3).

## Context

APP-08 puts one sans-IO `Runtime` in charge of the mesh adapter, identity and
store, but that PR is still a draft and cannot be imported from `origin/master`.
The chat UI (AND-11) and integration suite (APP-17) still need a service that
composes messages, seals them for a recipient, tracks delivery state, handles an
inbox, suppresses duplicates and maintains conversations and unread counts.

Two placements were considered:

1. Let `MessagingService` own a `Runtime`, call `submit_raw` directly and read
   events from its queue.
2. Keep the service independent of the runtime. It seals one opaque body,
   returns a `Submission` plan for the host to submit, and consumes typed status
   callbacks (`mark_carried`, `on_receipt`, `mark_failed`, `on_delivery`,
   `tick`).

Option 1 gives fewer seams once APP-08 is merged, but it cannot compile against
`origin/master`, and it would tie message state to one runtime lifetime and
force the service to understand transport errors. Option 2 keeps the service
testable with two simulated instances today, preserves ADR 0003's opaque
payload rule and lets the host decide which runtime submits the plan. The same
`Submission` shape can later be passed straight to `Runtime::submit_raw` with no
schema change.

For received-message deduplication, a bounded `MessageId -> expiry` map was
chosen over an unbounded `HashSet`. It is deterministic, evicts on injected time
and cannot be grown without limit by a neighbour. Outbound records, inbox items
and conversations use `BTreeMap` with configured caps so iteration order is
stable and memory is bounded; the outbox is a `VecDeque` for FIFO host handoff.

## Decision

`MessagingService` owns the outbound status machine (`Queued`, `Carried`,
`Delivered`, `Failed`, `Expired`), a bounded inbox, a bounded duplicate cache
and bounded conversations with unread counts. `compose` seals a body with
aoIdents `SealedMessage`, derives the mesh message id with aoMesh
`derive_msg_id` and returns a `Submission` containing the header, binding and
opaque wire bytes. `take_submissions` drains the outbox for the host or runtime.
`retry` and `cancel` act only on eligible outbound records; `tick` expires
non-terminal messages with injected time.

Payloads stay opaque bytes until APP-07 defines the app schema. No wall clock,
global RNG, task spawn, database or transport handle is used. Every mutation
returns typed events, and invalid transitions return typed errors.

## Consequences

- A test with two simulated services moves one message through `Queued`,
  `Carried`, `Delivered` and terminal states while the receiver's conversation
  contains it exactly once; duplicate delivery is ignored.
- The service compiles and passes its tests while APP-08 remains unmerged. The
  host is responsible for handing each `Submission` to a runtime and reporting
  the outcomes.
- Bounded tables return `MessagingError::Capacity` rather than growing
  silently; body size is checked before any seal allocation.
- This service is in-memory only. APP-05 (SQLite `MessageStore`) remains
  separate and will persist messages without changing this state machine.
- Once APP-07 lands, the encoder and decoder can be added here without
  changing submission, status or conversation APIs.
