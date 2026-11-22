//! Bounded pool of read-only connections for queries.

use super::{DatabaseKey, StorageError, connection};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A fixed set of read-only connections shared by all callers.
pub(super) struct ReadPool {
    connections: Vec<Mutex<Connection>>,
    next: AtomicUsize,
}

impl ReadPool {
    /// Open `count` keyed read-only connections after migrations have run.
    pub(super) fn open(path: &Path, key: &DatabaseKey, count: usize) -> Result<Self, StorageError> {
        let mut connections = Vec::with_capacity(count);
        for _ in 0..count {
            connections.push(Mutex::new(connection::open_reader(path, key)?));
        }
        Ok(Self {
            connections,
            next: AtomicUsize::new(0),
        })
    }

    /// Run a query on a free connection, or wait on the next one in turn.
    pub(super) fn with_connection<T, F>(&self, job: F) -> Result<T, StorageError>
    where
        F: FnOnce(&Connection) -> Result<T, StorageError>,
    {
        let count = self.connections.len();
        let start = self.next.fetch_add(1, Ordering::Relaxed) % count;
        for offset in 0..count {
            if let Ok(connection) = self.connections[(start + offset) % count].try_lock() {
                return job(&connection);
            }
        }
        // Every connection is busy; blocking on one keeps memory bounded.
        let connection = self.connections[start]
            .lock()
            .map_err(|_| StorageError::ReaderPoisoned)?;
        job(&connection)
    }
}
