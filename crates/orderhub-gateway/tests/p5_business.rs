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

//! P5 atomic admission, reservation, execution, and deterministic storage fault acceptance.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use orderhub_gateway::journal::{
    AdmissionRecord, BusinessJournal, JournalError, JournalOutcome, PendingEntry,
};
use orderhub_gateway::ledger::{ExecutionRecord, RiskContext};
use rstest::rstest;
use rust_decimal::Decimal;

fn record(id: &str, strategy: &str, side: &str, quantity: &str, price: &str) -> AdmissionRecord {
    AdmissionRecord {
        journal_epoch: 0,
        seq: 0,
        submitter_id: strategy.into(),
        request_id: id.into(),
        request_kind: "SubmitOrder".into(),
        request_fingerprint: format!("{strategy}:{side}:{quantity}:{price}"),
        strategy_id: strategy.into(),
        instrument_id: "AAPL.GOX".into(),
        order_side: side.into(),
        quantity: quantity.into(),
        price: price.into(),
        time_in_force: "DAY".into(),
        post_only: false,
        reduce_only: false,
        submit_before_ns: None,
        ts_init_ns: 1,
    }
}

fn context(budget: i64, owned: i64) -> RiskContext {
    RiskContext {
        account_id: "GOX-001".into(),
        strategy_budget: Decimal::from(budget),
        account_budget: Decimal::from(budget),
        owned_quantity: Decimal::from(owned),
        owned_cost: Decimal::from(owned * 10),
        account_quantity: Decimal::from(owned),
        strategy_exposure_limit: Decimal::from(2_000_000),
        account_exposure_limit: Decimal::from(2_000_000),
        reference_price: Decimal::from(10),
        fee_buffer: Decimal::ONE,
        maximum_quantity: Decimal::from(10_000),
        maximum_notional: Decimal::from(1_000_000),
        risk_config_version: "test-v1".into(),
    }
}

fn journal() -> (tempfile::TempDir, Arc<BusinessJournal>) {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(BusinessJournal::open(&dir.path().join("business.redb")).unwrap());
    (dir, journal)
}

fn fact(
    order: &orderhub_gateway::ledger::BusinessOrder,
    id: &str,
    kind: &str,
    quantity: &str,
    price: &str,
    fee: &str,
) -> ExecutionRecord {
    ExecutionRecord {
        native_event: None,
        event_id: format!("event-{id}"),
        trade_id: if kind == "FILL" {
            Some(id.into())
        } else {
            None
        },
        client_order_id: order.client_order_id.clone(),
        account_id: order.account_id.clone(),
        strategy_id: order.admission.strategy_id.clone(),
        instrument_id: order.admission.instrument_id.clone(),
        kind: kind.into(),
        quantity: quantity.into(),
        price: price.into(),
        fee: fee.into(),
        currency: "USD".into(),
        ts_event_ns: 100,
        ts_received_ns: 101,
        reason: String::new(),
    }
}

#[rstest]
fn test_concurrent_same_key_creates_one_durable_order_and_hold() {
    let (_dir, journal) = journal();
    let worker = orderhub_gateway::worker::PersistenceWorker::start(journal.clone());
    let mut threads = Vec::with_capacity(100);
    // Spawn all callers before joining any of them; lazy joining serializes this case.
    for _ in 0..100 {
        let worker = worker.clone();
        threads.push(std::thread::spawn(move || {
            worker
                .submit_business_admission(
                    record("same", "STRAT-S001", "BUY", "6", "10"),
                    Some(context(100, 0)),
                    None,
                )
                .unwrap()
        }));
    }
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert!(results.iter().all(|record| record.seq == results[0].seq));
    assert_eq!(journal.last_seq().unwrap(), 1);
    let (_, orders) = journal.snapshot_orders(32).unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].reserved_cash, "61");
    assert_eq!(
        journal
            .find_request("STRAT-S001", "SubmitOrder", "same")
            .unwrap()
            .unwrap()
            .result
            .result,
        "ADMITTED"
    );
}

