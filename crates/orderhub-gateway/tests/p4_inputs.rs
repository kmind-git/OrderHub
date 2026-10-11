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

//! P4/A01 deterministic input, native admission, and controlled-cancel acceptance.
//! Run with --test-threads=1 under cargo test because the message bus is thread-local state.

use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;
use std::sync::Arc;

use nautilus_common::cache::Cache;
use nautilus_common::clock::{Clock, VirtualClock};
use nautilus_common::live::clock::LiveClock;
use nautilus_common::messages::execution::TradingCommand;
use nautilus_common::msgbus::stubs::{
    TypedIntoMessageSavingHandler, get_typed_into_message_saving_handler,
};
use nautilus_common::msgbus::{self, MessagingSwitchboard};
use nautilus_common::timer::TimeEventCallback;
use nautilus_core::{DurationNanos, UnixNanos};
use nautilus_model::enums::{OrderSide, TimeInForce};
use nautilus_model::identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId, Venue};
use nautilus_model::instruments::Instrument;
use nautilus_model::orders::Order;
use nautilus_model::types::{Price, Quantity};
use nautilus_portfolio::Portfolio;
use nautilus_risk::engine::{RiskEngine, config::RiskEngineConfig};
use orderhub_gateway::auth::{SubmitterCredentials, SubmitterRegistry};
use orderhub_gateway::bridge::{BridgeError, LimitOrderRequest, OrderHubBridge};
use orderhub_gateway::grpc::{CoreHandle, OrderHubService};
use orderhub_gateway::inputs::{
    ACCOUNT_MAX_AGE_NS, INPUT_QUEUE_CAPACITY, INPUT_RECORD_MAX_BYTES, InputError, InputEvent,
    InputMeta, InputRuntime, InputSender, SOURCE_CLOCK_SKEW_NS, STATUS_MAX_AGE_NS, TradingStatus,
    read_jsonl_inputs,
};
use orderhub_gateway::journal::{BusinessJournal, JournalOutcome};
use orderhub_gateway::readiness::{Readiness, ReadinessGate};
use orderhub_proto::orderhub::v1::order_hub_server::OrderHub;
use orderhub_proto::orderhub::v1::{CancelOrderRequest, GetStateRequest, SubmitOrderRequest};
use rstest::rstest;
use rust_decimal::Decimal;
use tonic::{Code, Request};
use ustr::Ustr;

const NOW: u64 = 1_800_000_000_000_000_000;
const TWO_SECONDS_NS: u64 = 2_000_000_000;

struct Harness {
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<VirtualClock>>,
    portfolio: Portfolio,
    sender: InputSender,
    inputs: InputRuntime,
}

impl Harness {
    fn new(capacity: usize) -> Self {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(VirtualClock::new()));
        clock.borrow_mut().advance_time(UnixNanos::from(NOW), true);
        let shared_clock: Rc<RefCell<dyn Clock>> = clock.clone();
        let portfolio = Portfolio::new(shared_clock.clone(), cache.clone(), None);
        let (sender, inputs) = InputRuntime::channel(
            cache.clone(),
            portfolio.clone_shallow(),
            shared_clock,
            "local-fixture-v1",
            vec![StrategyId::from("ORDERHUB-S001")],
            capacity,
        )
        .unwrap();
        Self {
            cache,
            clock,
            portfolio,
            sender,
            inputs,
        }
    }

    fn enqueue(&self, event: InputEvent) {
        self.sender
            .try_send(event, self.clock.borrow().timestamp_ns())
            .unwrap();
    }

    fn send(&mut self, event: InputEvent) {
        self.enqueue(event);
        assert_eq!(self.inputs.poll().rejected, 0);
    }

    fn load(&mut self) {
        for event in fixture() {
            self.send(event);
        }
        assert!(self.inputs.check_ready().is_ok());
    }

    fn advance(&self, elapsed: u64) {
        self.clock
            .borrow_mut()
            .advance_time(UnixNanos::from(NOW + elapsed), true);
    }
}

fn fixture() -> Vec<InputEvent> {
    serde_json::from_str(include_str!("../test_data/p4_inputs.json")).unwrap()
}

fn meta_mut(event: &mut InputEvent) -> &mut InputMeta {
    match event {
        InputEvent::Instrument { meta, .. }
        | InputEvent::Quote { meta, .. }
        | InputEvent::Status { meta, .. }
        | InputEvent::Account { meta, .. } => meta,
    }
}

fn updated(index: usize, sequence: u64, elapsed: u64) -> InputEvent {
    let mut event = fixture().remove(index);
    *meta_mut(&mut event) = InputMeta {
        source_id: "local-fixture-v1".into(),
        event_time_ns: NOW + elapsed,
        sequence,
    };
    event
}

fn request(id: &str) -> LimitOrderRequest {
    LimitOrderRequest {
        submitter_id: "trader-1".into(),
        request_id: id.into(),
        strategy_id: StrategyId::from("ORDERHUB-S001"),
        instrument_id: InstrumentId::from("AAPL.GOX"),
        side: OrderSide::Buy,
        quantity: Quantity::from("7"),
        price: Price::from("153.22"),
        time_in_force: TimeInForce::Day,
        post_only: false,
        reduce_only: false,
        client_id: None,
        submit_before_ns: Some(NOW + 20_000_000_000),
    }
}

fn bridge(
    h: Harness,
    journal: Arc<BusinessJournal>,
) -> (
    OrderHubBridge,
    Rc<RefCell<RiskEngine>>,
    TypedIntoMessageSavingHandler<TradingCommand>,
) {
    let shared_clock: Rc<RefCell<dyn Clock>> = h.clock.clone();
    let risk = Rc::new(RefCell::new(RiskEngine::new(
        RiskEngineConfig::default(),
        h.portfolio,
        shared_clock.clone(),
        h.cache.clone(),
    )));
    RiskEngine::register_msgbus_handlers(&risk);
    let (handler, commands) = get_typed_into_message_saving_handler(None);
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        handler,
    );
    let bridge = OrderHubBridge::new(TraderId::from("ORDER-HUB"), shared_clock, h.cache, journal)
        .with_inputs(h.inputs);
    (bridge, risk, commands)
}

