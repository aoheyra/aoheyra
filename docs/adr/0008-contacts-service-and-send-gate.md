# ADR 0008: Contacts service wraps aoIdents and gates sending on key decisions

Status: accepted (APP-10, M3).

## Context

AND-12 needs add, verify, block and rename operations, a safety number for a
human to compare, and an exportable QR card. aoIdents already owns contact
cryptography: `ContactBook` verifies cards, detects key changes, applies signed
rotations and computes safety numbers. APP-06 will provide the SQLite
`ContactStore`, but that PR is not merged yet.

Two placements were considered:

1. Build the service directly on APP-06's SQLite store and import it now.
2. Make the service generic over aoIdents' `ContactStore` seam, using
   `MemoryContactStore` in tests and letting APP-06 inject SQLite later.

Option 1 cannot compile against `origin/master` and would bind contact policy
to one persistence backend. Option 2 keeps TOFU, key-change detection and
petname validation in aoIdents, keeps the service deterministic, and gives
APP-06 a single trait to implement.

For a changed key there are also two answers:

1. Auto-accept the newly presented keys as the new contact.
2. Raise an alert, leave the stored contact untouched, block sending, and let
   the user choose either a signed rotation statement or an explicit block.

Option 1 defeats trust on first use and makes a forged presentation silently
rewrite identity. Option 2 matches AGENTS.md: key changes never auto-accept
and never silently block. A signed rotation preserves Verified trust and the
local petname; a block keeps the refusal sticky so a deleted contact cannot
return as a fresh first use.

## Decision

`ContactsService<S: ContactStore>` wraps `ContactBook<S>`, a bounded set of
known node ids and a bounded map of pending key changes. `add_from_qr` parses
and verifies an `aoheyra:id:` card through aoIdents. `observe_claimed_card`
accepts a claimed node id plus a card; when aoIdents reports `KeyChanged`, the
service emits `KeyChangeAlert` once and records an injected-time decision.

`can_send` is the send gate: it refuses while a decision is pending, refuses a
blocked contact and refuses an unknown contact with typed errors. `verify` and
`rename` also refuse while a decision is pending. `accept_rotation` requires
the statement to chain the claimed identity to the exact offered identity from
the alert before it changes the book; `block_key_change` records the refusal.
No other path can clear a pending alert.

Safety numbers come from aoIdents' `safety_number` over the local and contact
key pairs, formatted as four groups of four uppercase hexadecimal digits.
`export_own_card` checks that the supplied identity is this service's local
identity, then uses `Identity::card` and `IdentityCard::to_qr_string`.

## Consequences

- A changed key raises one alert event and `can_send` blocks until the user
  accepts a signed rotation or blocks the identity; the stored keys do not
  change before that decision.
- A repeat of the same presentation does not raise duplicate alerts, and the
  pending table is capped at aoIdents' `MAX_CONTACTS`.
- Blocking is sticky and verified trust follows a signed rotation, inheriting
  aoIdents' behaviour rather than reimplementing it.
- The service compiles and passes tests while APP-06 is unmerged. APP-06 will
  inject a SQLite `ContactStore` without changing these APIs.
- QR onboarding is self-consistent because the QR text carries only a card;
  key-change detection belongs to presentation paths that carry both a claimed
  identity and a card.
