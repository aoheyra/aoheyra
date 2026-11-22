# ADR 0012: One bounded map item stream (APP-13)

Status: accepted (APP-13, M5).

## Context

The offline map screen and the debug overlay could each query the directory,
the location session and the contact store. Options considered:

1. Let each screen query every source and merge results locally.
2. Put one bounded map service in the Rust core, fed by typed facts, with a
   coalesced change stream and an explicit staleness policy.

Option 1 duplicates privacy logic and makes staleness inconsistent between
screens. Option 2 gives AND-17 one stream to render, matching MSH-20's
id-and-kind directory events.

## Decision

`MapService` owns a bounded set of items keyed by the shared aoIdents
`NodeId`, the same type aoMesh carries in its directory and neighbour
tables. Facts arrive from the local node, APP-12 location sessions and
aoMesh directory entries. Each item carries relative age and a `Fresh`/`Stale`
level; stale items remain displayed until the removal threshold, then emit a
`Removed` event. Changes are bounded and drained deterministically.
`topology` exposes neighbour counters and directory counts only, never hello
bytes, tags, peer keys or payloads.

Opaque directory location bytes are decoded with APP-07's `AppPayload` codec.
The map accepts only the `Location` variant and reports invalid bytes as a
typed error; it does not define a second location schema or ask the caller to
guess the wire layout.

## Consequences

- AND-17 renders one stream and never decides staleness itself.
- The map cannot accidentally expose a location session after its item has
  been removed; removal is an explicit event.
- Identity and location types stay owned by aoIdents and APP-07, so there is
  no conversion seam or duplicate type across crates.