#[rstest]
fn test_full_snapshot_projects_exact_cash_quote_and_metadata() {
    let mut h = Harness::new(INPUT_QUEUE_CAPACITY);
    h.load();
    let cache = h.cache.borrow();
    let instrument = cache.instrument(&InstrumentId::from("AAPL.GOX")).unwrap();
    let quote = cache.quote(&InstrumentId::from("AAPL.GOX")).unwrap();
    let balances = h.portfolio.balances_locked(&Venue::from("GOX"));

    assert_eq!(instrument.price_increment().to_string(), "0.01");
    assert_eq!(instrument.size_increment().to_string(), "1");
    assert_eq!(instrument.price_precision(), 2);
    assert_eq!(quote.bid_price.to_string(), "153.21");
    assert_eq!(quote.ask_price.to_string(), "153.23");
    assert_eq!(quote.bid_size.to_string(), "90");
    assert_eq!(quote.ask_size.to_string(), "110");
    assert_eq!(quote.ts_event.as_u64(), NOW);
    assert_eq!(quote.ts_init.as_u64(), NOW);
    assert_eq!(
        balances[&nautilus_model::types::Currency::USD()].as_decimal(),
        Decimal::from_str_exact("123.45").unwrap()
    );
    assert!(cache.account(&AccountId::from("GOX-001")).is_some());
    assert!(cache.positions(None, None, None, None, None).is_empty());
    assert!(h.inputs.validate_order(&request("fresh")).is_ok());
}

#[rstest]
fn test_valid_input_enters_native_risk_execution_path() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(BusinessJournal::open(&dir.path().join("journal.redb")).unwrap());
    let mut h = Harness::new(8);
    h.load();
    let cache = h.cache.clone();
    let sender = h.sender.clone();
    let (bridge, _risk, commands) = bridge(h, journal.clone());
    let receipt = bridge.submit_limit_order(&request("native")).unwrap();
    let order = cache
        .borrow()
        .order(&receipt.client_order_id)
        .unwrap()
        .clone();

    assert_eq!(order.strategy_id(), StrategyId::from("ORDERHUB-S001"));
    assert_eq!(order.instrument_id(), InstrumentId::from("AAPL.GOX"));
    assert_eq!(order.quantity().to_string(), "7");
    assert_eq!(order.price().unwrap().to_string(), "153.22");
    assert!(
        commands
            .get_messages()
            .iter()
            .any(|command| matches!(command, TradingCommand::SubmitOrder(_)))
    );
    assert_eq!(
        journal
            .find_request("trader-1", "SubmitOrder", "native")
            .unwrap()
            .unwrap()
            .admission
            .ts_init_ns,
        NOW
    );
    drop(sender);
}

#[rstest]
#[case(2, STATUS_MAX_AGE_NS)]
#[case(3, ACCOUNT_MAX_AGE_NS)]
fn test_exact_freshness_boundary_and_expiry(#[case] target: usize, #[case] max_age: u64) {
    let mut h = Harness::new(8);
    h.load();
    h.advance(max_age);

    for index in 1..=3 {
        if index != target {
            h.send(updated(index, 2, max_age));
        }
    }
    assert!(h.inputs.validate_order(&request("at-boundary")).is_ok());
    h.advance(max_age + 1);
    h.inputs.poll();
    assert!(h.inputs.validate_order(&request("after-boundary")).is_err());
}

#[rstest]
#[case(TWO_SECONDS_NS)]
#[case(TWO_SECONDS_NS + 1)]
#[case(60_000_000_000)]
fn test_last_legal_quote_has_no_age_limit(#[case] elapsed: u64) {
    let mut h = Harness::new(8);
    h.load();
    h.advance(elapsed);
    h.send(updated(2, 2, elapsed));
    h.send(updated(3, 2, elapsed));
    let mut order = request("old-quote");
    order.submit_before_ns = Some(NOW + elapsed + 20_000_000_000);
    assert!(h.inputs.validate_order(&order).is_ok());
    let context = h.inputs.risk_context(&order.strategy_id).unwrap();
    assert_eq!(context.reference_price, Decimal::new(15323, 2));
    assert_eq!(context.risk_config_version, "local-usd-v2");
    assert_eq!(
        h.cache
            .borrow()
            .quote(&order.instrument_id)
            .unwrap()
            .ts_event
            .as_u64(),
        NOW
    );
}

#[rstest]
fn test_duplicates_and_old_frames_do_not_refresh_or_roll_back() {
    let mut h = Harness::new(8);
    h.load();
    let mut new_quote = updated(1, 2, 0);
    if let InputEvent::Quote {
        bid_price,
        ask_price,
        snapshot,
        ..
    } = &mut new_quote
    {
        *bid_price = "154.11".into();
        *ask_price = "154.12".into();
        *snapshot = false;
    }
    h.send(new_quote.clone());
    h.advance(TWO_SECONDS_NS + 1);
    meta_mut(&mut new_quote).event_time_ns = NOW + TWO_SECONDS_NS + 1;
    h.enqueue(new_quote);
    h.enqueue(updated(1, 1, TWO_SECONDS_NS + 1));
    let report = h.inputs.poll();

    assert_eq!(report.ignored, 2);
    assert_eq!(report.applied, 0);
    assert_eq!(
        h.cache
            .borrow()
            .quote(&InstrumentId::from("AAPL.GOX"))
            .unwrap()
            .bid_price
            .to_string(),
        "154.11"
    );
    assert!(h.inputs.check_ready().is_ok());
}

