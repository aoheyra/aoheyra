# ADR 0014: Privacy-preserving diagnostics bundle (APP-15)

## Status

Accepted for `v0.1.0`.

## Context

Field debugging needs a shareable dump of link state, mesh counters, device
class and recent events. The same dump must never contain message text, keys,
coordinates or a device identifier. aoNetwork publishes `Snapshot` (NET-23)
and aoMesh publishes `MeshSnapshot` (MSH-30); both are plain observation
types. aoNetwork's `LinkSnapshot` includes a `PeerKey`, and `AppHello` carries
an arbitrary application blob, so serializing those types wholesale would be
unsafe.

Two archive options were considered:

1. Serialize sibling snapshot types directly with `serde`, enabling the
   optional `serde` feature on aoNetwork.
2. Map both snapshots into app-owned, allowlisted structs, then serialize
   those structs as deterministic JSON inside a framed byte archive.

Option 1 was rejected: `PeerKey` has a `Serialize` implementation that emits
hex, so a future field addition in a sibling could leak a key with no change
here. Option 2 makes the allowlist local and explicit. The app copies only
integers and closed enums; it deliberately never reads `link.peer`.

`serde` and `serde_json` are used for the archive itself. Both are mature and
permissively licensed; `serde` is already present in the pinned sibling
graph, and `serde_json` is already used by aoNetwork's own snapshot tests.
They replace hand-written JSON escaping, which would be harder to keep
correct if a field is added. The core remains sans-I/O: it returns bytes to
the caller and never opens a file or reads the clock.

## Decision

`aoheyra::diagnostics` defines:

- `DiagnosticLog`, a bounded ring of `LogEvent` values. Every event has a
  closed `EventCode`, an injected time, a count and an optional ordinal into
  the link table. There is no free-form message field.
- `NetworkReport`, `LinkRow` and `NetworkCounters`, mapped from
  `ao_network::Snapshot` without the `PeerKey`, `LinkId`, hello payloads or
  any radio identifier.
- `MeshCounters`, mapped from `ao_mesh::MeshSnapshot`. If aoMesh reports an
  unfamiliar snapshot version, known fields are zeroed and `recognized` is
  false rather than guessing a meaning.
- `DeviceInfo`, which carries only the crate version, platform class, an
  optional caller-supplied SDK level and a CPU class. No model, serial, IMEI,
  advertising name or coordinate is accepted.
- `DiagnosticsBundle`, whose `to_json()` and `to_archive()` are deterministic
  for a fixed set of inputs. The archive is `AOHDIAG1`, a version byte, a
  little-endian payload length and the JSON payload. No wall-clock timestamp
  or compression is added.

`MAX_ARCHIVE_LEN` bounds both generation and parsing. Link rows and log
events have injected caps. A full log drops the oldest event; a snapshot with
more links than the cap reports `links_omitted`.

## Consequences

- A reviewer can read the JSON inside the sample bundle and find only
  numeric counters and closed codes. The unit test drives a real two-node
  aoNetwork simulation with a known secret marker in the application hello
  and a known peer key, then asserts neither appears in the archive.
- Someone adding a field to a sibling snapshot cannot leak it here without
  also adding it to the app-owned allowlist and this ADR.
- The caller is responsible for asking the user before sharing the archive
  and for writing the returned bytes to storage; the core has no I/O.
- The archive is intentionally not encrypted. It contains no message content,
  but it is still user data and must be shared only with explicit consent.
