//! Encrypted, versioned SQLite storage owned by the Rust core (APP-04).
//!
//! One writer task owns the only read-write connection. Queries use separate
//! read-only connections. Every connection is opened with the key supplied by
//! Kotlin through the Android Keystore and is rejected before any schema or
//! payload access if that key does not open the file.
//!
//! The schema is defined by the private `migrations` module and uses
//! `user_version` as its version counter. APP-05 and APP-06 add their
//! behaviour on top of these tables; they do not create parallel state in
//! Kotlin.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

mod connection;
mod migrations;
mod reader;
mod writer;

pub use migrations::SCHEMA_VERSION;

/// Length of the raw AES key SQLCipher expects for a 256-bit key.
const DATABASE_KEY_LEN: usize = 32;
/// Default number of read-only connections in the pool.
const DEFAULT_READERS: usize = 4;
/// Default number of pending write requests before writers apply backpressure.
const DEFAULT_WRITE_QUEUE: usize = 8;
/// Busy timeout applied to every connection; writes are already serialised.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// A raw 256-bit database key obtained from the Kotlin `SecretVault`.
///
/// The bytes are wiped on drop. The key is never logged and is used only to
/// build the initial `PRAGMA key` statement for each SQLite connection.
pub struct DatabaseKey {
    bytes: [u8; DATABASE_KEY_LEN],
}

impl DatabaseKey {
    /// Validate the length of a key supplied by the platform.
    ///
    /// Returns a typed error instead of panicking: this value crosses UniFFI.
    pub fn new(bytes: &[u8]) -> Result<Self, StorageError> {
        if bytes.len() != DATABASE_KEY_LEN {
            return Err(StorageError::InvalidKeyLength {
                expected: DATABASE_KEY_LEN,
                actual: bytes.len(),
            });
        }
        let mut key = Self {
            bytes: [0; DATABASE_KEY_LEN],
        };
        key.bytes.copy_from_slice(bytes);
        Ok(key)
    }

    /// Hex literal for SQLCipher's raw-key form of `PRAGMA key`.
    ///
    /// SQLCipher does not accept bound parameters for `PRAGMA key`, so the key
    /// is hex-encoded (never interpolated as arbitrary text) to keep the
    /// statement injection-free.
    fn pragma_statement(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut hex = String::with_capacity(DATABASE_KEY_LEN * 4 + 16);
        // SQLCipher raw-key syntax wraps the blob literal in double quotes.
        hex.push_str("PRAGMA key = \"x'");
        for byte in self.bytes {
            hex.push(HEX[(byte >> 4) as usize] as char);
            hex.push(HEX[(byte & 0x0f) as usize] as char);
        }
        hex.push_str("'\";");
        hex
    }
}

impl Drop for DatabaseKey {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

impl fmt::Debug for DatabaseKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseKey([redacted])")
    }
}

/// Public storage configuration. Bounds are validated by [`Database::open`].
#[derive(Debug, Clone, Copy)]
pub struct DatabaseConfig {
    /// Number of read-only connections available for concurrent queries.
    pub readers: usize,
    /// Capacity of the bounded queue in front of the writer task.
    pub write_queue: usize,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            readers: DEFAULT_READERS,
            write_queue: DEFAULT_WRITE_QUEUE,
        }
    }
}

