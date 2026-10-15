// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

//! Nonblocking FIX diagnostics, with one file writer and bounded rotation.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::Duration;

use nautilus_core::datetime::unix_nanos_to_iso8601_millis;
use nautilus_core::time::get_atomic_clock_realtime;

use crate::fix_config::FixSessionConfig;

const LOG_QUEUE_CAPACITY: usize = 4_096;

/// Asynchronous message/event logger; disabled handles perform no file I/O.
#[derive(Debug, Clone, Default)]
pub struct FixLog {
    inner: Option<Arc<LogInner>>,
}

#[derive(Debug)]
struct LogInner {
    sender: Option<SyncSender<Command>>,
    worker: Option<JoinHandle<()>>,
    health: Arc<Health>,
}

#[derive(Debug, Default)]
struct Health {
    dropped: AtomicU64,
    warned_full: AtomicBool,
    failed: AtomicBool,
}

#[derive(Debug)]
enum Command {
    Record {
        timestamp: nautilus_core::UnixNanos,
        kind: Kind,
        text: String,
    },
    Flush(mpsc::Sender<Result<(), String>>),
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Incoming,
    Outgoing,
    Event,
}

impl FixLog {
    /// Opens the session's two logs and starts their dedicated writer.
    ///
    /// A session lock prevents concurrent processes rotating the same files.
    ///
    /// # Errors
    /// Returns invalid configuration, file/lock, or thread startup errors.
    pub fn start(config: &FixSessionConfig) -> io::Result<Self> {
        config
            .validate()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        if !config.logging.enabled {
            return Ok(Self::default());
        }
        std::fs::create_dir_all(&config.logging.directory)?;
        let prefix = config.log_prefix();
        let lock = open_lock(&config.logging.directory.join(format!("{prefix}.log.lock")))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "FIX session logs already in use",
                ));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
        let mut files = LogFiles {
            messages: RotatingFile::new(
                config
                    .logging
                    .directory
                    .join(format!("{prefix}.messages.current.log")),
                config.logging.max_bytes,
                config.logging.backups,
            )?,
            events: RotatingFile::new(
                config
                    .logging
                    .directory
                    .join(format!("{prefix}.event.current.log")),
                config.logging.max_bytes,
                config.logging.backups,
            )?,
            _lock: lock,
        };
        let (sender, receiver) = mpsc::sync_channel(LOG_QUEUE_CAPACITY);
        let health = Arc::new(Health::default());
        let worker_health = Arc::clone(&health);
        let worker = std::thread::Builder::new()
            .name("orderhub-fix-log".into())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    match command {
                        Command::Record {
                            timestamp,
                            kind,
                            text,
                        } => {
                            let result = files
                                .record_dropped(&worker_health)
                                .and_then(|()| files.write(timestamp, kind, &text));
                            if let Err(e) = result {
                                report_failure(&worker_health, &e);
                            }
                        }
                        Command::Flush(reply) => {
                            let result = if worker_health.failed.load(Ordering::Acquire) {
                                Err(io::Error::other("FIX diagnostic file writer failed"))
                            } else {
                                files
                                    .record_dropped(&worker_health)
                                    .and_then(|()| files.flush())
                            };
                            if let Err(e) = &result {
                                report_failure(&worker_health, e);
                            }
                            let _ = reply.send(result.map_err(|e| e.to_string()));
                        }
                    }
                }
                if let Err(e) = files
                    .record_dropped(&worker_health)
                    .and_then(|()| files.flush())
                {
                    report_failure(&worker_health, &e);
                }
            })?;
        let log = Self {
            inner: Some(Arc::new(LogInner {
                sender: Some(sender),
                worker: Some(worker),
                health,
            })),
        };
        log.event(format!(
            "LOGGER_STARTED address={} heartbeat={}s",
            config.address(),
            config.heartbeat_seconds
        ));
        Ok(log)
    }

    /// Enqueues a successful outbound FIX frame, without waiting for disk.
    pub(crate) fn outgoing(&self, message: &str) {
        self.record(Kind::Outgoing, message);
    }

    /// Enqueues a complete inbound FIX frame, without waiting for disk.
    pub(crate) fn incoming(&self, message: &str) {
        self.record(Kind::Incoming, message);
    }

    /// Enqueues a session diagnostic, without waiting for disk.
    pub fn event(&self, message: impl AsRef<str>) {
        self.record(Kind::Event, message.as_ref());
    }

    fn record(&self, kind: Kind, text: &str) {
        let Some(inner) = &self.inner else {
            return;
        };
        if inner.health.failed.load(Ordering::Acquire) {
            inner.health.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let command = Command::Record {
            timestamp: get_atomic_clock_realtime().get_time_ns(),
            kind,
            text: text.to_string(),
        };
        match inner
            .sender
            .as_ref()
            .expect("live log sender")
            .try_send(command)
        {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                inner.health.dropped.fetch_add(1, Ordering::Relaxed);
                if !inner.health.warned_full.swap(true, Ordering::Relaxed) {
                    eprintln!("FIX_LOG_QUEUE_FULL=diagnostic records dropped; execution continues");
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                report_failure(&inner.health, &io::Error::other("FIX log worker stopped"));
            }
        }
    }

    /// Waits for records already queued to reach the OS file buffer.
    ///
    /// This is a shutdown/test barrier, not an order-path durability guarantee.
    ///
    /// # Errors
    /// Returns queue, file, or timeout errors; this method never calls fsync.
    pub fn flush(&self, timeout: Duration) -> io::Result<()> {
        let Some(inner) = &self.inner else {
            return Ok(());
        };
        let (reply, receiver) = mpsc::channel();
        inner
            .sender
            .as_ref()
            .expect("live log sender")
            .try_send(Command::Flush(reply))
            .map_err(|e| io::Error::other(e.to_string()))?;
        receiver
            .recv_timeout(timeout)
            .map_err(|e| io::Error::other(e.to_string()))?
            .map_err(io::Error::other)
    }
}

