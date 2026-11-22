//! Versioned schema migrations for the encrypted database.
//!
//! `user_version` is the version counter. Each step runs in one transaction,
//! so a crash or a bad migration cannot leave a half-applied schema. v1 is the
//! original table set; v2 adds the indexes used by the mesh store's quota and
//! expiry queries (APP-05).

use super::StorageError;
use rusqlite::{Connection, TransactionBehavior};

/// Schema version written by this binary.
pub const SCHEMA_VERSION: u32 = 2;

/// v1: every table owned by aoheyra, keyed by opaque node IDs or message IDs.
const V1_SCHEMA: &str = "\
CREATE TABLE identity (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    node_id BLOB NOT NULL,
    seed BLOB NOT NULL,
    identity_card BLOB NOT NULL,
    created_at_ms INTEGER NOT NULL
);
CREATE TABLE contacts (
    node_id BLOB PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    identity_card BLOB NOT NULL,
    safety_number TEXT NOT NULL,
    verified INTEGER NOT NULL DEFAULT 0 CHECK (verified IN (0, 1)),
    blocked INTEGER NOT NULL DEFAULT 0 CHECK (blocked IN (0, 1)),
    key_changed INTEGER NOT NULL DEFAULT 0 CHECK (key_changed IN (0, 1)),
    added_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE conversations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    peer_node_id BLOB NOT NULL UNIQUE,
    last_message_at_ms INTEGER NOT NULL,
    unread_count INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE messages (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    sender_node_id BLOB NOT NULL,
    recipient_node_id BLOB NOT NULL,
    payload BLOB NOT NULL,
    status TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE mesh_store (
    msg_id BLOB PRIMARY KEY NOT NULL,
    origin BLOB NOT NULL,
    dst BLOB,
    class INTEGER NOT NULL,
    wire BLOB NOT NULL,
    size INTEGER NOT NULL CHECK (size > 0),
    copies_left INTEGER NOT NULL,
    first_seen_ms INTEGER NOT NULL,
    received_at_ms INTEGER NOT NULL,
    priority INTEGER NOT NULL,
    received_from BLOB,
    age_ms INTEGER NOT NULL,
    created_ms INTEGER NOT NULL,
    lifetime_s INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    payload_len INTEGER NOT NULL,
    ttl INTEGER NOT NULL,
    hops INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE directory_cache (
    node_id BLOB PRIMARY KEY NOT NULL,
    entry BLOB NOT NULL,
    expires_at_ms INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value BLOB NOT NULL
) WITHOUT ROWID;";

/// v2: indexes for the per-destination, expiry and quota access paths.
const V2_SCHEMA: &str = "\
CREATE INDEX IF NOT EXISTS idx_mesh_store_dst ON mesh_store(dst);
CREATE INDEX IF NOT EXISTS idx_mesh_store_expiry ON mesh_store(expires_at_ms);
CREATE INDEX IF NOT EXISTS idx_mesh_store_size ON mesh_store(size);
CREATE INDEX IF NOT EXISTS idx_mesh_store_origin_size ON mesh_store(origin, size);
CREATE INDEX IF NOT EXISTS idx_mesh_store_class_size ON mesh_store(class, size);
CREATE INDEX IF NOT EXISTS idx_directory_cache_expiry ON directory_cache(expires_at_ms);
CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id, created_at_ms);";

/// Read the current `user_version`.
pub(super) fn current_version(connection: &Connection) -> Result<u32, StorageError> {
    let raw: i32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    u32::try_from(raw).map_err(|_| StorageError::CorruptSchemaVersion(raw))
}

/// Migrate an open connection up to [`SCHEMA_VERSION`].
pub(super) fn run(connection: &mut Connection) -> Result<(), StorageError> {
    let version = current_version(connection)?;
    migrate_to(connection, version, SCHEMA_VERSION)
}

/// Apply one migration step atomically and advance `user_version`.
fn apply(connection: &mut Connection, version: u32, sql: &str) -> Result<(), StorageError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|source| StorageError::Migration { version, source })?;
    let result = transaction
        .execute_batch(sql)
        .and_then(|()| transaction.execute_batch(&format!("PRAGMA user_version = {version};")));
    match result {
        Ok(()) => transaction
            .commit()
            .map_err(|source| StorageError::Migration { version, source }),
        Err(source) => {
            // Dropping the uncommitted transaction rolls the step back.
            drop(transaction);
            Err(StorageError::Migration { version, source })
        }
    }
}

/// Migrate from `from` to `target`. Used by the previous-version tests.
#[cfg(test)]
pub(super) fn migrate_between(
    connection: &mut Connection,
    from: u32,
    target: u32,
) -> Result<(), StorageError> {
    migrate_to(connection, from, target)
}

