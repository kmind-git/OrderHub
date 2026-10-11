// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software distributed under
//  the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
//  KIND, either express or implied. See the License for the specific language governing
//  permissions and limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Recovery, backup, corruption, and real child-process termination acceptance.

#[path = "support/p6.rs"]
mod support;

use orderhub_gateway::journal::{BusinessJournal, JournalOutcome, RecoveryManifest};
use redb::{Database, TableDefinition};
use std::time::{Duration, Instant};
use support::{admit, context, directory, fill, identity, record};

#[test]
fn test_checkpoint_tail_and_restart_preserve_original_results_and_economics() {
    let dir = directory();
    let path = dir.path().join("journal.redb");
    let journal = BusinessJournal::open(&path).unwrap();
    let order = admit(&journal);
    let checkpoint = journal
        .checkpoint(&dir.path().join("backup"), identity())
        .unwrap();
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
            JournalOutcome::Execution(Box::new(fill(&order))),
        )
        .unwrap();
    let before = journal.order(order.admission.seq).unwrap().unwrap();
    assert_eq!(before.filled_quantity, "2");
    assert_eq!(
        before.fees.parse::<rust_decimal::Decimal>().unwrap(),
        rust_decimal::Decimal::new(50, 2)
    );
    assert!(
        journal
            .verify_checkpoint(&checkpoint)
            .unwrap()
            .replayed_commits
            >= 3
    );
    drop(journal);
    let restored = BusinessJournal::open(&path).unwrap();
    let report = restored.verify_recovery().unwrap();
    assert_eq!(report.business_orders, 1);
    assert!(report.uncertain_orders.is_empty());
    assert_eq!(
        restored
            .find_request("trader-1", "SubmitOrder", "p6-order")
            .unwrap()
            .unwrap()
            .result
            .result,
        "ADMITTED"
    );
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(restored.order(order.admission.seq).unwrap().unwrap()).unwrap()
    );
}

#[test]
fn test_corrupt_checkpoint_falls_back_only_to_verified_complete_history() {
    let dir = directory();
    let journal = BusinessJournal::open(&dir.path().join("j.redb")).unwrap();
    admit(&journal);
    let path = journal.checkpoint(dir.path(), identity()).unwrap();
    let manifest: RecoveryManifest =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    std::fs::write(dir.path().join(manifest.blob), b"incomplete").unwrap();
    assert!(journal.verify_checkpoint(&path).is_err());
    assert_eq!(journal.verify_recovery().unwrap().business_orders, 1);
}

#[test]
fn test_delta_gap_and_projection_tampering_are_not_ignored() {
    let dir = directory();
    let path = dir.path().join("j.redb");
    let journal = BusinessJournal::open(&path).unwrap();
    admit(&journal);
    drop(journal);
    let db = Database::open(&path).unwrap();
    let tx = db.begin_write().unwrap();
    tx.open_table(TableDefinition::<u64, &[u8]>::new(
        "orderhub_recovery_commits_v2",
    ))
    .unwrap()
    .remove(1)
    .unwrap();
    tx.commit().unwrap();
    drop(db);
    let journal = BusinessJournal::open(&path).unwrap();
    assert!(
        journal
            .verify_recovery()
            .unwrap_err()
            .to_string()
            .contains("mismatch")
    );
}