#[rstest]
fn test_two_orders_compete_for_account_budget_across_strategies() {
    let (_dir, journal) = journal();
    for (id, strategy) in [("one", "STRAT-S001"), ("two", "STRAT-S002")] {
        journal
            .admit_business(
                record(id, strategy, "BUY", "6", "10"),
                Some(context(100, 0)),
                None,
            )
            .unwrap();
    }
    let first = journal
        .find_request("STRAT-S001", "SubmitOrder", "one")
        .unwrap()
        .unwrap();
    let second = journal
        .find_request("STRAT-S002", "SubmitOrder", "two")
        .unwrap()
        .unwrap();
    assert_eq!(first.result.result, "ADMITTED");
    assert_eq!(second.result.result, "REJECTED");
    assert_eq!(second.result.reason, "ACCOUNT_BUDGET");
    assert!(journal.order(second.admission.seq).unwrap().is_none());
}

#[rstest]
fn test_strategy_limit_applies_with_larger_account_cap() {
    let (_dir, journal) = journal();
    let mut limits = context(100, 0);
    limits.strategy_budget = Decimal::from(50);
    journal
        .admit_business(
            record("one", "STRAT-S001", "BUY", "6", "10"),
            Some(limits),
            None,
        )
        .unwrap();
    assert_eq!(
        journal
            .find_request("STRAT-S001", "SubmitOrder", "one")
            .unwrap()
            .unwrap()
            .result
            .reason,
        "STRATEGY_BUDGET"
    );
}

#[rstest]
fn test_partial_fill_duplicate_cancel_race_and_late_fee_commit_once() {
    let (_dir, journal) = journal();
    let admission = journal
        .admit_business(
            record("one", "STRAT-S001", "BUY", "10", "10"),
            Some(context(1000, 0)),
            None,
        )
        .unwrap();
    let order = journal.order(admission.seq).unwrap().unwrap();
    journal
        .append_outcome(
            admission.seq,
            JournalOutcome::DispatchStarted {
                client_order_id: order.client_order_id.clone(),
            },
        )
        .unwrap();
    let fill = fact(&order, "trade1", "FILL", "3", "9", "0.25");
    journal
        .append_outcome(
            admission.seq,
            JournalOutcome::Execution(Box::new(fill.clone())),
        )
        .unwrap();
    let cursor = journal.last_seq().unwrap();
    let mut duplicate = fill;
    duplicate.event_id = "replay".into();
    duplicate.ts_event_ns = 200;
    duplicate.ts_received_ns = 201;
    journal
        .append_outcome(
            admission.seq,
            JournalOutcome::Execution(Box::new(duplicate)),
        )
        .unwrap();
    assert_eq!(journal.last_seq().unwrap(), cursor);
    let partial = journal.order(admission.seq).unwrap().unwrap();
    assert_eq!(partial.filled_quantity, "3");
    assert_eq!(partial.filled_notional, "27");
    assert_eq!(partial.reserved_cash, "70.75");
    assert_eq!(partial.status, "PARTIALLY_FILLED");
    journal
        .append_outcome(
            admission.seq,
            JournalOutcome::Execution(Box::new(fact(&order, "cancel", "CANCELED", "0", "0", "0"))),
        )
        .unwrap();
    let canceled = journal.order(admission.seq).unwrap().unwrap();
    assert_eq!(canceled.status, "CANCELED");
    assert_eq!(canceled.reserved_cash, "0.75");
    // A delayed earlier fill remains a fact after the cancellation arrived
    journal
        .append_outcome(
            admission.seq,
            JournalOutcome::Execution(Box::new(fact(&order, "trade2", "FILL", "2", "10", "0.1"))),
        )
        .unwrap();
    let late = fact(&order, "fee", "FEE", "0", "0", "0.2");
    journal
        .append_outcome(
            admission.seq,
            JournalOutcome::Execution(Box::new(late.clone())),
        )
        .unwrap();
    journal
        .append_outcome(admission.seq, JournalOutcome::Execution(Box::new(late)))
        .unwrap();
    let canceled = journal.order(admission.seq).unwrap().unwrap();
    assert_eq!(canceled.status, "CANCELED");
    assert_eq!(canceled.filled_quantity, "5");
    assert_eq!(
        Decimal::from_str_exact(&canceled.fees).unwrap(),
        Decimal::new(55, 2)
    );
    assert_eq!(
        Decimal::from_str_exact(&canceled.reserved_cash).unwrap(),
        Decimal::new(45, 2)
    );
    let original = journal
        .find_request("STRAT-S001", "SubmitOrder", "one")
        .unwrap()
        .unwrap();
    assert_eq!(original.result.result, "ADMITTED");
    assert_eq!(original.result.version, admission.seq);
}

