//! Opening and keying individual SQLite connections.

use super::{BUSY_TIMEOUT, DatabaseKey, StorageError};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::Path;

/// Open the single read-write connection for the writer task.
pub(super) fn open_writer(path: &Path, key: &DatabaseKey) -> Result<Connection, StorageError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
    let connection = open(path, key, flags)?;

    // WAL lets readers query while the writer is active. A filesystem that
    // cannot provide WAL must fail loudly rather than silently use rollback
    // journal and stall the app.
    let mode: String = connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(|source| StorageError::Open {
            path: path.to_path_buf(),
            source,
        })?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(StorageError::JournalMode(mode));
    }

    connection.execute_batch(
        "PRAGMA synchronous = NORMAL;\n\
         PRAGMA foreign_keys = ON;\n\
         PRAGMA secure_delete = ON;\n\
         PRAGMA temp_store = MEMORY;\n\
         PRAGMA wal_autocheckpoint = 1000;",
    )?;
    Ok(connection)
}

/// Open one read-only connection for the query pool.
pub(super) fn open_reader(path: &Path, key: &DatabaseKey) -> Result<Connection, StorageError> {
    let connection = open(path, key, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.execute_batch("PRAGMA query_only = ON; PRAGMA foreign_keys = ON;")?;
    Ok(connection)
}

/// Open a connection, apply the key and prove that it decrypts the header.
fn open(path: &Path, key: &DatabaseKey, flags: OpenFlags) -> Result<Connection, StorageError> {
    let connection =
        Connection::open_with_flags(path, flags).map_err(|source| StorageError::Open {
            path: path.to_path_buf(),
            source,
        })?;
    connection.busy_timeout(BUSY_TIMEOUT)?;

    // SQLCipher requires the key before any other database access. Reading the
    // schema version here turns a wrong key into a typed error at open time,
    // not a confusing failure on the first query.
    connection
        .execute_batch(&key.pragma_statement())
        .map_err(|source| StorageError::Open {
            path: path.to_path_buf(),
            source,
        })?;
    // Pin the on-disk cipher format so a future SQLCipher upgrade cannot
    // silently change how existing files are opened.
    connection
        .execute_batch("PRAGMA cipher_compatibility = 4;")
        .map_err(|source| StorageError::Open {
            path: path.to_path_buf(),
            source,
        })?;
    // A plain SQLite build accepts `PRAGMA key` as a no-op and returns no row
    // for `cipher_version`. Fail before any schema or payload access if the
    // cipher is not linked. The schema read below proves that the key works.
    let cipher_version: Option<String> = connection
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .optional()
        .map_err(|source| StorageError::Open {
            path: path.to_path_buf(),
            source,
        })?;
    if cipher_version.is_none() {
        return Err(StorageError::EncryptionUnavailable);
    }
    connection
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|source| StorageError::Open {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(connection)
}