impl Drop for LogInner {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn report_failure(health: &Health, error: &io::Error) {
    if !health.failed.swap(true, Ordering::AcqRel) {
        eprintln!("FIX_LOG_ERROR={error}; diagnostic logging stopped; execution continues");
    }
}

struct LogFiles {
    messages: RotatingFile,
    events: RotatingFile,
    _lock: File,
}

impl LogFiles {
    fn write(
        &mut self,
        timestamp: nautilus_core::UnixNanos,
        kind: Kind,
        text: &str,
    ) -> io::Result<()> {
        let label = match kind {
            Kind::Incoming => "in",
            Kind::Outgoing => "out",
            Kind::Event => "event",
        };
        let text = text.replace('\r', "\\r").replace('\n', "\\n");
        let line = format!(
            "{} {label} {text}\n",
            unix_nanos_to_iso8601_millis(timestamp)
        );
        match kind {
            Kind::Incoming | Kind::Outgoing => self.messages.write(line.as_bytes()),
            Kind::Event => self.events.write(line.as_bytes()),
        }
    }

    fn record_dropped(&mut self, health: &Health) -> io::Result<()> {
        let dropped = health.dropped.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            self.write(
                get_atomic_clock_realtime().get_time_ns(),
                Kind::Event,
                &format!("LOG_RECORDS_DROPPED count={dropped}"),
            )?;
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.messages.flush()?;
        self.events.flush()
    }
}

struct RotatingFile {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    max_bytes: u64,
    backups: u32,
}

impl RotatingFile {
    fn new(path: PathBuf, max_bytes: u64, backups: u32) -> io::Result<Self> {
        let file = open(&path)?;
        Ok(Self {
            size: file.metadata()?.len(),
            path,
            file: Some(file),
            max_bytes,
            backups,
        })
    }

    fn archive(&self, index: u32) -> PathBuf {
        let mut name = self.path.file_name().expect("log filename").to_os_string();
        name.push(format!(".{index}"));
        self.path.with_file_name(name)
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.size > 0 && self.size.saturating_add(bytes.len() as u64) > self.max_bytes {
            self.flush()?;
            self.file.take();
            let oldest = self.archive(self.backups);
            match std::fs::remove_file(oldest) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            for index in (1..self.backups).rev() {
                let source = self.archive(index);
                match std::fs::symlink_metadata(&source) {
                    Ok(metadata) => {
                        if !metadata.file_type().is_file() {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "FIX log archive is not a regular file",
                            ));
                        }
                        std::fs::rename(source, self.archive(index + 1))?;
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
            std::fs::rename(&self.path, self.archive(1))?;
            self.file = Some(open(&self.path)?);
            self.size = 0;
        }
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("FIX log rotation failed"))?
            .write_all(bytes)?;
        self.size += bytes.len() as u64;
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("FIX log file unavailable"))?
            .flush()
    }
}