#[rstest]
fn test_full_fill_wins_late_cancel_and_terminal_lookup_survives_reopen() {
    let (dir, journal) = journal();
    let admission = journal
        .admit_business(
            record("one", "STRAT-S001", "BUY", "2", "10"),
            Some(context(100, 0)),
            None,
        )
        .unwrap();
    let order = journal.order(admission.seq).unwrap().unwrap();
    journal
        .append_outcome(
            admission.seq,
            JournalOutcome::Execution(Box::new(fact(&order, "full", "FILL", "2", "10", "0"))),
        )
        .unwrap();
    journal
        .append_outcome(
            admission.seq,
            JournalOutcome::Execution(Box::new(fact(&order, "cancel", "CANCELED", "0", "0", "0"))),
        )
        .unwrap();
    let cursor = journal.last_seq().unwrap();
    let before = journal
        .snapshot("STRAT-S001", &["STRAT-S001".into()], 10)
        .unwrap();
    let epoch = journal.epoch();
    drop(journal);
    let reopened = BusinessJournal::open(&dir.path().join("business.redb")).unwrap();
    assert_eq!(reopened.last_seq().unwrap(), cursor);
    assert_eq!(reopened.epoch(), epoch);
    let after = reopened
        .snapshot("STRAT-S001", &["STRAT-S001".into()], 10)
        .unwrap();
    assert_eq!(after.positions, before.positions);
    assert_eq!(after.accounts, before.accounts);
    let (view_cursor, orders) = reopened
        .lookup_orders(
            &[order.client_order_id],
            "STRAT-S001",
            &["STRAT-S001".into()],
        )
        .unwrap();
    assert_eq!(view_cursor, cursor);
    assert_eq!(orders[0].status, "FILLED");
    assert_eq!(orders[0].filled_quantity, "2");
    assert_eq!(
        reopened
            .find_request("STRAT-S001", "SubmitOrder", "one")
            .unwrap()
            .unwrap()
            .result
            .result,
        "ADMITTED"
    );
}

#[rstest]
#[case("10001", "10", "SINGLE_ORDER_LIMIT")]
fn test_durable_business_limit_rejection(
    #[case] quantity: &str,
    #[case] price: &str,
    #[case] reason: &str,
) {
    let (_dir, journal) = journal();
    journal
        .admit_business(
            record("one", "STRAT-S001", "BUY", quantity, price),
            Some(context(1_000_000, 0)),
            None,
        )
        .unwrap();
    let result = journal
        .find_request("STRAT-S001", "SubmitOrder", "one")
        .unwrap()
        .unwrap();
    assert_eq!(result.result.result, "REJECTED");
    assert_eq!(result.result.reason, reason);
    assert!(journal.order(result.admission.seq).unwrap().is_none());
}

#[rstest]
#[case("BUY", "8.99")]
#[case("BUY", "11.01")]
#[case("SELL", "8.99")]
#[case("SELL", "11.01")]
fn test_orders_outside_the_retired_price_band_keep_their_limit(
    #[case] side: &str,
    #[case] price: &str,
) {
    let (_dir, journal) = journal();
    let admitted = journal
        .admit_business(
            record("outside-band", "STRAT-S001", side, "5", price),
            Some(context(1_000_000, 10)),
            None,
        )
        .unwrap();
    let original = journal
        .find_request("STRAT-S001", "SubmitOrder", "outside-band")
        .unwrap()
        .unwrap();
    assert_eq!(original.result.result, "ADMITTED");
    assert_eq!(original.admission.price, price);
    let order = journal.order(admitted.seq).unwrap().unwrap();
    assert_eq!(order.admission.price, price);
    assert_eq!(
        order.reserved_quantity,
        if side == "SELL" { "5" } else { "0" }
    );
}

