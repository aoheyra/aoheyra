# Storage (APP-04 and APP-05)

All app data lives in one SQLite database owned by the Rust core. Kotlin has no
Room database and no authoritative cache (ADR 0001). The file is encrypted with
SQLCipher; the raw 256-bit key is supplied by Kotlin from the Android Keystore
through the `SecretVault` callback (APP-06/AND-13).

## Encryption and connections

| Setting | Value | Reason |
|---|---|---|
| Library | `rusqlite` 0.42 with `bundled-sqlcipher-vendored-openssl` | One SQLCipher build for host tests and every Android ABI; no host `libcrypto` dependency at runtime. The bundled SQLite clears RUSTSEC-2026-0459; issue #62 tracks the remaining licence allow-list decision. |
| Key | 32 raw bytes, hex encoded into SQLCipher's `"x'...'"` raw-key pragma | Fixed-length key from the Keystore; the hex form cannot inject SQL. |
| Cipher format | `PRAGMA cipher_compatibility = 4` pinned on every connection | The on-disk format does not change silently with a dependency upgrade. |
| Key check | `PRAGMA cipher_version` is present, then `SELECT count(*) FROM sqlite_master` | Rejects a plain SQLite build and turns a wrong key into a typed error at open time. |
| Journal | WAL, `synchronous = NORMAL` | Readers do not block the writer; bounded durability loss on power failure is acceptable for mesh data. |
| Writers | One thread owns the only read-write connection; bounded queue of 8 jobs | SQLite has one writer; serialising in Rust removes lock retries. |
| Readers | Pool of four read-only connections, `query_only = ON` | Queries can run concurrently without a write handle. |
| Other pragmas | `foreign_keys = ON`, `secure_delete = ON`, `temp_store = MEMORY`, `wal_autocheckpoint = 1000` | Referential integrity, no plaintext residue in deleted pages, bounded WAL size. |
| Backup | `allowBackup=false`, `fullBackupContent=false`, `dataExtractionRules` excludes every domain | A copied encrypted file without its Keystore key is useless; a backup with the key would defeat encryption. |

`DatabaseKey` wipes its bytes on drop and redacts itself in `Debug`. Keys are
never logged.

## Schema

`PRAGMA user_version` is the migration counter. Each migration runs in one
`BEGIN IMMEDIATE` transaction, so an interrupted upgrade leaves the old schema
intact.

| Version | Change |
|---|---|
| 0 | Fresh file; no tables. |
| 1 | `identity`, `contacts`, `conversations`, `messages`, `mesh_store`, `directory_cache`, `settings`. |
| 2 | Indexes for `mesh_store(dst)`, expiry, size, origin/class quota scopes, `directory_cache(expiry)` and conversation ordering. |

Node IDs and message IDs are stored as 16-byte blobs; opaque payloads are
blobs. The app never interprets payload bytes here (APP-07 owns that schema).
`mesh_store` mirrors aoMesh's `StoredMessage`: the complete wire packet plus
local metadata (`copies_left`, `first_seen`, `received_at`, `priority`,
`received_from`, `age_ms`) and derived `expires_at_ms` for single-statement
expiry and quota queries.

## Mesh store (APP-05)

`SqliteMessageStore` implements aoMesh's `MessageStore` over the `mesh_store`
table. It owns no second copy of packet bytes: lookups decode one row into an
owned `StoredMessage`, and writes run on the single writer thread in one
transaction. Live-row checks use `expires_at_ms > now`. Backend failures are
mapped to `StoreError::Backend` with a typed operation and SQLite code; the
infallible trait methods record the same failure on
`SqliteMessageStore::last_backend_error` rather than hiding it.

Local times and ages are encoded by flipping the sign bit before they are
written to SQLite `INTEGER` columns. This preserves `u64` order and keeps the
expiry and eviction indexes usable across the full range instead of overflowing
at `i64::MAX`.