#[test]
fn test_restore_forks_epoch_keeps_original_ids_and_requires_reconciliation() {
    let dir = directory();
    let source = BusinessJournal::open(&dir.path().join("source.redb")).unwrap();
    let order = admit(&source);
    let manifest = source
        .checkpoint(&dir.path().join("backup"), identity())
        .unwrap();
    let restored =
        BusinessJournal::restore_backup(&manifest, &dir.path().join("restored.redb")).unwrap();
    assert_ne!(restored.epoch(), source.epoch());
    assert!(restored.verify_recovery().unwrap().backup_restore_fenced);
    let (_, orders) = restored
        .lookup_orders(
            std::slice::from_ref(&order.client_order_id),
            "trader-1",
            &["ORDERHUB-S001".into()],
        )
        .unwrap();
    assert_eq!(
        orders[0]
            .reserved_cash
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        rust_decimal::Decimal::from(301)
    );
    assert_eq!(
        restored
            .find_request("trader-1", "SubmitOrder", "p6-order")
            .unwrap()
            .unwrap()
            .result
            .client_order_id,
        order.client_order_id
    );
    let next = restored
        .admit_business(record(), Some(context()), None)
        .unwrap();
    assert_eq!(next.seq, order.admission.seq);
    assert_eq!(
        restored
            .snapshot("trader-1", &["ORDERHUB-S001".into()], 100)
            .unwrap()
            .control
            .unwrap()
            .readiness,
        "HALTED"
    );
    let existing = dir.path().join("do-not-overwrite");
    std::fs::write(&existing, b"preserve").unwrap();
    assert!(BusinessJournal::restore_backup(&manifest, &existing).is_err());
    assert_eq!(std::fs::read(existing).unwrap(), b"preserve");
}

#[test]
fn test_p5_complete_material_upgrades_without_changing_economic_fields() {
    let dir = directory();
    let path = dir.path().join("j.redb");
    let journal = BusinessJournal::open(&path).unwrap();
    let order = admit(&journal);
    journal
        .append_outcome(
            order.admission.seq,
            JournalOutcome::Execution(Box::new(fill(&order))),
        )
        .unwrap();
    let before =
        serde_json::to_value(journal.order(order.admission.seq).unwrap().unwrap()).unwrap();
    drop(journal);
    let db = Database::open(&path).unwrap();
    let tx = db.begin_write().unwrap();
    tx.delete_table(TableDefinition::<&str, &[u8]>::new("orderhub_recovery_v2"))
        .unwrap();
    tx.delete_table(TableDefinition::<u64, &[u8]>::new(
        "orderhub_recovery_commits_v2",
    ))
    .unwrap();
    tx.open_table(TableDefinition::<u64, u64>::new("orderhub_meta"))
        .unwrap()
        .remove(5)
        .unwrap();
    tx.commit().unwrap();
    drop(db);
    let journal = BusinessJournal::open(&path).unwrap();
    let report = journal.verify_recovery().unwrap();
    assert_eq!(report.base_cursor, report.cursor);
    assert_eq!(
        before,
        serde_json::to_value(journal.order(order.admission.seq).unwrap().unwrap()).unwrap()
    );
}

#[test]
fn test_same_database_exclusive_lock_blocks_a_second_live_owner() {
    let dir = directory();
    let path = dir.path().join("j.redb");
    let _first = BusinessJournal::open(&path).unwrap();
    assert!(BusinessJournal::open(&path).is_err());
}

#[test]
fn test_projection_changes_and_corrupt_base_are_detected_independently() {
    for table in ["orderhub_orders_v1", "orderhub_recovery_v2"] {
        let dir = directory();
        let path = dir.path().join("j.redb");
        let journal = BusinessJournal::open(&path).unwrap();
        admit(&journal);
        drop(journal);
        let db = Database::open(&path).unwrap();
        let tx = db.begin_write().unwrap();
        if table == "orderhub_orders_v1" {
            tx.open_table(TableDefinition::<u64, &[u8]>::new(table))
                .unwrap()
                .remove(1)
                .unwrap();
        } else {
            tx.open_table(TableDefinition::<&str, &[u8]>::new(table))
                .unwrap()
                .insert("base_hash", b"corrupt".as_slice())
                .unwrap();
        }
        tx.commit().unwrap();
        drop(db);
        let journal = BusinessJournal::open(&path).unwrap();
        assert!(
            journal.verify_recovery().is_err(),
            "{table} corruption was ignored"
        );
    }
}

