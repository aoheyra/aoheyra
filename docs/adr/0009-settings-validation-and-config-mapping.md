# ADR 0009: Validate settings at the app boundary and map them to sibling configs

Status: accepted (APP-11, M3).

## Context

AND-14 renders user-facing settings, but the values that change mesh behaviour
belong to aoNetwork, aoMesh and the app runtime. A setting must be rejected
with a human-readable message before it reaches a sibling config, and every
accepted setting must have an observable effect. APP-08's runtime is still a
draft, so its config type cannot be imported from `origin/master`.

Two placements were considered:

1. Let the UI write sibling configs directly and rely on their validators.
2. Put one `SettingsService` in the Rust core that validates a typed snapshot,
   applies changes atomically and maps the snapshot onto every downstream
   config in one deterministic function.

Option 1 would scatter validation across Kotlin, give no single snapshot for
diagnostics or persistence, and could partially apply a change. Option 2 keeps
one source of truth behind UniFFI, returns typed errors with the rejected value
and range, and makes the mapping directly testable.

## Decision

`AppSettings` holds six settings and validates them in its constructor.
`SettingsService::update` validates a copy and only then replaces the stored
snapshot, so a rejected change leaves the old settings intact. `map` builds a
`MappedConfig` from aoNetwork's `NetworkConfig`, aoMesh's `MeshConfig`,
`store::StoreLimits`, a runtime settings seam, local `BeaconFlags` and app-level
privacy gates.

| Setting | Range / values | Default | Mapping |
|---|---|---|---|
| Max links | 1..=32 | 5 | `NetworkConfig::with_max_links` |
| Relay enabled | on/off | on | Local beacon `relay_willing` and runtime seam |
| Store quota | 64 KiB..=64 MiB | 8 MiB | `StoreLimits.total_bytes`, entry count `quota / 8 KiB` capped at 1024, per-origin `quota / 8`, per-class Direct `quota`, Broadcast `quota / 4`, Presence `quota / 32`, Telemetry 0 |
| Battery profile | PowerSaver, Balanced, Performance | Balanced | Discovery 30/10/5 s, keepalive 60/15/10 s, runtime event capacity 16/64/128, low-battery beacon in PowerSaver |
| Presence enabled | on/off | on | `MeshConfig.presence.enabled` and privacy gate |
| Location precision | Off, Coarse, Precise | Off | Privacy gate `allow_precise_coordinates`, true only for Precise |

The link range and default come from aoNetwork's `NetworkConfig`. The store
bounds keep the carried store meaningful on a phone while capping metadata;
the 8 KiB per-entry budget and 1024-entry cap match aoMesh's default entry
count. Battery intervals are bounded by aoNetwork's validators and are
documented mapping choices, not measured battery claims; APP-17 and AND-19 own
that evidence. Location precision defaults to Off so exact coordinates are
never enabled by an accepted default.

Runtime values use APP-08's documented defaults (TTL 8, lifetime 3600 s,
event capacity 64 at Balanced). `RuntimeSettings` is a plain data seam with the
same bounds; the host will construct APP-08's `RuntimeConfig` from it when that
PR merges.

## Consequences

- Out-of-range values return `SettingsError` whose `Display` text names the
  value and valid range; the stored snapshot does not change.
- Each setting has a test that changes it and inspects the resulting mapped
  config or privacy gate.
- Relay settings currently change the advertised beacon flag and the runtime
  seam only. aoMesh v0.1.0 has no seam to stop onward relays while preserving
  local submissions; [aoMesh #93](https://github.com/aoheyra/aoMesh/issues/93)
  tracks that sibling change. This PR does not claim enforced relay behaviour.
- Presence and precise-location gates are app-level inputs. Downstream services
  must consume them; mapping them does not by itself send or withhold data.
- Store quota mapping cannot be simulated in this repo until APP-17 lands; the
  test asserts the concrete `StoreLimits` values that aoMesh uses for admission
  and eviction.
