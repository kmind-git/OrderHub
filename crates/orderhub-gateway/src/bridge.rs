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

//! Admission bridge from external submissions into the native execution path.
//!
//! The kernel's `Strategy::submit_order` performs four steps before a
//! `SubmitOrder` command can safely enter the risk engine: construct the order,
//! register it in the `Cache` (the risk engine reads orders back out of the
//! cache), publish `OrderInitialized`, and dispatch the command to the risk
//! engine's queued endpoint. The engine cannot be reached by simply forwarding
//! a `SubmitOrder` command, which is why this bridge exists.
//!
//! [`OrderHubBridge`] replicates that native path for external requests while
//! adding the OrderHub admission semantics from the design contracts:
//!
//! - Idempotency: retries with the same request key return the original
//!   admission; a different fingerprint for the same key is rejected.
//! - Durable admission: the request is committed to the business journal
//!   before the native chain is entered.
//! - Deterministic order IDs: `client_order_id` is a pure function of
//!   `(journal_epoch, seq)`, so restart replays never reuse an ID.
//! - Attribution: the submitting strategy identity flows through the order,
//!   command, and resulting events unchanged.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use nautilus_common::cache::Cache;
use nautilus_common::clock::Clock;
use nautilus_common::messages::execution::{SubmitOrder, TradingCommand};
use nautilus_common::msgbus::{self, MessagingSwitchboard};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::enums::TimeInForce;
use nautilus_model::events::OrderEventAny;
use nautilus_model::identifiers::{ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId};
use nautilus_model::orders::{LimitOrder, Order, OrderAny};
use nautilus_model::types::{Price, Quantity};

use crate::journal::{AdmissionRecord, BusinessJournal, JournalOutcome, RequestRecord};

/// Errors from the admission bridge.
#[derive(Debug)]
pub enum BridgeError {
    /// A bounded ingress queue or caller allocation is exhausted before admission.
    Capacity(String),
    /// Queue residence exceeded the deadline before any admission was attempted.
    QueueExpired,
    /// The caller does not own the target order or lacks its current strategy authorization.
    PermissionDenied,
    /// Required market/account facts are unusable or the request is unsupported.
    Input(crate::inputs::InputError),
    /// The same request key was retried with different business parameters.
    IdempotencyConflict {
        submitter_id: String,
        request_id: String,
        original_fingerprint: String,
        new_fingerprint: String,
    },
    /// The business journal failed (storage or encoding).
    Journal(String),
    /// The order could not be constructed from the request.
    Order(String),
    /// The order could not be registered in the cache.
    Cache(String),
    /// The readiness gate is not admitting new orders.
    NotReady {
        /// Current readiness state name.
        state: String,
        /// Halt or restoring reason.
        reason: String,
    },
}

impl std::error::Error for BridgeError {}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Capacity(reason) => write!(f, "{reason}"),
            Self::QueueExpired => write!(f, "admission queue wait expired before processing"),
            Self::PermissionDenied => write!(f, "order not authorized for this submitter"),
            Self::Input(e) => write!(f, "{e}"),
            Self::IdempotencyConflict {
                submitter_id,
                request_id,
                ..
            } => write!(
                f,
                "idempotency conflict for key {submitter_id}|SubmitOrder|{request_id}"
            ),
            Self::Journal(err) => write!(f, "journal error: {err}"),
            Self::Order(err) => write!(f, "order construction error: {err}"),
            Self::Cache(err) => write!(f, "cache registration error: {err}"),
            Self::NotReady { state, reason } => {
                write!(f, "order hub not ready ({state}): {reason}")
            }
        }
    }
}

impl From<crate::journal::JournalError> for BridgeError {
    fn from(err: crate::journal::JournalError) -> Self {
        Self::Journal(err.to_string())
    }
}

