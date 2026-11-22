//! aoMesh `MessageStore` backed by the encrypted SQLite database (APP-05).
//!
//! The mesh store is a view over the `mesh_store` table created by APP-04.
//! It owns no second copy of packet bytes. Every lookup decodes one row into an
//! owned `StoredMessage`; every write runs on the database's single writer
//! thread inside one transaction. Quota checks are single SQL aggregate
//! statements, so a quota decision never loads rows into Rust just to count
//! them.

use super::{Database, StorageError};
use ao_mesh::expiry::LifetimePolicy;
use ao_mesh::packet::MessageId;
use ao_mesh::store::{
    MessageStore, NodeId, StoreBackendError, StoreBackendOperation, StoreError, StoreLimits,
    StoredMessage,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::sync::Mutex;

/// Number of victims selected and deleted in one eviction statement.
///
/// Large stores lose far more entries than one per quota check. A bounded
/// chunk keeps the 100k-row case fast without turning a small conformance case
/// into an over-eviction: the exact number of ordered victims is computed
/// before the delete.
const EVICTION_CHUNK: usize = 512;

const SELECT_LIVE_IDS: &str =
    "SELECT msg_id FROM mesh_store WHERE expires_at_ms > ?1 ORDER BY msg_id ASC";
const SELECT_DST_IDS: &str = "SELECT msg_id FROM mesh_store \
     WHERE expires_at_ms > ?1 AND dst = ?2 ORDER BY msg_id ASC";
const SELECT_EXPIRED_IDS: &str = "SELECT msg_id FROM mesh_store \
     WHERE expires_at_ms <= ?1 ORDER BY msg_id ASC";

const SELECT_VICTIMS_ALL: &str = "SELECT msg_id, size FROM mesh_store \
     ORDER BY copies_left ASC, priority DESC, first_seen_ms ASC, msg_id ASC";
const SELECT_VICTIMS_ORIGIN: &str = "SELECT msg_id, size FROM mesh_store WHERE origin = ?1 \
     ORDER BY copies_left ASC, priority DESC, first_seen_ms ASC, msg_id ASC";
const SELECT_VICTIMS_CLASS: &str = "SELECT msg_id, size FROM mesh_store WHERE class = ?1 \
     ORDER BY copies_left ASC, priority DESC, first_seen_ms ASC, msg_id ASC";
const DELETE_VICTIMS_ALL: &str = "DELETE FROM mesh_store WHERE msg_id IN \
     (SELECT msg_id FROM mesh_store \
      ORDER BY copies_left ASC, priority DESC, first_seen_ms ASC, msg_id ASC LIMIT ?1)";
const DELETE_VICTIMS_ORIGIN: &str = "DELETE FROM mesh_store WHERE msg_id IN \
     (SELECT msg_id FROM mesh_store WHERE origin = ?1 \
      ORDER BY copies_left ASC, priority DESC, first_seen_ms ASC, msg_id ASC LIMIT ?2)";
const DELETE_VICTIMS_CLASS: &str = "DELETE FROM mesh_store WHERE msg_id IN \
     (SELECT msg_id FROM mesh_store WHERE class = ?1 \
      ORDER BY copies_left ASC, priority DESC, first_seen_ms ASC, msg_id ASC LIMIT ?2)";

/// Preserve `u64` order in SQLite's signed `INTEGER` columns.
///
/// Flipping the sign bit maps the full `u64` range onto `i64` in increasing
/// order, so index comparisons such as `expires_at_ms > now` remain correct at
/// `u64::MAX` instead of overflowing.
fn encode_u64(value: u64) -> i64 {
    (value ^ (1_u64 << 63)) as i64
}

/// Inverse of [`encode_u64`].
fn decode_u64(value: i64) -> u64 {
    (value as u64) ^ (1_u64 << 63)
}

/// Which deterministic eviction scope is currently over quota.
#[derive(Debug, Clone, Copy)]
enum Scope<'a> {
    /// Global entry or byte quota.
    All,
    /// Per-origin byte quota for this origin.
    Origin(&'a [u8]),
    /// Per-class byte quota for this class.
    Class(i64),
}

/// SQLite implementation of aoMesh's durable packet store.
///
/// Cloning shares the same database handles and limits. The store has no
/// background caches and never holds a key: keying and encryption stay inside
/// [`Database`]. Backend failures that cannot be returned through a trait
/// method are recorded and exposed by [`Self::last_backend_error`].
pub struct SqliteMessageStore {
    database: Database,
    limits: StoreLimits,
    lifetime_policy: LifetimePolicy,
    last_backend_error: Mutex<Option<StoreBackendError>>,
}

impl SqliteMessageStore {
    /// Build a store over an already migrated database with default lifetimes.
    pub fn new(database: Database, limits: StoreLimits) -> Self {
        Self::with_policy(database, limits, LifetimePolicy::default())
    }

    /// Build a store with explicit local lifetime ceilings.
    pub fn with_policy(
        database: Database,
        limits: StoreLimits,
        lifetime_policy: LifetimePolicy,
    ) -> Self {
        Self {
            database,
            limits,
            lifetime_policy,
            last_backend_error: Mutex::new(None),
        }
    }

    /// Shared handle to the underlying encrypted database.
    pub fn database(&self) -> &Database {
        &self.database
    }

    /// Most recent backend failure observed on an infallible trait method.
    ///
    /// `MessageStore::get`, `by_destination`, `summary`, `expire` and `evict`
    /// cannot return an error. They return the empty result and record the
    /// typed backend failure here instead of hiding it. Fallible `put` also
    /// records the failure it returns.
    pub fn last_backend_error(&self) -> Option<StoreBackendError> {
        let slot = self
            .last_backend_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot
    }

    fn record(&self, operation: StoreBackendOperation, source: &StorageError) {
        let error = backend_error(operation, source);
        let mut slot = self
            .last_backend_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(error);
    }

    fn fail(&self, operation: StoreBackendOperation, source: StorageError) -> StoreError {
        self.record(operation, &source);
        StoreError::Backend(backend_error(operation, &source))
    }

    fn read_or<F, T>(&self, operation: StoreBackendOperation, job: F) -> Option<T>
    where
        F: FnOnce(&Connection) -> Result<T, StorageError>,
    {
        match self.database.read(job) {
            Ok(value) => Some(value),
            Err(source) => {
                self.record(operation, &source);
                None
            }
        }
    }

    fn write_or<F, T>(&self, operation: StoreBackendOperation, job: F) -> Option<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StorageError> + Send + 'static,
    {
        match self.database.write(job) {
            Ok(value) => Some(value),
            Err(source) => {
                self.record(operation, &source);
                None
            }
        }
    }

    fn query_ids(&self, sql: &str, now: u64, dst: Option<&NodeId>) -> Vec<MessageId> {
        let result = self.read_or(StoreBackendOperation::Query, |connection| {
            let mut statement = connection.prepare(sql)?;
            let rows = match dst {
                Some(dst) => statement.query_map(params![encode_u64(now), &dst[..]], map_id)?,
                None => statement.query_map(params![encode_u64(now)], map_id)?,
            };
            let mut ids = Vec::new();
            for row in rows {
                ids.push(decode_id(&row?)?);
            }
            Ok(ids)
        });
        result.unwrap_or_default()
    }
}

fn backend_error(operation: StoreBackendOperation, source: &StorageError) -> StoreBackendError {
    let code = match source {
        StorageError::Sql(rusqlite::Error::SqliteFailure(error, _)) => Some(error.extended_code),
        _ => None,
    };
    StoreBackendError::new(operation, code)
}

fn decode_id(bytes: &[u8]) -> Result<MessageId, StorageError> {
    let array: [u8; 16] = bytes.try_into().map_err(|_| StorageError::InvalidNodeId)?;
    Ok(MessageId::from_bytes(array))
}

fn map_id(row: &rusqlite::Row<'_>) -> Result<Vec<u8>, rusqlite::Error> {
    row.get(0)
}

fn map_victim(row: &rusqlite::Row<'_>) -> Result<(Vec<u8>, i64), rusqlite::Error> {
    Ok((row.get(0)?, row.get(1)?))
}

fn decode_node(bytes: Option<Vec<u8>>) -> Result<Option<NodeId>, StorageError> {
    match bytes {
        None => Ok(None),
        Some(bytes) => {
            let array: [u8; 16] = bytes.try_into().map_err(|_| StorageError::InvalidNodeId)?;
            Ok(Some(array))
        }
    }
}

/// Reconstruct one owned entry from a row. The wire bytes remain authoritative
/// for the header; the indexed columns only supply local metadata.
fn decode_row(row: &rusqlite::Row<'_>) -> Result<StoredMessage, StorageError> {
    let wire: Vec<u8> = row.get(0)?;
    let metadata = ao_mesh::store::StoreMetadata {
        copies_left: u8::try_from(row.get::<_, i64>(1)?)
            .map_err(|_| StorageError::InvalidCopiesLeft)?,
        first_seen: decode_u64(row.get(2)?),
        received_at: decode_u64(row.get(3)?),
        priority: u8::try_from(row.get::<_, i64>(4)?).map_err(|_| StorageError::InvalidPriority)?,
        received_from: decode_node(row.get(5)?)?,
        age_ms: decode_u64(row.get(6)?),
    };
    StoredMessage::new(&wire, metadata).map_err(|_| StorageError::InvalidStoredMessage)
}

/// Adapter for `rusqlite::query_row`, whose callback error type is fixed.
fn decode_row_rusqlite(row: &rusqlite::Row<'_>) -> Result<StoredMessage, rusqlite::Error> {
    decode_row(row).map_err(|source| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(source))
    })
}

