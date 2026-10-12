//! Persistence policy and background promotion of visible redb transactions.
//!
//! Async commits use `Durability::None`. They become visible in this process
//! before the flusher promotes the entire committed prefix to durable storage.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use redb::{Database, Durability, ReadableDatabase, ReadableTable, TableDefinition};

use crate::journal::JournalError;

const META: TableDefinition<u64, u64> = TableDefinition::new("orderhub_meta");
const LAST_SEQ: u64 = 2;
const FLUSH_INTERVAL: Duration = Duration::from_millis(5);

/// Whether successful admission waits for durable storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceMode {
    /// Every business commit waits for immediate durability.
    Sync,
    /// Business commits become visible before background durable promotion.
    Async,
}

impl PersistenceMode {
    /// Parses the explicit node persistence setting.
    pub fn parse(value: &str) -> Result<Self, JournalError> {
        match value {
            "sync" => Ok(Self::Sync),
            "async" => Ok(Self::Async),
            _ => Err(JournalError::Encoding(
                "ORDERHUB_PERSISTENCE must be async or sync".into(),
            )),
        }
    }

    /// Returns the public configuration name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Async => "async",
        }
    }

    pub(crate) const fn durability(self) -> Durability {
        match self {
            Self::Sync => Durability::Immediate,
            Self::Async => Durability::None,
        }
    }
}

#[derive(Debug)]
struct FlushState {
    durable_cursor: AtomicU64,
    failure: Mutex<Option<JournalError>>,
}

impl FlushState {
    fn check(&self) -> Result<(), JournalError> {
        match self
            .failure
            .lock()
            .expect("persistence failure lock")
            .as_ref()
        {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }

    fn flush(&self, db: &Database) -> Result<(), JournalError> {
        self.check()?;
        let result: Result<(), JournalError> = (|| {
            let mut write = db.begin_write()?;
            write.set_durability(Durability::Immediate)?;
            let cursor = write
                .open_table(META)?
                .get(LAST_SEQ)?
                .map_or(0, |value| value.value());
            write.commit()?;
            self.durable_cursor.fetch_max(cursor, Ordering::Release);
            Ok(())
        })();
        if let Err(e) = &result {
            *self.failure.lock().expect("persistence failure lock") = Some(e.clone());
        }
        result
    }
}

/// Owns one background flusher without holding an Arc to the business journal.
#[derive(Debug)]
pub(crate) struct AsyncFlusher {
    db: Arc<Database>,
    state: Arc<FlushState>,
    stop: mpsc::Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl AsyncFlusher {
    pub(crate) fn start(db: Arc<Database>, cursor: u64) -> Result<Self, JournalError> {
        let state = Arc::new(FlushState {
            durable_cursor: AtomicU64::new(cursor),
            failure: Mutex::new(None),
        });
        let (stop, receive) = mpsc::channel();
        let writer_db = Arc::clone(&db);
        let writer_state = Arc::clone(&state);
        let handle = std::thread::Builder::new()
            .name("orderhub-journal-flusher".into())
            .spawn(move || {
                while matches!(
                    receive.recv_timeout(FLUSH_INTERVAL),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    let visible: Result<u64, JournalError> = (|| {
                        let read = writer_db.begin_read()?;
                        Ok(read
                            .open_table(META)?
                            .get(LAST_SEQ)?
                            .map_or(0, |value| value.value()))
                    })();
                    match visible {
                        Ok(cursor)
                            if cursor <= writer_state.durable_cursor.load(Ordering::Acquire) => {}
                        Ok(_) => {
                            if writer_state.flush(&writer_db).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            *writer_state
                                .failure
                                .lock()
                                .expect("persistence failure lock") =
                                Some(JournalError::Storage(e.to_string()));
                            break;
                        }
                    }
                }
                if let Err(e) = writer_state.flush(&writer_db) {
                    log::error!("background journal flush failed: {e}");
                }
            })
            .map_err(|e| JournalError::Storage(format!("cannot start journal flusher: {e}")))?;
        Ok(Self {
            db,
            state,
            stop,
            handle: Some(handle),
        })
    }

    pub(crate) fn check(&self) -> Result<(), JournalError> {
        self.state.check()
    }

    pub(crate) fn durable_cursor(&self) -> u64 {
        self.state.durable_cursor.load(Ordering::Acquire)
    }

    pub(crate) fn flush(&self) -> Result<(), JournalError> {
        self.state.flush(&self.db)
    }
}

impl Drop for AsyncFlusher {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persistence_setting_rejects_unknown_values() {
        assert_eq!(
            PersistenceMode::parse("async").unwrap(),
            PersistenceMode::Async
        );
        assert_eq!(
            PersistenceMode::parse("sync").unwrap(),
            PersistenceMode::Sync
        );
        assert!(PersistenceMode::parse("asnyc").is_err());
        assert!(PersistenceMode::parse("").is_err());
    }
}
