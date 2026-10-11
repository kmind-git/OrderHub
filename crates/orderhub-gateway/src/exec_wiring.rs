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

//! Execution-path plumbing shared by node assemblies and tests.
//!
//! The execution client itself is the local FIX exchange client
//! ([`crate::local_client`]); this module holds the pieces around it that the
//! core thread and the journal need regardless of venue wiring:
//!
//! - the deferred execution-event channel and its pump (re-entrancy safe
//!   forwarding into the execution engine);
//! - the durable outcome recorder subscribing to `events.order.*`;
//! - cancel-command dispatch toward the execution engine;
//! - the journal-sequence recovery from derived client order IDs.
//!
//! Must be used on the core thread (same thread as the risk engine and
//! bridge), because the message-bus state is single-threaded.

use nautilus_common::messages::execution::CancelOrder;
use nautilus_common::msgbus::MessagingSwitchboard;
use nautilus_common::msgbus::{self, TypedHandler};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::events::OrderEventAny;
use nautilus_model::identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId};

use crate::worker::PersistenceWorker;

/// Deferred execution events channel (order events and account states).
pub type ExecEventRx = tokio::sync::mpsc::Receiver<nautilus_common::messages::ExecutionEvent>;

/// Forwards queued execution events into the execution engine on the current
/// (core) thread. Mirrors the live runner's event loop forwarding.
pub fn pump_exec_events(rx: &mut Option<ExecEventRx>) {
    let Some(rx_ref) = rx else { return };
    while let Ok(event) = rx_ref.try_recv() {
        use nautilus_common::messages::ExecutionEvent;
        match event {
            ExecutionEvent::Order(order_event) => {
                if persist_before_execution(&order_event) {
                    msgbus::send_order_event(
                        MessagingSwitchboard::exec_engine_process(),
                        order_event,
                    );
                }
            }
            ExecutionEvent::Account(state) => {
                msgbus::send_account_state(MessagingSwitchboard::exec_engine_process(), &state);
            }
            _ => {}
        }
    }
}

/// Subscribes a recorder that durably journals denial, fill, and cancellation
/// events from the `events.order.*` topics.
///
/// The origin admission sequence is recovered deterministically from the
/// client order ID (`O-{epoch}-{seq}`), so no extra coupling between the
/// bridge and the recorder is needed. Must be called on the core thread.
pub fn attach_outcome_recorder(worker: PersistenceWorker) {
    attach_outcome_recorder_with_gate(worker, crate::readiness::ReadinessGate::new());
}

/// Attaches a recorder whose failures close the shared admission gate.
pub fn attach_outcome_recorder_with_gate(
    worker: PersistenceWorker,
    gate: std::sync::Arc<crate::readiness::ReadinessGate>,
) {
    OUTCOME_RECORDER.with(|slot| {
        *slot.borrow_mut() = Some((worker.clone(), std::sync::Arc::clone(&gate)));
    });
    let handler = TypedHandler::from(move |event: &OrderEventAny| {
        record_order_fact(&worker, &gate, event);
    });
    msgbus::subscribe_order_events("events.order.*".into(), handler, Some(10));
}
/// Parses the journal sequence out of a derived client order ID.
#[must_use]
pub fn seq_from_client_order_id(client_order_id: &ClientOrderId) -> Option<u64> {
    let value = client_order_id.to_string();
    let mut parts = value.splitn(3, '-');
    let epoch = parts.next()?;
    if epoch != "O" {
        return None;
    }
    let epoch_number = parts.next()?.parse::<u64>().ok()?;
    let seq = parts.next()?;
    let seq = seq.parse::<u64>().ok()?;
    if epoch_number == 0 || seq == 0 || value != format!("O-{epoch_number}-{seq:012}") {
        return None;
    }
    Some(seq)
}

/// Builds and dispatches a `CancelOrder` command toward the execution engine.
pub fn dispatch_cancel(
    trader_id: TraderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    ts_init_ns: u64,
) {
    let command = CancelOrder::new(
        trader_id,
        None, // venue_order_id
        strategy_id,
        instrument_id,
        client_order_id,
        None, // reason
        UUID4::new(),
        UnixNanos::from(ts_init_ns),
        None, // correlation_id
        None, // params?
    );
    msgbus::send_trading_command(
        MessagingSwitchboard::exec_engine_queue_execute(),
        nautilus_common::messages::execution::TradingCommand::CancelOrder(command),
    );
}

type DispatchGuard = Box<dyn Fn(&ClientOrderId) -> Result<(), String>>;

thread_local! {
    static OUTCOME_RECORDER: std::cell::RefCell<Option<(PersistenceWorker, std::sync::Arc<crate::readiness::ReadinessGate>)>> = const { std::cell::RefCell::new(None) };
    static DISPATCH_GUARD: std::cell::RefCell<Option<DispatchGuard>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn install_dispatch_guard(
    guard: impl Fn(&ClientOrderId) -> Result<(), String> + 'static,
) {
    DISPATCH_GUARD.with(|slot| *slot.borrow_mut() = Some(Box::new(guard)));
}

pub(crate) fn check_dispatch_allowed(id: &ClientOrderId) -> Result<(), String> {
    let check =
        DISPATCH_GUARD.with(|slot| slot.borrow().as_ref().map_or(Ok(()), |guard| guard(id)));
    if let Err(reason) = &check {
        OUTCOME_RECORDER.with(|slot| {
            if let Some((worker, gate)) = slot.borrow().as_ref()
                && let Some(seq) = seq_from_client_order_id(id)
                && let Err(e) = worker.submit_outcome(
                    seq,
                    crate::journal::JournalOutcome::Denied {
                        reason: reason.clone(),
                    },
                )
            {
                gate.set(crate::readiness::Readiness::Halted, &e.to_string());
            }
        });
    }
    check
}

fn persist_before_execution(event: &OrderEventAny) -> bool {
    OUTCOME_RECORDER.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_none_or(|(worker, gate)| record_order_fact(worker, gate, event))
    })
}