fn counters(
    transaction: &Transaction<'_>,
    origin: &[u8],
    class: i64,
) -> Result<(u128, u128, u128, u128), StorageError> {
    let total: i64 =
        transaction.query_row("SELECT COALESCE(SUM(size), 0) FROM mesh_store", [], |row| {
            row.get(0)
        })?;
    let origin_bytes: i64 = transaction.query_row(
        "SELECT COALESCE(SUM(size), 0) FROM mesh_store WHERE origin = ?1",
        params![origin],
        |row| row.get(0),
    )?;
    let class_bytes: i64 = transaction.query_row(
        "SELECT COALESCE(SUM(size), 0) FROM mesh_store WHERE class = ?1",
        params![class],
        |row| row.get(0),
    )?;
    let count: i64 =
        transaction.query_row("SELECT COUNT(*) FROM mesh_store", [], |row| row.get(0))?;
    let saturate = |value: i64| u128::from(u64::try_from(value).unwrap_or(u64::MAX));
    Ok((
        saturate(total),
        saturate(origin_bytes),
        saturate(class_bytes),
        saturate(count),
    ))
}

/// Choose the smallest ordered prefix that brings the selected scope under its
/// target. At most `EVICTION_CHUNK` candidates are returned per call, so the
/// caller repeats for very large stores.
fn victims_for(
    transaction: &Transaction<'_>,
    scope: Scope<'_>,
    current_bytes: u128,
    target_bytes: u128,
    current_count: u128,
    target_count: Option<u128>,
) -> Result<Vec<MessageId>, StorageError> {
    let mut statement = match scope {
        Scope::All => transaction.prepare(SELECT_VICTIMS_ALL)?,
        Scope::Origin(_) => transaction.prepare(SELECT_VICTIMS_ORIGIN)?,
        Scope::Class(_) => transaction.prepare(SELECT_VICTIMS_CLASS)?,
    };
    let rows = match scope {
        Scope::All => statement.query_map([], map_victim)?,
        Scope::Origin(origin) => statement.query_map(params![origin], map_victim)?,
        Scope::Class(class) => statement.query_map(params![class], map_victim)?,
    };

    let mut ids = Vec::new();
    let mut removed_bytes = 0_u128;
    for row in rows {
        if ids.len() == EVICTION_CHUNK {
            break;
        }
        let (id, size) = row?;
        ids.push(decode_id(&id)?);
        removed_bytes =
            removed_bytes.saturating_add(u128::from(u64::try_from(size).unwrap_or(u64::MAX)));
        let count_ok = target_count
            .is_none_or(|target| current_count.saturating_sub(ids.len() as u128) <= target);
        if current_bytes.saturating_sub(removed_bytes) <= target_bytes && count_ok {
            break;
        }
    }
    Ok(ids)
}

