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

//! P5 committed views, identity isolation, pagination, replay, and bounded stream acceptance.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use nautilus_common::cache::Cache;
use nautilus_common::clock::{Clock, VirtualClock};
use nautilus_core::UnixNanos;
use nautilus_model::identifiers::{StrategyId, TraderId};
use nautilus_portfolio::Portfolio;
use orderhub_gateway::auth::{SubmitterCredentials, SubmitterRegistry};
use orderhub_gateway::bridge::OrderHubBridge;
use orderhub_gateway::grpc::{CoreHandle, OrderHubService};
use orderhub_gateway::inputs::{InputEvent, InputRuntime, InputSender};
use orderhub_gateway::journal::{AdmissionRecord, BusinessJournal, JournalOutcome};
use orderhub_gateway::ledger::RiskContext;
use orderhub_gateway::readiness::{Readiness, ReadinessGate};
use orderhub_proto::orderhub::v1::order_hub_server::OrderHub;
use orderhub_proto::orderhub::v1::{
    CancelOrderRequest, GetStateRequest, OrderLookup, RequestLookup, SnapshotQuery,
    SubmitOrderRequest, WatchEventsRequest,
};
use orderhub_proto::orderhub::v1::{get_state_request::Query, journal_event::Kind};
use rust_decimal::Decimal;
use tokio_stream::StreamExt;
use tonic::{Code, Request};

const NOW: u64 = 1_800_000_000_000_000_000;
const STRATEGY: &str = "ORDERHUB-S001";

struct Harness {
    _dir: tempfile::TempDir,
    _sender: InputSender,
    service: OrderHubService,
    journal: Arc<BusinessJournal>,
}

fn harness() -> Harness {
    harness_with_quote_age(0)
}

fn harness_with_quote_age(quote_age: u64) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(BusinessJournal::open(&dir.path().join("business.redb")).unwrap());
    let journal_for_core = journal.clone();
    let gate = ReadinessGate::new();
    let gate_for_core = gate.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let registry = Arc::new(SubmitterRegistry::new(vec![
        SubmitterCredentials {
            token: "one".into(),
            submitter_id: "owner".into(),
            strategies: vec![STRATEGY.into()],
        },
        SubmitterCredentials {
            token: "two".into(),
            submitter_id: "other".into(),
            strategies: vec!["OTHER-S002".into()],
        },
    ]));
    let core = CoreHandle::spawn(
        move || {
            let cache = Rc::new(RefCell::new(Cache::default()));
            let clock = Rc::new(RefCell::new(VirtualClock::new()));
            clock.borrow_mut().advance_time(UnixNanos::from(NOW), true);
            let shared: Rc<RefCell<dyn Clock>> = clock;
            let portfolio = Portfolio::new(shared.clone(), cache.clone(), None);
            let (sender, inputs) = InputRuntime::channel(
                cache.clone(),
                portfolio,
                shared.clone(),
                "local-fixture-v1",
                vec![StrategyId::from(STRATEGY)],
                8,
            )
            .unwrap();
            let fixtures: Vec<InputEvent> =
                serde_json::from_str(include_str!("../test_data/p4_inputs.json")).unwrap();
            for mut event in fixtures {
                if let InputEvent::Quote { meta, .. } = &mut event {
                    meta.event_time_ns -= quote_age;
                }
                sender.try_send(event, UnixNanos::from(NOW)).unwrap();
            }
            let bridge = OrderHubBridge::new(
                TraderId::from("ORDER-HUB"),
                shared,
                cache.clone(),
                journal_for_core,
            )
            .with_gate(gate_for_core.clone())
            .with_inputs(inputs);
            bridge.poll_inputs();
            gate_for_core.set(Readiness::Ready, "");
            ready_tx.send(sender).unwrap();
            (bridge, cache, None, None)
        },
        journal.clone(),
        registry,
        gate,
    );
    let sender = ready_rx.recv().unwrap();
    Harness {
        _dir: dir,
        _sender: sender,
        service: OrderHubService::new(core),
        journal,
    }
}

fn authorized<T>(payload: T, token: &str) -> Request<T> {
    let mut request = Request::new(payload);
    request
        .metadata_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    request
}

fn request(id: &str) -> SubmitOrderRequest {
    SubmitOrderRequest {
        journal_epoch: "1".into(),
        submitter_id: "forged".into(),
        request_id: id.into(),
        strategy_id: STRATEGY.into(),
        instrument_id: "AAPL.GOX".into(),
        side: "BUY".into(),
        quantity: "2".into(),
        price: "153.22".into(),
        time_in_force: "DAY".into(),
        post_only: false,
        reduce_only: false,
        submit_before_ns: Some(NOW + 1_000_000_000),
    }
}

