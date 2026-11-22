# ADR 0017: Jan signed discovery records

Status: accepted (JAN-01).

## Context

The app needs a way to find nearby people by a user-chosen username and a
locally generated ten-digit number without a central server. Every device owns
a key pair and a key-derived `NodeId`; that id is authoritative for
authentication and must never be replaced by a human-readable handle.

Options considered:

1. Put the username and number in the BLE advertisement and add a new GATT
   characteristic for discovery.
2. Add a new `AppPayload` variant and packet wire format for discovery.
3. Carry a signed discovery record in the existing `Profile` payload under the
   existing signed `Presence` packet class.

Option 1 leaks a personal handle to every scanner, has too little space for a
username and would duplicate identity code in Kotlin. Option 2 changes the
application wire format and needs a compatibility negotiation for no routing
benefit. Option 3 reuses the already signed, bounded, relayable presence path
and leaves aoMesh opaque. Option 3 was chosen.

## Decision

The networking engine is named **Jan**. A Jan discovery record consists of:

- a username chosen by the user, at most 32 UTF-8 bytes and without control
  characters;
- a random ten-digit number generated once on the device from its injected
  CSPRNG, using rejection sampling so no digit is favoured;
- the sender's key-derived `NodeId` from the signed packet header.

The record is encoded as an `AppPayload::Profile` with `display_name` set to the
username and `bio` set to `jan:v1:<ten digits>`. No avatar hash is used. It is
submitted only under `PacketClass::Presence`, never as a text or broadcast
payload. A device broadcasts the record only while the user has enabled the
"searchable via network" setting; turning it off stops future announcements and
does not require a revocation packet because the directory is ephemeral.

Receivers parse only profiles whose bio begins with `jan:v1:`. Malformed records
are ignored. Records are kept in a bounded directory keyed by `NodeId`
(default 256 entries, hard cap 4096) with oldest-first eviction. Search matches
by username (ASCII case-insensitive), by exact number, or by both. Repeated
numbers do not merge identities because the directory key remains the signed
`NodeId` and a two-field search can disambiguate.

The number is a discovery hint, not a phone number from the SIM and not a secret.
It is never used as an authentication factor, an address or a key.

## Consequences

- No new packet header, signature coverage or GATT characteristic is needed.
- aoMesh still sees opaque bytes and does not interpret the username or number.
- Discovery is authenticated by the existing Presence signature and replay
  controls; a forged username cannot be attributed to another `NodeId`.
- The directory is bounded and ephemeral, so memory use is fixed and a device
  leaving the mesh eventually disappears after stale records are removed.
- A ten-digit number alone has a birthday collision risk at large scale. The
  design accepts this because the `NodeId` is authoritative and the username is
  a second user-chosen field; neither collision can redirect a message.
