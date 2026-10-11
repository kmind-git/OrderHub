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

//! Versioned, bounded market and account inputs for the local OrderHub profile.
//!
//! Adapters stamp receipt time locally. Only the core thread mutates the cache and portfolio.
//! External position facts and confirmed strategy attribution stay separate from native positions;
//! a snapshot does not manufacture fills. The current profile is USD whole-share AAPL.GOX cash.

use std::cell::RefCell;
use std::io::{BufRead, Read};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};

use nautilus_common::cache::Cache;
use nautilus_common::clock::Clock;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::data::{InstrumentStatus, QuoteTick};
use nautilus_model::enums::{AccountType, MarketStatusAction, OrderSide, TimeInForce};
use nautilus_model::events::AccountState;
use nautilus_model::identifiers::{AccountId, InstrumentId, StrategyId, Symbol};
use nautilus_model::instruments::{Equity, InstrumentAny};
use nautilus_model::types::{AccountBalance, Currency, Money, Price, Quantity};
use nautilus_portfolio::Portfolio;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::bridge::LimitOrderRequest;

/// Current local admission policy, without a price band or quote age gate.
pub const LOCAL_RISK_CONFIG_VERSION: &str = "local-usd-v2";
/// Maximum trading-status age from the frozen v2 contract.
pub const STATUS_MAX_AGE_NS: u64 = 5_000_000_000;
/// Maximum account and position age from the frozen v2 contract.
pub const ACCOUNT_MAX_AGE_NS: u64 = 10_000_000_000;
/// Maximum trusted source/receipt clock difference.
pub const SOURCE_CLOCK_SKEW_NS: u64 = 500_000_000;
/// Default input queue capacity, independent of the order/cancel queue.
pub const INPUT_QUEUE_CAPACITY: usize = 1_024;
/// Maximum encoded input record size, including its line ending.
pub const INPUT_RECORD_MAX_BYTES: usize = 16_384;

/// Reads bounded JSONL records from a trusted local adapter pipe.
///
/// EOF, unknown malformed records, and non-quote queue loss invalidate continuity. Recognized
/// invalid quotes are dropped without invalidating other streams. Receipt time is stamped by
/// `now` at ingestion; JSON never supplies receipt timestamps. This adapter fabricates no heartbeats.
///
/// # Errors
///
/// Returns an error for unknown malformed/oversized input, I/O failure, or non-quote queue saturation.
pub fn read_jsonl_inputs(
    mut reader: impl BufRead,
    sender: InputSender,
    now: impl Fn() -> UnixNanos,
) -> Result<(), InputError> {
    let result = (|| {
        loop {
            let mut line = Vec::new();
            let count = reader
                .by_ref()
                .take((INPUT_RECORD_MAX_BYTES + 1) as u64)
                .read_until(b'\n', &mut line)
                .map_err(|e| InputError::Invalid(e.to_string()))?;

            if count == 0 {
                return Ok(());
            }

            if count > INPUT_RECORD_MAX_BYTES {
                return Err(InputError::Invalid("input record exceeds 16 KiB".into()));
            }
            let event = match serde_json::from_slice::<InputEvent>(&line) {
                Ok(event) => event,
                Err(e) => {
                    let quote = serde_json::from_slice::<serde_json::Value>(&line)
                        .is_ok_and(|value| value["kind"] == "quote");
                    if quote {
                        log::warn!("OrderHub quote decoding rejected: {e}");
                        continue;
                    }
                    return Err(InputError::Invalid(e.to_string()));
                }
            };
            let quote = matches!(event, InputEvent::Quote { .. });
            if let Err(e) = sender.try_send(event, now()) {
                if quote && matches!(e, InputError::Invalid(_) | InputError::QueueFull) {
                    log::warn!("OrderHub quote input dropped: {e}");
                    continue;
                }
                return Err(e);
            }
        }
    })();
    sender.invalidate();
    drop(sender);
    result
}

/// Source identity, event time, and strictly increasing stream sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputMeta {
    /// Configured trusted adapter identity.
    pub source_id: String,
    /// Source event timestamp, not the time of a later HTTP request.
    pub event_time_ns: u64,
    /// Stream sequence; instrument, quote, status, and account have separate streams.
    pub sequence: u64,
}

/// Trading state supplied by a versioned calendar/status source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingStatus {
    /// Regular session, subject to the supplied session interval.
    Regular,
    /// Trading suspended.
    Halted,
    /// Outside the supported regular session.
    Closed,
    /// The source cannot establish the trading state.
    Unknown,
}

/// Confirmed strategy allocation of an external position fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedHolding {
    /// Strategy identity configured on the node.
    pub strategy_id: String,
    /// Whole shares confirmed as belonging to this strategy.
    pub quantity: String,
    /// Confirmed USD cost basis; missing positive-position cost closes the P5 risk gate.
    pub cost_basis: Option<String>,
}