fn delete_victims(
    transaction: &Transaction<'_>,
    scope: Scope<'_>,
    count: usize,
) -> Result<(), StorageError> {
    let count = count as i64;
    match scope {
        Scope::All => {
            transaction.execute(DELETE_VICTIMS_ALL, params![count])?;
        }
        Scope::Origin(origin) => {
            transaction.execute(DELETE_VICTIMS_ORIGIN, params![origin, count])?;
        }
        Scope::Class(class) => {
            transaction.execute(DELETE_VICTIMS_CLASS, params![class, count])?;
        }
    }
    Ok(())
}

fn expire_in(transaction: &Transaction<'_>, now: i64) -> Result<Vec<MessageId>, StorageError> {
    let mut statement = transaction.prepare(SELECT_EXPIRED_IDS)?;
    let rows = statement.query_map(params![now], map_id)?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(decode_id(&row?)?);
    }
    transaction.execute(
        "DELETE FROM mesh_store WHERE expires_at_ms <= ?1",
        params![now],
    )?;
    Ok(ids)
}

fn insert_row(
    transaction: &Transaction<'_>,
    entry: &StoredMessage,
    expires_at: i64,
) -> Result<(), StorageError> {
    let header = entry.header();
    let metadata = entry.metadata();
    let received_from = metadata.received_from.map(|node| node.to_vec());
    transaction.execute(
        "INSERT INTO mesh_store (
            msg_id, origin, dst, class, wire, size, copies_left, first_seen_ms,
            received_at_ms, priority, received_from, age_ms, created_ms,
            lifetime_s, expires_at_ms, payload_len, ttl, hops
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            &header.msg_id.as_bytes()[..],
            &header.origin[..],
            header.dst.as_ref().map(|dst| &dst[..]),
            header.class as i64,
            entry.wire(),
            i64::try_from(entry.size()).map_err(|_| StorageError::InvalidStoredMessage)?,
            i64::from(metadata.copies_left),
            encode_u64(metadata.first_seen),
            encode_u64(metadata.received_at),
            i64::from(metadata.priority),
            received_from.as_deref(),
            encode_u64(metadata.age_ms),
            encode_u64(header.created_ms),
            encode_u64(header.lifetime_s),
            expires_at,
            i64::try_from(header.payload_len).map_err(|_| StorageError::InvalidStoredMessage)?,
            i64::from(header.ttl),
            i64::from(header.hops),
        ],
    )?;
    Ok(())
}