/// Typed storage failure. No key, path or message content appears in errors.
#[derive(Debug)]
pub enum StorageError {
    /// The platform supplied a key of the wrong length.
    InvalidKeyLength {
        /// Required number of raw key bytes.
        expected: usize,
        /// Number of bytes supplied.
        actual: usize,
    },
    /// A read/write or read-only connection could not be opened or keyed.
    Open {
        /// Database path that failed to open.
        path: PathBuf,
        /// Underlying SQLite error.
        source: rusqlite::Error,
    },
    /// A migration step failed; the transaction was rolled back.
    Migration {
        /// Schema version being applied.
        version: u32,
        /// Underlying SQLite error.
        source: rusqlite::Error,
    },
    /// The file advertises a schema newer than this binary understands.
    UnsupportedSchemaVersion(u32),
    /// The `user_version` header cannot be interpreted as a version.
    CorruptSchemaVersion(i32),
    /// A query or write failed at the SQLite layer.
    Sql(rusqlite::Error),
    /// A reader connection was poisoned by a panic in a previous query.
    ReaderPoisoned,
    /// The writer task could not be started.
    WriterStart(String),
    /// The writer task stopped before the reply arrived.
    WriterStopped,
    /// A caller's write closure panicked; the writer task remains usable.
    WriterPanic,
    /// Configuration requested no readers or no queue capacity.
    InvalidConfig(&'static str),
    /// SQLite could not switch the database to WAL mode.
    JournalMode(String),
    /// SQLCipher did not report an active, keyed cipher on the connection.
    EncryptionUnavailable,
    /// An internal reply from the writer had an unexpected type.
    InvalidWriterReply,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKeyLength { expected, actual } => {
                write!(f, "database key must be {expected} bytes, got {actual}")
            }
            Self::Open { path, source } => {
                write!(f, "cannot open database {}: {source}", path.display())
            }
            Self::Migration { version, source } => {
                write!(f, "schema migration to v{version} failed: {source}")
            }
            Self::UnsupportedSchemaVersion(version) => {
                write!(f, "unsupported database schema version {version}")
            }
            Self::CorruptSchemaVersion(raw) => {
                write!(f, "corrupt database schema version {raw}")
            }
            Self::Sql(source) => write!(f, "database operation failed: {source}"),
            Self::ReaderPoisoned => f.write_str("database reader connection was poisoned"),
            Self::WriterStart(reason) => write!(f, "cannot start database writer: {reason}"),
            Self::WriterStopped => f.write_str("database writer task is not running"),
            Self::WriterPanic => f.write_str("database write closure panicked"),
            Self::InvalidConfig(reason) => write!(f, "invalid storage configuration: {reason}"),
            Self::JournalMode(mode) => write!(f, "expected WAL journal mode, got {mode}"),
            Self::EncryptionUnavailable => {
                f.write_str("SQLCipher encryption is not active on the database connection")
            }
            Self::InvalidWriterReply => f.write_str("database writer returned an unexpected type"),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Open { source, .. } | Self::Migration { source, .. } | Self::Sql(source) => {
                Some(source)
            }
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(source: rusqlite::Error) -> Self {
        Self::Sql(source)
    }
}

/// An open encrypted database with a single writer and a pool of readers.
///
/// Cloning shares the same writer task and reader pool. Each write runs on the
/// writer task in submission order; queries run on the caller thread using a
/// read-only connection from the pool.
#[derive(Clone)]
pub struct Database {
    inner: Arc<DatabaseInner>,
}

struct DatabaseInner {
    path: PathBuf,
    writer: writer::WriterTask,
    readers: reader::ReadPool,
}

impl Database {
    /// Open (or create) the database, validate the key and run migrations.
    ///
    /// Migrations run on the writer task before any read connection is opened,
    /// so a reader can never observe a partially migrated schema.
    pub fn open(
        path: impl AsRef<Path>,
        key: DatabaseKey,
        config: DatabaseConfig,
    ) -> Result<Self, StorageError> {
        if config.readers == 0 {
            return Err(StorageError::InvalidConfig("readers must be non-zero"));
        }
        if config.write_queue == 0 {
            return Err(StorageError::InvalidConfig("write_queue must be non-zero"));
        }
        let path = path.as_ref().to_path_buf();
        let writer = writer::WriterTask::start(&path, &key, config.write_queue)?;
        // A failed migration must also stop the writer task gracefully.
        if let Err(error) = writer.write(migrations::run) {
            drop(writer);
            return Err(error);
        }
        let readers = reader::ReadPool::open(&path, &key, config.readers)?;
        Ok(Self {
            inner: Arc::new(DatabaseInner {
                path,
                writer,
                readers,
            }),
        })
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Run a closure on the single writer connection.
    ///
    /// The closure must own any data it captures because it runs on another
    /// thread. Writes are applied in submission order; the bounded queue
    /// applies backpressure instead of allocating without limit.
    pub fn write<T, F>(&self, job: F) -> Result<T, StorageError>
    where
        T: Send + 'static,
        F: FnOnce(&mut rusqlite::Connection) -> Result<T, StorageError> + Send + 'static,
    {
        self.inner.writer.write(job)
    }

    /// Run a closure on a read-only connection from the pool.
    ///
    /// The closure runs on the caller thread and can borrow local data. It
    /// cannot write: the connection is opened with `SQLITE_OPEN_READ_ONLY`
    /// and `PRAGMA query_only=ON`.
    pub fn read<T, F>(&self, job: F) -> Result<T, StorageError>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<T, StorageError>,
    {
        self.inner.readers.with_connection(job)
    }

    /// Current `user_version`, exposed for diagnostics and restart checks.
    pub fn schema_version(&self) -> Result<u32, StorageError> {
        self.read(migrations::current_version)
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DB: AtomicU64 = AtomicU64::new(0);

    /// A uniquely named database file below `TMPDIR`, removed on drop.
    pub(super) struct TempDb {
        path: PathBuf,
    }

    impl TempDb {
        pub(super) fn new() -> Self {
            let sequence = NEXT_DB.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "aoheyra-storage-{}-{sequence}.db",
                std::process::id()
            ));
            Self { path }
        }

        pub(super) fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm", "-journal"] {
                let mut name = self.path.clone().into_os_string();
                name.push(suffix);
                let _ = std::fs::remove_file(PathBuf::from(name));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TempDb;
    use super::{Database, DatabaseConfig, DatabaseKey, SCHEMA_VERSION, StorageError};
    use rusqlite::OptionalExtension;
    use std::sync::Arc;
    use std::thread;

    fn key(byte: u8) -> DatabaseKey {
        DatabaseKey::new(&[byte; 32]).unwrap_or_else(|e| panic!("test key: {e}"))
    }

    #[test]
    fn key_length_is_validated_and_never_printed() {
        assert!(matches!(
            DatabaseKey::new(&[0; 31]),
            Err(StorageError::InvalidKeyLength {
                expected: 32,
                actual: 31
            })
        ));
        let secret = key(0xa5);
        let printed = format!("{secret:?}");
        assert!(!printed.contains("a5a5"));
        assert!(printed.contains("redacted"));
    }

    #[test]
    fn open_from_empty_runs_all_migrations_and_enables_wal() {
        let db = TempDb::new();
        let database = Database::open(db.path(), key(1), DatabaseConfig::default()).unwrap();
        assert_eq!(database.schema_version().unwrap(), SCHEMA_VERSION);
        let journal: String = database
            .read(|connection| {
                Ok(connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?)
            })
            .unwrap();
        assert_eq!(journal, "wal");
        let cipher_version: Option<String> = database
            .read(|connection| {
                Ok(connection
                    .query_row("PRAGMA cipher_version", [], |row| row.get(0))
                    .optional()?)
            })
            .unwrap();
        assert!(cipher_version.is_some_and(|version| !version.is_empty()));
        assert_eq!(database.path(), db.path());
    }

    #[test]
    fn file_is_unreadable_without_the_key_or_with_a_wrong_key() {
        let db = TempDb::new();
        let database = Database::open(db.path(), key(2), DatabaseConfig::default()).unwrap();
        database
            .write(|connection| {
                connection.execute_batch("INSERT INTO settings VALUES ('probe', x'01');")?;
                Ok(())
            })
            .unwrap();
        drop(database);

        let header = std::fs::read(db.path()).unwrap();
        assert_ne!(&header[..16], b"SQLite format 3\0");

        let without_key = rusqlite::Connection::open(db.path()).unwrap();
        assert!(
            without_key
                .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
                    row.get::<_, i64>(0)
                })
                .is_err()
        );
        drop(without_key);