/// A versioned full snapshot or contiguous top-of-book replacement.
///
/// Decimal strings are mandatory. Quote deltas replace all four top-of-book fields; this
/// interface does not claim to implement depth-book patches. Accounts always carry full coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputEvent {
    /// Full instrument definition for the supported local cash-equity profile.
    Instrument {
        /// Versioned source envelope.
        meta: InputMeta,
        /// Exact contract and route identity.
        instrument_id: String,
        /// Currency code.
        currency: String,
        /// Metadata revision, independent of stream sequence.
        revision: u64,
        /// Definition effective timestamp.
        effective_time_ns: u64,
        /// Minimum price increment as an exact decimal string.
        price_increment: String,
        /// Price decimal precision.
        price_precision: u8,
        /// Minimum quantity increment; one share in this profile.
        size_increment: String,
        /// Quantity precision; zero in this profile.
        size_precision: u8,
    },
    /// Complete top-of-book data, either a snapshot or a contiguous replacement.
    Quote {
        /// Versioned source envelope.
        meta: InputMeta,
        /// Exact instrument identity.
        instrument_id: String,
        /// Instrument revision used to decode this quote.
        instrument_revision: u64,
        /// True when this is a complete resynchronization snapshot.
        snapshot: bool,
        /// Explicit source completeness flag.
        complete: bool,
        /// Exact bid price.
        bid_price: String,
        /// Exact ask price.
        ask_price: String,
        /// Whole-share bid size.
        bid_size: String,
        /// Whole-share ask size.
        ask_size: String,
    },
    /// Full trading-state and calendar snapshot.
    Status {
        /// Versioned source envelope.
        meta: InputMeta,
        /// Exact instrument identity.
        instrument_id: String,
        /// Instrument revision covered by this status.
        instrument_revision: u64,
        /// State reported by the trusted source.
        status: TradingStatus,
        /// Calendar identity and revision (including time-zone/DST rules at the source).
        calendar_revision: String,
        /// Explicit regular-session opening time.
        session_open_ns: u64,
        /// Explicit regular-session closing time, exclusive.
        session_close_ns: u64,
    },
    /// Complete cash-account and position snapshot.
    Account {
        /// Versioned source envelope.
        meta: InputMeta,
        /// Account identity, including venue.
        account_id: String,
        /// Exact instrument covered by the holding snapshot.
        instrument_id: String,
        /// Account currency.
        currency: String,
        /// True only when balances, buying power, and holdings are fully covered.
        complete: bool,
        /// Total cash balance.
        total: String,
        /// Cash locked by the external account.
        locked: String,
        /// Free cash balance.
        free: String,
        /// Cash buying power, excluding financing.
        buying_power: String,
        /// External total whole-share position.
        position: String,
        /// Confirmed strategy allocations, separate from the external position total.
        ownership: Vec<OwnedHolding>,
    },
}

/// Stable input/admission failure categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    /// The request is outside the approved local execution capabilities.
    UnsupportedCapability,
    /// Input content, scope, precision, or ordering is invalid.
    Invalid(String),
    /// Required facts need a full resynchronization or are not currently usable.
    Unavailable(String),
    /// The bounded adapter queue is saturated.
    QueueFull,
    /// The input receiver has stopped.
    Disconnected,
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedCapability => write!(f, "UNSUPPORTED_CAPABILITY"),
            Self::Invalid(reason) => write!(f, "INVALID_INPUT: {reason}"),
            Self::Unavailable(reason) => write!(f, "INPUT_NOT_READY: {reason}"),
            Self::QueueFull => write!(f, "INPUT_QUEUE_FULL"),
            Self::Disconnected => write!(f, "INPUT_DISCONNECTED"),
        }
    }
}

impl std::error::Error for InputError {}

/// Trusted adapter handle; it never accesses Cache or Portfolio.
#[derive(Debug, Clone)]
pub struct InputSender {
    tx: SyncSender<ReceivedInput>,
    generation: Arc<AtomicU64>,
    quote_generation: Arc<AtomicU64>,
}

impl InputSender {
    /// Enqueues an event with a locally observed receipt timestamp.
    ///
    /// The adapter supplies this timestamp from its runtime clock, never from client JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if the queue is full or the core thread has stopped.
    pub fn try_send(&self, event: InputEvent, received_at: UnixNanos) -> Result<(), InputError> {
        let quote = matches!(event, InputEvent::Quote { .. });
        if !event.within_limits() {
            if !quote {
                self.invalidate();
            }
            return Err(InputError::Invalid(
                "input field exceeds adapter limits".into(),
            ));
        }
        let input = ReceivedInput {
            event,
            received_at: received_at.as_u64(),
            generation: self.generation.load(Ordering::Acquire),
            quote_generation: self.quote_generation.load(Ordering::Acquire),
        };

        match self.tx.try_send(input) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                if !quote {
                    self.invalidate();
                }
                Err(InputError::QueueFull)
            }
            Err(TrySendError::Disconnected(_)) => Err(InputError::Disconnected),
        }
    }

    /// Invalidates all streams on transport loss, malformed framing, or queue loss.
    ///
    /// Already queued events from the preceding generation cannot restore admission.
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Marks an isolated quote connection loss while retaining the last legal valuation.
    ///
    /// The next quote snapshot may establish a new sequence baseline. Already queued quotes
    /// from the old connection are ignored; account and trading-status continuity is preserved.
    pub fn invalidate_quotes(&self) {
        self.quote_generation.fetch_add(1, Ordering::AcqRel);
    }
}

