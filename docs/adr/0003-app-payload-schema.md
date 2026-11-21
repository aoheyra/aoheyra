# ADR 0003: The app payload schema lives here and is versioned

Status: accepted (APP-01, M0).

## Context

aoMesh carries opaque bytes plus a `PayloadClass`; it must not learn text,
location, profile or receipt fields. The application needs a compact wire
format that can evolve without breaking older phones.

Options considered:

1. Let aoMesh define typed payloads and encode them in the routing crate.
2. Keep a versioned `AppPayload` enum in aoheyra and pass the encoded bytes to
   aoMesh as an opaque payload.

Option 1 crosses a repository boundary and makes every app field a protocol
change in the router. Option 1 was rejected: it would make aoMesh interpret app
data. Option 2 keeps the router generic and lets aoheyra own compatibility.

## Decision

`AppPayload` is owned by aoheyra. It is a versioned compact binary enum with
`Text`, `Location`, `Profile` and `Receipt` variants. A version byte is part of
the encoding; unknown versions are rejected as typed errors. Location updates
carry quantized coordinates, accuracy, timestamp and optional heading, with
three explicit precision modes: precise, about 100 m and off. Size limits are
per variant. APP-07 owns the exact layout and its round-trip tests.

## Exact layout (APP-07)

A hand-rolled, big-endian layout was chosen over a generic serializer such as
`postcard`: the wire format is stable, the fixed prefix can be checked before
any payload allocation, and the std-only core needs no new dependency. A
serializer would still need the same caps and header checks around it.

Every payload starts with `ver = 1` (one byte) and `kind` (one byte).

| Variant | Kind | Layout | Encoded cap | aoMesh class |
|---|---|---|---|---|
| `Text` | 1 | `text_len:u16 BE`, UTF-8 text (1..=1024 bytes) | 1028 B | `Direct` |
| `Location` | 2 | `precision_flags`, `timestamp_s:u32 BE`, then coordinates and optional heading | 19 B | `Telemetry` |
| `Profile` | 3 | `flags`, `name_len:u8`, name (<=32 B), `bio_len:u8`, bio (<=160 B), optional 8-byte avatar hash | 205 B | `Presence` |
| `Receipt` | 4 | `status:u8`, 16-byte message id | 19 B | `Direct` |

The class mapping uses the four classes actually defined by aoMesh
(`PacketClass::Direct`, `Broadcast`, `Presence`, `Telemetry`). Text and receipts
are directed 1:1 traffic, including the acknowledgement travelling back to the
origin; locations are latest-wins telemetry that must not be stored; profiles
are public node state refreshed by presence gossip. Broadcast stays reserved
for channel content. All four caps are compile-time asserted to fit inside
aoMesh's `MAX_PAYLOAD` of 2048 bytes.

In `precision_flags`, bits 0..1 carry the precision (0 precise, 1 about 100 m,
2 off) and bit 7 signals a heading; other bits are rejected. Coordinates are
signed i32 values in 1e-6 degrees, accuracy is a u16 in metres and heading is
a u16 in tenths of a degree. This gives a 19-byte worst case: 3 bytes of
version/kind/flags, 4 bytes of Unix seconds, 8 bytes of coordinates, 2 bytes of
accuracy and 2 bytes of heading.

- **Precise:** 17 bytes without heading, 19 with heading.
- **Approx100m:** coordinates are rounded to a 0.001 degree grid (1000 units,
  about 111 m of latitude at the equator) in the constructor, so neither the
  in-memory value nor the wire bytes hold the precise input. Accuracy is at
  least 150 m. Decoding rejects off-grid coordinates rather than accepting a
  claimed-approximate payload that actually carries precise coordinates.
- **Off:** 7 bytes: version, kind, precision flags and timestamp only. No
  coordinates, accuracy or heading are encoded, and coordinate accessors
  return `None`.

## Consequences

- Adding a payload variant is an app change, not a mesh protocol change.
- aoMesh sees only opaque bytes and a payload class, as required by its ADRs.
- Precision policy is enforced before encryption or submission; privacy tests
  inspect outgoing payloads (APP-07, APP-12).