/// A validated external limit-order request.
///
/// Quantities and prices use the exact Nautilus domain types; the journal
/// stores them as decimal strings.
#[derive(Debug, Clone)]
pub struct LimitOrderRequest {
    /// Server-side submitter identity.
    pub submitter_id: String,
    /// Client-generated unique request ID, stable across retries.
    pub request_id: String,
    /// Strategy which owns this order (attribution, limits, PnL).
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub side: nautilus_model::enums::OrderSide,
    pub quantity: Quantity,
    pub price: Price,
    pub time_in_force: TimeInForce,
    pub post_only: bool,
    pub reduce_only: bool,
    /// Optional routing client for the execution engine.
    pub client_id: Option<ClientId>,
    /// Business deadline for first dispatch (ns since epoch), if any.
    pub submit_before_ns: Option<u64>,
}

/// The result of admitting a request through the bridge.
#[derive(Debug, Clone)]
pub struct AdmissionReceipt {
    pub result: String,
    pub reason: String,
    /// Journal epoch of the admission.
    pub journal_epoch: u64,
    /// Committed journal sequence of the admission.
    pub seq: u64,
    /// The order ID assigned at first admission.
    pub client_order_id: ClientOrderId,
    /// `true` when this call was an idempotent retry of an existing request.
    pub already_admitted: bool,
}

/// Bridge into the native `Cache -> RiskEngine -> ExecutionEngine` chain.
#[derive(Debug)]
pub struct OrderHubBridge {
    trader_id: TraderId,
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    journal: Arc<BusinessJournal>,
    worker: Option<crate::worker::PersistenceWorker>,
    quota: Option<Rc<RefCell<crate::quota::QuotaLedger>>>,
    gate: Option<std::sync::Arc<crate::readiness::ReadinessGate>>,
    inputs: Option<Rc<RefCell<crate::inputs::InputRuntime>>>,
    storage_fault: RefCell<Option<String>>,
    initial_readiness: bool,
    runtime_owners: Vec<Box<dyn std::fmt::Debug>>,
}

impl OrderHubBridge {
    /// Creates a bridge over the node's shared clock, cache, and journal.
    ///
    /// Without a persistence worker the journal commits synchronously on the
    /// calling thread (M0 behaviour); install one with [`Self::with_worker`]
    /// for group-commit batching.
    pub fn new(
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        journal: Arc<BusinessJournal>,
    ) -> Self {
        Self {
            trader_id,
            clock,
            cache,
            journal,
            worker: None,
            quota: None,
            gate: None,
            inputs: None,
            storage_fault: RefCell::new(None),
            initial_readiness: false,
            runtime_owners: Vec::new(),
        }
    }

    /// Retains an engine owner until the core stops, instead of leaking its resources.
    #[must_use]
    pub fn with_runtime_owner(mut self, owner: impl std::fmt::Debug + 'static) -> Self {
        self.runtime_owners.push(Box::new(owner));
        self
    }

    /// Enables initial readiness for a verified journal with no business exposure.
    #[must_use]
    pub fn with_initial_readiness(mut self) -> Self {
        self.initial_readiness = true;
        self
    }

    /// Publishes a durable nontrading lifecycle transition.
    ///
    /// # Errors
    /// Returns persistence failure and closes the gate.
    pub fn transition(
        &self,
        state: crate::readiness::Readiness,
        reason: &str,
    ) -> Result<(), crate::journal::JournalError> {
        if let Some(gate) = &self.gate {
            gate.set(state, reason);
        }
        let result = self.journal.commit_control(crate::ledger::ControlRecord {
            readiness: state.as_str().into(),
            writable: false,
            connected: None,
            reason: reason.into(),
        });
        if let Err(e) = &result {
            self.halt(&e.to_string());
        }
        result
    }

    /// Publishes a recovery checkpoint while dispatch is paused on the core thread.
    pub fn checkpoint(
        &self,
        path: &std::path::Path,
        identity: crate::journal::RecoveryIdentity,
    ) -> Result<std::path::PathBuf, crate::journal::JournalError> {
        self.journal.checkpoint(path, identity)
    }

    /// Installs the group-commit persistence worker for durable writes.
    #[must_use]
    pub fn with_worker(mut self, worker: crate::worker::PersistenceWorker) -> Self {
        self.worker = Some(worker);
        self
    }

    /// Installs the readiness gate; submissions are refused unless READY.
    #[must_use]
    pub fn with_gate(mut self, gate: std::sync::Arc<crate::readiness::ReadinessGate>) -> Self {
        self.gate = Some(gate);
        self
    }