/// Counts observable results of one bounded core-thread poll.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct InputPoll {
    /// Successfully applied frames.
    pub applied: usize,
    /// Duplicate, older, or invalidated-generation frames.
    pub ignored: usize,
    /// Invalid frames.
    pub rejected: usize,
}

/// Core-thread input state and projection writer.
#[derive(Debug)]
pub struct InputRuntime {
    cache: Rc<RefCell<Cache>>,
    portfolio: Portfolio,
    clock: Rc<RefCell<dyn Clock>>,
    source_id: String,
    strategies: Vec<StrategyId>,
    rx: Receiver<ReceivedInput>,
    generation: Arc<AtomicU64>,
    observed_generation: u64,
    quote_generation: Arc<AtomicU64>,
    observed_quote_generation: u64,
    quote_snapshot_required: bool,
    quote_revision: Option<u64>,
    capacity: usize,
    connected: bool,
    instrument_stream: Stream,
    quote_stream: Stream,
    status_stream: Stream,
    account_stream: Stream,
    instrument: Option<Definition>,
    status: Option<Session>,
    account: Option<AccountFacts>,
    last_error: Option<InputError>,
    committed_candidates: Vec<(InputEvent, u64)>,
}

impl InputRuntime {
    /// Creates the bounded input channel for the AAPL.GOX / GOX-001 cash profile.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty source/strategy scope or a capacity outside 1..=4096.
    pub fn channel(
        cache: Rc<RefCell<Cache>>,
        portfolio: Portfolio,
        clock: Rc<RefCell<dyn Clock>>,
        source_id: &str,
        strategies: Vec<StrategyId>,
        capacity: usize,
    ) -> Result<(InputSender, Self), InputError> {
        if source_id.is_empty()
            || source_id.len() > 64
            || strategies.is_empty()
            || strategies.len() > 128
            || !(1..=4_096).contains(&capacity)
        {
            return Err(InputError::Invalid(
                "invalid adapter scope or queue capacity".into(),
            ));
        }

        let (tx, rx) = mpsc::sync_channel(capacity);
        let generation = Arc::new(AtomicU64::new(0));
        let quote_generation = Arc::new(AtomicU64::new(0));
        let sender = InputSender {
            tx,
            generation: Arc::clone(&generation),
            quote_generation: Arc::clone(&quote_generation),
        };
        let runtime = Self {
            cache,
            portfolio,
            clock,
            source_id: source_id.into(),
            strategies,
            rx,
            generation,
            observed_generation: 0,
            quote_generation,
            observed_quote_generation: 0,
            quote_snapshot_required: true,
            quote_revision: None,
            capacity,
            connected: true,
            instrument_stream: Stream::default(),
            quote_stream: Stream::default(),
            status_stream: Stream::default(),
            account_stream: Stream::default(),
            instrument: None,
            status: None,
            account: None,
            last_error: None,
            committed_candidates: Vec::new(),
        };
        Ok((sender, runtime))
    }

