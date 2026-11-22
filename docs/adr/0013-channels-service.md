# ADR 0013: Local public-channel service (APP-14)

## Status

Accepted for `v0.1.0`.

## Context

APP-14 asks the app to expose public channels with local history, a cap and
per-origin spam controls. aoMesh owns the signed channel envelope,
domain-separated id derivation and relay budgets (MSH-23). Those rules must
not be duplicated in aoheyra. The app still needs local state that a relay
deliberately does not keep: which names this user joined, an ordered history
for the UI, duplicate removal and mute/rate-limit policy.

Two implementation options were considered for the joined-name registry:

1. Wrap `ao_mesh::ChannelRules` directly.
2. Keep a small app-owned `BTreeMap<ChannelId, Vec<u8>>`, using aoMesh's public
   `derive_channel_id` and `ChannelError` types.

Option 1 was rejected: `ChannelRules::new` is `pub(crate)` in aoMesh, so the
app cannot construct one. A public constructor would need a new tagged aoMesh
release, which is outside this change. Option 2 uses only real public seams,
keeps the wire id derivation in its one owner and adds no new dependency. The
app registry is bounded by the same config values and uses the same fail-closed
collision behaviour as aoMesh's private registry.

## Decision

`aoheyra::channels::ChannelService` owns:

- a bounded join/leave name registry keyed by ids from
  `ao_mesh::derive_channel_id`;
- one bounded, arrival-ordered history across all channels;
- duplicate removal by verified `MessageId`;
- a bounded mute list and a bounded per-origin sliding-window counter.

aoMesh continues to own envelope encoding/decoding, id derivation and relay
budgets. The app calls `ChannelEnvelope::new` only as a convenience for
callers and tests; it never writes its own wire format. The service consumes
the opaque body after aoMesh has verified the signed Broadcast packet and
decoded the envelope.

Ingestion order is deliberate: subscription and size checks, then duplicate
removal, then mute and rate-limit checks. A replay is therefore a typed
`IngestOutcome::Duplicate` and cannot consume a sender's rate allowance.
History ordering follows local arrival (`now_ms` passed in by the caller),
never a remote timestamp, so a skewed sender clock cannot reorder the UI.

Origins are shared aoIdents `NodeId` values from verified packet headers. No
app-owned byte newtype is needed, and no identity logic is duplicated.

## Consequences

- The app can join/leave channels and show ordered, duplicate-free history
  without waiting for another tagged sibling release.
- Capacity is explicit: a full mute or sender table returns a typed error
  rather than silently evicting user policy.
- If aoMesh later exposes `ChannelRules::new`, this service can delegate the
  registry while keeping history and mute policy local.
