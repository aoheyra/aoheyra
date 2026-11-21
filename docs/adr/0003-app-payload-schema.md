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

## Consequences

- Adding a payload variant is an app change, not a mesh protocol change.
- aoMesh sees only opaque bytes and a payload class, as required by its ADRs.
- Precision policy is enforced before encryption or submission; privacy tests
  inspect outgoing payloads (APP-07, APP-12).