Quota checks are single SQL statements: `SUM(size)` for total, per-origin and
per-class bytes, and `COUNT(*)` for entries. Rows are never loaded just to
count them. Eviction follows aoMesh's in-memory policy: expired entries first,
then the first over-quota scope in origin, class, global order. Victims are
ordered by `copies_left` ascending, `priority` descending, `first_seen` and
`msg_id` ascending. Large stores delete up to 512 ordered victims per
statement, so a 100k-row store does not issue 99k one-row deletes.

The shared aoMesh conformance suite runs against this backend. The 100k-row
eviction test seeds 100,000 live rows, inserts one more entry with a 1,000-entry
limit, expects 99,001 deterministic removals and asserts a 60-second debug
bound.

## Verification

`cargo test` covers:

- migrating an empty file to v2 and checking every table and index;
- migrating a seeded v1 file to v2 and checking that all seven tables keep
  their rows;
- rejecting a schema version newer than this binary;
- encrypted header bytes (not the `SQLite format 3` magic), rejection without
  a key and with a wrong key, successful reopen with the right key, and a
  non-empty `cipher_version` on an open connection;
- WAL mode, concurrent writers through the single writer task and visibility
  of commits to readers;
- invalid key lengths and configuration, neither of which creates a file.

## SQLCipher integration options considered

Build measurements were taken on a 12-core x86_64 Linux host with empty target
directories. On Rust 1.85.1, a minimal crate that depends only on `rusqlite`
built in 15.7 s unencrypted and 385.5 s with the 0.38 SQLCipher feature. After
the upgrade to `rusqlite` 0.42 on Rust 1.89.0, the full workspace release build
took 537.0 s.

| Option | Correctness / portability | Licence | Build and runtime impact |
|---|---|---|---|
| `rusqlite` + `sqlcipher` (link system SQLCipher) | Needs a matching SQLCipher on every host and each Android ABI. `pkg-config --exists sqlcipher` failed on the development host, so this is not portable without an unmanaged native library. | SQLCipher BSD-style. | Small build, but an unmanaged native dependency and ABI risk. Not measured here because the native library is absent. |
| `rusqlite` + `bundled-sqlcipher` | Compiles SQLCipher from source but links a system `libcrypto`; cross-compiling needs a matching crypto library per ABI. | MIT (rusqlite), BSD-style (SQLCipher), OpenSSL/LibreSSL system licence. | Avoids vendoring crypto but adds a per-ABI system dependency; not selected. |
| `rusqlite` 0.38 + `bundled-sqlcipher-vendored-openssl` (superseded draft) | Identical cipher build on host and device; crypto linked statically. | MIT, BSD-style and Apache-2.0 (OpenSSL 3.x); all allowed by `deny.toml`. | Cold release build measured at 385.5 s for the minimal crate, versus 15.7 s for plain `rusqlite` 0.38. Larger native library, no runtime system dependency. Superseded because SQLite is affected by RUSTSEC-2026-0459. |
| `rusqlite` 0.42 + `bundled-sqlcipher-vendored-openssl` (selected) | Uses `libsqlite3-sys` 0.40 and `sqlcipher-src` 420.0.0 (SQLCipher 4.20.0 / SQLite 3.54.1), which carries the patched SQLite. Needs Rust 1.89, provided by the separate MSRV change. | MIT, BSD-style, SQLite Blessing, WTFPL and Apache-2.0 (OpenSSL 3.x). The last two are not in the current `deny.toml` allow-list; issue #62 tracks that separate decision. | Full workspace cold release build measured at 537.0 s on Rust 1.89.0. No runtime system dependency; `cargo deny check advisories` passes. |

The selected option keeps CI and all three Android ABIs on one cipher format
without vendoring a native `.so` or adding an unmanaged system package. The
advisory is clear: `cargo deny check advisories` passes and all nine storage
tests pass on Rust 1.89.0. The full `cargo deny check` still fails only on
licences: `sqlcipher-src` declares `blessing` and `WTFPL`, which are not in the
existing allow-list. This change deliberately does not edit `deny.toml`; issue
#62 tracks the separate allow-list decision. The draft must remain open until
that change lands and the full `cargo deny check` passes.