#[rstest]
fn test_historical_price_band_rejection_is_not_rejudged_after_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy-policy.redb");
    let journal = BusinessJournal::open(&path).unwrap();
    let request = record("old", "STRAT-S001", "BUY", "5", "11.01");
    let mut old_context = context(1_000_000, 0);
    old_context.risk_config_version = "local-usd-v1".into();
    let original = journal
        .admit_business(
            request.clone(),
            Some(old_context),
            Some("PRICE_BAND".into()),
        )
        .unwrap();
    drop(journal);
    let reopened = BusinessJournal::open(&path).unwrap();
    let before = reopened.last_seq().unwrap();
    let mut new_context = context(1_000_000, 0);
    new_context.risk_config_version = "local-usd-v2".into();
    let repeated = reopened
        .admit_business(request, Some(new_context.clone()), None)
        .unwrap();
    assert_eq!(repeated.seq, original.seq);
    assert_eq!(reopened.last_seq().unwrap(), before);
    let previous = reopened
        .find_request("STRAT-S001", "SubmitOrder", "old")
        .unwrap()
        .unwrap();
    assert_eq!(previous.result.result, "REJECTED");
    assert_eq!(previous.result.reason, "PRICE_BAND");
    assert!(reopened.order(original.seq).unwrap().is_none());
    let next = reopened
        .admit_business(
            record("new", "STRAT-S001", "BUY", "5", "11.01"),
            Some(new_context),
            None,
        )
        .unwrap();
    assert_eq!(
        reopened
            .order(next.seq)
            .unwrap()
            .unwrap()
            .risk_config_version,
        "local-usd-v2"
    );
    assert_eq!(reopened.last_seq().unwrap(), before + 1);
}

#[rstest]
fn test_sell_holds_and_reduced_external_ownership_are_conservative() {
    let (_dir, journal) = journal();
    let admitted = journal
        .admit_business(
            record("one", "STRAT-S001", "SELL", "6", "10"),
            Some(context(1000, 10)),
            None,
        )
        .unwrap();
    let order = journal.order(admitted.seq).unwrap().unwrap();
    assert_eq!(order.reserved_quantity, "6");
    let mut newer = context(1000, 5);
    newer.owned_quantity = Decimal::from(5);
    journal
        .admit_business(
            record("two", "STRAT-S001", "SELL", "1", "10"),
            Some(newer),
            None,
        )
        .unwrap();
    assert_eq!(
        journal
            .find_request("STRAT-S001", "SubmitOrder", "two")
            .unwrap()
            .unwrap()
            .result
            .reason,
        "SELLABLE_QUANTITY"
    );
}

#[rstest]
fn test_conflicting_fill_identity_aborts_all_economic_changes() {
    let (_dir, journal) = journal();
    let admitted = journal
        .admit_business(
            record("one", "STRAT-S001", "BUY", "10", "10"),
            Some(context(1000, 0)),
            None,
        )
        .unwrap();
    let order = journal.order(admitted.seq).unwrap().unwrap();
    let fill = fact(&order, "trade1", "FILL", "2", "10", "0");
    journal
        .append_outcome(
            admitted.seq,
            JournalOutcome::Execution(Box::new(fill.clone())),
        )
        .unwrap();
    let cursor = journal.last_seq().unwrap();
    let mut conflict = fill;
    conflict.quantity = "3".into();
    assert!(
        journal
            .append_outcome(admitted.seq, JournalOutcome::Execution(Box::new(conflict)))
            .is_err()
    );
    assert_eq!(journal.last_seq().unwrap(), cursor);
    assert_eq!(
        journal
            .order(admitted.seq)
            .unwrap()
            .unwrap()
            .filled_quantity,
        "2"
    );
}

#[rstest]
fn test_failed_mixed_transaction_does_not_leave_request_or_reservation() {
    let (_dir, journal) = journal();
    let items = [
        PendingEntry::BusinessAdmission {
            record: Box::new(record("one", "STRAT-S001", "BUY", "2", "10")),
            context: Some(Box::new(context(100, 0))),
            rejection: None,
        },
        PendingEntry::Outcome {
            origin_seq: 999,
            outcome: JournalOutcome::Execution(Box::new(ExecutionRecord {
                native_event: None,
                event_id: "bad".into(),
                trade_id: None,
                client_order_id: "bad".into(),
                account_id: "bad".into(),
                strategy_id: "bad".into(),
                instrument_id: "bad".into(),
                kind: "FILL".into(),
                quantity: "1".into(),
                price: "10".into(),
                fee: "0".into(),
                currency: "USD".into(),
                ts_event_ns: 0,
                ts_received_ns: 0,
                reason: String::new(),
            })),
        },
    ];
    assert!(journal.append_batch(&items).is_err());
    assert_eq!(journal.last_seq().unwrap(), 0);
    assert!(
        journal
            .find_request("STRAT-S001", "SubmitOrder", "one")
            .unwrap()
            .is_none()
    );
    assert!(journal.snapshot_orders(10).unwrap().1.is_empty());
}