#[rstest]
fn test_quote_gaps_accept_complete_top_of_book_replacements() {
    let mut h = Harness::new(8);
    h.load();
    let mut gap = updated(1, 3, 0);
    if let InputEvent::Quote { snapshot, .. } = &mut gap {
        *snapshot = false;
    }
    h.enqueue(gap);
    assert_eq!(h.inputs.poll().applied, 1);
    assert!(h.inputs.check_ready().is_ok());
    let mut incremental = updated(1, 4, 0);
    if let InputEvent::Quote { snapshot, .. } = &mut incremental {
        *snapshot = false;
    }
    h.enqueue(incremental);
    assert_eq!(h.inputs.poll().applied, 1);
    h.send(updated(1, 5, 0));
    assert!(h.inputs.check_ready().is_ok());
    h.advance(TWO_SECONDS_NS + 1);
    h.inputs.poll();
    let mut incremental = updated(1, 9, TWO_SECONDS_NS + 1);
    if let InputEvent::Quote { snapshot, .. } = &mut incremental {
        *snapshot = false;
    }
    h.enqueue(incremental);
    assert_eq!(h.inputs.poll().applied, 1);
    assert!(h.inputs.check_ready().is_ok());
}

#[rstest]
#[case(0, 1)]
#[case(1, 1)]
#[case(2, 1)]
#[case(3, 1)]
fn test_future_time_and_source_clock_skew_are_rejected(#[case] index: usize, #[case] offset: u64) {
    let mut h = Harness::new(8);
    h.load();
    let mut event = updated(index, 2, 0);

    if offset == 1 {
        meta_mut(&mut event).event_time_ns = NOW + 1;
    } else {
        meta_mut(&mut event).event_time_ns = NOW - offset;
    }
    h.enqueue(event);
    assert_eq!(h.inputs.poll().rejected, 1);
    assert_eq!(h.inputs.check_ready().is_ok(), index == 1);
}

#[rstest]
#[case(SOURCE_CLOCK_SKEW_NS, true)]
#[case(SOURCE_CLOCK_SKEW_NS + 1, false)]
fn test_source_clock_skew_boundary_is_inclusive(#[case] skew: u64, #[case] valid: bool) {
    let mut h = Harness::new(8);
    let mut account = updated(3, 1, 0);
    meta_mut(&mut account).event_time_ns = NOW - skew;
    h.send(updated(0, 1, 0));
    h.send(updated(1, 1, 0));
    h.enqueue(account);
    assert_eq!(h.inputs.poll().rejected, usize::from(!valid));
    h.send(updated(2, 1, 0));
    assert_eq!(h.inputs.check_ready().is_ok(), valid);
}

#[rstest]
fn test_delayed_quote_does_not_have_a_hidden_source_delay_limit() {
    let mut h = Harness::new(8);
    h.send(updated(0, 1, 0));
    let mut quote = updated(1, 1, 0);
    meta_mut(&mut quote).event_time_ns = NOW - 60_000_000_000;
    h.send(quote);
    h.send(updated(2, 1, 0));
    h.send(updated(3, 1, 0));
    assert!(h.inputs.check_ready().is_ok());
}

#[rstest]
fn test_same_sequence_conflicting_metadata_is_not_treated_as_a_normal_duplicate() {
    let mut h = Harness::new(8);
    h.load();
    let mut definition = updated(0, 1, 0);
    if let InputEvent::Instrument {
        price_increment, ..
    } = &mut definition
    {
        *price_increment = "0.05".into();
    }
    h.enqueue(definition);
    assert_eq!(h.inputs.poll().rejected, 1);
    assert!(h.inputs.check_ready().is_err());
    h.send(updated(0, 2, 0));
    assert!(h.inputs.check_ready().is_err());
    h.send(updated(1, 2, 0));
    h.send(updated(2, 2, 0));
    assert!(h.inputs.check_ready().is_ok());
}

#[rstest]
#[case("conflicting-revision")]
#[case("tick-precision")]
#[case("currency")]
#[case("quantity-step")]
#[case("future-effective-time")]
fn test_invalid_instrument_requires_resynchronizing_dependent_streams(#[case] case: &str) {
    let mut h = Harness::new(8);
    h.load();
    let mut definition = updated(0, 2, 0);
    if let InputEvent::Instrument {
        price_increment,
        currency,
        size_increment,
        effective_time_ns,
        ..
    } = &mut definition
    {
        match case {
            "conflicting-revision" => *price_increment = "0.05".into(),
            "tick-precision" => *price_increment = "0.001".into(),
            "currency" => *currency = "EUR".into(),
            "quantity-step" => *size_increment = "0.5".into(),
            "future-effective-time" => *effective_time_ns = NOW + 1,
            _ => unreachable!(),
        }
    }
    h.enqueue(definition);
    assert_eq!(h.inputs.poll().rejected, 1);
    assert!(h.inputs.check_ready().is_err());
    h.send(updated(0, 3, 0));
    assert!(h.inputs.check_ready().is_err());
    h.send(updated(1, 2, 0));
    h.send(updated(2, 2, 0));
    assert!(h.inputs.check_ready().is_ok());
}

#[rstest]
#[case(TradingStatus::Halted)]
#[case(TradingStatus::Closed)]
#[case(TradingStatus::Unknown)]
fn test_non_regular_status_blocks_new_orders(#[case] state: TradingStatus) {
    let mut h = Harness::new(8);
    h.load();
    let mut status = updated(2, 2, 0);
    if let InputEvent::Status { status, .. } = &mut status {
        *status = state;
    }
    h.send(status);
    h.enqueue(updated(2, 1, 0));
    assert_eq!(h.inputs.poll().ignored, 1);
    assert!(h.inputs.validate_order(&request("halted")).is_err());
    h.send(updated(2, 3, 0));
    assert!(h.inputs.check_ready().is_ok());
}

#[rstest]
#[case(NOW + 1, NOW + 100)]
#[case(NOW - 100, NOW)]
fn test_explicit_session_boundaries(#[case] open: u64, #[case] close: u64) {
    let mut h = Harness::new(8);
    h.load();
    let mut status = updated(2, 2, 0);
    if let InputEvent::Status {
        session_open_ns,
        session_close_ns,
        ..
    } = &mut status
    {
        *session_open_ns = open;
        *session_close_ns = close;
    }
    h.send(status);
    assert!(h.inputs.check_ready().is_err());
}

