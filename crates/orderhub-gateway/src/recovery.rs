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

//! Cache reconstruction from verified committed facts; historical commands are never broadcast.

use std::cell::RefCell;
use std::rc::Rc;

use nautilus_common::cache::Cache;
use nautilus_common::factories::event::OrderEventFactory;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::enums::{AccountType, OmsType, OrderSide, TimeInForce};
use nautilus_model::events::OrderEventAny;
use nautilus_model::identifiers::{
    AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId,
};
use nautilus_model::orders::{LimitOrder, Order, OrderAny};
use nautilus_model::position::Position;
use nautilus_model::types::{Price, Quantity};

use crate::inputs::InputRuntime;
use crate::journal::{BusinessJournal, JournalEntry, JournalError, JournalOutcome, RecoveryReport};
use crate::ledger::{BusinessOrder, decimal, invalid};

/// Rebuilds cache inputs, orders, venue mappings, and native execution positions.
///
/// # Errors
/// Returns verification errors or missing native material. No historical command or event
/// is published to the bus, and restored input timestamps do not establish live freshness.
pub fn restore_native(
    journal: &BusinessJournal,
    inputs: &mut InputRuntime,
    cache: &Rc<RefCell<Cache>>,
) -> Result<RecoveryReport, JournalError> {
    let report = journal.verify_recovery()?;
    if report.business_orders > 8_192 {
        return Err(invalid("native recovery cache limit exceeded"));
    }
    let mut admission_sequences = Vec::with_capacity(report.business_orders);
    let mut after = 0;
    loop {
        let page = journal.scan_page(after, 256)?;
        if page.is_empty() {
            break;
        }
        for (seq, entry) in page {
            after = seq;
            match entry {
                JournalEntry::Input {
                    event,
                    received_at_ns,
                } => {
                    inputs
                        .restore_committed_fact(*event, received_at_ns)
                        .map_err(|e| invalid(&format!("historical input restore failed: {e}")))?;
                }
                JournalEntry::Admission(record) if record.request_kind == "SubmitOrder" => {
                    if let Some(business) = journal.order(seq)? {
                        cache
                            .borrow_mut()
                            .add_order(rebuild_order(&business)?, None, None, false)
                            .map_err(|e| invalid(&e.to_string()))?;
                        admission_sequences.push(seq);
                    }
                }
                JournalEntry::Outcome {
                    origin_seq,
                    outcome: JournalOutcome::Execution(fact),
                } => {
                    if fact.kind == "FEE" {
                        continue;
                    }
                    let native = fact.native_event.as_ref().ok_or_else(|| invalid(
                        "legacy execution lacks native recovery material; explicit offline enrichment required",
                    ))?;
                    let event: OrderEventAny = serde_json::from_str(native)?;
                    let native_kind = match &event {
                        OrderEventAny::Submitted(_) => "SUBMITTED",
                        OrderEventAny::Accepted(_) => "ACCEPTED",
                        OrderEventAny::Filled(_) => "FILL",
                        OrderEventAny::Canceled(_) => "CANCELED",
                        OrderEventAny::Rejected(_) | OrderEventAny::Denied(_) => "REJECTED",
                        OrderEventAny::Expired(_) => "EXPIRED",
                        OrderEventAny::CancelRejected(_) => "CANCEL_REJECTED",
                        _ => return Err(invalid("unsupported native recovery event")),
                    };
                    if native_kind != fact.kind {
                        return Err(invalid("native event kind differs from economic fact"));
                    }
                    if event.client_order_id().as_str() != fact.client_order_id
                        || event.strategy_id().as_str() != fact.strategy_id
                        || event.instrument_id().to_string() != fact.instrument_id
                        || event
                            .account_id()
                            .is_some_and(|account| account.as_str() != fact.account_id)
                    {
                        return Err(invalid("native recovery attribution mismatch"));
                    }
                    if let OrderEventAny::Filled(fill) = &event
                        && (fill.trade_id.to_string() != fact.trade_id.clone().unwrap_or_default()
                            || fill.last_qty.as_decimal() != decimal(&fact.quantity)?
                            || fill.last_px.as_decimal() != decimal(&fact.price)?
                            || fill.currency.to_string() != fact.currency
                            || fill
                                .commission
                                .is_some_and(|fee| fee.currency.to_string() != fact.currency)
                            || fill
                                .commission
                                .map_or(rust_decimal::Decimal::ZERO, |fee| fee.as_decimal())
                                != decimal(&fact.fee)?)
                    {
                        return Err(invalid("native recovery fill differs from economic fact"));
                    }
                    let current = cache
                        .borrow()
                        .order(&event.client_order_id())
                        .map(|order| order.clone())
                        .ok_or_else(|| invalid("recovered execution has no native order"))?;
                    if let OrderEventAny::Filled(fill) = &event
                        && (fill.order_side != current.order_side()
                            || fill.order_type != current.order_type())
                    {
                        return Err(invalid("native recovery fill direction differs from order"));
                    }
                    // A full fill is authoritative over a subsequently received cancel
                    if current.is_closed() && matches!(&event, OrderEventAny::Canceled(_)) {
                        continue;
                    }
                    cache
                        .borrow_mut()
                        .update_order(&event)
                        .map_err(|e| invalid(&e.to_string()))?;
                    if let OrderEventAny::Filled(mut fill) = event {
                        let id = PositionId::new_checked(format!(
                            "{}-{}",
                            fill.instrument_id, fill.strategy_id
                        ))
                        .map_err(|e| invalid(&e.to_string()))?;
                        fill.position_id = Some(id);
                        let existing = cache.borrow().position(&id).is_some();
                        if existing {
                            cache
                                .borrow_mut()
                                .update_position_from_fill(id, &fill)
                                .map_err(|e| invalid(&e.to_string()))?;
                        } else {
                            let instrument = cache
                                .borrow()
                                .instrument(&fill.instrument_id)
                                .cloned()
                                .ok_or_else(|| invalid("recovered fill has no instrument"))?;
                            let position = Position::new_checked(&instrument, fill)
                                .map_err(|e| invalid(&e.to_string()))?;
                            cache
                                .borrow_mut()
                                .add_position(&position, OmsType::Netting)
                                .map_err(|e| invalid(&e.to_string()))?;
                        }
                    }
                    let _ = origin_seq;
                }
                JournalEntry::Outcome {
                    origin_seq,
                    outcome: JournalOutcome::Denied { reason },
                } => {
                    if let Some(order) = journal.order(origin_seq)? {
                        let id = ClientOrderId::new_checked(&order.client_order_id)
                            .map_err(|e| invalid(&e.to_string()))?;
                        let current = { cache.borrow().order(&id).map(|order| order.clone()) };
                        if let Some(current) = current {
                            let factory = factory(&order)?;
                            let event = factory.generate_order_denied(
                                &current,
                                &reason,
                                UnixNanos::from(order.admission.ts_init_ns),
                            );
                            cache
                                .borrow_mut()
                                .update_order(&event)
                                .map_err(|e| invalid(&e.to_string()))?;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for sequence in admission_sequences {
        if let Some(business) = journal.order(sequence)? {
            let record = &business.admission;
            let id = ClientOrderId::new_checked(&business.client_order_id)
                .map_err(|e| invalid(&e.to_string()))?;
            let native = cache
                .borrow()
                .order(&id)
                .map(|order| order.clone())
                .ok_or_else(|| invalid("native order missing after recovery"))?;
            if native.filled_qty().as_decimal() != decimal(&business.filled_quantity)? {
                return Err(invalid(
                    "native recovered fill quantity differs from ledger",
                ));
            }
            if business.status == "CANCELED" && !native.is_closed() {
                let event = factory(&business)?.generate_order_canceled(
                    &native,
                    native.venue_order_id(),
                    UnixNanos::from(record.ts_init_ns),
                    UnixNanos::from(record.ts_init_ns),
                );
                cache
                    .borrow_mut()
                    .update_order(&event)
                    .map_err(|e| invalid(&e.to_string()))?;
            }
        }
    }
    Ok(report)
}

fn factory(order: &BusinessOrder) -> Result<OrderEventFactory, JournalError> {
    Ok(OrderEventFactory::new(
        TraderId::from("ORDER-HUB"),
        AccountId::new_checked(&order.account_id).map_err(|e| invalid(&e.to_string()))?,
        AccountType::Cash,
        None,
    ))
}

fn rebuild_order(order: &BusinessOrder) -> Result<OrderAny, JournalError> {
    let record = &order.admission;
    let digest = blake3::hash(order.client_order_id.as_bytes());
    let mut init_bytes = [0; 16];
    init_bytes.copy_from_slice(&digest.as_bytes()[..16]);
    LimitOrder::new_checked(
        TraderId::from("ORDER-HUB"),
        StrategyId::new_checked(&record.strategy_id).map_err(|e| invalid(&e.to_string()))?,
        InstrumentId::from_as_ref(&record.instrument_id).map_err(|e| invalid(&e.to_string()))?,
        ClientOrderId::new_checked(&order.client_order_id).map_err(|e| invalid(&e.to_string()))?,
        if record.order_side == "BUY" {
            OrderSide::Buy
        } else if record.order_side == "SELL" {
            OrderSide::Sell
        } else {
            return Err(invalid("invalid recovered side"));
        },
        record
            .quantity
            .parse::<Quantity>()
            .map_err(|e| invalid(&e))?,
        record.price.parse::<Price>().map_err(|e| invalid(&e))?,
        TimeInForce::Day,
        None,
        record.post_only,
        record.reduce_only,
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
        UUID4::from_bytes(init_bytes),
        UnixNanos::from(record.ts_init_ns),
    )
    .map(OrderAny::Limit)
    .map_err(|e| invalid(&e.to_string()))
}