fn open_lock(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn open(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(directory: &Path) -> FixSessionConfig {
        let path = directory.join("fix.cfg");
        std::fs::write(
            &path,
            include_str!("../../../config/orderhub-fix.cfg")
                .replace("../logs/fix", "fix-logs")
                .replace("FileLogMaxBytes=10485760", "FileLogMaxBytes=65536")
                .replace("FileLogBackups=5", "FileLogBackups=2"),
        )
        .unwrap();
        FixSessionConfig::load(path).unwrap()
    }

    #[test]
    fn message_event_separation_raw_soh_and_session_lock() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        let log = FixLog::start(&config).unwrap();
        assert!(
            FixLog::start(&config).is_err(),
            "same files must have one writer"
        );
        log.outgoing("35=A\u{1}34=1\u{1}");
        log.incoming("35=1\u{1}112=probe-2\u{1}");
        log.event("TEST_REQUEST probe-2\nsecond-line");
        log.flush(Duration::from_secs(5)).unwrap();
        let messages = std::fs::read_to_string(
            config
                .logging
                .directory
                .join("FIX.4.2-ORDERHUB-GOX.messages.current.log"),
        )
        .unwrap();
        let events = std::fs::read_to_string(
            config
                .logging
                .directory
                .join("FIX.4.2-ORDERHUB-GOX.event.current.log"),
        )
        .unwrap();
        assert!(messages.contains(" out 35=A\u{1}"));
        assert!(messages.contains(" in 35=1\u{1}112=probe-2\u{1}"));
        assert!(!messages.contains("TEST_REQUEST"));
        assert!(events.contains("TEST_REQUEST probe-2\\nsecond-line"));
        drop(log);
        assert!(FixLog::start(&config).is_ok(), "lock released on shutdown");
    }

    #[test]
    fn rotation_bounds_both_files_and_preserves_newest_records() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        let log = FixLog::start(&config).unwrap();
        let payload = "x".repeat(1024);
        for i in 0..200 {
            log.outgoing(&format!("message-{i:03} {payload}"));
            log.event(format!("event-{i:03} {payload}"));
        }
        log.flush(Duration::from_secs(5)).unwrap();
        for suffix in ["messages", "event"] {
            let current = config
                .logging
                .directory
                .join(format!("FIX.4.2-ORDERHUB-GOX.{suffix}.current.log"));
            assert!(std::fs::metadata(&current).unwrap().len() <= 65_536);
            assert!(PathBuf::from(format!("{}.1", current.display())).exists());
            assert!(PathBuf::from(format!("{}.2", current.display())).exists());
            assert!(!PathBuf::from(format!("{}.3", current.display())).exists());
            assert!(std::fs::read_to_string(current).unwrap().contains("-199"));
        }
    }

    #[test]
    fn disabled_logging_does_not_create_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = config(dir.path());
        config.logging.enabled = false;
        let log = FixLog::start(&config).unwrap();
        log.event("disabled");
        log.flush(Duration::from_secs(1)).unwrap();
        assert!(!config.logging.directory.exists());
    }

    #[test]
    fn full_diagnostic_queue_never_blocks_the_producer() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let health = Arc::new(Health::default());
        let log = FixLog {
            inner: Some(Arc::new(LogInner {
                sender: Some(sender),
                worker: None,
                health: Arc::clone(&health),
            })),
        };
        log.event("first");
        let (done, finished) = mpsc::channel();
        let producer = std::thread::spawn(move || {
            log.event("overflow");
            done.send(()).unwrap();
        });
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(health.dropped.load(Ordering::Relaxed), 1);
        drop(receiver);
        producer.join().unwrap();
    }

    #[test]
    fn rotation_failure_is_reported_without_panicking_or_blocking_later_records() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        let log = FixLog::start(&config).unwrap();
        std::fs::create_dir(
            config
                .logging
                .directory
                .join("FIX.4.2-ORDERHUB-GOX.messages.current.log.1"),
        )
        .unwrap();
        for _ in 0..100 {
            log.outgoing(&"x".repeat(1024));
        }
        assert!(log.flush(Duration::from_secs(5)).is_err());
        assert!(
            log.inner
                .as_ref()
                .unwrap()
                .health
                .failed
                .load(Ordering::Acquire)
        );
        log.event("after failure");
    }
}