#[rstest]
fn test_cancel_before_dispatch_releases_hold_atomically() {
    let (_dir, journal) = journal();
    let admission = journal
        .admit_business(
            record("one", "STRAT-S001", "BUY", "6", "10"),
            Some(context(100, 0)),
            None,
        )
        .unwrap();
    let order = journal.order(admission.seq).unwrap().unwrap();
    let mut cancel = record("cancel", "STRAT-S001", "CANCEL", "-", "-");
    cancel.request_kind = "CancelOrder".into();
    cancel.instrument_id = format!("CANCEL:{}", order.client_order_id);
    journal.admit_business(cancel, None, None).unwrap();
    let terminal = journal.order(admission.seq).unwrap().unwrap();
    assert_eq!(terminal.status, "CANCELED");
    assert_eq!(terminal.reserved_cash, "0");
    assert_eq!(
        journal
            .find_request("STRAT-S001", "CancelOrder", "cancel")
            .unwrap()
            .unwrap()
            .result
            .reason,
        "LOCAL_CANCELED"
    );
    journal
        .admit_business(
            record("two", "STRAT-S001", "BUY", "6", "10"),
            Some(context(100, 0)),
            None,
        )
        .unwrap();
    assert_eq!(
        journal
            .find_request("STRAT-S001", "SubmitOrder", "two")
            .unwrap()
            .unwrap()
            .result
            .result,
        "ADMITTED"
    );
}

#[derive(Debug)]
struct FailingSync {
    inner: redb::backends::InMemoryBackend,
    failed: Arc<AtomicBool>,
}

