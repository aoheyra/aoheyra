# ADR 0010: Location share sessions and the privacy boundary

Status: Accepted (APP-12)

## Context

APP-12 owns location share sessions. The Android location provider (AND-16),
share-location screen (AND-18) and map data service (APP-13) all need one place
that decides who receives a position, for how long and at what precision.
APP-07 already owns the wire schema: `LocationUpdate::new` quantizes
`Approx100m` and removes coordinates entirely for `Off`.

The service must not read a clock, use global randomness, open sockets, persist
state or interpret map data. It receives `now_ms` and an injected CSPRNG, and
returns prepared packets plus events.

## Decision

`LocationService<R>` is a sans-IO owner of bounded session state.

- A `SessionConfig` carries chosen contacts, start time, duration, publish
  interval, precision and an initial sample. Invalid values are rejected with a
  named field; all maps and vectors have hard caps.
- `start_session` emits one `SessionStarted` event and one sealed
  `RecipientUpdate` per chosen contact.
- `next_publish` returns one due local telemetry plan. The host calls
  `mark_published` after submission so the sequence advances once.
- `stop_session` removes the session before returning, emits `SessionStopped`
  and sealed stop notices to every chosen contact. Publishing from that
  session is impossible after the call.
- `reap_expired` removes sessions at their absolute expiry time and emits the
  same sealed stop notices plus `SessionExpired`.
- `receive` authenticates a sealed envelope with aoIdents, checks the header
  message id, sender identity, sequence replay and session/sequence binding,
  then aggregates one latest position per `(session, sender)`.
- Sealed recipient updates use `SealedMessage` from aoIdents. The `message_id`
  associated data is derived from the session id, sequence, start time and
  recipient, so a ciphertext cannot be replayed under a different header.

Precision is applied at the share boundary: every outgoing location is created
with the session's `LocationPrecision` before APP-07 encoding. `Off` produces
the 7-byte payload with no coordinates; `Approx100m` produces only rounded
coordinates; `Precise` is the only mode that can carry the raw sample.

## Options considered

### Sealing scheme

1. aoIdents `SealedMessage` with injected CSPRNG. Already merged, offline,
   recipient-bound and signed by the sender. It has no forward secrecy against
   later recipient-key compromise (IDN-19).
2. aoIdents prekey sealed messages (IDN-15). Better forward secrecy, but not
   merged and would add prekey-fetch state to a service that is supposed to be
   seam-independent.

Option 1 was chosen for this PR. The lack of forward secrecy is documented in
the threat model and is not hidden here.

### Privacy enforcement point

1. Quantize in `LocationUpdate::new` before encoding, for both sealed updates
   and local publishes. One owner, testable by inspecting outgoing bytes.
2. Quantize in the Android provider before calling the service. The Rust core
   would still be able to leak a precise sample from another caller.

Option 1 was chosen. The Android provider may still coarsen earlier, but it
cannot be the only barrier.

### Receiving authority

1. Accept any envelope that authenticates to the local identity, with per
   `(session, sender)` replay tracking. This supports invited receivers that do
   not own a matching local session.
2. Require a local session with the same id and compare its policy. This
   rejects legitimate invitations when the receiver is not also sharing.

Option 1 was chosen. Authorization for *sending* remains the chosen-contact
list; receiving an authenticated update does not expose local data.

## Verification

- `precise_coordinates_are_only_encoded_for_precise_sessions` opens every
  sealed recipient update and decodes every local publish for `Off`,
  `Approx100m` and `Precise`, then inspects the exact outgoing payload bytes.
  It proves that the precise coordinate byte pattern is absent for the first
  two modes.
- `session_publishes_once_per_interval_and_stops_immediately` covers sequence,
  duplicate suppression and immediate revocation.
- `session_expires_and_notifies_recipients` covers self-expiry.
- `receive_authenticates_aggregates_and_rejects_replay` and
  `receive_rejects_wrong_sender_and_tampered_message_id` cover untrusted input.

## Consequences

- The service does not submit packets to aoMesh. APP-08 remains a draft, so the
  host must take the prepared `header` and payload and submit them itself. No
  fake runtime integration is claimed.
- aoMesh issue #90 still forces direct aoIdents/aoNetwork branch sources in
  `Cargo.toml` so there is one `Identity` type in the graph.
- The sealed share payload has a small private header (version, kind, session
  id, sequence, start/end) in addition to the APP-07 location bytes. It is a
  service-level framing inside the encrypted envelope, not a change to the
  APP-07 schema or the aoMesh wire format.