#[test]
fn test_checkpoint_write_failure_keeps_an_existing_verified_backup() {
    let dir = directory();
    let journal = BusinessJournal::open(&dir.path().join("j.redb")).unwrap();
    admit(&journal);
    let good = journal
        .checkpoint(&dir.path().join("good"), identity())
        .unwrap();
    let blocked = dir.path().join("blocked");
    std::fs::write(&blocked, b"this is a file, not a directory").unwrap();
    assert!(journal.checkpoint(&blocked, identity()).is_err());
    journal.verify_checkpoint(&good).unwrap();
    assert_eq!(
        std::fs::read(blocked).unwrap(),
        b"this is a file, not a directory"
    );
}

#[test]
fn test_unpublished_manifest_and_orphan_blob_cannot_replace_a_verified_backup() {
    let dir = directory();
    let journal = BusinessJournal::open(&dir.path().join("j.redb")).unwrap();
    let backup = dir.path().join("backup");
    let published = journal.checkpoint(&backup, identity()).unwrap();
    let pending = backup.join("interrupted.pending");
    std::fs::copy(&published, &pending).unwrap();
    std::fs::write(backup.join("unreferenced.blob.json"), b"half-written").unwrap();
    assert!(journal.verify_checkpoint(&pending).is_err());
    assert!(BusinessJournal::restore_backup(&pending, &dir.path().join("invalid.redb")).is_err());
    assert!(!dir.path().join("invalid.redb").exists());
    journal.verify_checkpoint(&published).unwrap();
}

#[test]
fn test_missing_sealed_base_or_future_recovery_version_is_not_resealed() {
    for missing_base in [true, false] {
        let dir = directory();
        let path = dir.path().join("j.redb");
        drop(BusinessJournal::open(&path).unwrap());
        let db = Database::open(&path).unwrap();
        let tx = db.begin_write().unwrap();
        if missing_base {
            tx.open_table(TableDefinition::<&str, &[u8]>::new("orderhub_recovery_v2"))
                .unwrap()
                .remove("base")
                .unwrap();
        } else {
            tx.open_table(TableDefinition::<u64, u64>::new("orderhub_meta"))
                .unwrap()
                .insert(5, 3)
                .unwrap();
        }
        tx.commit().unwrap();
        drop(db);
        assert!(BusinessJournal::open(&path).is_err());
    }
}

#[test]
fn test_legacy_economic_facts_without_native_material_require_explicit_enrichment() {
    use nautilus_common::cache::Cache;
    use nautilus_common::clock::{Clock, VirtualClock};
    use nautilus_model::identifiers::StrategyId;
    use nautilus_portfolio::Portfolio;
    use orderhub_gateway::inputs::InputRuntime;
    use std::cell::RefCell;
    use std::rc::Rc;
    let dir = directory();
    let journal = BusinessJournal::open(&dir.path().join("j.redb")).unwrap();
    let order = admit(&journal);
    journal
        .append_outcome(
            order.admission.seq,
            JournalOutcome::Execution(Box::new(fill(&order))),
        )
        .unwrap();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let portfolio = Portfolio::new(clock.clone(), cache.clone(), None);
    let (_sender, mut inputs) = InputRuntime::channel(
        cache.clone(),
        portfolio,
        clock,
        "local-fixture-v1",
        vec![StrategyId::from("ORDERHUB-S001")],
        1024,
    )
    .unwrap();
    let error =
        orderhub_gateway::recovery::restore_native(&journal, &mut inputs, &cache).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("legacy execution lacks native recovery material")
    );
    assert_eq!(
        journal
            .order(order.admission.seq)
            .unwrap()
            .unwrap()
            .filled_quantity,
        "2"
    );
    assert!(nautilus_common::runner::trading_cmd_queue_is_empty());
}