#[rstest]
fn test_metadata_change_requires_revision_matched_quote_and_status() {
    let mut h = Harness::new(8);
    h.load();
    let mut definition = updated(0, 2, 0);
    if let InputEvent::Instrument {
        revision,
        price_increment,
        ..
    } = &mut definition
    {
        *revision = 2;
        *price_increment = "0.05".into();
    }
    h.send(definition);
    assert!(h.inputs.check_ready().is_err());
    h.enqueue(updated(1, 2, 0));
    assert_eq!(h.inputs.poll().rejected, 1);
    let mut quote = updated(1, 3, 0);
    if let InputEvent::Quote {
        instrument_revision,
        bid_price,
        ask_price,
        ..
    } = &mut quote
    {
        *instrument_revision = 2;
        *bid_price = "153.20".into();
        *ask_price = "153.25".into();
    }
    h.send(quote);
    let mut status = updated(2, 2, 0);
    if let InputEvent::Status {
        instrument_revision,
        ..
    } = &mut status
    {
        *instrument_revision = 2;
    }
    h.send(status);
    assert!(h.inputs.check_ready().is_ok());
    assert!(h.inputs.validate_order(&request("old-tick")).is_err());
    let mut supported = request("new-tick");
    supported.price = Price::from("153.25");
    assert!(h.inputs.validate_order(&supported).is_ok());
}

#[rstest]
#[case(0)]
#[case(1)]
#[case(2)]
#[case(3)]
fn test_missing_required_component_blocks_admission(#[case] missing: usize) {
    let mut h = Harness::new(8);
    for (index, event) in fixture().into_iter().enumerate() {
        if index != missing {
            h.enqueue(event);
        }
    }
    h.inputs.poll();
    assert!(h.inputs.validate_order(&request("missing")).is_err());
}

#[rstest]
#[case("GTC")]
#[case("IOC")]
#[case("FOK")]
#[case("fractional")]
#[case("post")]
#[case("reduce")]
#[case("instrument")]
#[case("route")]
fn test_unsupported_capabilities_are_stable(#[case] case: &str) {
    let mut h = Harness::new(8);
    h.load();
    let mut req = request("unsupported");
    match case {
        "GTC" => req.time_in_force = TimeInForce::Gtc,
        "IOC" => req.time_in_force = TimeInForce::Ioc,
        "FOK" => req.time_in_force = TimeInForce::Fok,
        "fractional" => req.quantity = Quantity::from("1.5"),
        "post" => req.post_only = true,
        "reduce" => req.reduce_only = true,
        "instrument" => req.instrument_id = InstrumentId::from("MSFT.GOX"),
        "route" => req.client_id = Some(ClientId::from("OTHER")),
        _ => unreachable!(),
    }
    assert_eq!(
        h.inputs.validate_order(&req),
        Err(InputError::UnsupportedCapability)
    );
}

#[rstest]
#[case("0", "153.22")]
#[case("7", "0.00")]
#[case("7", "153.221")]
#[case("999999", "153.22")]
fn test_invalid_steps_zero_values_and_cash_limit(#[case] quantity: &str, #[case] price: &str) {
    let mut h = Harness::new(8);
    h.load();
    let mut req = request("invalid-value");
    req.quantity = quantity.parse().unwrap();
    req.price = price.parse().unwrap();
    assert!(h.inputs.validate_order(&req).is_err());
}

#[rstest]
fn test_sell_uses_confirmed_strategy_ownership() {
    let mut h = Harness::new(8);
    h.load();
    let mut sell = request("owned-sell");
    sell.side = OrderSide::Sell;
    sell.quantity = Quantity::from("20");
    assert!(h.inputs.validate_order(&sell).is_ok());
    sell.quantity = Quantity::from("21");
    assert!(h.inputs.validate_order(&sell).is_err());
    sell.strategy_id = StrategyId::from("OTHER-S001");
    sell.quantity = Quantity::from("1");
    assert!(h.inputs.validate_order(&sell).is_err());
}

#[rstest]
#[case("coverage")]
#[case("identity")]
#[case("currency")]
#[case("balances")]
#[case("financing")]
#[case("precision")]
#[case("oversold")]
#[case("duplicate-owner")]
#[case("foreign-owner")]
fn test_invalid_account_snapshot_closes_account_gate(#[case] case: &str) {
    let mut h = Harness::new(8);
    h.load();
    let mut event = updated(3, 2, 0);
    if let InputEvent::Account {
        complete,
        account_id,
        currency,
        free,
        buying_power,
        ownership,
        position,
        ..
    } = &mut event
    {
        match case {
            "coverage" => *complete = false,
            "identity" => *account_id = "GOX-002".into(),
            "currency" => *currency = "EUR".into(),
            "balances" => *free = "1.00".into(),
            "financing" => *buying_power = "1000001.00".into(),
            "precision" => *buying_power = "99.001".into(),
            "oversold" => *position = "19".into(),
            "duplicate-owner" => ownership.push(ownership[0].clone()),
            "foreign-owner" => ownership[0].strategy_id = "OTHER-S001".into(),
            _ => unreachable!(),
        }
    }
    h.enqueue(event);
    assert_eq!(h.inputs.poll().rejected, 1);
    assert!(h.inputs.check_ready().is_err());
}