impl MessageStore for SqliteMessageStore {
    fn put(&mut self, now: u64, entry: StoredMessage) -> Result<Vec<MessageId>, StoreError> {
        let metadata = *entry.metadata();
        if metadata.received_at > now {
            return Err(StoreError::InvalidTime);
        }
        if entry.is_expired_with(&self.lifetime_policy, now) {
            return Err(StoreError::Expired);
        }

        let limits = self.limits;
        let policy = self.lifetime_policy;
        let size = entry.size();
        let class_code = entry.header().class as i64;
        let class_index = class_code as usize - 1;
        if limits.entries == 0
            || size > limits.total_bytes
            || size > limits.per_origin_bytes
            || size > limits.per_class_bytes[class_index]
        {
            return Err(StoreError::Quota);
        }
        let origin = entry.header().origin;
        let now_encoded = encode_u64(now);
        let incoming_bytes = u128::from(u64::try_from(size).unwrap_or(u64::MAX));
        let expires_at = encode_u64(metadata.received_at.saturating_add(policy.remaining_ms(
            entry.header(),
            metadata.received_at,
            metadata.age_ms,
            metadata.received_at,
        )));

        let result = self.database.write(move |connection| {
            let transaction = connection.transaction()?;
            let duplicate: Option<i64> = transaction
                .query_row(
                    "SELECT 1 FROM mesh_store WHERE msg_id = ?1 AND expires_at_ms > ?2 LIMIT 1",
                    params![&entry.header().msg_id.as_bytes()[..], now_encoded],
                    |row| row.get(0),
                )
                .optional()?;
            if duplicate.is_some() {
                return Err(StorageError::DuplicateStoredMessage);
            }

            let mut removed = expire_in(&transaction, now_encoded)?;
            loop {
                let (total, origin_bytes, class_bytes, count) =
                    counters(&transaction, &origin, class_code)?;
                let origin_limit =
                    u128::from(u64::try_from(limits.per_origin_bytes).unwrap_or(u64::MAX));
                let class_limit = u128::from(
                    u64::try_from(limits.per_class_bytes[class_index]).unwrap_or(u64::MAX),
                );
                let total_limit = u128::from(u64::try_from(limits.total_bytes).unwrap_or(u64::MAX));
                let entry_limit = u128::from(u64::try_from(limits.entries).unwrap_or(u64::MAX));
                let origin_full = origin_bytes > origin_limit.saturating_sub(incoming_bytes);
                let class_full = class_bytes > class_limit.saturating_sub(incoming_bytes);
                if !origin_full
                    && !class_full
                    && count < entry_limit
                    && total <= total_limit.saturating_sub(incoming_bytes)
                {
                    break;
                }

                let scope = if origin_full {
                    Scope::Origin(&origin)
                } else if class_full {
                    Scope::Class(class_code)
                } else {
                    Scope::All
                };
                let (current_bytes, target_bytes, target_count) = match scope {
                    Scope::Origin(_) => (origin_bytes, origin_limit - incoming_bytes, None),
                    Scope::Class(_) => (class_bytes, class_limit - incoming_bytes, None),
                    Scope::All => (total, total_limit - incoming_bytes, Some(entry_limit - 1)),
                };
                let victims = victims_for(
                    &transaction,
                    scope,
                    current_bytes,
                    target_bytes,
                    count,
                    target_count,
                )?;
                if victims.is_empty() {
                    return Err(StorageError::QuotaRejected);
                }
                delete_victims(&transaction, scope, victims.len())?;
                removed.extend(victims);
            }

            insert_row(&transaction, &entry, expires_at)?;
            transaction.commit()?;
            Ok(removed)
        });

        match result {
            Ok(removed) => Ok(removed),
            Err(StorageError::DuplicateStoredMessage) => Err(StoreError::Duplicate),
            Err(StorageError::QuotaRejected) => Err(StoreError::Quota),
            Err(source) => Err(self.fail(StoreBackendOperation::Transaction, source)),
        }
    }

