//! Async acknowledgment, durability, failure propagation and restart boundaries.

#[path = "support/p6.rs"]
#[allow(dead_code)]
mod support;

use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use orderhub_gateway::journal::{BusinessJournal, JournalOutcome, PersistenceMode};
use redb::StorageBackend;

#[derive(Debug, Default)]
struct SyncControl {
    paused: Mutex<bool>,
    resume: Condvar,
    calls: AtomicUsize,
    fail: AtomicBool,
}

impl SyncControl {
    fn release(&self) {
        *self.paused.lock().unwrap() = false;
        self.resume.notify_all();
    }
}

struct ReleaseOnDrop(Arc<SyncControl>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[derive(Debug)]
struct ControlledBackend {
    inner: redb::backends::InMemoryBackend,
    control: Arc<SyncControl>,
}

impl StorageBackend for ControlledBackend {
    fn len(&self) -> Result<u64, io::Error> {
        self.inner.len()
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> Result<(), io::Error> {
        self.inner.read(offset, out)
    }
    fn set_len(&self, len: u64) -> Result<(), io::Error> {
        self.inner.set_len(len)
    }
    fn write(&self, offset: u64, data: &[u8]) -> Result<(), io::Error> {
        self.inner.write(offset, data)
    }
    fn sync_data(&self) -> Result<(), io::Error> {
        self.control.calls.fetch_add(1, Ordering::SeqCst);
        let mut paused = self.control.paused.lock().unwrap();
        while *paused {
            paused = self.control.resume.wait(paused).unwrap();
        }
        if self.control.fail.load(Ordering::SeqCst) {
            return Err(io::Error::other("injected background flush failure"));
        }
        self.inner.sync_data()
    }
}

fn controlled(mode: PersistenceMode) -> (BusinessJournal, Arc<SyncControl>) {
    let control = Arc::new(SyncControl::default());
    let db = redb::Database::builder()
        .create_with_backend(ControlledBackend {
            inner: redb::backends::InMemoryBackend::new(),
            control: Arc::clone(&control),
        })
        .unwrap();
    (
        BusinessJournal::from_database_with_mode(db, mode).unwrap(),
        control,
    )
}

fn directory() -> tempfile::TempDir {
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "background persistence did not respond"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn test_async_admission_is_visible_while_disk_sync_is_blocked() {
    let (journal, control) = controlled(PersistenceMode::Async);
    let calls = control.calls.load(Ordering::SeqCst);
    *control.paused.lock().unwrap() = true;
    let release = ReleaseOnDrop(Arc::clone(&control));
    let order = support::admit(&journal);
    wait_until(|| control.calls.load(Ordering::SeqCst) > calls);
    assert_eq!(journal.durable_cursor().unwrap(), 0);
    let result = journal
        .find_request("trader-1", "SubmitOrder", "p6-order")
        .unwrap()
        .unwrap();
    assert_eq!(result.result.result, "ADMITTED");
    assert_eq!(result.admission.seq, order.admission.seq);
    assert_eq!(
        journal
            .order(order.admission.seq)
            .unwrap()
            .unwrap()
            .reserved_cash
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        rust_decimal::Decimal::from(301)
    );
    assert_eq!(
        journal
            .snapshot("trader-1", &["ORDERHUB-S001".into()], 100)
            .unwrap()
            .cursor,
        order.admission.seq
    );
    drop(release);
    journal.flush().unwrap();
    assert_eq!(
        journal.durable_cursor().unwrap(),
        journal.last_seq().unwrap()
    );
}

#[test]
fn test_sync_admission_waits_for_storage_sync() {
    let (journal, control) = controlled(PersistenceMode::Sync);
    let calls = control.calls.load(Ordering::SeqCst);
    let order = support::admit(&journal);
    assert!(control.calls.load(Ordering::SeqCst) > calls);
    assert_eq!(journal.durable_cursor().unwrap(), order.admission.seq);
}

#[test]
fn test_background_failure_closes_idle_health_and_rejects_more_writes() {
    use orderhub_gateway::bridge::OrderHubBridge;
    use orderhub_gateway::readiness::{Readiness, ReadinessGate};
    use std::cell::RefCell;
    use std::rc::Rc;
    let (journal, control) = controlled(PersistenceMode::Async);
    let journal = Arc::new(journal);
    control.fail.store(true, Ordering::SeqCst);
    support::admit(&journal);
    wait_until(|| journal.check_persistence().is_err());
    let gate = ReadinessGate::new();
    gate.set(Readiness::Ready, "");
    let bridge = OrderHubBridge::new(
        nautilus_model::identifiers::TraderId::from("ORDER-HUB"),
        Rc::new(RefCell::new(nautilus_common::clock::VirtualClock::default())),
        Rc::new(RefCell::new(nautilus_common::cache::Cache::default())),
        Arc::clone(&journal),
    )
    .with_gate(Arc::clone(&gate));
    bridge.commit_health(Some(true));
    assert_eq!(gate.get(), Readiness::Halted);
    assert!(gate.halt_reason().contains("flush failure"));
    assert!(journal.append_admission(support::record()).is_err());
    assert!(journal.flush().is_err());
}

#[test]
fn test_graceful_flush_checkpoint_and_restart_preserve_exact_economics() {
    let dir = directory();
    let path = dir.path().join("journal.redb");
    let journal = BusinessJournal::open_with_mode(&path, PersistenceMode::Async).unwrap();
    let epoch = journal.epoch();
    let order = support::admit(&journal);
    journal
        .append_outcome(
            order.admission.seq,
            JournalOutcome::DispatchStarted {
                client_order_id: order.client_order_id.clone(),
            },
        )
        .unwrap();
    journal
        .append_outcome(
            order.admission.seq,
            JournalOutcome::Execution(Box::new(support::fill(&order))),
        )
        .unwrap();
    let before =
        serde_json::to_value(journal.order(order.admission.seq).unwrap().unwrap()).unwrap();
    journal.finish_async_session().unwrap();
    let manifest = journal
        .checkpoint(&dir.path().join("backup"), support::identity())
        .unwrap();
    assert_eq!(
        journal.durable_cursor().unwrap(),
        journal.last_seq().unwrap()
    );
    drop(journal);
    let reopened = BusinessJournal::open_with_mode(&path, PersistenceMode::Async).unwrap();
    assert_ne!(reopened.epoch(), epoch);
    let report = reopened.verify_recovery().unwrap();
    assert!(!report.unclean_async_restart);
    assert_eq!(report.business_orders, 1);
    assert_eq!(
        before,
        serde_json::to_value(reopened.order(order.admission.seq).unwrap().unwrap()).unwrap()
    );
    assert!(reopened.verify_checkpoint(&manifest).is_err());
    assert!(
        reopened
            .find_request("trader-1", "SubmitOrder", "p6-order")
            .unwrap()
            .is_some()
    );
    let restored =
        BusinessJournal::restore_backup(&manifest, &dir.path().join("restored.redb")).unwrap();
    assert!(restored.verify_recovery().unwrap().backup_restore_fenced);
}

#[test]
fn test_unfinished_empty_async_session_requires_reconciliation_after_restart() {
    let dir = directory();
    let path = dir.path().join("empty.redb");
    let first = BusinessJournal::open_with_mode(&path, PersistenceMode::Async).unwrap();
    let epoch = first.epoch();
    assert!(!first.verify_recovery().unwrap().unclean_async_restart);
    drop(first);
    let second = BusinessJournal::open_with_mode(&path, PersistenceMode::Async).unwrap();
    let report = second.verify_recovery().unwrap();
    assert!(report.unclean_async_restart);
    assert_eq!(report.business_orders, 0);
    assert_ne!(epoch, second.epoch());
    second.finish_async_session().unwrap();
    drop(second);
    let third = BusinessJournal::open_with_mode(&path, PersistenceMode::Async).unwrap();
    assert!(third.verify_recovery().unwrap().unclean_async_restart);
}

#[test]
fn test_async_process_termination_keeps_a_verified_prefix_and_restart_fence() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let dir = directory();
    let path = dir.path().join("crash.redb");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "test_async_child_process", "--nocapture"])
        .env("ORDERHUB_ASYNC_TEST_CHILD", &path)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let output = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if let Some((_, epoch)) = line.split_once("ASYNC_ACK ") {
                let _ = send.send(epoch.parse::<u64>().unwrap());
                break;
            }
        }
    });
    let acknowledged = receive.recv_timeout(Duration::from_secs(15));
    let _ = child.kill();
    child.wait().unwrap();
    let old_epoch = acknowledged.expect("child did not acknowledge its order");
    let reopened = BusinessJournal::open_with_mode(&path, PersistenceMode::Async).unwrap();
    let report = reopened.verify_recovery().unwrap();
    assert!(report.unclean_async_restart);
    assert!(report.cursor <= 1);
    assert_ne!(old_epoch, reopened.epoch());
}