#[rstest]
#[case("float")]
#[case("future-receipt")]
#[case("source")]
#[case("zero-sequence")]
#[case("backwards-time")]
#[case("incomplete")]
#[case("crossed")]
fn test_invalid_quote_frames_preserve_the_last_legal_quote(#[case] case: &str) {
    let mut h = Harness::new(8);
    h.load();
    let mut event = updated(1, 2, 0);
    match case {
        "source" => meta_mut(&mut event).source_id = "untrusted".into(),
        "zero-sequence" => meta_mut(&mut event).sequence = 0,
        "backwards-time" => meta_mut(&mut event).event_time_ns = NOW - 1,
        _ => {}
    }
    if let InputEvent::Quote {
        bid_price,
        complete,
        ..
    } = &mut event
    {
        match case {
            "float" => *bid_price = "NaN".into(),
            "incomplete" => *complete = false,
            "crossed" => *bid_price = "160.00".into(),
            _ => {}
        }
    }
    h.sender
        .try_send(
            event,
            UnixNanos::from(NOW + u64::from(case == "future-receipt")),
        )
        .unwrap();
    assert_eq!(h.inputs.poll().rejected, 1);
    assert!(h.inputs.check_ready().is_ok());
    assert_eq!(
        h.cache
            .borrow()
            .quote(&InstrumentId::from("AAPL.GOX"))
            .unwrap()
            .ask_price
            .to_string(),
        "153.23"
    );
    let mut valid = updated(1, 2, 0);
    if let InputEvent::Quote { ask_price, .. } = &mut valid {
        *ask_price = "153.24".into();
    }
    h.send(valid);
    assert_eq!(
        h.cache
            .borrow()
            .quote(&InstrumentId::from("AAPL.GOX"))
            .unwrap()
            .ask_price
            .to_string(),
        "153.24"
    );
}

#[rstest]
#[case(4)]
#[case(INPUT_QUEUE_CAPACITY)]
fn test_queue_saturation_invalidates_queued_generation(#[case] capacity: usize) {
    let mut h = Harness::new(capacity);
    h.load();
    for sequence in 2..=(capacity as u64 + 1) {
        h.enqueue(updated(3, sequence, 0));
    }
    assert_eq!(
        h.sender
            .try_send(updated(3, capacity as u64 + 2, 0), UnixNanos::from(NOW)),
        Err(InputError::QueueFull)
    );
    assert!(h.inputs.check_ready().is_err());
    assert_eq!(h.inputs.poll().ignored, capacity);
    assert!(h.inputs.check_ready().is_err());
    for index in 0..4 {
        h.send(updated(index, capacity as u64 + 3, 0));
    }
    assert!(h.inputs.check_ready().is_ok());
}

#[rstest]
fn test_quote_queue_loss_does_not_invalidate_required_inputs() {
    let mut h = Harness::new(4);
    h.load();
    for sequence in 2..=5 {
        h.enqueue(updated(1, sequence, 0));
    }
    assert_eq!(
        h.sender.try_send(updated(1, 6, 0), UnixNanos::from(NOW)),
        Err(InputError::QueueFull)
    );
    assert!(h.inputs.check_ready().is_ok());
    assert_eq!(h.inputs.poll().applied, 4);
    h.send(updated(1, 7, 0));
    assert!(h.inputs.check_ready().is_ok());
}

#[rstest]
fn test_quote_reconnect_keeps_valuation_and_discards_old_connection_frames() {
    let mut h = Harness::new(8);
    h.load();
    let mut obsolete = updated(1, 100, 0);
    if let InputEvent::Quote { ask_price, .. } = &mut obsolete {
        *ask_price = "160.00".into();
    }
    h.enqueue(obsolete);
    h.sender.invalidate_quotes();
    assert!(h.inputs.check_ready().is_ok());
    assert_eq!(h.inputs.poll().ignored, 1);
    let context = h
        .inputs
        .risk_context(&StrategyId::from("ORDERHUB-S001"))
        .unwrap();
    assert_eq!(context.reference_price, Decimal::new(15323, 2));
    let mut without_snapshot = updated(1, 1, 0);
    if let InputEvent::Quote { snapshot, .. } = &mut without_snapshot {
        *snapshot = false;
    }
    h.enqueue(without_snapshot);
    assert_eq!(h.inputs.poll().rejected, 1);
    assert!(h.inputs.check_ready().is_ok());
    let mut replacement = updated(1, 1, 0);
    if let InputEvent::Quote { ask_price, .. } = &mut replacement {
        *ask_price = "154.00".into();
    }
    h.send(replacement);
    assert!(h.inputs.check_ready().is_ok());
    assert_eq!(
        h.inputs
            .risk_context(&StrategyId::from("ORDERHUB-S001"))
            .unwrap()
            .reference_price,
        Decimal::from(154)
    );
}

#[rstest]
fn test_disconnect_requires_full_resynchronization_of_all_streams() {
    let mut h = Harness::new(8);
    h.load();
    h.sender.invalidate();
    assert!(h.inputs.check_ready().is_err());
    h.inputs.poll();
    for index in 0..3 {
        h.send(updated(index, 2, 0));
    }
    assert!(h.inputs.check_ready().is_err());
    h.send(updated(3, 2, 0));
    assert!(h.inputs.check_ready().is_ok());
    drop(h.sender);
    h.inputs.poll();
    assert_eq!(h.inputs.last_error(), Some(&InputError::Disconnected));
    assert!(h.inputs.check_ready().is_err());
}

#[rstest]
fn test_deadline_and_authorized_cancel_survive_stale_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(BusinessJournal::open(&dir.path().join("journal.redb")).unwrap());
    let mut h = Harness::new(8);
    h.load();
    let clock = h.clock.clone();
    let sender = h.sender.clone();
    let (bridge, _risk, _commands) = bridge(h, journal.clone());
    let mut late = request("deadline");
    late.submit_before_ns = Some(NOW);
    assert!(matches!(
        bridge.submit_limit_order(&late),
        Err(BridgeError::Input(_))
    ));
    let rejection = journal
        .find_request("trader-1", "SubmitOrder", "deadline")
        .unwrap()
        .unwrap();
    assert_eq!(rejection.result.result, "REJECTED");
    assert!(journal.order(rejection.admission.seq).unwrap().is_none());
    let admitted = bridge.submit_limit_order(&request("live-order")).unwrap();
    clock
        .borrow_mut()
        .advance_time(UnixNanos::from(NOW + ACCOUNT_MAX_AGE_NS + 1), true);
    assert!(bridge.submit_limit_order(&request("stale-new")).is_err());
    assert!(matches!(
        bridge.cancel_order("trader-2", "foreign", admitted.client_order_id.as_ref()),
        Err(BridgeError::PermissionDenied)
    ));
    assert!(matches!(
        bridge.authorize_cancel("trader-1", admitted.client_order_id.as_ref(), &[]),
        Err(BridgeError::PermissionDenied)
    ));
    bridge
        .authorize_cancel(
            "trader-1",
            admitted.client_order_id.as_ref(),
            &["ORDERHUB-S001".into()],
        )
        .unwrap();
    let cancelled = bridge
        .cancel_order("trader-1", "cancel", admitted.client_order_id.as_ref())
        .unwrap();
    assert_eq!(cancelled.client_order_id, admitted.client_order_id);
    assert!(!cancelled.already_admitted);
    assert!(
        journal
            .find_request("trader-1", "CancelOrder", "cancel")
            .unwrap()
            .is_some()
    );
    drop(sender);
}

