//! The single writer task.
//!
//! A dedicated thread owns the only read-write connection. Callers submit
//! owned closures through a bounded channel and wait for a typed reply, so
//! SQLite never sees two writers and never needs a retry loop to resolve
//! lock contention.

use super::{DatabaseKey, StorageError, connection};
use rusqlite::Connection;
use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};

type BoxedResult = Result<Box<dyn Any + Send>, StorageError>;

struct Request {
    job: Box<dyn FnOnce(&mut Connection) -> BoxedResult + Send>,
    reply: SyncSender<BoxedResult>,
}

/// Handle to the writer thread. Dropping it drains the queue and joins.
pub(super) struct WriterTask {
    sender: Option<mpsc::SyncSender<Request>>,
    worker: Option<JoinHandle<()>>,
}

impl WriterTask {
    /// Open the write connection and start its worker thread.
    pub(super) fn start(
        path: &Path,
        key: &DatabaseKey,
        queue: usize,
    ) -> Result<Self, StorageError> {
        let connection = connection::open_writer(path, key)?;
        let (sender, receiver) = mpsc::sync_channel(queue);
        let worker = thread::Builder::new()
            .name("aoheyra-db-writer".to_owned())
            .spawn(move || Self::run(receiver, connection))
            .map_err(|source| StorageError::WriterStart(source.to_string()))?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
        })
    }

    /// Submit a write and return its typed result.
    pub(super) fn write<T, F>(&self, job: F) -> Result<T, StorageError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StorageError> + Send + 'static,
    {
        let (reply, response) = mpsc::sync_channel(1);
        let job = move |connection: &mut Connection| -> BoxedResult {
            let value = job(connection)?;
            Ok(Box::new(value))
        };
        self.sender
            .as_ref()
            .ok_or(StorageError::WriterStopped)?
            .send(Request {
                job: Box::new(job),
                reply,
            })
            .map_err(|_| StorageError::WriterStopped)?;
        let result = response.recv().map_err(|_| StorageError::WriterStopped)??;
        result
            .downcast::<T>()
            .map(|value| *value)
            .map_err(|_| StorageError::InvalidWriterReply)
    }

    fn run(receiver: Receiver<Request>, mut connection: Connection) {
        while let Ok(request) = receiver.recv() {
            // A panic in a caller's closure must not kill the only writer or
            // leave its connection in an unknown state. The closure holds no
            // borrowed state and the panic payload is discarded; this is
            // defence in depth, not a substitute for panic-free FFI paths.
            let outcome = catch_unwind(AssertUnwindSafe(|| (request.job)(&mut connection)));
            let result = outcome.unwrap_or(Err(StorageError::WriterPanic));
            let _ = request.reply.send(result);
        }
    }
}

impl Drop for WriterTask {
    fn drop(&mut self) {
        // Closing the last sender lets the worker drain accepted jobs and exit.
        drop(self.sender.take());
        // Never try to join the writer from itself.
        if let Some(worker) = self.worker.take()
            && thread::current().id() != worker.thread().id()
        {
            let _ = worker.join();
        }
    }
}
