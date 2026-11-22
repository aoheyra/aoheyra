# ADR 0016: APP-17 integration suite uses aoMesh's seeded simulator

## Status

Accepted for `v0.1.0`.

## Context

APP-17 must run acceptance scenarios 1-6 on every pull request. The suite
needs real radio, link, routing, store and identity behaviour, but it must not
reimplement those layers in the application.

Two options were considered:

1. Build a bespoke `SimWorld` harness in aoheyra that wires `MeshEngine`,
   `NetworkAdapter`, aoIdents identities and aoNetwork links directly.
2. Call `ao_mesh::sim::run`, which already drives those real siblings on
   aoNetwork's paused `SimWorld` with fixed seeds and metrics.

Option 1 would duplicate simulator setup and version bookkeeping. All four
crates use untagged git sources before the coordinated `v0.1.0` tag, so the
shared `NodeId` and link types cross the seam without a local `[patch]`.
Option 2 keeps identity types inside the sibling and needs no new dependency
beyond enabling the host-side `sim` feature.

## Decision

The suite calls `ao_mesh::sim::run` behind a new `e2e` Cargo feature that
enables `ao_mesh/sim`. CI's existing `cargo test --workspace --all-features`
runs it on every pull request; production Android builds do not enable the
feature, so the simulator's adapter and RNG seams stay out of the `cdylib`.

Scenarios use aoMesh's published schedule helpers where possible and override
only the fields each acceptance case needs. Seeds are literal and fixed. Every
scenario asserts explicit delivery, link, receipt, purge or directory metrics;
none is ignored or conditionally skipped. The two-node direct chat and
eight-message degree-cap runs use the current aoMesh destination reflection,
fixed in aoMesh#92, instead of reduced schedules.

## Consequences

The suite is deterministic and needs no new dependency. It exercises real
simulator delivery, receipts, link bounds and directory expiry. Section 9
device scenarios 7 and 8 stay outside this suite because a paused `SimWorld`
cannot provide restart or eight-hour battery evidence.