    /// Installs the per-strategy quota ledger for admission checks.
    #[must_use]
    pub fn with_quota(mut self, quota: Rc<RefCell<crate::quota::QuotaLedger>>) -> Self {
        self.quota = Some(quota);
        self
    }

    /// Installs market/account admission checks and core-thread projection updates.
    #[must_use]
    pub fn with_inputs(mut self, inputs: crate::inputs::InputRuntime) -> Self {
        let inputs = Rc::new(RefCell::new(inputs));
        let dispatch_inputs = Rc::downgrade(&inputs);
        let journal = Arc::downgrade(&self.journal);
        let gate = self.gate.clone();
        crate::exec_wiring::install_dispatch_guard(move |id| {
            if gate.as_ref().is_some_and(|gate| !gate.is_admissible()) {
                return Err("new-risk gate closed before dispatch".into());
            }
            let dispatch_inputs = dispatch_inputs
                .upgrade()
                .ok_or_else(|| "dispatch inputs stopped".to_string())?;
            let journal = journal
                .upgrade()
                .ok_or_else(|| "dispatch journal stopped".to_string())?;
            let seq = crate::exec_wiring::seq_from_client_order_id(id)
                .ok_or_else(|| "invalid dispatch order ID".to_string())?;
            let record = journal
                .order(seq)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "missing committed dispatch order".to_string())?;
            let admission = record.admission;
            let request = LimitOrderRequest {
                submitter_id: admission.submitter_id,
                request_id: admission.request_id,
                strategy_id: StrategyId::new_checked(&admission.strategy_id)
                    .map_err(|e| e.to_string())?,
                instrument_id: admission
                    .instrument_id
                    .parse::<InstrumentId>()
                    .map_err(|e| e.to_string())?,
                side: if admission.order_side == "BUY" {
                    nautilus_model::enums::OrderSide::Buy
                } else {
                    nautilus_model::enums::OrderSide::Sell
                },
                quantity: admission.quantity.parse::<Quantity>()?,
                price: admission.price.parse::<Price>()?,
                time_in_force: TimeInForce::Day,
                post_only: admission.post_only,
                reduce_only: admission.reduce_only,
                client_id: None,
                submit_before_ns: admission.submit_before_ns,
            };
            dispatch_inputs
                .borrow()
                .validate_order(&request)
                .map_err(|e| e.to_string())
        });
        self.inputs = Some(inputs);
        self
    }

    /// Drains a bounded batch of input frames on the core thread.
    pub fn poll_inputs(&self) {
        if let Some(inputs) = &self.inputs {
            let facts = {
                let mut inputs = inputs.borrow_mut();
                inputs.poll();
                inputs.take_committed_candidates()
            };
            if !facts.is_empty() {
                let entries: Vec<_> = facts
                    .into_iter()
                    .map(
                        |(event, received_at_ns)| crate::journal::PendingEntry::Input {
                            event: Box::new(event),
                            received_at_ns,
                        },
                    )
                    .collect();
                if let Err(e) = self.journal.append_batch(&entries) {
                    self.halt(&e.to_string());
                }
            }
        }
    }

    /// Returns whether the installed market/account inputs permit new risk.
    #[must_use]
    pub fn inputs_ready(&self) -> bool {
        self.storage_fault.borrow().is_none()
            && self
                .inputs
                .as_ref()
                .is_none_or(|inputs| inputs.borrow().check_ready().is_ok())
    }

    /// Records changed business health independently from the live health response.
    pub fn commit_health(&self, connected: Option<bool>) {
        if self.storage_fault.borrow().is_some() {
            return;
        }
        if self.initial_readiness
            && connected == Some(true)
            && self
                .gate
                .as_ref()
                .is_some_and(|gate| gate.get() == crate::readiness::Readiness::Reconciling)
            && self
                .inputs
                .as_ref()
                .is_some_and(|inputs| inputs.borrow().check_ready().is_ok())
        {
            let ready = crate::ledger::ControlRecord {
                readiness: "READY".into(),
                writable: true,
                connected,
                reason: "valid account/status, legal quote and connection; no historical business exposure".into(),
            };
            if let Err(e) = self.journal.commit_control(ready) {
                self.halt(&e.to_string());
                return;
            }
            if let Some(gate) = &self.gate {
                gate.set(crate::readiness::Readiness::Ready, "");
            }
        }
        let state = self
            .gate
            .as_ref()
            .map_or(crate::readiness::Readiness::Booting, |gate| gate.get());
        let inputs_ready = self.inputs_ready();
        let control = crate::ledger::ControlRecord {
            readiness: state.as_str().into(),
            writable: state == crate::readiness::Readiness::Ready
                && inputs_ready
                && connected != Some(false),
            connected,
            reason: if inputs_ready {
                self.gate
                    .as_ref()
                    .map_or_else(String::new, |gate| gate.halt_reason())
            } else {
                "INPUTS_UNAVAILABLE".into()
            },
        };
        if let Err(e) = self.journal.commit_control(control) {
            self.halt(&e.to_string());
        }
    }

    fn check_inputs(&self, request: &LimitOrderRequest) -> Result<(), BridgeError> {
        self.poll_inputs();
        if let Some(reason) = self.storage_fault.borrow().as_ref() {
            return Err(BridgeError::Journal(reason.clone()));
        }

        if let Some(inputs) = &self.inputs {
            inputs
                .borrow()
                .validate_order(request)
                .map_err(BridgeError::Input)?;
        }
        Ok(())
    }

    /// Admits and submits a limit order through the native execution path.
    ///
    /// Order of operations (the core waits for the durable writer result):
    ///
    /// 1. Idempotency check against the journal's request index.
    /// 2. Atomic request, order, reservation, pending dispatch and original-result commit.
    /// 3. Construct the `LimitOrder` and register it in the cache.
    /// 4. Publish `OrderInitialized` on the strategy events topic.
    /// 5. Durable `DISPATCH_STARTED` and current input/deadline recheck.
    /// 6. Dispatch `SubmitOrder` to the risk engine's queued endpoint.
    pub fn submit_limit_order(
        &self,
        req: &LimitOrderRequest,
    ) -> Result<AdmissionReceipt, BridgeError> {
        validate_request_ids(&req.submitter_id, &req.request_id)?;
        let fingerprint = fingerprint(req);
        // Original results remain queryable even while the new-risk gate is closed
        if let Some(record) = self
            .journal
            .find_request(&req.submitter_id, "SubmitOrder", &req.request_id)
            .map_err(|e| {
                self.halt(&e.to_string());
                BridgeError::from(e)
            })?
        {
            return self.receipt_for_existing(record, req, fingerprint);
        }
        if let Some(gate) = &self.gate
            && !gate.is_admissible()
        {
            return Err(BridgeError::NotReady {
                state: gate.get().as_str().into(),
                reason: gate.halt_reason(),
            });
        }
        let ts_init_ns = self.clock.borrow().timestamp_ns().as_u64();
        let input_error = self.check_inputs(req).err();
        if let Some(BridgeError::Journal(reason)) = &input_error {
            return Err(BridgeError::Journal(reason.clone()));
        }
        let mut context = match &self.inputs {
            Some(inputs) if input_error.is_none() => Some(
                inputs
                    .borrow()
                    .risk_context(&req.strategy_id)
                    .map_err(BridgeError::Input)?,
            ),
            _ => None,
        };
        if input_error.is_none()
            && self
                .cache
                .borrow()
                .orders_total_count(None, None, None, None, None)
                >= 8_192
        {
            self.halt("native order cache limit reached; P6 checkpoint/reconciliation required");
            return Err(BridgeError::Journal(
                "native order cache limit reached".into(),
            ));
        }
        if let (Some(context), Some(quota)) = (&mut context, &self.quota) {
            let quota = quota.borrow();
            context.strategy_budget = quota.budget(req.strategy_id.as_ref());
            context.account_budget = context.account_budget.min(quota.account_budget());
        }
        let tentative = if context.is_none() && input_error.is_none() {
            self.quota
                .as_ref()
                .map(|ledger| {
                    let mut ledger = ledger.borrow_mut();
                    match req.side {
                        nautilus_model::enums::OrderSide::Buy => ledger.try_tentative_buy(
                            req.strategy_id.as_ref(),
                            notional_of(&req.quantity, &req.price)?,
                        ),
                        nautilus_model::enums::OrderSide::Sell => ledger.try_tentative_sell(
                            req.strategy_id.as_ref(),
                            &req.instrument_id.to_string(),
                            req.quantity.as_decimal(),
                        ),
                    }
                    .map_err(|e| BridgeError::Order(e.to_string()))
                })
                .transpose()?
        } else {
            None
        };
        let record = AdmissionRecord {
            journal_epoch: 0,
            seq: 0,
            submitter_id: req.submitter_id.clone(),
            request_id: req.request_id.clone(),
            request_kind: "SubmitOrder".into(),
            request_fingerprint: fingerprint,
            strategy_id: req.strategy_id.to_string(),
            instrument_id: req.instrument_id.to_string(),
            order_side: req.side.to_string(),
            quantity: req.quantity.to_string(),
            price: req.price.to_string(),
            time_in_force: req.time_in_force.to_string(),
            post_only: req.post_only,
            reduce_only: req.reduce_only,
            submit_before_ns: req.submit_before_ns,
            ts_init_ns,
        };
        let rejection = input_error.as_ref().map(ToString::to_string);
        let admission = match &self.worker {
            Some(worker) => worker.submit_business_admission(record, context, rejection),
            None => self.journal.admit_business(record, context, rejection),
        };
        let admission = match admission {
            Ok(record) => record,
            Err(e) => {
                // Only a definite pre-commit failure can release a tentative hold
                if !matches!(e, crate::journal::JournalError::CommitUnknown(_))
                    && let (Some(id), Some(quota)) = (tentative, &self.quota)
                {
                    quota
                        .borrow_mut()
                        .release_tentative(id)
                        .map_err(|e| BridgeError::Order(e.to_string()))?;
                }
                self.halt(&e.to_string());
                return Err(e.into());
            }
        };
        let result = self
            .journal
            .find_request(&req.submitter_id, "SubmitOrder", &req.request_id)
            .map_err(|e| {
                self.halt(&e.to_string());
                BridgeError::from(e)
            })?
            .ok_or_else(|| {
                self.halt("committed result missing");
                BridgeError::Journal("committed result missing".into())
            })?
            .result;
        if result.result == "REJECTED" {
            if let Some(e) = input_error {
                return Err(e);
            }
            return Ok(receipt(&admission, &result, false));
        }
        // The complete durable reservation is authoritative before ACK or dispatch
        if let (Some(id), Some(quota)) = (tentative, &self.quota) {
            quota
                .borrow_mut()
                .commit(id)
                .map_err(|e| BridgeError::Order(e.to_string()))?;
        }
        let accepted = receipt(&admission, &result, false);
        if let Err(e) = self.check_inputs(req) {
            self.commit_outcome(
                admission.seq,
                JournalOutcome::Denied {
                    reason: e.to_string(),
                },
            )?;
            return Err(e);
        }
        let client_order_id = accepted.client_order_id;
        let order = LimitOrder::new_checked(
            self.trader_id,
            req.strategy_id,
            req.instrument_id,
            client_order_id,
            req.side,
            req.quantity,
            req.price,
            req.time_in_force,
            None,
            req.post_only,
            req.reduce_only,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(ts_init_ns),
        )
        .map(OrderAny::Limit);
        let order = match order {
            Ok(order) => order,
            Err(e) => {
                self.commit_outcome(
                    admission.seq,
                    JournalOutcome::Denied {
                        reason: e.to_string(),
                    },
                )?;
                return Err(BridgeError::Order(e.to_string()));
            }
        };
        let registration =
            self.cache
                .borrow_mut()
                .add_order(order.clone(), None, req.client_id, true);
        if let Err(e) = registration {
            self.commit_outcome(
                admission.seq,
                JournalOutcome::Denied {
                    reason: e.to_string(),
                },
            )?;
            return Err(BridgeError::Cache(e.to_string()));
        }
        let topic = format!("events.order.{}", order.strategy_id());
        msgbus::publish_order_event(
            topic.into(),
            &OrderEventAny::Initialized(order.init_event().clone()),
        );
        // This durable boundary must precede every path that may emit an order
        self.commit_outcome(
            admission.seq,
            JournalOutcome::DispatchStarted {
                client_order_id: client_order_id.to_string(),
            },
        )?;
        if let Err(e) = self.check_inputs(req) {
            self.commit_outcome(
                admission.seq,
                JournalOutcome::Denied {
                    reason: e.to_string(),
                },
            )?;
            return Err(e);
        }
        let command = SubmitOrder::new(
            self.trader_id,
            req.client_id,
            req.strategy_id,
            req.instrument_id,
            client_order_id,
            order.init_event().clone(),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(ts_init_ns),
            None,
        );
        msgbus::send_trading_command(
            MessagingSwitchboard::risk_engine_queue_execute(),
            TradingCommand::SubmitOrder(command),
        );
        Ok(accepted)
    }

    fn commit_outcome(&self, origin: u64, outcome: JournalOutcome) -> Result<u64, BridgeError> {
        let result = match &self.worker {
            Some(worker) => worker.submit_outcome(origin, outcome),
            None => self.journal.append_outcome(origin, outcome),
        };
        result.map_err(|e| {
            self.halt(&e.to_string());
            e.into()
        })
    }

    fn halt(&self, reason: &str) {
        *self.storage_fault.borrow_mut() = Some(reason.into());
        if let Some(gate) = &self.gate {
            gate.set(crate::readiness::Readiness::Halted, reason);
        }
    }
    /// The journal epoch of the underlying business journal.
    #[must_use]
    pub fn journal_epoch(&self) -> u64 {
        self.journal.epoch()
    }

    /// Unique instrument IDs referenced by journal admissions.
    ///
    /// Instruments stay enumerable after their orders reach terminal states.
    pub fn journal_instrument_ids(&self) -> Vec<String> {
        let Ok(entries) = self.journal.scan_all() else {
            return Vec::new();
        };
        let mut ids: Vec<String> = entries
            .into_iter()
            .filter_map(|(_, entry)| match entry {
                crate::journal::JournalEntry::Admission(record) => Some(record.instrument_id),
                crate::journal::JournalEntry::Outcome { .. }
                | crate::journal::JournalEntry::Input { .. }
                | crate::journal::JournalEntry::Control(_) => None,
            })
            .filter(|id| !id.starts_with("CANCEL:"))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// The last committed journal sequence.
    ///
    /// # Errors
    ///
    /// Returns an error if the journal cannot be read.
    pub fn journal_last_seq(&self) -> Result<u64, crate::journal::JournalError> {
        self.journal.last_seq()
    }

    /// Durably records a cancel intent for `client_order_id` (M1 scope).
    ///
    /// Per the admission contract this persists the cancellation *request*
    /// only: acceptance means the intent is durable, not that the order is
    /// cancelled (fills may still arrive). Dispatching the cancel to the
    /// execution engine requires the node wiring and lands with the sandbox
    /// loop integration.
    pub fn cancel_order(
        &self,
        submitter_id: &str,
        request_id: &str,
        client_order_id: &str,
    ) -> Result<AdmissionReceipt, BridgeError> {
        validate_request_ids(submitter_id, request_id)?;
        let owner_strategy = self.cancel_owner(submitter_id, client_order_id)?;
        let fingerprint = format!("CANCEL|{client_order_id}");

        if let Some(record) = self
            .journal
            .find_request(submitter_id, "CancelOrder", request_id)
            .map_err(|e| {
                self.halt(&e.to_string());
                BridgeError::from(e)
            })?
        {
            if record.admission.request_fingerprint != fingerprint {
                return Err(BridgeError::IdempotencyConflict {
                    submitter_id: submitter_id.to_string(),
                    request_id: request_id.to_string(),
                    original_fingerprint: record.admission.request_fingerprint,
                    new_fingerprint: fingerprint,
                });
            }
            let receipt_coid = record
                .admission
                .instrument_id
                .strip_prefix("CANCEL:")
                .unwrap_or(&record.admission.instrument_id)
                .to_string();
            return Ok(AdmissionReceipt {
                result: record.result.result,
                reason: record.result.reason,
                journal_epoch: record.admission.journal_epoch,
                seq: record.admission.seq,
                client_order_id: ClientOrderId::from(receipt_coid),
                already_admitted: true,
            });
        }

        let ts_init_ns = self.clock.borrow().timestamp_ns().as_u64();
        let admission_record = AdmissionRecord {
            journal_epoch: 0,
            seq: 0,
            submitter_id: submitter_id.to_string(),
            request_id: request_id.to_string(),
            request_kind: "CancelOrder".to_string(),
            request_fingerprint: fingerprint,
            strategy_id: owner_strategy,
            instrument_id: format!("CANCEL:{client_order_id}"),
            order_side: "CANCEL".to_string(),
            quantity: "-".to_string(),
            price: "-".to_string(),
            time_in_force: "-".to_string(),
            post_only: false,
            reduce_only: false,
            submit_before_ns: None,
            ts_init_ns,
        };
        let admission = match &self.worker {
            Some(worker) => worker.submit_business_admission(admission_record, None, None),
            None => self.journal.admit_business(admission_record, None, None),
        }
        .map_err(|e| {
            self.halt(&e.to_string());
            BridgeError::from(e)
        })?;
        let original = self
            .journal
            .find_request(submitter_id, "CancelOrder", request_id)
            .map_err(|e| {
                self.halt(&e.to_string());
                BridgeError::from(e)
            })?
            .ok_or_else(|| {
                self.halt("cancel result missing");
                BridgeError::Journal("cancel result missing".into())
            })?
            .result;
        if original.result == "ALREADY_TERMINAL" || !original.reason.is_empty() {
            return Ok(receipt(&admission, &original, false));
        }

        // Dispatch the cancel toward the execution engine when the order is
        // live in the cache (no-op if no execution engine is wired).
        let cancel_target = {
            let cache = self.cache.borrow();
            cache
                .order(&ClientOrderId::from(client_order_id))
                .map(|order| {
                    (
                        order.strategy_id(),
                        order.instrument_id(),
                        order.client_order_id(),
                    )
                })
        };
        if let Some((strategy_id, instrument_id, order_id)) = cancel_target {
            crate::exec_wiring::dispatch_cancel(
                self.trader_id,
                strategy_id,
                instrument_id,
                order_id,
                ts_init_ns,
            );
        }

        Ok(AdmissionReceipt {
            result: "CANCEL_INTENT_ACCEPTED".into(),
            reason: String::new(),
            journal_epoch: admission.journal_epoch,
            seq: admission.seq,
            client_order_id: ClientOrderId::from(client_order_id),
            already_admitted: false,
        })
    }

    /// Checks current credential scope for a controlled cancellation, independent of new-risk gates.
    ///
    /// # Errors
    ///
    /// Returns an error if the target lacks a matching committed owner or strategy permission.
    pub fn authorize_cancel(
        &self,
        submitter_id: &str,
        client_order_id: &str,
        strategies: &[String],
    ) -> Result<(), BridgeError> {
        let strategy = self.cancel_owner(submitter_id, client_order_id)?;

        if !strategies.iter().any(|id| id == &strategy) {
            return Err(BridgeError::PermissionDenied);
        }
        Ok(())
    }

    fn cancel_owner(
        &self,
        submitter_id: &str,
        client_order_id: &str,
    ) -> Result<String, BridgeError> {
        let prefix = format!("O-{}-", self.journal.epoch());
        let seq = client_order_id
            .strip_prefix(&prefix)
            .and_then(|seq| seq.parse::<u64>().ok())
            .ok_or(BridgeError::PermissionDenied)?;

        if derive_client_order_id(self.journal.epoch(), seq).as_ref() != client_order_id {
            return Err(BridgeError::PermissionDenied);
        }
        match self.journal.entry(seq)? {
            Some(crate::journal::JournalEntry::Admission(record))
                if record.request_kind == "SubmitOrder" && record.submitter_id == submitter_id =>
            {
                let original = self
                    .journal
                    .find_request(submitter_id, "SubmitOrder", &record.request_id)?
                    .ok_or(BridgeError::PermissionDenied)?;
                if original.result.result != "ADMITTED"
                    || original.result.client_order_id != client_order_id
                {
                    return Err(BridgeError::PermissionDenied);
                }
                Ok(record.strategy_id)
            }
            _ => Err(BridgeError::PermissionDenied),
        }
    }

    fn receipt_for_existing(
        &self,
        record: RequestRecord,
        req: &LimitOrderRequest,
        fingerprint: String,
    ) -> Result<AdmissionReceipt, BridgeError> {
        if record.admission.request_fingerprint != fingerprint {
            return Err(BridgeError::IdempotencyConflict {
                submitter_id: req.submitter_id.clone(),
                request_id: req.request_id.clone(),
                original_fingerprint: record.admission.request_fingerprint,
                new_fingerprint: fingerprint,
            });
        }
        let client_order_id = match &record.outcome {
            Some(
                JournalOutcome::Admitted {
                    client_order_id, ..
                }
                | JournalOutcome::DispatchStarted { client_order_id },
            ) => ClientOrderId::from(client_order_id.as_str()),
            _ => derive_client_order_id(record.admission.journal_epoch, record.admission.seq),
        };
        Ok(AdmissionReceipt {
            result: record.result.result,
            reason: record.result.reason,
            journal_epoch: record.admission.journal_epoch,
            seq: record.admission.seq,
            client_order_id,
            already_admitted: true,
        })
    }
}