#[test]
fn test_async_child_process() {
    let Some(path) = std::env::var_os("ORDERHUB_ASYNC_TEST_CHILD") else {
        return;
    };
    let journal =
        BusinessJournal::open_with_mode(std::path::Path::new(&path), PersistenceMode::Async)
            .unwrap();
    support::admit(&journal);
    println!("ASYNC_ACK {}", journal.epoch());
    loop {
        std::thread::park();
    }
}

#[test]
#[ignore = "explicit local performance comparison; no environment-independent speed target"]
fn test_compare_sync_and_async_admission_latency() {
    for mode in [PersistenceMode::Sync, PersistenceMode::Async] {
        let dir = directory();
        let journal =
            BusinessJournal::open_with_mode(&dir.path().join("bench.redb"), mode).unwrap();
        let started = Instant::now();
        for index in 0..250 {
            let mut record = support::record();
            record.request_id = format!("bench-{index}");
            journal
                .admit_business(record, Some(support::context()), None)
                .unwrap();
        }
        let acknowledged = started.elapsed();
        journal.flush().unwrap();
        println!(
            "mode={} admissions=250 ack_ms={:.3} durable_total_ms={:.3}",
            mode.as_str(),
            acknowledged.as_secs_f64() * 1000.0,
            started.elapsed().as_secs_f64() * 1000.0
        );
        assert_eq!(journal.last_seq().unwrap(), 250);
        assert_eq!(journal.durable_cursor().unwrap(), 250);
    }
}