impl redb::StorageBackend for FailingSync {
    fn len(&self) -> std::io::Result<u64> {
        redb::StorageBackend::len(&self.inner)
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> std::io::Result<()> {
        redb::StorageBackend::read(&self.inner, offset, out)
    }
    fn set_len(&self, len: u64) -> std::io::Result<()> {
        redb::StorageBackend::set_len(&self.inner, len)
    }
    fn write(&self, offset: u64, data: &[u8]) -> std::io::Result<()> {
        redb::StorageBackend::write(&self.inner, offset, data)
    }
    fn sync_data(&self) -> std::io::Result<()> {
        if self.failed.load(Ordering::Acquire) {
            Err(std::io::Error::other("injected sync failure"))
        } else {
            redb::StorageBackend::sync_data(&self.inner)
        }
    }
}

#[rstest]
fn test_actual_redb_sync_failure_is_unknown_and_keeps_tentative_occupancy() {
    use nautilus_common::clock::{Clock, VirtualClock};
    use nautilus_model::enums::{OrderSide, TimeInForce};
    use nautilus_model::identifiers::{InstrumentId, StrategyId, TraderId};
    use nautilus_model::types::{Price, Quantity};
    use orderhub_gateway::bridge::{LimitOrderRequest, OrderHubBridge};
    use orderhub_gateway::readiness::{Readiness, ReadinessGate};
    use std::cell::RefCell;
    use std::rc::Rc;

    let failed = Arc::new(AtomicBool::new(false));
    let db = redb::Database::builder()
        .create_with_backend(FailingSync {
            inner: redb::backends::InMemoryBackend::new(),
            failed: failed.clone(),
        })
        .unwrap();
    let journal = Arc::new(BusinessJournal::from_database(db).unwrap());
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let quota = Rc::new(RefCell::new(orderhub_gateway::quota::QuotaLedger::new()));
    quota
        .borrow_mut()
        .set_budget("STRAT-S001", Decimal::from(100));
    let gate = ReadinessGate::new();
    gate.set(Readiness::Ready, "");
    let bridge = OrderHubBridge::new(
        TraderId::from("ORDER-HUB"),
        clock,
        Rc::new(RefCell::new(nautilus_common::cache::Cache::default())),
        journal,
    )
    .with_quota(quota.clone())
    .with_gate(gate.clone());
    failed.store(true, Ordering::Release);
    let request = LimitOrderRequest {
        submitter_id: "STRAT-S001".into(),
        request_id: "fault".into(),
        strategy_id: StrategyId::from("STRAT-S001"),
        instrument_id: InstrumentId::from("AAPL.GOX"),
        side: OrderSide::Buy,
        quantity: Quantity::from("1"),
        price: Price::from("60"),
        time_in_force: TimeInForce::Day,
        post_only: false,
        reduce_only: false,
        client_id: None,
        submit_before_ns: None,
    };
    assert!(bridge.submit_limit_order(&request).is_err());
    assert_eq!(gate.get(), Readiness::Halted);
    assert_eq!(quota.borrow().available("STRAT-S001"), Decimal::from(40));
    assert!(gate.halt_reason().contains("unknown"));
}

#[rstest]
#[case(None, 1)]
#[case(Some(99), 0)]
fn test_unversioned_or_unknown_schema_is_rejected_without_business_migration(
    #[case] schema: Option<u64>,
    #[case] last_seq: u64,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.redb");
    let db = redb::Database::create(&path).unwrap();
    let write = db.begin_write().unwrap();
    {
        let mut meta = write
            .open_table(redb::TableDefinition::<u64, u64>::new("orderhub_meta"))
            .unwrap();
        meta.insert(2, last_seq).unwrap();
        if let Some(schema) = schema {
            meta.insert(3, schema).unwrap();
        }
    }
    write.commit().unwrap();
    drop(db);
    assert!(matches!(
        BusinessJournal::open(&path),
        Err(JournalError::Encoding(_))
    ));
}

#[rstest]
fn test_sell_cost_release_rounds_conservatively_and_closes_exactly() {
    let (_dir, journal) = journal();
    let mut limits = context(100, 3);
    limits.owned_cost = Decimal::new(1_000, 2);
    let admitted = journal
        .admit_business(
            record("sell", "STRAT-S001", "SELL", "3", "10"),
            Some(limits),
            None,
        )
        .unwrap();
    let order = journal.order(admitted.seq).unwrap().unwrap();
    journal
        .append_outcome(
            admitted.seq,
            JournalOutcome::Execution(Box::new(fact(&order, "one", "FILL", "1", "10", "0"))),
        )
        .unwrap();
    let view = journal
        .snapshot("STRAT-S001", &["STRAT-S001".into()], 10)
        .unwrap();
    assert_eq!(
        view.positions[0].cost.parse::<Decimal>().unwrap(),
        Decimal::new(667, 2)
    );
    assert_eq!(view.positions[0].quantity, "2");
    journal
        .append_outcome(
            admitted.seq,
            JournalOutcome::Execution(Box::new(fact(&order, "rest", "FILL", "2", "10", "0"))),
        )
        .unwrap();
    let view = journal
        .snapshot("STRAT-S001", &["STRAT-S001".into()], 10)
        .unwrap();
    assert_eq!(
        view.positions[0].cost.parse::<Decimal>().unwrap(),
        Decimal::ZERO
    );
    assert_eq!(view.positions[0].quantity, "0");
}

fn native_fill(
    order: &orderhub_gateway::ledger::BusinessOrder,
    fee: &str,
) -> nautilus_model::events::OrderEventAny {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::enums::{LiquiditySide, OrderSide, OrderType};
    use nautilus_model::identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
    };
    use nautilus_model::types::{Currency, Money, Price, Quantity};
    nautilus_model::events::OrderEventAny::Filled(nautilus_model::events::OrderFilled::new(
        TraderId::from("ORDER-HUB"),
        StrategyId::from(order.admission.strategy_id.as_str()),
        InstrumentId::from("AAPL.GOX"),
        ClientOrderId::from(order.client_order_id.as_str()),
        VenueOrderId::from("V1"),
        AccountId::from("GOX-001"),
        TradeId::from("T1"),
        OrderSide::Buy,
        OrderType::Limit,
        Quantity::from("1"),
        Price::from("10"),
        Currency::USD(),
        LiquiditySide::Taker,
        UUID4::new(),
        UnixNanos::from(100),
        UnixNanos::from(101),
        false,
        None,
        Some(Money::from_decimal(fee.parse().unwrap(), Currency::USD()).unwrap()),
        None,
    ))
}