#[tokio::test]
async fn test_quotes_are_absent_from_snapshots_and_persistent_streams() {
    let h = harness();
    let initial = h
        .service
        .get_state(authorized(GetStateRequest::default(), "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(initial.inputs.len(), 2);
    for input in &initial.inputs {
        let json: serde_json::Value = serde_json::from_str(&input.json).unwrap();
        assert_ne!(json["kind"], "quote");
    }
    let fixtures: Vec<InputEvent> =
        serde_json::from_str(include_str!("../test_data/p4_inputs.json")).unwrap();
    let mut quote = fixtures[1].clone();
    if let InputEvent::Quote { meta, .. } = &mut quote {
        meta.sequence = 2;
    }
    h._sender.try_send(quote, UnixNanos::from(NOW)).unwrap();
    let current = h
        .service
        .get_state(authorized(GetStateRequest::default(), "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(current.state_cursor, initial.state_cursor);
    assert!(current.writable);
    let marker = append(&h.journal, "after-quote", "owner", STRATEGY);
    let mut stream = h
        .service
        .watch_events(authorized(
            WatchEventsRequest {
                after: initial.state_cursor,
                journal_epoch: initial.journal_epoch,
                resume_token: initial.resume_token,
            },
            "one",
        ))
        .await
        .unwrap()
        .into_inner();
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.seq, marker.to_string());
    assert!(matches!(event.kind, Some(Kind::Admission(_))));
}

#[tokio::test]
async fn test_old_quote_and_isolated_quote_loss_keep_rpc_writable() {
    let h = harness_with_quote_age(60_000_000_000);
    let get_state = || authorized(GetStateRequest::default(), "one");
    let initial = h.service.get_state(get_state()).await.unwrap().into_inner();
    assert!(initial.writable);
    assert!(!initial.stale);
    h._sender.invalidate_quotes();
    let after_loss = h.service.get_state(get_state()).await.unwrap().into_inner();
    assert!(after_loss.writable);
    assert!(!after_loss.stale);
    let mut order = request("old-quote-outside-band");
    order.price = "170.00".into();
    let result = h
        .service
        .submit_order(authorized(order, "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(result.result, "ADMITTED");
    let stored = h
        .journal
        .find_request("owner", "SubmitOrder", "old-quote-outside-band")
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.admission.price.parse::<Decimal>().unwrap(),
        Decimal::from(170)
    );
    assert_eq!(
        h.journal
            .order(stored.admission.seq)
            .unwrap()
            .unwrap()
            .risk_config_version,
        "local-usd-v2"
    );
}

fn append(journal: &BusinessJournal, id: &str, owner: &str, strategy: &str) -> u64 {
    let record = AdmissionRecord {
        journal_epoch: 0,
        seq: 0,
        submitter_id: owner.into(),
        request_id: id.into(),
        request_kind: "SubmitOrder".into(),
        request_fingerprint: id.into(),
        strategy_id: strategy.into(),
        instrument_id: "AAPL.GOX".into(),
        order_side: "BUY".into(),
        quantity: "2".into(),
        price: "153.22".into(),
        time_in_force: "DAY".into(),
        post_only: false,
        reduce_only: false,
        submit_before_ns: None,
        ts_init_ns: NOW,
    };
    journal
        .admit_business(
            record,
            Some(RiskContext {
                account_id: "GOX-001".into(),
                strategy_budget: Decimal::from(1_000_000),
                account_budget: Decimal::from(1_000_000),
                owned_quantity: Decimal::ZERO,
                owned_cost: Decimal::ZERO,
                account_quantity: Decimal::ZERO,
                strategy_exposure_limit: Decimal::from(2_000_000),
                account_exposure_limit: Decimal::from(2_000_000),
                reference_price: Decimal::new(15323, 2),
                fee_buffer: Decimal::ONE,
                maximum_quantity: Decimal::from(10_000),
                maximum_notional: Decimal::from(1_000_000),
                risk_config_version: "test-v1".into(),
            }),
            None,
        )
        .unwrap()
        .seq
}

#[tokio::test]
async fn test_rpc_canonical_idempotency_epoch_and_persistent_rejection() {
    let h = harness();
    let first = h
        .service
        .submit_order(authorized(request("one"), "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(first.result, "ADMITTED");
    let mut equivalent = request("one");
    equivalent.quantity = "2.000".into();
    equivalent.price = "153.2200".into();
    let repeated = h
        .service
        .submit_order(authorized(equivalent, "one"))
        .await
        .unwrap()
        .into_inner();
    assert!(repeated.already_admitted);
    assert_eq!(repeated.client_order_id, first.client_order_id);
    let mut conflict = request("one");
    conflict.quantity = "3".into();
    assert_eq!(
        h.service
            .submit_order(authorized(conflict, "one"))
            .await
            .unwrap_err()
            .code(),
        Code::AlreadyExists
    );
    let mut old = request("old");
    old.journal_epoch = "0".into();
    assert_eq!(
        h.service
            .submit_order(authorized(old, "one"))
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    let mut unsupported = request("unsupported");
    unsupported.time_in_force = "IOC".into();
    let rejected = h
        .service
        .submit_order(authorized(unsupported.clone(), "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(rejected.result, "REJECTED");
    assert!(rejected.client_order_id.is_empty());
    let guessed_id = format!("O-1-{:012}", rejected.seq.parse::<u64>().unwrap());
    assert_eq!(
        h.service
            .cancel_order(authorized(
                CancelOrderRequest {
                    journal_epoch: "1".into(),
                    submitter_id: String::new(),
                    request_id: "reject-cancel".into(),
                    client_order_id: guessed_id
                },
                "one"
            ))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let retry = h
        .service
        .submit_order(authorized(unsupported, "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(retry.result, rejected.result);
    assert_eq!(retry.reason, rejected.reason);
    assert!(retry.already_admitted);
    let query = GetStateRequest {
        query: Some(Query::RequestLookup(RequestLookup {
            journal_epoch: "1".into(),
            rpc_method: "SubmitOrder".into(),
            request_id: "unsupported".into(),
        })),
    };
    let result = h
        .service
        .get_state(authorized(query, "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(result.request_result.unwrap().result, "REJECTED");
}

#[tokio::test]
async fn test_snapshot_lookup_and_stream_do_not_leak_other_submitters() {
    let h = harness();
    let initial = h
        .service
        .get_state(authorized(GetStateRequest::default(), "one"))
        .await
        .unwrap()
        .into_inner();
    let other = append(&h.journal, "other-request", "other", "OTHER-S002");
    let own = append(&h.journal, "own-request", "owner", STRATEGY);
    let snapshot = h
        .service
        .get_state(authorized(GetStateRequest::default(), "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(snapshot.active_orders.len(), 1);
    assert_eq!(
        snapshot.active_orders[0].client_order_id,
        h.journal.order(own).unwrap().unwrap().client_order_id
    );
    assert!(
        snapshot
            .accounts
            .iter()
            .all(|account| account.strategy_id == STRATEGY)
    );
    let lookup = GetStateRequest {
        query: Some(Query::OrderLookup(OrderLookup {
            client_order_ids: vec![h.journal.order(other).unwrap().unwrap().client_order_id],
        })),
    };
    assert_eq!(
        h.service
            .get_state(authorized(lookup, "one"))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let mut stream = h
        .service
        .watch_events(authorized(
            WatchEventsRequest {
                after: initial.state_cursor,
                journal_epoch: initial.journal_epoch,
                resume_token: initial.resume_token,
            },
            "one",
        ))
        .await
        .unwrap()
        .into_inner();

    for _ in 0..4 {
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Some(Kind::Admission(admission)) = event.kind {
            assert_eq!(admission.submitter_id, "owner");
            assert_eq!(admission.request_id, "own-request");
            return;
        }
    }
    panic!("own committed admission missing from replay");
}

#[tokio::test]
async fn test_pages_keep_fixed_cursor_while_replay_covers_later_commits() {
    let h = harness();
    let first = append(&h.journal, "a", "owner", STRATEGY);
    let second = append(&h.journal, "b", "owner", STRATEGY);
    append(&h.journal, "c", "owner", STRATEGY);
    let query = GetStateRequest {
        query: Some(Query::Snapshot(SnapshotQuery {
            page_size: 1,
            page_token: String::new(),
        })),
    };
    let page_one = h
        .service
        .get_state(authorized(query, "one"))
        .await
        .unwrap()
        .into_inner();
    let cursor = page_one.state_cursor.clone();
    h.journal
        .append_outcome(
            second,
            JournalOutcome::Denied {
                reason: "after-snapshot".into(),
            },
        )
        .unwrap();
    let query = GetStateRequest {
        query: Some(Query::Snapshot(SnapshotQuery {
            page_size: 1,
            page_token: page_one.next_page_token,
        })),
    };
    let page_two = h
        .service
        .get_state(authorized(query, "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(page_two.state_cursor, cursor);
    assert_eq!(page_two.snapshot_id, page_one.snapshot_id);
    assert_eq!(page_two.active_orders[0].status, "PENDING_DISPATCH");
    assert_eq!(
        page_one.active_orders[0].client_order_id,
        h.journal.order(first).unwrap().unwrap().client_order_id
    );
    let mut stream = h
        .service
        .watch_events(authorized(
            WatchEventsRequest {
                after: cursor,
                journal_epoch: "1".into(),
                resume_token: page_one.resume_token,
            },
            "one",
        ))
        .await
        .unwrap()
        .into_inner();

    let event = stream.next().await.unwrap().unwrap();
    assert!(
        matches!(event.kind, Some(Kind::Outcome(outcome)) if outcome.reason == "after-snapshot")
    );
}

#[tokio::test]
async fn test_signed_cursor_rejects_forgery_and_other_credentials() {
    let h = harness();
    let snapshot = h
        .service
        .get_state(authorized(GetStateRequest::default(), "one"))
        .await
        .unwrap()
        .into_inner();
    let query = WatchEventsRequest {
        after: snapshot.state_cursor,
        journal_epoch: snapshot.journal_epoch,
        resume_token: snapshot.resume_token,
    };
    assert_eq!(
        h.service
            .watch_events(authorized(query.clone(), "two"))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let mut forged = query;
    let last = forged.resume_token.pop().unwrap();
    forged
        .resume_token
        .push(if last == 'a' { 'b' } else { 'a' });
    assert_eq!(
        h.service
            .watch_events(authorized(forged, "one"))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
}

#[tokio::test]
async fn test_oversized_fields_and_decimal_exponents_are_rejected_before_journaling() {
    let h = harness();
    let before = h.journal.last_seq().unwrap();
    for value in ["1e3".to_string(), "-1".into(), "1".repeat(41)] {
        let mut invalid = request("bad");
        invalid.quantity = value;
        assert_eq!(
            h.service
                .submit_order(authorized(invalid, "one"))
                .await
                .unwrap_err()
                .code(),
            Code::InvalidArgument
        );
    }
    let mut invalid = request("bad");
    invalid.request_id = "x".repeat(65);
    assert_eq!(
        h.service
            .submit_order(authorized(invalid, "one"))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert_eq!(h.journal.last_seq().unwrap(), before);
}

#[tokio::test]
async fn test_snapshot_final_page_releases_lease_capacity() {
    let h = harness();
    for _ in 0..20 {
        let state = h
            .service
            .get_state(authorized(GetStateRequest::default(), "one"))
            .await
            .unwrap()
            .into_inner();
        assert!(state.next_page_token.is_empty());
        assert!(!state.resume_token.is_empty());
    }
}

#[tokio::test]
async fn test_slow_stream_has_bounded_buffer_and_explicit_resynchronization_error() {
    let h = harness();
    let initial = h
        .service
        .get_state(authorized(GetStateRequest::default(), "one"))
        .await
        .unwrap()
        .into_inner();
    let mut stream = h
        .service
        .watch_events(authorized(
            WatchEventsRequest {
                after: initial.state_cursor,
                journal_epoch: initial.journal_epoch,
                resume_token: initial.resume_token,
            },
            "one",
        ))
        .await
        .unwrap()
        .into_inner();
    for index in 0..257 {
        append(&h.journal, &format!("slow-{index}"), "owner", STRATEGY);
    }
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;

    let mut events = 0;
    while let Some(event) = stream.next().await {
        match event {
            Ok(_) => events += 1,
            Err(e) => {
                assert_eq!(e.code(), Code::OutOfRange);
                assert!(events <= 255);
                return;
            }
        }
    }
    panic!("slow stream ended without a resynchronization error");
}

#[tokio::test]
async fn test_replay_uses_the_projection_at_each_commit_and_json_times_stay_exact() {
    use tokio_stream::StreamExt;
    let h = harness();
    let initial = h
        .service
        .get_state(authorized(GetStateRequest::default(), "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(initial.positions[0].quantity, "20");
    assert_eq!(
        initial.positions[0].cost.parse::<Decimal>().unwrap(),
        Decimal::from(3000)
    );
    for input in &initial.inputs {
        let json: serde_json::Value = serde_json::from_str(&input.json).unwrap();
        assert!(json["meta"]["event_time_ns"].is_string());
        assert!(json["meta"]["sequence"].is_string());
    }
    let origin = append(&h.journal, "view", "owner", STRATEGY);
    h.journal
        .append_outcome(
            origin,
            JournalOutcome::Denied {
                reason: "later-denial".into(),
            },
        )
        .unwrap();
    let mut stream = h
        .service
        .watch_events(authorized(
            WatchEventsRequest {
                after: initial.state_cursor,
                journal_epoch: initial.journal_epoch,
                resume_token: initial.resume_token,
            },
            "one",
        ))
        .await
        .unwrap()
        .into_inner();
    let first = stream.next().await.unwrap().unwrap();
    assert!(matches!(first.kind, Some(Kind::Admission(_))));
    let first_view = first.view.unwrap();
    assert_eq!(first_view.order.unwrap().status, "PENDING_DISPATCH");
    assert_eq!(first_view.account.unwrap().strategy_id, STRATEGY);
    let second = stream.next().await.unwrap().unwrap();
    assert!(matches!(second.kind, Some(Kind::Outcome(_))));
    assert_eq!(second.view.unwrap().order.unwrap().status, "REJECTED");
}
