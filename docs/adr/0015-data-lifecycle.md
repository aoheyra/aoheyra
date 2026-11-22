# ADR 0015: Data lifecycle policy and seams (APP-16)

## Status

Accepted for `v0.1.0`.

## Context

APP-16 asks for retention settings, a panic wipe of keys and database,
encrypted backup and restore of identity and contacts, and vacuum. The
identity seams are real: `ao_idents::export_backup` / `import_backup` seal the
seed with Argon2id plus XChaCha20-Poly1305 (IDN-06), and `SealedMessage`
provides authenticated public-key encryption with a verified sender (IDN-03).
The workspace already owns one SQLCipher SQLite store (APP-04), so APP-16 must
not introduce a second persistence implementation.

Two options were considered for storage:

1. Add a second SQLite implementation here.
2. Define a small typed `LifecycleStore` trait and let the existing SQLite
   store own the SQL.

Option 1 is rejected: aoheyra already owns persistence, and two stores would
split quotas, migrations and vacuum semantics. The trait keeps one owner while
leaving the policy deterministic and testable without a database.

## Decision

`aoheyra::lifecycle` owns policy, not persistence:

- `RetentionPolicy` bounds age and entry count. `apply_retention` delegates to
  `LifecycleStore::retain` with injected time.
- `panic_wipe` wipes the store first, then asks `IdentityVault` to destroy the
  old key and install a fresh seed, then vacuums. A partial failure is an
  error. The order means that if keystore replacement fails, no old data is
  left readable.
- `LifecycleStore` and `IdentityVault` are app-owned traits with typed errors.
  The SQLite store and Android Keystore bridge are the intended implementers;
  no SQL or keystore code is duplicated here.
- Identity backup uses real `ao_idents::export_backup` / `import_backup`.
  Contacts are encoded into a bounded, versioned plaintext and sealed to the
  same identity with `SealedMessage::seal`. Restore calls `SealedMessage::open`
  and requires the authenticated sender to equal the restored identity before
  changing the vault.
- The bundle framing caps the identity blob at 512 bytes and the sealed
  contacts at 128 KiB before allocation. Contact labels are opaque bytes,
  capped at 128 bytes, with at most 256 contacts.

Nothing in this module logs a seed, a label, a node id or a message body.
`Debug` for `ContactRecord` and `BackupBundle` prints lengths only.

## Consequences

- A test fixture with a volatile store and keystore demonstrates the "Done
  when" line: after a wipe, a simulated restart has a different `NodeId` and
  none of the old message bytes remain.
- The existing SQLite store and Android Keystore bridge are the intended
  implementers of `LifecycleStore` and `IdentityVault`; policy code does not
  change when those seams are wired.
- The real keystore wipe and SQL `VACUUM` remain the host's responsibility.
  This module makes them explicit, typed operations rather than claiming a
  wipe it cannot perform without those seams.
- See `README.md` for the unmerged APP-04 dependency.
