# ADR 0001: Rust owns all persistence

Status: accepted (APP-01, M0).

## Context

The app needs identity material, contacts, conversations, messages, carried mesh
packets, a directory cache and settings to survive a restart. Kotlin and Rust
both have convenient storage APIs; two sources of truth would make restart,
encryption and conformance testing unreliable.

Options considered:

1. Kotlin Room/SharedPreferences as the source of truth, with Rust asking
   Kotlin for records through callbacks.
2. Rust SQLite (rusqlite with SQLCipher) as the single source of truth, with
   Kotlin reading through UniFFI.

Option 1 adds a second schema, a second encryption story and a cross-language
transaction boundary. Option 2 keeps migrations, quotas, conformance tests and
the database key in one place. It also lets aoMesh's shared `MessageStore`
suite run against the production implementation.

## Decision

The Rust core owns all persistence. Kotlin never writes its own database and
never keeps authoritative app state. Kotlin supplies the database key from the
Android Keystore through a `SecretVault` callback; the encrypted file and its
migrations live in Rust. The planned modules are `storage`, `storage/contacts`,
`storage/mesh_store` and `settings`.

## Consequences

- Restart behaviour is tested in Rust, including key failure from a mock vault.
- Android auto-backup must exclude the database file.
- Kotlin screens are projections of Rust state; they render what Rust reports.
- SQLite additions belong to APP-04 and APP-05; this ADR does not choose a
  schema or claim those issues are implemented.

APP-04 implements this decision with one SQLCipher database, a 32-byte key
supplied by Kotlin from the Keystore, WAL, versioned migrations, one writer
task and a bounded read-only connection pool. The file format and schema are
documented in `docs/storage.md`.