#[derive(Debug)]
struct CommitClock {
    base: VirtualClock,
    journal: Arc<BusinessJournal>,
    after_commit_ns: u64,
    read_step_ns: u64,
    reads: std::cell::Cell<u64>,
}

impl Clock for CommitClock {
    fn timestamp_ns(&self) -> UnixNanos {
        let elapsed = self.read_step_ns * self.reads.get();
        self.reads.set(self.reads.get() + 1);
        UnixNanos::from(
            NOW + if self
                .journal
                .find_request("trader-1", "SubmitOrder", "post-commit")
                .unwrap()
                .is_some()
            {
                self.after_commit_ns
            } else {
                0
            } + elapsed,
        )
    }

    fn timestamp_us(&self) -> u64 {
        self.timestamp_ns().as_u64() / 1_000
    }

    fn timestamp_ms(&self) -> u64 {
        self.timestamp_ns().as_u64() / 1_000_000
    }

    fn timestamp(&self) -> f64 {
        self.timestamp_ns().as_u64() as f64 / 1_000_000_000.0
    }

    fn timer_names(&self) -> Vec<&str> {
        self.base.timer_names()
    }

    fn timer_count(&self) -> usize {
        self.base.timer_count()
    }

    fn timer_exists(&self, name: &Ustr) -> bool {
        self.base.timer_exists(name)
    }

    fn register_default_handler(&mut self, callback: TimeEventCallback) {
        self.base.register_default_handler(callback);
    }

    fn cancel_default_handler(&mut self) {
        self.base.cancel_default_handler();
    }

    fn cancel_callbacks(&mut self) {
        self.base.cancel_callbacks();
    }

    fn set_time_alert_ns(
        &mut self,
        name: &str,
        time: UnixNanos,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
    ) -> anyhow::Result<()> {
        self.base
            .set_time_alert_ns(name, time, callback, allow_past)
    }

    fn set_timer_ns(
        &mut self,
        name: &str,
        interval: DurationNanos,
        start: Option<UnixNanos>,
        stop: Option<UnixNanos>,
        callback: Option<TimeEventCallback>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> anyhow::Result<()> {
        self.base.set_timer_ns(
            name,
            interval,
            start,
            stop,
            callback,
            allow_past,
            fire_immediately,
        )
    }

    fn next_time_ns(&self, name: &str) -> Option<UnixNanos> {
        self.base.next_time_ns(name)
    }

    fn cancel_timer(&mut self, name: &str) {
        self.base.cancel_timer(name);
    }

    fn cancel_timers(&mut self) {
        self.base.cancel_timers();
    }

    fn reset(&mut self) {
        self.base.reset();
    }
}

#[rstest]
#[case(TWO_SECONDS_NS + 1, 20_000_000_000, true)]
#[case(STATUS_MAX_AGE_NS + 1, 20_000_000_000, false)]
#[case(1, 1, false)]
fn test_commit_rechecks_freshness_and_deadline_before_native_dispatch(
    #[case] elapsed: u64,
    #[case] deadline: u64,
    #[case] admitted: bool,
) {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(BusinessJournal::open(&dir.path().join("journal.redb")).unwrap());
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(CommitClock {
        base: VirtualClock::new(),
        journal: journal.clone(),
        after_commit_ns: elapsed,
        read_step_ns: 0,
        reads: std::cell::Cell::new(0),
    }));
    let portfolio = Portfolio::new(clock.clone(), cache.clone(), None);
    let (sender, mut inputs) = InputRuntime::channel(
        cache.clone(),
        portfolio.clone_shallow(),
        clock.clone(),
        "local-fixture-v1",
        vec![StrategyId::from("ORDERHUB-S001")],
        8,
    )
    .unwrap();
    let risk = Rc::new(RefCell::new(RiskEngine::new(
        RiskEngineConfig::default(),
        portfolio,
        clock.clone(),
        cache.clone(),
    )));
    RiskEngine::register_msgbus_handlers(&risk);
    let (handler, commands) = get_typed_into_message_saving_handler(None);
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        handler,
    );
    for event in fixture() {
        sender.try_send(event, UnixNanos::from(NOW)).unwrap();
    }
    assert_eq!(inputs.poll().applied, 4);
    let bridge = OrderHubBridge::new(
        TraderId::from("ORDER-HUB"),
        clock,
        cache.clone(),
        journal.clone(),
    )
    .with_inputs(inputs);
    let mut req = request("post-commit");
    req.submit_before_ns = Some(NOW + deadline);
    if admitted {
        req.price = Price::from("170.00");
        req.quantity = Quantity::from("1");
        let result = bridge.submit_limit_order(&req).unwrap();
        assert_eq!(
            cache
                .borrow()
                .order(&result.client_order_id)
                .unwrap()
                .price()
                .unwrap(),
            req.price
        );
        assert!(
            commands
                .get_messages()
                .iter()
                .any(|command| matches!(command, TradingCommand::SubmitOrder(_)))
        );
    } else {
        assert!(matches!(
            bridge.submit_limit_order(&req),
            Err(BridgeError::Input(_))
        ));
        assert!(
            cache
                .borrow()
                .orders(None, None, None, None, None)
                .is_empty()
        );
        assert!(commands.get_messages().is_empty());
    }
    let record = journal
        .find_request("trader-1", "SubmitOrder", "post-commit")
        .unwrap()
        .unwrap();
    assert_eq!(
        matches!(record.outcome, Some(JournalOutcome::DispatchStarted { .. })),
        admitted
    );
    assert_eq!(
        matches!(record.outcome, Some(JournalOutcome::Denied { .. })),
        !admitted
    );
    assert_eq!(record.result.result, "ADMITTED");
    assert_eq!(journal.last_seq().unwrap(), record.admission.seq + 1);
}