#[test]
fn test_real_process_kill_at_business_boundaries_keeps_safe_recovery_decisions() {
    for stage in [
        "before-admission",
        "after-admission",
        "after-dispatch",
        "after-execution",
    ] {
        let dir = directory();
        let path = dir.path().join("j.redb");
        let marker = dir.path().join("ready");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "p6_crash_child", "--nocapture"])
            .env("ORDERHUB_TEST_CRASH_STAGE", stage)
            .env("ORDERHUB_TEST_CRASH_JOURNAL", &path)
            .env("ORDERHUB_TEST_CRASH_MARKER", &marker)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(15);
        while !marker.exists() {
            assert!(
                child.try_wait().unwrap().is_none(),
                "crash child exited before {stage}"
            );
            if Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                panic!("crash marker timed out: {stage}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let journal = BusinessJournal::open(&path).unwrap();
        let report = journal.verify_recovery().unwrap();
        match stage {
            "before-admission" => assert!(
                journal
                    .find_request("trader-1", "SubmitOrder", "p6-order")
                    .unwrap()
                    .is_none()
            ),
            "after-admission" => assert_eq!(report.unsent_orders.len(), 1),
            "after-dispatch" => {
                assert_eq!(report.uncertain_orders.len(), 1);
                assert_eq!(
                    journal
                        .order(1)
                        .unwrap()
                        .unwrap()
                        .reserved_cash
                        .parse::<rust_decimal::Decimal>()
                        .unwrap(),
                    rust_decimal::Decimal::from(301)
                );
            }
            "after-execution" => {
                assert_eq!(journal.order(1).unwrap().unwrap().status, "FILLED");
                assert!(report.uncertain_orders.is_empty());
            }
            _ => unreachable!(),
        }
        if stage != "before-admission" {
            let before = journal.last_seq().unwrap();
            journal
                .admit_business(record(), Some(context()), None)
                .unwrap();
            assert_eq!(journal.last_seq().unwrap(), before);
        }
        println!(
            "P6_PROCESS_KILL stage={stage} epoch={} cursor={} unsent={} uncertain={}",
            report.epoch,
            report.cursor,
            report.unsent_orders.len(),
            report.uncertain_orders.len()
        );
    }
}

