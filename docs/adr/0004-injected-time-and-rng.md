# ADR 0004: Inject time and RNG; no shared utility crate yet

Status: accepted (APP-01, M0).

## Context

The sibling engines are sans-IO: `now` and randomness are parameters so
simulators and tests are deterministic. The app could wrap them in a shared
utility crate, or it could pass clocks and RNGs explicitly at each call site.

Options considered:

1. A shared `aoheyra_util` crate with global helpers for time and randomness.
2. Constructor-injected clock and RNG in the runtime, passed into each crate.

Option 1 hides inputs behind a global and makes restart and simulator tests
harder to reproduce. Option 2 matches the sibling engines and keeps determinism
visible in tests.

## Decision

The Rust core takes `now` and an RNG as inputs and passes them to aoMesh,
aoNetwork and aoIdents. The app uses platform clocks and OS randomness only at
the edge (for example when the runtime is constructed). No shared utility
crate is created. A util crate may be proposed later only when a concrete,
repeated implementation exists in at least two places.

## Consequences

- Integration tests use a fixed clock and seeded RNG; no test depends on the
  wall clock or global state.
- Sensor timestamps and SQLite times are normalised at the edge.
- Adding a shared crate is a new ADR, not an implicit refactor.