#[rstest]
#[case(false)]
#[case(true)]
fn test_execution_recorder_commits_before_native_publish_and_halts_on_fault(
    #[case] fail_sync: bool,
) {
    use nautilus_common::messages::ExecutionEvent;
    use nautilus_common::msgbus::{self, MessagingSwitchboard, TypedIntoHandler};
    use orderhub_gateway::exec_wiring::{attach_outcome_recorder_with_gate, pump_exec_events};
    use orderhub_gateway::readiness::{Readiness, ReadinessGate};
    use std::cell::Cell;
    use std::rc::Rc;
    let failed = Arc::new(AtomicBool::new(false));
    let db = redb::Database::builder()
        .create_with_backend(FailingSync {
            inner: redb::backends::InMemoryBackend::new(),
            failed: failed.clone(),
        })
        .unwrap();
    let journal = Arc::new(BusinessJournal::from_database(db).unwrap());
    let admitted = journal
        .admit_business(
            record("one", "STRAT-S001", "BUY", "2", "10"),
            Some(context(100, 0)),
            None,
        )
        .unwrap();
    let order = journal.order(admitted.seq).unwrap().unwrap();
    let before = journal.last_seq().unwrap();
    let worker = orderhub_gateway::worker::PersistenceWorker::start(journal.clone());
    let gate = ReadinessGate::new();
    gate.set(Readiness::Ready, "");
    attach_outcome_recorder_with_gate(worker, gate.clone());
    let forwarded = Rc::new(Cell::new(0));
    let count = forwarded.clone();
    let journal_at_publish = journal.clone();
    msgbus::register_order_event_endpoint(
        MessagingSwitchboard::exec_engine_process(),
        TypedIntoHandler::from(move |_: nautilus_model::events::OrderEventAny| {
            assert_eq!(
                journal_at_publish
                    .order(admitted.seq)
                    .unwrap()
                    .unwrap()
                    .filled_quantity,
                "1"
            );
            count.set(count.get() + 1);
        }),
    );
    failed.store(fail_sync, Ordering::Release);
    let (tx, rx) = tokio::sync::mpsc::channel(2);
    tx.try_send(ExecutionEvent::Order(native_fill(&order, "0")))
        .unwrap();
    let mut rx = Some(rx);
    pump_exec_events(&mut rx);
    if fail_sync {
        assert_eq!(forwarded.get(), 0);
        assert_eq!(gate.get(), Readiness::Halted);
        assert_eq!(journal.last_seq().unwrap(), before);
        assert_eq!(
            journal
                .order(order.admission.seq)
                .unwrap()
                .unwrap()
                .reserved_cash,
            "21"
        );
    } else {
        assert_eq!(forwarded.get(), 1);
        assert_eq!(gate.get(), Readiness::Ready);
        tx.try_send(ExecutionEvent::Order(native_fill(&order, "0")))
            .unwrap();
        pump_exec_events(&mut rx);
        assert_eq!(journal.last_seq().unwrap(), before + 1);
        assert_eq!(
            journal
                .order(order.admission.seq)
                .unwrap()
                .unwrap()
                .filled_quantity,
            "1"
        );
    }
}

#[rstest]
fn test_fee_overrun_is_persisted_then_closes_new_risk() {
    use orderhub_gateway::exec_wiring::{attach_outcome_recorder_with_gate, pump_exec_events};
    use orderhub_gateway::readiness::{Readiness, ReadinessGate};
    let (_dir, journal) = journal();
    let admitted = journal
        .admit_business(
            record("fee", "STRAT-S001", "BUY", "2", "10"),
            Some(context(100, 0)),
            None,
        )
        .unwrap();
    let order = journal.order(admitted.seq).unwrap().unwrap();
    let gate = ReadinessGate::new();
    gate.set(Readiness::Ready, "");
    attach_outcome_recorder_with_gate(
        orderhub_gateway::worker::PersistenceWorker::start(journal.clone()),
        gate.clone(),
    );
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.try_send(nautilus_common::messages::ExecutionEvent::Order(
        native_fill(&order, "2.00"),
    ))
    .unwrap();
    pump_exec_events(&mut Some(rx));
    assert_eq!(
        journal
            .order(admitted.seq)
            .unwrap()
            .unwrap()
            .fees
            .parse::<Decimal>()
            .unwrap(),
        Decimal::from(2)
    );
    assert_eq!(gate.get(), Readiness::Halted);
    assert!(gate.halt_reason().contains("fees exceed"));
}