    fn get(&self, now: u64, id: &MessageId) -> Option<StoredMessage> {
        let result = self.read_or(StoreBackendOperation::Query, |connection| {
            let found = connection
                .query_row(
                    "SELECT wire, copies_left, first_seen_ms, received_at_ms, priority, \
                     received_from, age_ms FROM mesh_store \
                     WHERE msg_id = ?1 AND expires_at_ms > ?2",
                    params![&id.as_bytes()[..], encode_u64(now)],
                    decode_row_rusqlite,
                )
                .optional()
                .map_err(StorageError::from)?;
            Ok(found)
        });
        result.unwrap_or_default()
    }

    fn by_destination(&self, now: u64, dst: &NodeId) -> Vec<MessageId> {
        self.query_ids(SELECT_DST_IDS, now, Some(dst))
    }

    fn summary(&self, now: u64) -> Vec<MessageId> {
        self.query_ids(SELECT_LIVE_IDS, now, None)
    }

    fn expire(&mut self, now: u64) -> Vec<MessageId> {
        let now = encode_u64(now);
        self.write_or(StoreBackendOperation::Transaction, move |connection| {
            let transaction = connection.transaction()?;
            let ids = expire_in(&transaction, now)?;
            transaction.commit()?;
            Ok(ids)
        })
        .unwrap_or_default()
    }