#[rstest]
fn test_receive_time_is_checked_at_each_frame_not_at_batch_start() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(BusinessJournal::open(&dir.path().join("clock.redb")).unwrap());
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(CommitClock {
        base: VirtualClock::new(),
        journal,
        after_commit_ns: 0,
        read_step_ns: 1,
        reads: std::cell::Cell::new(0),
    }));
    let shared_clock: Rc<RefCell<dyn Clock>> = clock.clone();
    let portfolio = Portfolio::new(shared_clock.clone(), cache.clone(), None);
    let (sender, mut inputs) = InputRuntime::channel(
        cache,
        portfolio,
        shared_clock,
        "local-fixture-v1",
        vec![StrategyId::from("ORDERHUB-S001")],
        8,
    )
    .unwrap();
    for mut event in fixture() {
        meta_mut(&mut event).event_time_ns = NOW + 1;
        sender.try_send(event, UnixNanos::from(NOW + 1)).unwrap();
    }
    clock.borrow().reads.set(0);
    let result = inputs.poll();
    assert_eq!(result.applied, 4);
    assert_eq!(result.rejected, 0);
    assert!(inputs.check_ready().is_ok());
}

#[rstest]
#[case("{}")]
#[case("{\"kind\":\"unknown\"}")]
fn test_jsonl_malformed_frames_invalidate_continuity(#[case] line: &str) {
    let mut h = Harness::new(8);
    h.load();
    assert!(
        read_jsonl_inputs(Cursor::new(line), h.sender.clone(), || UnixNanos::from(NOW)).is_err()
    );
    assert!(h.inputs.check_ready().is_err());
}

#[rstest]
fn test_jsonl_eof_oversize_and_decimal_number_rejection() {
    let mut h = Harness::new(8);
    h.load();
    let mut value = serde_json::to_value(updated(1, 2, 0)).unwrap();
    value["bid_price"] = serde_json::json!(153.21);
    assert!(
        read_jsonl_inputs(Cursor::new(value.to_string()), h.sender.clone(), || {
            UnixNanos::from(NOW)
        })
        .is_ok()
    );
    assert!(
        read_jsonl_inputs(
            Cursor::new(vec![b' '; INPUT_RECORD_MAX_BYTES + 1]),
            h.sender.clone(),
            || UnixNanos::from(NOW)
        )
        .is_err()
    );
    assert!(
        read_jsonl_inputs(Cursor::new(Vec::<u8>::new()), h.sender.clone(), || {
            UnixNanos::from(NOW)
        })
        .is_ok()
    );
    assert!(h.inputs.check_ready().is_err());
}

#[rstest]
fn test_jsonl_valid_stream_projects_before_eof_then_closes_gate() {
    let h = Harness::new(8);
    let sender = h.sender.clone();
    let inputs = RefCell::new(h.inputs);
    let ready_seen = std::cell::Cell::new(false);
    let lines = fixture()
        .into_iter()
        .chain([updated(1, 1, 0)])
        .map(|event| serde_json::to_string(&event).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    read_jsonl_inputs(Cursor::new(lines), sender, || {
        let mut runtime = inputs.borrow_mut();
        runtime.poll();
        ready_seen.set(ready_seen.get() || runtime.check_ready().is_ok());
        UnixNanos::from(NOW)
    })
    .unwrap();
    inputs.borrow_mut().poll();
    assert!(ready_seen.get());
    assert!(inputs.borrow().check_ready().is_err());
    assert_eq!(
        h.cache
            .borrow()
            .quote(&InstrumentId::from("AAPL.GOX"))
            .unwrap()
            .ask_price
            .to_string(),
        "153.23"
    );
}

#[rstest]
fn test_known_bad_quote_json_is_dropped_while_required_inputs_continue() {
    let mut h = Harness::new(8);
    h.load();
    let mut malformed = serde_json::to_value(updated(1, 2, 0)).unwrap();
    malformed["bid_price"] = serde_json::json!(153.21);
    let lines = [
        malformed.to_string(),
        serde_json::to_string(&updated(2, 2, 0)).unwrap(),
        serde_json::to_string(&updated(3, 2, 0)).unwrap(),
        serde_json::to_string(&updated(1, 2, 0)).unwrap(),
    ]
    .join("\n");
    let sender = h.sender.clone();
    let inputs = RefCell::new(h.inputs);
    let observed = std::cell::Cell::new(0);
    read_jsonl_inputs(Cursor::new(lines), sender, || {
        let mut runtime = inputs.borrow_mut();
        runtime.poll();
        assert!(runtime.check_ready().is_ok());
        observed.set(observed.get() + 1);
        UnixNanos::from(NOW)
    })
    .unwrap();
    assert_eq!(observed.get(), 3);
    inputs.borrow_mut().poll();
    assert!(inputs.borrow().check_ready().is_err());
    assert_eq!(
        h.cache
            .borrow()
            .quote(&InstrumentId::from("AAPL.GOX"))
            .unwrap()
            .ask_price
            .to_string(),
        "153.23"
    );
}

#[rstest]
fn test_live_clock_advances_without_manual_updates() {
    let clock = LiveClock::new(None);
    let first = clock.timestamp_ns();
    let second = clock.timestamp_ns();
    assert!(second > first);
}

#[rstest]
fn test_live_timer_callback_runs_on_its_core_owner_thread() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(BusinessJournal::open(&dir.path().join("timer.redb")).unwrap());
    let core_journal = journal.clone();
    let gate = ReadinessGate::new();
    let (tx, rx) = std::sync::mpsc::channel();
    let core = CoreHandle::spawn(
        move || {
            let cache = Rc::new(RefCell::new(Cache::default()));
            let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(LiveClock::default()));
            tx.send(std::thread::current().id()).unwrap();
            let callback = TimeEventCallback::RustLocal(Rc::new(move |_| {
                tx.send(std::thread::current().id()).unwrap();
            }));
            let scheduled = clock.borrow().timestamp_ns() + DurationNanos::new(1_000_000);
            clock
                .borrow_mut()
                .set_time_alert_ns("p4-core-timer", scheduled, Some(callback), Some(true))
                .unwrap();
            let bridge = OrderHubBridge::new(
                TraderId::from("ORDER-HUB"),
                clock,
                cache.clone(),
                core_journal,
            );
            (bridge, cache, None, None)
        },
        journal,
        Arc::new(SubmitterRegistry::new(vec![SubmitterCredentials {
            token: "timer-test".into(),
            submitter_id: "timer".into(),
            strategies: vec!["ORDERHUB-S001".into()],
        }])),
        gate,
    );
    let owner = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    let callback_owner = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    assert_eq!(callback_owner, owner);
    drop(core);
}