/// Derives the order ID deterministically from `(journal_epoch, seq)`.
///
/// The epoch scoping prevents ID reuse after restoring an older backup, as
/// required by the admission contract.
#[must_use]
pub fn derive_client_order_id(journal_epoch: u64, seq: u64) -> ClientOrderId {
    ClientOrderId::from(format!("O-{journal_epoch}-{seq:012}"))
}

/// Canonical digest of the normalized business fields.
///
/// Covers the fields enumerated in the admission contract: strategy, routing,
/// instrument, side, quantity, price, order type, time in force, the trading
/// instruction flags, and `submit_before`. Transient fields (tokens, transport
/// deadlines) are excluded.
fn fingerprint(req: &LimitOrderRequest) -> String {
    let canonical = serde_json::to_vec(&(
        "LIMIT",
        req.strategy_id.as_ref(),
        req.client_id.map(|id| id.to_string()),
        req.instrument_id.to_string(),
        req.side.to_string(),
        req.quantity.as_decimal().normalize().to_string(),
        req.price.as_decimal().normalize().to_string(),
        req.time_in_force.to_string(),
        req.post_only,
        req.reduce_only,
        req.submit_before_ns,
    ))
    .expect("business tuple serialization");
    blake3::hash(&canonical).to_hex().to_string()
}

fn notional_of(quantity: &Quantity, price: &Price) -> Result<rust_decimal::Decimal, BridgeError> {
    quantity
        .as_decimal()
        .checked_mul(price.as_decimal())
        .ok_or_else(|| BridgeError::Order("notional overflow".into()))
}

fn validate_request_ids(submitter: &str, request: &str) -> Result<(), BridgeError> {
    if [submitter, request]
        .iter()
        .any(|id| id.is_empty() || id.len() > 64 || id.chars().any(char::is_control))
    {
        return Err(BridgeError::Order(
            "identity or request ID exceeds permitted format".into(),
        ));
    }
    Ok(())
}

fn receipt(
    admission: &AdmissionRecord,
    result: &crate::ledger::RequestResult,
    retry: bool,
) -> AdmissionReceipt {
    AdmissionReceipt {
        journal_epoch: admission.journal_epoch,
        seq: admission.seq,
        client_order_id: if result.client_order_id.is_empty() {
            derive_client_order_id(admission.journal_epoch, admission.seq)
        } else {
            ClientOrderId::from(result.client_order_id.as_str())
        },
        already_admitted: retry,
        result: result.result.clone(),
        reason: result.reason.clone(),
    }
}