        assert!(matches!(
            Database::open(db.path(), key(3), DatabaseConfig::default()),
            Err(StorageError::Open { .. })
        ));
        // The right key still opens the same file after both failures.
        let reopened = Database::open(db.path(), key(2), DatabaseConfig::default()).unwrap();
        let rows: i64 = reopened
            .read(|connection| {
                Ok(connection.query_row("SELECT count(*) FROM settings", [], |row| row.get(0))?)
            })
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn writer_serialises_concurrent_clients_and_readers_see_commits() {
        let db = TempDb::new();
        let database =
            Arc::new(Database::open(db.path(), key(4), DatabaseConfig::default()).unwrap());
        let mut handles = Vec::new();
        for value in 0..64u32 {
            let database = Arc::clone(&database);
            handles.push(thread::spawn(move || {
                database
                    .write(move |connection| {
                        connection.execute(
                            "INSERT INTO settings (key, value) VALUES (?1, ?2)",
                            rusqlite::params![format!("key-{value}"), value.to_le_bytes()],
                        )?;
                        Ok(())
                    })
                    .unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let rows: i64 = database
            .read(|connection| {
                Ok(connection.query_row("SELECT count(*) FROM settings", [], |row| row.get(0))?)
            })
            .unwrap();
        assert_eq!(rows, 64);
    }

    #[test]
    fn invalid_config_is_rejected_before_a_file_is_created() {
        let db = TempDb::new();
        assert!(matches!(
            Database::open(
                db.path(),
                key(5),
                DatabaseConfig {
                    readers: 0,
                    ..DatabaseConfig::default()
                }
            ),
            Err(StorageError::InvalidConfig(_))
        ));
        assert!(matches!(
            Database::open(
                db.path(),
                key(5),
                DatabaseConfig {
                    write_queue: 0,
                    ..DatabaseConfig::default()
                }
            ),
            Err(StorageError::InvalidConfig(_))
        ));
        assert!(!db.path().exists());
    }
}