fn rpc_request<T>(message: T, token: &str) -> Request<T> {
    let mut req = Request::new(message);
    req.metadata_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    req
}

#[tokio::test]
async fn test_rpc_gates_bad_values_capabilities_and_cancel_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(BusinessJournal::open(&dir.path().join("journal.redb")).unwrap());
    let registry = Arc::new(SubmitterRegistry::new(vec![
        SubmitterCredentials {
            token: "one".into(),
            submitter_id: "trader-1".into(),
            strategies: vec!["ORDERHUB-S001".into()],
        },
        SubmitterCredentials {
            token: "two".into(),
            submitter_id: "trader-2".into(),
            strategies: vec!["OTHER-S001".into()],
        },
    ]));
    let gate = ReadinessGate::new();
    gate.set(Readiness::Ready, "");
    let (tx, rx) = std::sync::mpsc::channel();
    let core_journal = journal.clone();
    let core = CoreHandle::spawn(
        move || {
            let mut h = Harness::new(8);
            h.load();
            tx.send(h.sender.clone()).unwrap();
            let cache = h.cache.clone();
            let (bridge, risk, _commands) = bridge(h, core_journal);
            #[expect(
                clippy::mem_forget,
                reason = "core thread keeps risk bus handlers alive"
            )]
            std::mem::forget(risk);
            (bridge, cache, None, None)
        },
        journal.clone(),
        registry,
        gate,
    );
    let sender = rx.recv().unwrap();
    let service = OrderHubService::new(core);
    let initial_state = service
        .get_state(rpc_request(GetStateRequest::default(), "one"))
        .await
        .unwrap()
        .into_inner();
    assert!(initial_state.writable);
    assert_eq!(initial_state.instruments.len(), 1);
    assert_eq!(initial_state.instruments[0].instrument_id, "AAPL.GOX");
    assert_eq!(initial_state.instruments[0].price_increment, "0.01");
    assert_eq!(initial_state.instruments[0].size_increment, "1");
    assert_eq!(initial_state.instruments[0].price_precision, 2);
    let base = SubmitOrderRequest {
        journal_epoch: "1".into(),
        submitter_id: "client-supplied-identity-is-ignored".into(),
        request_id: "rpc".into(),
        strategy_id: "ORDERHUB-S001".into(),
        instrument_id: "AAPL.GOX".into(),
        side: "BUY".into(),
        quantity: "7".into(),
        price: "153.22".into(),
        time_in_force: "DAY".into(),
        post_only: false,
        reduce_only: false,
        submit_before_ns: None,
    };
    let mut bad = base.clone();
    bad.quantity = "garbage".into();
    assert_eq!(
        service
            .submit_order(rpc_request(bad, "one"))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    let mut bad = base.clone();
    bad.price = "garbage".into();
    assert_eq!(
        service
            .submit_order(rpc_request(bad, "one"))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    let mut unsupported = base.clone();
    unsupported.request_id = "rpc-unsupported".into();
    unsupported.time_in_force = "GTC".into();
    let rejected = service
        .submit_order(rpc_request(unsupported, "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(rejected.result, "REJECTED");
    assert_eq!(rejected.reason, "UNSUPPORTED_CAPABILITY");
    assert!(rejected.client_order_id.is_empty());
    assert!(journal.snapshot_orders(10).unwrap().1.is_empty());
    let response = service
        .submit_order(rpc_request(base.clone(), "one"))
        .await
        .unwrap()
        .into_inner();
    sender.invalidate();
    let state = service
        .get_state(rpc_request(GetStateRequest::default(), "one"))
        .await
        .unwrap()
        .into_inner();
    assert!(!state.writable);
    let mut next = base;
    next.request_id = "blocked".into();
    let rejected = service
        .submit_order(rpc_request(next, "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(rejected.result, "REJECTED");
    assert!(rejected.client_order_id.is_empty());
    let cancel = CancelOrderRequest {
        journal_epoch: "1".into(),
        submitter_id: "client-supplied-identity-is-ignored".into(),
        request_id: "cancel".into(),
        client_order_id: response.client_order_id,
    };
    assert_eq!(
        service
            .cancel_order(rpc_request(cancel.clone(), "two"))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let response = service
        .cancel_order(rpc_request(cancel, "one"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(response.result, "CANCEL_INTENT_ACCEPTED");
}

#[rstest]
fn test_missing_confirmed_position_cost_closes_risk_gate() {
    let mut h = Harness::new(8);
    let mut events = fixture();
    if let InputEvent::Account { ownership, .. } = &mut events[3] {
        ownership[0].cost_basis = None;
    }
    for event in events {
        h.send(event);
    }
    assert!(h.inputs.check_ready().is_err());
    assert!(h.inputs.validate_order(&request("missing-cost")).is_err());
}