    fn evict(&mut self, id: &MessageId) -> Option<StoredMessage> {
        let id = *id;
        self.write_or(StoreBackendOperation::Query, move |connection| {
            let transaction = connection.transaction()?;
            let entry: Option<StoredMessage> = transaction
                .query_row(
                    "SELECT wire, copies_left, first_seen_ms, received_at_ms, priority, \
                     received_from, age_ms FROM mesh_store WHERE msg_id = ?1",
                    params![&id.as_bytes()[..]],
                    decode_row_rusqlite,
                )
                .optional()?;
            if entry.is_some() {
                transaction.execute(
                    "DELETE FROM mesh_store WHERE msg_id = ?1",
                    params![&id.as_bytes()[..]],
                )?;
            }
            transaction.commit()?;
            Ok(entry)
        })
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{DatabaseConfig, DatabaseKey, test_support::TempDb};
    use ao_mesh::packet::PacketClass;
    use ao_mesh::store::conformance;
    use std::time::Instant;

    struct TestStore {
        _db: TempDb,
        store: SqliteMessageStore,
    }

    impl TestStore {
        fn new(limits: StoreLimits) -> Self {
            let db = TempDb::new();
            let database = Database::open(
                db.path(),
                DatabaseKey::new(&[0x5a; 32]).unwrap_or_else(|error| panic!("test key: {error}")),
                DatabaseConfig {
                    readers: 1,
                    write_queue: 1,
                },
            )
            .unwrap_or_else(|error| panic!("test database: {error}"));
            Self {
                _db: db,
                store: SqliteMessageStore::new(database, limits),
            }
        }
    }

    impl MessageStore for TestStore {
        fn put(&mut self, now: u64, entry: StoredMessage) -> Result<Vec<MessageId>, StoreError> {
            self.store.put(now, entry)
        }

        fn get(&self, now: u64, id: &MessageId) -> Option<StoredMessage> {
            self.store.get(now, id)
        }

        fn by_destination(&self, now: u64, dst: &NodeId) -> Vec<MessageId> {
            self.store.by_destination(now, dst)
        }

        fn summary(&self, now: u64) -> Vec<MessageId> {
            self.store.summary(now)
        }

        fn expire(&mut self, now: u64) -> Vec<MessageId> {
            self.store.expire(now)
        }

        fn evict(&mut self, id: &MessageId) -> Option<StoredMessage> {
            self.store.evict(id)
        }
    }

    #[test]
    fn sqlite_backend_passes_the_shared_conformance_suite() {
        conformance::check(TestStore::new);
    }

    #[test]
    fn eviction_and_single_statement_quota_queries_handle_100k_rows() {
        let fixture = conformance::entry(1, 1, PacketClass::Direct, 1, 1, 0);
        let size = i64::try_from(fixture.size()).unwrap_or(i64::MAX);
        let limits = StoreLimits {
            entries: 1000,
            total_bytes: usize::MAX,
            per_origin_bytes: usize::MAX,
            per_class_bytes: [usize::MAX; 4],
        };
        let test = TestStore::new(limits);
        test.store
            .database()
            .write(move |connection| {
                let transaction = connection.transaction()?;
                {
                    let mut statement = transaction.prepare(
                        "INSERT INTO mesh_store (
                            msg_id, origin, dst, class, wire, size, copies_left,
                            first_seen_ms, received_at_ms, priority, received_from,
                            age_ms, created_ms, lifetime_s, expires_at_ms,
                            payload_len, ttl, hops
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                            ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                    )?;
                    let far_future = encode_u64(u64::MAX);
                    for index in 0..100_000_u32 {
                        let mut id = [0_u8; 16];
                        id[..4].copy_from_slice(&index.to_be_bytes());
                        let origin = [index as u8; 16];
                        statement.execute(params![
                            &id[..],
                            &origin[..],
                            &[9_u8; 16][..],
                            PacketClass::Direct as i64,
                            fixture.wire(),
                            size,
                            1_i64,
                            encode_u64(0),
                            encode_u64(0),
                            1_i64,
                            Option::<&[u8]>::None,
                            encode_u64(0),
                            encode_u64(0),
                            encode_u64(10),
                            far_future,
                            4_i64,
                            8_i64,
                            0_i64,
                        ])?;
                    }
                }
                transaction.commit()?;
                Ok(())
            })
            .unwrap_or_else(|error| panic!("seed: {error}"));

        let mut test = test;
        let started = Instant::now();
        let incoming = conformance::entry(255, 255, PacketClass::Direct, 1, 1, 0);
        let removed = test
            .store
            .put(0, incoming)
            .unwrap_or_else(|error| panic!("put: {error}"));
        assert_eq!(removed.len(), 99_001);
        assert_eq!(test.store.summary(0).len(), 1000);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "100k-row eviction took {:?}",
            started.elapsed()
        );
    }
}