#[test]
#[ignore = "child helper invoked only by the process-boundary test"]
fn p6_crash_child() {
    let stage = std::env::var("ORDERHUB_TEST_CRASH_STAGE").unwrap();
    let path = std::env::var_os("ORDERHUB_TEST_CRASH_JOURNAL").unwrap();
    let journal = BusinessJournal::open(std::path::Path::new(&path)).unwrap();
    if stage != "before-admission" {
        let order = admit(&journal);
        if stage == "after-dispatch" || stage == "after-execution" {
            journal
                .append_outcome(
                    order.admission.seq,
                    JournalOutcome::DispatchStarted {
                        client_order_id: order.client_order_id.clone(),
                    },
                )
                .unwrap();
        }
        if stage == "after-execution" {
            journal
                .append_outcome(
                    order.admission.seq,
                    JournalOutcome::Execution(Box::new(fill(&order))),
                )
                .unwrap();
        }
    }
    std::fs::write(
        std::env::var_os("ORDERHUB_TEST_CRASH_MARKER").unwrap(),
        b"ready",
    )
    .unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn test_native_cache_rebuilds_real_events_without_republishing_commands_or_freshness() {
    use nautilus_common::cache::Cache;
    use nautilus_common::clock::{Clock, VirtualClock};
    use nautilus_common::factories::event::OrderEventFactory;
    use nautilus_core::UnixNanos;
    use nautilus_model::enums::{AccountType, LiquiditySide};
    use nautilus_model::identifiers::{
        AccountId, ClientOrderId, PositionId, StrategyId, TradeId, TraderId, VenueOrderId,
    };
    use nautilus_model::orders::Order;
    use nautilus_model::types::{Currency, Money, Price, Quantity};
    use nautilus_portfolio::Portfolio;
    use orderhub_gateway::inputs::{InputEvent, InputRuntime};
    use orderhub_gateway::journal::PendingEntry;
    use std::cell::RefCell;
    use std::rc::Rc;
    let dir = directory();
    let journal = BusinessJournal::open(&dir.path().join("native.redb")).unwrap();
    let facts: Vec<InputEvent> =
        serde_json::from_str(include_str!("../test_data/p4_inputs.json")).unwrap();
    for event in facts {
        journal
            .append_batch(&[PendingEntry::Input {
                event: Box::new(event),
                received_at_ns: 1_800_000_000_000_000_000,
            }])
            .unwrap();
    }
    let order = admit(&journal);
    journal
        .append_outcome(
            order.admission.seq,
            JournalOutcome::DispatchStarted {
                client_order_id: order.client_order_id.clone(),
            },
        )
        .unwrap();
    let make_runtime = || {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
        let portfolio = Portfolio::new(clock.clone(), cache.clone(), None);
        let (sender, inputs) = InputRuntime::channel(
            cache.clone(),
            portfolio,
            clock,
            "local-fixture-v1",
            vec![StrategyId::from("ORDERHUB-S001")],
            1024,
        )
        .unwrap();
        (cache, sender, inputs)
    };
    let (cache, _sender, mut inputs) = make_runtime();
    orderhub_gateway::recovery::restore_native(&journal, &mut inputs, &cache).unwrap();
    let id = ClientOrderId::from(order.client_order_id.as_str());
    let native = cache.borrow().order(&id).unwrap().clone();
    let factory = OrderEventFactory::new(
        TraderId::from("ORDER-HUB"),
        AccountId::from("GOX-001"),
        AccountType::Cash,
        None,
    );
    let ts = UnixNanos::from(1_800_000_000_000_000_020);
    let events = [
        ("SUBMITTED", factory.generate_order_submitted(&native, ts)),
        (
            "ACCEPTED",
            factory.generate_order_accepted(&native, VenueOrderId::from("V-P6"), ts, ts),
        ),
        (
            "FILL",
            factory.generate_order_filled(
                &native,
                VenueOrderId::from("V-P6"),
                None,
                TradeId::from("p6-trade"),
                Quantity::from("2"),
                Price::from("150.00"),
                Currency::USD(),
                Some(
                    Money::from_decimal(rust_decimal::Decimal::new(50, 2), Currency::USD())
                        .unwrap(),
                ),
                LiquiditySide::Taker,
                ts,
                ts,
            ),
        ),
    ];
    for (kind, event) in events {
        let mut fact = fill(&order);
        fact.kind = kind.into();
        fact.event_id = match &event {
            nautilus_model::events::OrderEventAny::Submitted(value) => value.event_id,
            nautilus_model::events::OrderEventAny::Accepted(value) => value.event_id,
            nautilus_model::events::OrderEventAny::Filled(value) => value.event_id,
            _ => unreachable!(),
        }
        .to_string();
        if kind != "FILL" {
            fact.trade_id = None;
            fact.quantity = "0".into();
            fact.price = "0".into();
            fact.fee = "0".into();
        }
        fact.native_event = Some(serde_json::to_string(&event).unwrap());
        journal
            .append_outcome(
                order.admission.seq,
                JournalOutcome::Execution(Box::new(fact)),
            )
            .unwrap();
    }
    let cursor = journal.last_seq().unwrap();
    let (cache, _sender, mut inputs) = make_runtime();
    let report = orderhub_gateway::recovery::restore_native(&journal, &mut inputs, &cache).unwrap();
    assert_eq!(report.cursor, cursor);
    assert!(report.uncertain_orders.is_empty());
    assert!(
        inputs.check_ready().is_err(),
        "historical facts must not become fresh inputs"
    );
    let native = cache.borrow().order(&id).unwrap().clone();
    assert_eq!(
        native.filled_qty().as_decimal(),
        rust_decimal::Decimal::from(2)
    );
    assert_eq!(native.venue_order_id().unwrap().as_str(), "V-P6");
    let position = PositionId::from("AAPL.GOX-ORDERHUB-S001");
    assert_eq!(
        cache
            .borrow()
            .position(&position)
            .unwrap()
            .quantity
            .as_decimal(),
        rust_decimal::Decimal::from(2)
    );
    assert!(nautilus_common::runner::trading_cmd_queue_is_empty());
    assert_eq!(journal.last_seq().unwrap(), cursor);
}

#[rstest::rstest]
#[case::checkpoint(false)]
#[case::write_failure(true)]
#[tokio::test]
async fn test_graceful_core_shutdown_commits_checkpoint_or_reports_failure(
    #[case] write_failure: bool,
) {
    use nautilus_common::cache::Cache;
    use nautilus_common::clock::VirtualClock;
    use nautilus_model::identifiers::TraderId;
    use orderhub_gateway::auth::{SubmitterCredentials, SubmitterRegistry};
    use orderhub_gateway::bridge::OrderHubBridge;
    use orderhub_gateway::grpc::CoreHandle;
    use orderhub_gateway::readiness::{Readiness, ReadinessGate};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    let dir = directory();
    let journal = Arc::new(BusinessJournal::open(&dir.path().join("stop.redb")).unwrap());
    let gate = ReadinessGate::new();
    gate.set(Readiness::Ready, "");
    let core_journal = journal.clone();
    let core_gate = gate.clone();
    let core = CoreHandle::spawn(
        move || {
            let cache = Rc::new(RefCell::new(Cache::default()));
            let bridge = OrderHubBridge::new(
                TraderId::from("ORDER-HUB"),
                Rc::new(RefCell::new(VirtualClock::new())),
                cache.clone(),
                core_journal,
            )
            .with_gate(core_gate);
            (bridge, cache, None, None)
        },
        journal.clone(),
        Arc::new(SubmitterRegistry::new(vec![SubmitterCredentials {
            token: "p6".into(),
            submitter_id: "owner".into(),
            strategies: vec!["ORDERHUB-S001".into()],
        }])),
        gate.clone(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    let service = orderhub_proto::orderhub::v1::order_hub_server::OrderHubServer::new(
        orderhub_gateway::grpc::OrderHubService::new(core.clone()),
    );
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming_shutdown(incoming, async {
                let _ = stop_rx.await;
            })
            .await
            .unwrap();
    });
    let mut client = orderhub_proto::orderhub::v1::order_hub_client::OrderHubClient::connect(
        format!("http://{address}"),
    )
    .await
    .unwrap();
    let mut query = tonic::Request::new(orderhub_proto::orderhub::v1::GetStateRequest {
        query: Some(
            orderhub_proto::orderhub::v1::get_state_request::Query::Snapshot(
                orderhub_proto::orderhub::v1::SnapshotQuery {
                    page_size: 10,
                    page_token: String::new(),
                },
            ),
        ),
    });
    query
        .metadata_mut()
        .insert("authorization", "Bearer p6".parse().unwrap());
    let state = client.get_state(query).await.unwrap().into_inner();
    let mut watch = tonic::Request::new(orderhub_proto::orderhub::v1::WatchEventsRequest {
        journal_epoch: state.journal_epoch,
        after: state.state_cursor,
        resume_token: state.resume_token,
    });
    watch
        .metadata_mut()
        .insert("authorization", "Bearer p6".parse().unwrap());
    let mut stream = client.watch_events(watch).await.unwrap().into_inner();
    let checkpoint_dir = dir.path().join("checkpoints");
    if write_failure {
        std::fs::write(&checkpoint_dir, b"preserve blocker").unwrap();
    }
    let result = core.shutdown(checkpoint_dir, identity()).await;
    let expected = if write_failure {
        assert!(result.is_err());
        Readiness::Halted
    } else {
        let checkpoint = result.unwrap();
        assert!(checkpoint.exists());
        journal.verify_checkpoint(&checkpoint).unwrap();
        Readiness::Stopped
    };
    assert_eq!(gate.get(), expected);
    assert_eq!(
        journal
            .snapshot("owner", &[], 10)
            .unwrap()
            .control
            .unwrap()
            .readiness,
        expected.as_str()
    );
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        while let Ok(Some(_)) = stream.message().await {}
    })
    .await;
    assert!(closed.is_ok(), "stopped node did not close its wire stream");
    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
}