    /// Applies at most one queue capacity of frames and expires stale facts.
    pub fn poll(&mut self) -> InputPoll {
        let now = self.clock.borrow().timestamp_ns().as_u64();
        self.expire(now);
        self.observe_generation();
        let mut result = InputPoll::default();

        for _ in 0..self.capacity {
            match self.rx.try_recv() {
                Ok(input) => {
                    self.observe_generation();

                    if input.generation != self.observed_generation
                        || (matches!(input.event, InputEvent::Quote { .. })
                            && input.quote_generation != self.observed_quote_generation)
                    {
                        result.ignored += 1;
                        continue;
                    }

                    let now = self.clock.borrow().timestamp_ns().as_u64();
                    self.expire(now);
                    let candidate = (input.event.clone(), input.received_at);
                    match self.apply(input, now) {
                        Ok(true) => {
                            result.applied += 1;
                            if self.committed_candidates.len() < 4_096 {
                                self.committed_candidates.push(candidate);
                            } else {
                                self.invalidate_streams();
                                self.last_error = Some(InputError::QueueFull);
                                break;
                            }
                        }
                        Ok(false) => result.ignored += 1,
                        Err(e) => {
                            log::warn!("OrderHub input rejected: {e}");
                            self.last_error = Some(e);
                            result.rejected += 1;
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.connected = false;
                    self.invalidate_streams();
                    self.last_error = Some(InputError::Disconnected);
                    break;
                }
            }
        }
        result
    }

    /// Transfers validated input facts to the business projection writer.
    pub fn take_committed_candidates(&mut self) -> Vec<(InputEvent, u64)> {
        std::mem::take(&mut self.committed_candidates)
    }

    /// Restores a verified historical cache fact without restoring input freshness.
    ///
    /// # Errors
    /// Returns invalid historical definitions or account facts. Fresh snapshots are
    /// still required by every live stream before new orders can become writable.
    pub fn restore_committed_fact(
        &mut self,
        event: InputEvent,
        received_at: u64,
    ) -> Result<(), InputError> {
        self.project(event, received_at, true)
    }

    /// Returns the most recent rejected input or transport failure.
    #[must_use]
    pub fn last_error(&self) -> Option<&InputError> {
        self.last_error.as_ref()
    }

    /// Checks required account/status freshness and the existence of a matching legal quote.
    ///
    /// # Errors
    ///
    /// Returns an error when any required stream is unknown, stale, disconnected, or halted.
    pub fn check_ready(&self) -> Result<(), InputError> {
        let now = self.clock.borrow().timestamp_ns().as_u64();

        if !self.connected || self.generation.load(Ordering::Acquire) != self.observed_generation {
            return Err(InputError::Unavailable(
                "transport needs full resynchronization".into(),
            ));
        }

        for (name, stream, age) in [
            ("instrument", &self.instrument_stream, u64::MAX),
            ("trading status", &self.status_stream, STATUS_MAX_AGE_NS),
            ("account", &self.account_stream, ACCOUNT_MAX_AGE_NS),
        ] {
            if !stream.usable(now, age) {
                return Err(InputError::Unavailable(format!(
                    "{name} needs a fresh full snapshot"
                )));
            }
        }

        if self.quote_revision
            != self
                .instrument
                .as_ref()
                .map(|definition| definition.revision)
            || self.quote_revision.is_none()
        {
            return Err(unavailable(
                "quote matching the current instrument revision",
            ));
        }

        if !self.status.as_ref().is_some_and(|session| {
            session.status == TradingStatus::Regular && session.open <= now && now < session.close
        }) {
            return Err(InputError::Unavailable(
                "outside a known regular trading session".into(),
            ));
        }
        if self.account.as_ref().is_some_and(|account| {
            account.ownership.iter().any(|(id, quantity)| {
                *quantity > Decimal::ZERO && !account.costs.iter().any(|(cost_id, _)| cost_id == id)
            })
        }) {
            return Err(InputError::Unavailable(
                "positive owned holding lacks confirmed cost basis".into(),
            ));
        }
        Ok(())
    }

    /// Validates supported capabilities, exact increments, ownership, and first-dispatch deadline.
    ///
    /// # Errors
    ///
    /// Returns an error for unavailable inputs, an unsupported request, or invalid decimal steps.
    pub fn validate_order(&self, request: &LimitOrderRequest) -> Result<(), InputError> {
        if request.instrument_id != InstrumentId::from("AAPL.GOX")
            || request.time_in_force != TimeInForce::Day
            || request.post_only
            || request.reduce_only
            || request.quantity.precision != 0
            || request.client_id.is_some()
        {
            return Err(InputError::UnsupportedCapability);
        }
        self.check_ready()?;
        if !self.strategies.contains(&request.strategy_id) {
            return Err(InputError::Invalid(
                "strategy outside configured input scope".into(),
            ));
        }
        let now = self.clock.borrow().timestamp_ns().as_u64();

        if request
            .submit_before_ns
            .is_some_and(|deadline| now >= deadline)
        {
            return Err(InputError::Unavailable(
                "first-dispatch deadline reached".into(),
            ));
        }
        let definition = self
            .instrument
            .as_ref()
            .ok_or_else(|| unavailable("instrument"))?;
        let quantity = request.quantity.as_decimal();
        let price = request.price.as_decimal();

        if quantity <= Decimal::ZERO
            || price <= Decimal::ZERO
            || request.price.precision > definition.precision
            || price % definition.tick != Decimal::ZERO
        {
            return Err(InputError::Invalid(
                "order price/quantity violates instrument increments".into(),
            ));
        }
        let facts = self
            .account
            .as_ref()
            .ok_or_else(|| unavailable("account"))?;

        if request.side == OrderSide::Sell {
            let owned = facts
                .ownership
                .iter()
                .find(|(id, _)| *id == request.strategy_id)
                .map_or(Decimal::ZERO, |(_, quantity)| *quantity);

            if quantity > owned || quantity > facts.position {
                return Err(InputError::Unavailable(
                    "sell quantity lacks confirmed ownership".into(),
                ));
            }
        } else if price
            .checked_mul(quantity)
            .is_none_or(|notional| notional > facts.buying_power)
        {
            return Err(InputError::Unavailable(
                "buy notional exceeds cash buying power".into(),
            ));
        }
        Ok(())
    }

    /// Returns current account facts and the last legal quote for atomic reservation checks.
    ///
    /// # Errors
    ///
    /// Returns an error when required input facts are unavailable.
    pub fn risk_context(
        &self,
        strategy: &StrategyId,
    ) -> Result<crate::ledger::RiskContext, InputError> {
        self.check_ready()?;
        let account = self
            .account
            .as_ref()
            .ok_or_else(|| unavailable("account"))?;
        let cache = self.cache.borrow();
        let quote = cache
            .quote(&InstrumentId::from("AAPL.GOX"))
            .ok_or_else(|| unavailable("quote"))?;
        Ok(crate::ledger::RiskContext {
            account_id: "GOX-001".into(),
            strategy_budget: Decimal::from(1_000_000),
            account_budget: account.buying_power,
            owned_quantity: account
                .ownership
                .iter()
                .find(|(id, _)| id == strategy)
                .map_or(Decimal::ZERO, |(_, quantity)| *quantity),
            account_quantity: account.position,
            owned_cost: account
                .costs
                .iter()
                .find(|(id, _)| id == strategy)
                .map_or(Decimal::ZERO, |(_, cost)| *cost),
            strategy_exposure_limit: Decimal::from(1_000_000),
            account_exposure_limit: Decimal::from(2_000_000),
            reference_price: quote.ask_price.as_decimal(),
            fee_buffer: Decimal::ONE,
            maximum_quantity: Decimal::from(10_000),
            maximum_notional: Decimal::from(1_000_000),
            risk_config_version: LOCAL_RISK_CONFIG_VERSION.into(),
        })
    }

    fn observe_generation(&mut self) {
        let generation = self.generation.load(Ordering::Acquire);

        if generation != self.observed_generation {
            self.observed_generation = generation;
            self.invalidate_streams();
            self.last_error = Some(InputError::Unavailable("adapter lost continuity".into()));
        }
        let quote_generation = self.quote_generation.load(Ordering::Acquire);
        if quote_generation != self.observed_quote_generation {
            self.observed_quote_generation = quote_generation;
            self.quote_stream = Stream::default();
            self.quote_snapshot_required = true;
            self.last_error = Some(InputError::Unavailable("quote connection lost".into()));
        }
    }

    fn invalidate_streams(&mut self) {
        self.instrument_stream.valid = false;
        self.quote_stream.valid = false;
        self.quote_revision = None;
        self.quote_snapshot_required = true;
        self.status_stream.valid = false;
        self.account_stream.valid = false;
    }

    fn expire(&mut self, now: u64) {
        for (stream, age) in [
            (&mut self.status_stream, STATUS_MAX_AGE_NS),
            (&mut self.account_stream, ACCOUNT_MAX_AGE_NS),
        ] {
            if !stream.usable(now, age) {
                stream.valid = false;
            }
        }
    }

    fn apply(&mut self, input: ReceivedInput, now: u64) -> Result<bool, InputError> {
        if matches!(input.event, InputEvent::Quote { .. }) {
            return self.apply_quote(input, now);
        }
        let is_definition = matches!(&input.event, InputEvent::Instrument { .. });
        let conflicting_duplicate = self.conflicting_definition(&input.event);
        let (meta, stream, max_age, snapshot) = match &input.event {
            InputEvent::Instrument { meta, .. } => {
                (meta, &mut self.instrument_stream, u64::MAX, true)
            }
            InputEvent::Quote { .. } => {
                unreachable!("quotes use their independent validation path")
            }
            InputEvent::Status { meta, .. } => {
                (meta, &mut self.status_stream, STATUS_MAX_AGE_NS, true)
            }
            InputEvent::Account { meta, .. } => {
                (meta, &mut self.account_stream, ACCOUNT_MAX_AGE_NS, true)
            }
        };

        if meta.source_id != self.source_id {
            self.invalidate_streams();
            return Err(InputError::Invalid("unconfigured source identity".into()));
        }

        // Older frames cannot refresh timestamps, nor overwrite a later halt or snapshot
        if meta.sequence != 0 && meta.sequence <= stream.sequence {
            if conflicting_duplicate {
                self.instrument_stream.valid = false;
                self.quote_stream.valid = false;
                self.quote_revision = None;
                self.status_stream.valid = false;
                return Err(InputError::Invalid(
                    "conflicting duplicate instrument definition".into(),
                ));
            }
            return Ok(false);
        }
        let previous_time = stream.event_time;
        let contiguous = stream.sequence.checked_add(1) == Some(meta.sequence);
        stream.sequence = meta.sequence.max(stream.sequence);

        if meta.sequence == 0
            || !fresh(meta.event_time_ns, input.received_at, now, max_age)
            || input.received_at.saturating_sub(meta.event_time_ns) > SOURCE_CLOCK_SKEW_NS
            || meta.event_time_ns < previous_time
            || (!snapshot && (!stream.valid || !contiguous))
        {
            stream.valid = false;
            if is_definition {
                self.quote_stream.valid = false;
                self.quote_revision = None;
                self.status_stream.valid = false;
            }
            return Err(InputError::Unavailable(
                "invalid time or sequence; full snapshot required".into(),
            ));
        }
        stream.valid = false;
        let sequence = meta.sequence;
        let event_time = meta.event_time_ns;
        let received_at = input.received_at;
        let stream_kind = match &input.event {
            InputEvent::Instrument { .. } => 0,
            InputEvent::Quote { .. } => 1,
            InputEvent::Status { .. } => 2,
            InputEvent::Account { .. } => 3,
        };
        if let Err(e) = self.project(input.event, received_at, false) {
            if is_definition {
                self.quote_stream.valid = false;
                self.quote_revision = None;
                self.status_stream.valid = false;
            }
            return Err(e);
        }
        let stream = match stream_kind {
            0 => &mut self.instrument_stream,
            1 => &mut self.quote_stream,
            2 => &mut self.status_stream,
            _ => &mut self.account_stream,
        };
        *stream = Stream {
            sequence,
            event_time,
            received_at,
            valid: true,
        };
        Ok(true)
    }

    fn apply_quote(&mut self, input: ReceivedInput, now: u64) -> Result<bool, InputError> {
        let InputEvent::Quote {
            meta,
            snapshot,
            instrument_revision,
            ..
        } = &input.event
        else {
            unreachable!("only quote events enter quote validation")
        };
        if meta.source_id != self.source_id {
            return Err(InputError::Invalid(
                "unconfigured quote source identity".into(),
            ));
        }
        if meta.sequence == 0
            || meta.event_time_ns == 0
            || meta.event_time_ns > input.received_at
            || input.received_at > now
        {
            return Err(InputError::Invalid("invalid quote time or sequence".into()));
        }
        if meta.sequence <= self.quote_stream.sequence {
            return Ok(false);
        }
        if meta.event_time_ns < self.quote_stream.event_time {
            return Err(InputError::Invalid("quote time moved backwards".into()));
        }
        if self.quote_snapshot_required && !snapshot {
            return Err(InputError::Unavailable(
                "quote connection needs its initial snapshot".into(),
            ));
        }
        let sequence = meta.sequence;
        let event_time = meta.event_time_ns;
        let revision = *instrument_revision;
        let gap = self.quote_stream.sequence != 0
            && self.quote_stream.sequence.checked_add(1) != Some(sequence);
        self.project(input.event, input.received_at, false)?;
        self.quote_stream = Stream {
            sequence,
            event_time,
            received_at: input.received_at,
            valid: true,
        };
        self.quote_revision = Some(revision);
        self.quote_snapshot_required = false;
        if gap {
            log::warn!("OrderHub quote sequence gap; applied complete top-of-book replacement");
        }
        Ok(true)
    }

    fn conflicting_definition(&self, event: &InputEvent) -> bool {
        let Some(old) = &self.instrument else {
            return false;
        };
        let InputEvent::Instrument {
            instrument_id,
            currency,
            revision,
            effective_time_ns,
            price_increment,
            price_precision,
            size_increment,
            size_precision,
            ..
        } = event
        else {
            return false;
        };
        *revision >= old.revision
            && (*revision != old.revision
                || instrument_id != "AAPL.GOX"
                || currency != "USD"
                || *effective_time_ns != old.effective_time
                || *price_precision != old.precision
                || decimal(price_increment).ok() != Some(old.tick)
                || decimal(size_increment).ok() != Some(Decimal::ONE)
                || *size_precision != 0)
    }

    fn project(
        &mut self,
        event: InputEvent,
        received_at: u64,
        historical: bool,
    ) -> Result<(), InputError> {
        match event {
            InputEvent::Instrument {
                meta,
                instrument_id,
                currency,
                revision,
                effective_time_ns,
                price_increment,
                price_precision,
                size_increment,
                size_precision,
            } => {
                check_instrument(&instrument_id)?;
                let tick = decimal(&price_increment)?;

                if currency != "USD"
                    || revision == 0
                    || effective_time_ns == 0
                    || effective_time_ns > meta.event_time_ns
                    || price_precision > 9
                    || tick <= Decimal::ZERO
                    || tick.scale() > u32::from(price_precision)
                    || decimal(&size_increment)? != Decimal::ONE
                    || size_precision != 0
                {
                    return Err(InputError::Invalid("invalid cash-equity definition".into()));
                }
                let definition = Definition {
                    revision,
                    precision: price_precision,
                    tick,
                    effective_time: effective_time_ns,
                };

                if let Some(old) = &self.instrument {
                    if revision < old.revision || (revision == old.revision && definition != *old) {
                        return Err(InputError::Invalid(
                            "conflicting instrument revision".into(),
                        ));
                    }

                    if definition != *old {
                        self.quote_stream.valid = false;
                        self.quote_revision = None;
                        self.status_stream.valid = false;
                    }
                }
                let price_increment = Price::from_decimal_dp(tick, price_precision)
                    .map_err(|e| InputError::Invalid(e.to_string()))?;
                let equity = Equity::builder()
                    .instrument_id(InstrumentId::from("AAPL.GOX"))
                    .raw_symbol(Symbol::from("AAPL"))
                    .currency(Currency::USD())
                    .price_precision(price_precision)
                    .price_increment(price_increment)
                    .ts_event(UnixNanos::from(meta.event_time_ns))
                    .ts_init(UnixNanos::from(received_at))
                    .build()
                    .map_err(|e| InputError::Invalid(e.to_string()))?;
                self.cache
                    .borrow_mut()
                    .add_instrument(InstrumentAny::Equity(equity))
                    .map_err(|e| InputError::Invalid(e.to_string()))?;
                self.instrument = Some(definition);
            }
            InputEvent::Quote {
                meta,
                instrument_id,
                instrument_revision,
                complete,
                bid_price,
                ask_price,
                bid_size,
                ask_size,
                ..
            } => {
                let definition =
                    self.definition(&instrument_id, instrument_revision, historical)?;

                if !complete {
                    return Err(InputError::Invalid("incomplete quote".into()));
                }
                let bid = exact_price(&bid_price, definition)?;
                let ask = exact_price(&ask_price, definition)?;

                if bid > ask {
                    return Err(InputError::Invalid("crossed quote".into()));
                }
                let quote = QuoteTick::new_checked(
                    InstrumentId::from("AAPL.GOX"),
                    bid,
                    ask,
                    whole_quantity(&bid_size)?,
                    whole_quantity(&ask_size)?,
                    UnixNanos::from(meta.event_time_ns),
                    UnixNanos::from(received_at),
                )
                .map_err(|e| InputError::Invalid(e.to_string()))?;
                self.cache
                    .borrow_mut()
                    .add_quote(quote)
                    .map_err(|e| InputError::Invalid(e.to_string()))?;
                self.portfolio.update_quote_tick(&quote);
            }
            InputEvent::Status {
                meta,
                instrument_id,
                instrument_revision,
                status,
                calendar_revision,
                session_open_ns,
                session_close_ns,
                ..
            } => {
                self.definition(&instrument_id, instrument_revision, historical)?;

                if calendar_revision.is_empty()
                    || calendar_revision.len() > 128
                    || session_open_ns >= session_close_ns
                {
                    return Err(InputError::Invalid(
                        "missing calendar revision or session interval".into(),
                    ));
                }
                self.status = Some(Session {
                    status,
                    open: session_open_ns,
                    close: session_close_ns,
                });
                let action = match status {
                    TradingStatus::Regular => MarketStatusAction::Trading,
                    TradingStatus::Halted => MarketStatusAction::Halt,
                    TradingStatus::Closed | TradingStatus::Unknown => MarketStatusAction::None,
                };
                self.cache
                    .borrow_mut()
                    .add_instrument_status(InstrumentStatus::new(
                        InstrumentId::from("AAPL.GOX"),
                        action,
                        UnixNanos::from(meta.event_time_ns),
                        UnixNanos::from(received_at),
                        None,
                        None,
                        Some(status == TradingStatus::Regular),
                        None,
                        Some(true),
                    ))
                    .map_err(|e| InputError::Invalid(e.to_string()))?;
            }
            InputEvent::Account {
                meta,
                account_id,
                instrument_id,
                currency,
                complete,
                total,
                locked,
                free,
                buying_power,
                position,
                ownership,
            } => {
                check_instrument(&instrument_id)?;

                if account_id != "GOX-001"
                    || currency != "USD"
                    || !complete
                    || ownership.len() > 128
                {
                    return Err(InputError::Invalid(
                        "account identity or snapshot coverage mismatch".into(),
                    ));
                }
                let total = cash(&total)?;
                let locked = cash(&locked)?;
                let free = cash(&free)?;
                let buying_power = cash(&buying_power)?;

                if locked.checked_add(free) != Some(total) || buying_power > free {
                    return Err(InputError::Invalid(
                        "inconsistent cash balances or buying power".into(),
                    ));
                }
                let position = whole_decimal(&position)?;
                let mut allocations = Vec::with_capacity(ownership.len());
                let mut costs = Vec::with_capacity(ownership.len());
                let mut allocated = Decimal::ZERO;

                for holding in ownership {
                    let strategy_id = self
                        .strategies
                        .iter()
                        .find(|id| id.as_ref() == holding.strategy_id)
                        .copied()
                        .ok_or_else(|| {
                            InputError::Invalid("unconfigured strategy ownership".into())
                        })?;

                    if allocations.iter().any(|(id, _)| *id == strategy_id) {
                        return Err(InputError::Invalid("duplicate strategy ownership".into()));
                    }
                    let quantity = whole_decimal(&holding.quantity)?;
                    if let Some(cost) = holding.cost_basis {
                        let cost = cash(&cost)?;
                        if quantity == Decimal::ZERO && cost != Decimal::ZERO {
                            return Err(InputError::Invalid(
                                "nonzero cost for zero holding".into(),
                            ));
                        }
                        costs.push((strategy_id, cost));
                    }
                    allocated = allocated
                        .checked_add(quantity)
                        .ok_or_else(|| InputError::Invalid("holding total overflow".into()))?;
                    allocations.push((strategy_id, quantity));
                }

                if allocated > position {
                    return Err(InputError::Invalid(
                        "ownership exceeds external position".into(),
                    ));
                }
                let balance = AccountBalance::new_checked(
                    Money::from_decimal(total, Currency::USD())
                        .map_err(|e| InputError::Invalid(e.to_string()))?,
                    Money::from_decimal(locked, Currency::USD())
                        .map_err(|e| InputError::Invalid(e.to_string()))?,
                    Money::from_decimal(free, Currency::USD())
                        .map_err(|e| InputError::Invalid(e.to_string()))?,
                )
                .map_err(|e| InputError::Invalid(e.to_string()))?;
                let state = AccountState::new(
                    AccountId::from("GOX-001"),
                    AccountType::Cash,
                    vec![balance],
                    vec![],
                    true,
                    UUID4::new(),
                    UnixNanos::from(meta.event_time_ns),
                    UnixNanos::from(received_at),
                    Some(Currency::USD()),
                );
                self.cache
                    .borrow_mut()
                    .update_account_state(&state)
                    .map_err(|e| InputError::Invalid(e.to_string()))?;
                self.portfolio.update_account(&state);
                // Authoritative input history lives in the business journal; keep one Cache state.
                self.cache
                    .borrow_mut()
                    .purge_account_events(UnixNanos::from(received_at), 0);
                self.account = Some(AccountFacts {
                    buying_power,
                    position,
                    ownership: allocations,
                    costs,
                });
            }
        }
        Ok(())
    }

    fn definition(
        &self,
        instrument: &str,
        revision: u64,
        historical: bool,
    ) -> Result<&Definition, InputError> {
        check_instrument(instrument)?;
        self.instrument
            .as_ref()
            .filter(|definition| {
                (historical || self.instrument_stream.valid) && definition.revision == revision
            })
            .ok_or_else(|| {
                InputError::Unavailable("instrument revision needs resynchronization".into())
            })
    }
}

#[derive(Debug)]
struct ReceivedInput {
    event: InputEvent,
    received_at: u64,
    generation: u64,
    quote_generation: u64,
}

#[derive(Debug, Default)]
struct Stream {
    sequence: u64,
    event_time: u64,
    received_at: u64,
    valid: bool,
}

impl Stream {
    fn usable(&self, now: u64, max_age: u64) -> bool {
        self.valid && fresh(self.event_time, self.received_at, now, max_age)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Definition {
    revision: u64,
    precision: u8,
    tick: Decimal,
    effective_time: u64,
}

impl InputEvent {
    fn within_limits(&self) -> bool {
        let (meta, identifiers, numbers) = match self {
            Self::Instrument {
                meta,
                instrument_id,
                currency,
                price_increment,
                size_increment,
                ..
            } => (
                meta,
                vec![instrument_id, currency],
                vec![price_increment, size_increment],
            ),
            Self::Quote {
                meta,
                instrument_id,
                bid_price,
                ask_price,
                bid_size,
                ask_size,
                ..
            } => (
                meta,
                vec![instrument_id],
                vec![bid_price, ask_price, bid_size, ask_size],
            ),
            Self::Status {
                meta,
                instrument_id,
                calendar_revision,
                ..
            } => {
                if calendar_revision.len() > 128 {
                    return false;
                }
                (meta, vec![instrument_id], vec![])
            }
            Self::Account {
                meta,
                account_id,
                instrument_id,
                currency,
                total,
                locked,
                free,
                buying_power,
                position,
                ownership,
                ..
            } => {
                if ownership.len() > 128
                    || ownership.iter().any(|owned| {
                        owned.strategy_id.len() > 64
                            || owned.quantity.len() > 40
                            || owned
                                .cost_basis
                                .as_ref()
                                .is_some_and(|cost| cost.len() > 40)
                    })
                {
                    return false;
                }
                (
                    meta,
                    vec![account_id, instrument_id, currency],
                    vec![total, locked, free, buying_power, position],
                )
            }
        };
        meta.source_id.len() <= 64
            && identifiers.iter().all(|id| id.len() <= 64)
            && numbers.iter().all(|number| number.len() <= 40)
    }
}

#[derive(Debug)]
struct Session {
    status: TradingStatus,
    open: u64,
    close: u64,
}

#[derive(Debug)]
struct AccountFacts {
    buying_power: Decimal,
    position: Decimal,
    ownership: Vec<(StrategyId, Decimal)>,
    costs: Vec<(StrategyId, Decimal)>,
}

fn fresh(event_time: u64, received_at: u64, now: u64, max_age: u64) -> bool {
    event_time != 0
        && event_time <= received_at
        && received_at <= now
        && now - event_time <= max_age
        && now - received_at <= max_age
}

fn unavailable(name: &str) -> InputError {
    InputError::Unavailable(format!("missing {name}"))
}

fn check_instrument(instrument: &str) -> Result<(), InputError> {
    if instrument == "AAPL.GOX" {
        Ok(())
    } else {
        Err(InputError::Invalid(
            "instrument/route outside configured scope".into(),
        ))
    }
}

fn decimal(value: &str) -> Result<Decimal, InputError> {
    if value.is_empty()
        || value.len() > 40
        || !value.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Err(InputError::Invalid(
            "expected unsigned plain decimal string".into(),
        ));
    }
    Decimal::from_str_exact(value).map_err(|e| InputError::Invalid(e.to_string()))
}

fn cash(value: &str) -> Result<Decimal, InputError> {
    let value = decimal(value)?;

    if value.scale() > 2 {
        return Err(InputError::Invalid(
            "USD precision exceeds two decimals".into(),
        ));
    }
    Ok(value)
}

fn whole_decimal(value: &str) -> Result<Decimal, InputError> {
    let value = decimal(value)?;

    if value.scale() != 0 {
        return Err(InputError::Invalid("expected whole shares".into()));
    }
    Ok(value)
}

fn whole_quantity(value: &str) -> Result<Quantity, InputError> {
    whole_decimal(value)?;
    value.parse().map_err(InputError::Invalid)
}

fn exact_price(value: &str, definition: &Definition) -> Result<Price, InputError> {
    let value = decimal(value)?;

    if value <= Decimal::ZERO
        || value.scale() > u32::from(definition.precision)
        || value % definition.tick != Decimal::ZERO
    {
        return Err(InputError::Invalid(
            "quote violates price precision or tick".into(),
        ));
    }
    Price::from_decimal_dp(value, definition.precision)
        .map_err(|e| InputError::Invalid(e.to_string()))
}