fn migrate_to(
    connection: &mut Connection,
    mut version: u32,
    target: u32,
) -> Result<(), StorageError> {
    if version > SCHEMA_VERSION || target > SCHEMA_VERSION || version > target {
        return Err(StorageError::UnsupportedSchemaVersion(version));
    }
    while version < target {
        let next = version + 1;
        let sql = match next {
            1 => V1_SCHEMA,
            2 => V2_SCHEMA,
            _ => return Err(StorageError::UnsupportedSchemaVersion(next)),
        };
        apply(connection, next, sql)?;
        version = next;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Database, DatabaseKey, test_support::TempDb};

    fn key() -> DatabaseKey {
        DatabaseKey::new(&[0x42; 32]).unwrap_or_else(|e| panic!("test key: {e}"))
    }

    fn raw_connection(db: &TempDb) -> Connection {
        let connection = Connection::open(db.path()).unwrap_or_else(|e| panic!("open: {e}"));
        connection
            .execute_batch(
                "PRAGMA key = \"x'4242424242424242424242424242424242424242424242424242424242424242'\";",
            )
            .unwrap_or_else(|e| panic!("key: {e}"));
        connection
    }

    #[test]
    fn migrate_from_empty_creates_all_tables_and_indexes() {
        let db = TempDb::new();
        let connection = raw_connection(&db);
        let mut connection = connection;
        migrate_between(&mut connection, 0, SCHEMA_VERSION).unwrap();

        assert_eq!(current_version(&connection).unwrap(), SCHEMA_VERSION);
        let expected_tables = [
            "identity",
            "contacts",
            "conversations",
            "messages",
            "mesh_store",
            "directory_cache",
            "settings",
        ];
        for table in expected_tables {
            let found: i64 = connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(found, 1, "missing table {table}");
        }
        let indexes: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexes, 7);
    }

    #[test]
    fn migrate_from_previous_version_preserves_rows() {
        let db = TempDb::new();
        let mut connection = raw_connection(&db);
        migrate_between(&mut connection, 0, 1).unwrap();
        connection
            .execute_batch(
                "INSERT INTO identity VALUES (1, x'01', x'02', x'03', 7);\n\
                 INSERT INTO contacts (node_id, display_name, identity_card, safety_number, added_at_ms, updated_at_ms)\n\
                 VALUES (x'0a', 'Ada', x'04', '1234', 1, 2);\n\
                 INSERT INTO conversations (id, peer_node_id, last_message_at_ms)\n\
                 VALUES (1, x'0a', 3);\n\
                 INSERT INTO messages (id, conversation_id, sender_node_id, recipient_node_id, payload, status, created_at_ms, updated_at_ms)\n\
                 VALUES ('m1', 1, x'0a', x'0b', x'05', 'Carried', 4, 5);\n\
                 INSERT INTO mesh_store (msg_id, origin, class, wire, size, copies_left, first_seen_ms, received_at_ms, priority, age_ms, created_ms, lifetime_s, expires_at_ms, payload_len, ttl, hops)\n\
                 VALUES (x'06', x'0c', 1, x'07', 1, 2, 1, 1, 0, 0, 0, 10, 10000, 0, 1, 0);\n\
                 INSERT INTO directory_cache VALUES (x'0d', x'08', 9);\n\
                 INSERT INTO settings VALUES ('relay', x'01');",
            )
            .unwrap();
        drop(connection);

        let database = Database::open(db.path(), key(), Default::default()).unwrap();
        assert_eq!(database.schema_version().unwrap(), SCHEMA_VERSION);
        let preserved: String = database
            .read(|connection| {
                let name: String = connection.query_row(
                    "SELECT display_name FROM contacts WHERE node_id = x'0a'",
                    [],
                    |row| row.get(0),
                )?;
                let body: Vec<u8> = connection.query_row(
                    "SELECT payload FROM messages WHERE id = 'm1'",
                    [],
                    |row| row.get(0),
                )?;
                let setting: Vec<u8> = connection.query_row(
                    "SELECT value FROM settings WHERE key = 'relay'",
                    [],
                    |row| row.get(0),
                )?;
                Ok(format!("{name}:{:?}:{}", body, setting.len()))
            })
            .unwrap();
        assert_eq!(preserved, "Ada:[5]:1");
        let indexes: i64 = database
            .read(|connection| {
                Ok(connection.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%'",
                    [],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(indexes, 7);
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let db = TempDb::new();
        let connection = raw_connection(&db);
        let mut connection = connection;
        migrate_between(&mut connection, 0, SCHEMA_VERSION).unwrap();
        connection
            .execute_batch(&format!("PRAGMA user_version = {};", SCHEMA_VERSION + 1))
            .unwrap();
        assert!(matches!(
            run(&mut connection),
            Err(StorageError::UnsupportedSchemaVersion(_))
        ));
    }
}