#[test]
fn test_native_recovery_rejects_account_direction_and_fee_mismatch() {
    use nautilus_common::cache::Cache;
    use nautilus_common::clock::{Clock, VirtualClock};
    use nautilus_common::factories::event::OrderEventFactory;
    use nautilus_core::UnixNanos;
    use nautilus_model::enums::{AccountType, LiquiditySide, OrderSide};
    use nautilus_model::events::OrderEventAny;
    use nautilus_model::identifiers::{
        AccountId, ClientOrderId, StrategyId, TradeId, TraderId, VenueOrderId,
    };
    use nautilus_model::types::{Currency, Money, Price, Quantity};
    use nautilus_portfolio::Portfolio;
    use orderhub_gateway::inputs::InputRuntime;
    use std::cell::RefCell;
    use std::rc::Rc;
    for mismatch in ["account", "direction", "fee"] {
        let dir = directory();
        let journal = BusinessJournal::open(&dir.path().join("j.redb")).unwrap();
        let order = admit(&journal);
        let make_runtime = || {
            let cache = Rc::new(RefCell::new(Cache::default()));
            let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
            let portfolio = Portfolio::new(clock.clone(), cache.clone(), None);
            let (_sender, inputs) = InputRuntime::channel(
                cache.clone(),
                portfolio,
                clock,
                "local-fixture-v1",
                vec![StrategyId::from("ORDERHUB-S001")],
                1024,
            )
            .unwrap();
            (cache, inputs)
        };
        let (cache, mut inputs) = make_runtime();
        orderhub_gateway::recovery::restore_native(&journal, &mut inputs, &cache).unwrap();
        let native = cache
            .borrow()
            .order(&ClientOrderId::from(order.client_order_id.as_str()))
            .unwrap()
            .clone();
        let factory = OrderEventFactory::new(
            TraderId::from("ORDER-HUB"),
            AccountId::from(if mismatch == "account" {
                "GOX-999"
            } else {
                "GOX-001"
            }),
            AccountType::Cash,
            None,
        );
        let ts = UnixNanos::from(1_800_000_000_000_000_020);
        let mut event = factory.generate_order_filled(
            &native,
            VenueOrderId::from("V-P6"),
            None,
            TradeId::from("p6-trade"),
            Quantity::from("2"),
            Price::from("150.00"),
            Currency::USD(),
            Some(Money::from_decimal(rust_decimal::Decimal::new(50, 2), Currency::USD()).unwrap()),
            LiquiditySide::Taker,
            ts,
            ts,
        );
        if let OrderEventAny::Filled(value) = &mut event {
            if mismatch == "direction" {
                value.order_side = OrderSide::Sell;
            }
            if mismatch == "fee" {
                value.commission = Some(
                    Money::from_decimal(rust_decimal::Decimal::new(75, 2), Currency::USD())
                        .unwrap(),
                );
            }
        }
        let mut fact = fill(&order);
        fact.native_event = Some(serde_json::to_string(&event).unwrap());
        journal
            .append_outcome(
                order.admission.seq,
                JournalOutcome::Execution(Box::new(fact)),
            )
            .unwrap();
        let (cache, mut inputs) = make_runtime();
        let error =
            orderhub_gateway::recovery::restore_native(&journal, &mut inputs, &cache).unwrap_err();
        assert!(
            error.to_string().contains(match mismatch {
                "account" => "attribution mismatch",
                "direction" => "direction differs",
                _ => "fill differs",
            }),
            "{mismatch}: {error}"
        );
        assert!(nautilus_common::runner::trading_cmd_queue_is_empty());
    }
}