fn record_order_fact(
    worker: &PersistenceWorker,
    gate: &std::sync::Arc<crate::readiness::ReadinessGate>,
    event: &OrderEventAny,
) -> bool {
    if !matches!(
        event,
        OrderEventAny::Filled(_)
            | OrderEventAny::Submitted(_)
            | OrderEventAny::Accepted(_)
            | OrderEventAny::Canceled(_)
            | OrderEventAny::Rejected(_)
            | OrderEventAny::Expired(_)
            | OrderEventAny::CancelRejected(_)
            | OrderEventAny::Denied(_)
            | OrderEventAny::FillVoided(_)
    ) {
        return true;
    }
    let Some(origin) = seq_from_client_order_id(&event.client_order_id()) else {
        return true;
    };
    let result = (|| -> Result<(), crate::journal::JournalError> {
        let journal = worker.journal();
        let Some(order) = journal.order(origin)? else {
            return Err(crate::journal::JournalError::Encoding(
                "execution has no durable business order".into(),
            ));
        };
        if event.client_order_id().as_str() != order.client_order_id
            || event.strategy_id().as_str() != order.admission.strategy_id
            || event.instrument_id().to_string() != order.admission.instrument_id
        {
            return Err(crate::journal::JournalError::Encoding(
                "execution owner or epoch mismatch".into(),
            ));
        }
        let (kind, event_id, trade_id, quantity, price, fee, reason) = match event {
            OrderEventAny::Submitted(e) => (
                "SUBMITTED",
                e.event_id.to_string(),
                None,
                "0".into(),
                "0".into(),
                "0".into(),
                String::new(),
            ),
            OrderEventAny::Filled(fill) => {
                if fill.currency.to_string() != "USD"
                    || fill
                        .commission
                        .is_some_and(|fee| fee.currency.to_string() != "USD")
                {
                    return Err(crate::journal::JournalError::Encoding(
                        "execution fee currency mismatch".into(),
                    ));
                }
                (
                    "FILL",
                    fill.event_id.to_string(),
                    Some(fill.trade_id.to_string()),
                    fill.last_qty.as_decimal().normalize().to_string(),
                    fill.last_px.as_decimal().normalize().to_string(),
                    fill.commission.map_or_else(
                        || "0".into(),
                        |fee| fee.as_decimal().normalize().to_string(),
                    ),
                    String::new(),
                )
            }
            OrderEventAny::Accepted(e) => (
                "ACCEPTED",
                e.event_id.to_string(),
                None,
                "0".into(),
                "0".into(),
                "0".into(),
                String::new(),
            ),
            OrderEventAny::Canceled(e) => (
                "CANCELED",
                e.event_id.to_string(),
                None,
                "0".into(),
                "0".into(),
                "0".into(),
                String::new(),
            ),
            OrderEventAny::Rejected(e) => (
                "REJECTED",
                e.event_id.to_string(),
                None,
                "0".into(),
                "0".into(),
                "0".into(),
                e.reason.to_string(),
            ),
            OrderEventAny::Expired(e) => (
                "EXPIRED",
                e.event_id.to_string(),
                None,
                "0".into(),
                "0".into(),
                "0".into(),
                String::new(),
            ),
            OrderEventAny::CancelRejected(e) => (
                "CANCEL_REJECTED",
                e.event_id.to_string(),
                None,
                "0".into(),
                "0".into(),
                "0".into(),
                e.reason.to_string(),
            ),
            OrderEventAny::Denied(e) => (
                "REJECTED",
                e.event_id.to_string(),
                None,
                "0".into(),
                "0".into(),
                "0".into(),
                e.reason.to_string(),
            ),
            OrderEventAny::FillVoided(_) => {
                return Err(crate::journal::JournalError::Encoding(
                    "fill correction requires reconciliation".into(),
                ));
            }
            _ => return Ok(()),
        };
        let account_id = event
            .account_id()
            .map_or_else(|| order.account_id.clone(), |id| id.to_string());
        worker.submit_outcome(
            origin,
            crate::journal::JournalOutcome::Execution(Box::new(crate::ledger::ExecutionRecord {
                native_event: Some(serde_json::to_string(event)?),
                event_id,
                trade_id,
                client_order_id: event.client_order_id().to_string(),
                account_id,
                strategy_id: event.strategy_id().to_string(),
                instrument_id: event.instrument_id().to_string(),
                kind: kind.into(),
                quantity,
                price,
                fee,
                currency: "USD".into(),
                ts_event_ns: event.ts_event().as_u64(),
                ts_received_ns: nautilus_core::time::get_atomic_clock_realtime()
                    .get_time_ns()
                    .as_u64(),
                reason,
            })),
        )?;
        if let Some(committed) = journal.order(origin)?
            && crate::ledger::decimal(&committed.fees)?
                > crate::ledger::decimal(&committed.fee_buffer)?
        {
            gate.set(
                crate::readiness::Readiness::Halted,
                "committed fees exceed configured buffer; reconciliation required",
            );
        }
        Ok(())
    })();
    if let Err(e) = result {
        log::error!("OrderHub execution persistence failed: {e}");
        gate.set(crate::readiness::Readiness::Halted, &e.to_string());
        return false;
    }

    true
}
