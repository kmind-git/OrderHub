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

//! Exact business projections updated inside the journal transaction.

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};

use crate::journal::{AdmissionRecord, JournalError};

/// Committed business and execution-connection health at a journal cursor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlRecord {
    pub readiness: String,
    pub writable: bool,
    pub connected: Option<bool>,
    pub reason: String,
}

/// Admission limits and authoritative input facts sampled on the core thread.
#[derive(Debug, Clone)]
pub struct RiskContext {
    pub account_id: String,
    pub strategy_budget: Decimal,
    pub account_budget: Decimal,
    pub owned_quantity: Decimal,
    pub owned_cost: Decimal,
    pub account_quantity: Decimal,
    pub strategy_exposure_limit: Decimal,
    pub account_exposure_limit: Decimal,
    pub reference_price: Decimal,
    pub fee_buffer: Decimal,
    pub maximum_quantity: Decimal,
    pub maximum_notional: Decimal,
    pub risk_config_version: String,
}

/// Immutable result of the original RPC, independent of later order state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestResult {
    pub result: String,
    pub reason: String,
    pub client_order_id: String,
    pub version: u64,
}

/// Complete committed order and reservation projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessOrder {
    pub admission: AdmissionRecord,
    pub client_order_id: String,
    pub account_id: String,
    pub risk_config_version: String,
    pub status: String,
    pub version: u64,
    pub filled_quantity: String,
    pub filled_notional: String,
    pub fees: String,
    pub remaining_quantity: String,
    pub reserved_cash: String,
    pub reserved_quantity: String,
    pub fee_buffer: String,
    pub dispatch_started: bool,
    pub cancel_pending: bool,
}

/// Execution facts with stable identity, attribution, and exact economics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionRecord {
    /// Original native fact for cache reconstruction; older records may lack it.
    #[serde(default)]
    pub native_event: Option<String>,
    pub event_id: String,
    pub trade_id: Option<String>,
    pub client_order_id: String,
    pub account_id: String,
    pub strategy_id: String,
    pub instrument_id: String,
    pub kind: String,
    pub quantity: String,
    pub price: String,
    pub fee: String,
    pub currency: String,
    pub ts_event_ns: u64,
    pub ts_received_ns: u64,
    pub reason: String,
}

/// Durable aggregate balances for the first single-instrument USD profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RiskBalance {
    pub reserved_cash: String,
    pub spent_cash: String,
    pub reserved_quantity: String,
    pub quantity: String,
    pub cost: String,
    pub paid_fees: String,
    pub active_orders: u64,
}

impl RiskBalance {
    pub(crate) fn initial(quantity: Decimal, cost: Decimal) -> Result<Self, JournalError> {
        if quantity < Decimal::ZERO
            || cost < Decimal::ZERO
            || (quantity.is_zero() && !cost.is_zero())
        {
            return Err(invalid("invalid confirmed position cost"));
        }
        Ok(Self {
            reserved_cash: "0".into(),
            spent_cash: "0".into(),
            reserved_quantity: "0".into(),
            quantity: quantity.to_string(),
            cost: cost.to_string(),
            paid_fees: "0".into(),
            active_orders: 0,
        })
    }

    pub(crate) fn update(
        &mut self,
        before: Option<&BusinessOrder>,
        after: &BusinessOrder,
    ) -> Result<(), JournalError> {
        let was_active = before.is_some_and(|order| !order.is_terminal());
        let is_active = !after.is_terminal();
        if was_active != is_active {
            self.active_orders = if is_active {
                self.active_orders.checked_add(1)
            } else {
                self.active_orders.checked_sub(1)
            }
            .ok_or_else(|| invalid("active order count overflow"))?;
        }
        let old = |get: fn(&BusinessOrder) -> &String| {
            before.map_or(Ok(Decimal::ZERO), |order| decimal(get(order)))
        };
        self.reserved_cash = add(
            sub(decimal(&self.reserved_cash)?, old(|o| &o.reserved_cash)?)?,
            decimal(&after.reserved_cash)?,
        )?
        .to_string();
        self.reserved_quantity = add(
            sub(
                decimal(&self.reserved_quantity)?,
                old(|o| &o.reserved_quantity)?,
            )?,
            decimal(&after.reserved_quantity)?,
        )?
        .to_string();
        let quantity_delta = sub(
            decimal(&after.filled_quantity)?,
            old(|o| &o.filled_quantity)?,
        )?;
        let notional_delta = sub(
            decimal(&after.filled_notional)?,
            old(|o| &o.filled_notional)?,
        )?;
        let fee_delta = sub(decimal(&after.fees)?, old(|o| &o.fees)?)?;
        self.paid_fees = add(decimal(&self.paid_fees)?, fee_delta)?.to_string();
        if after.admission.order_side == "BUY" {
            self.quantity = add(decimal(&self.quantity)?, quantity_delta)?.to_string();
            self.cost = add(decimal(&self.cost)?, notional_delta)?.to_string();
            self.spent_cash =
                add(decimal(&self.spent_cash)?, add(notional_delta, fee_delta)?)?.to_string();
        } else {
            let quantity = decimal(&self.quantity)?;
            if quantity_delta > quantity {
                return Err(invalid("execution exceeds owned position"));
            }
            if quantity > Decimal::ZERO {
                let basis = if quantity_delta == quantity {
                    decimal(&self.cost)?
                } else {
                    mul(decimal(&self.cost)?, quantity_delta)?
                        .checked_div(quantity)
                        .ok_or_else(|| invalid("cost basis division overflow"))?
                        .round_dp_with_strategy(2, RoundingStrategy::ToZero)
                };
                self.cost = sub(decimal(&self.cost)?, basis)?.to_string();
            }
            self.quantity = sub(quantity, quantity_delta)?.to_string();
            self.spent_cash = add(decimal(&self.spent_cash)?, fee_delta)?.to_string();
        }
        Ok(())
    }
}

impl BusinessOrder {
    pub(crate) fn new(
        record: AdmissionRecord,
        context: &RiskContext,
    ) -> Result<Self, JournalError> {
        let quantity = decimal(&record.quantity)?;
        let price = decimal(&record.price)?;
        if quantity <= Decimal::ZERO
            || price <= Decimal::ZERO
            || !matches!(record.order_side.as_str(), "BUY" | "SELL")
            || context.fee_buffer < Decimal::ZERO
            || context.reference_price <= Decimal::ZERO
            || [
                context.strategy_budget,
                context.account_budget,
                context.strategy_exposure_limit,
                context.account_exposure_limit,
                context.maximum_quantity,
                context.maximum_notional,
            ]
            .iter()
            .any(|value| *value < Decimal::ZERO)
        {
            return Err(invalid("nonpositive order economics"));
        }
        let buy = record.order_side == "BUY";
        let cash = if buy {
            add(mul(quantity, price)?, context.fee_buffer)?
        } else {
            context.fee_buffer
        };
        Ok(Self {
            client_order_id: format!("O-{}-{:012}", record.journal_epoch, record.seq),
            account_id: context.account_id.clone(),
            risk_config_version: context.risk_config_version.clone(),
            status: "PENDING_DISPATCH".into(),
            version: record.seq,
            filled_quantity: "0".into(),
            filled_notional: "0".into(),
            fees: "0".into(),
            remaining_quantity: quantity.to_string(),
            reserved_cash: cash.to_string(),
            reserved_quantity: if buy {
                "0".into()
            } else {
                quantity.to_string()
            },
            fee_buffer: context.fee_buffer.to_string(),
            dispatch_started: false,
            cancel_pending: false,
            admission: record,
        })
    }

    /// Returns whether an execution terminal state has been committed.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status.as_str(),
            "FILLED" | "CANCELED" | "REJECTED" | "EXPIRED"
        )
    }

    pub(crate) fn apply(
        &mut self,
        event: &ExecutionRecord,
        version: u64,
    ) -> Result<(), JournalError> {
        if event.client_order_id != self.client_order_id
            || event.account_id != self.account_id
            || event.strategy_id != self.admission.strategy_id
            || event.instrument_id != self.admission.instrument_id
            || event.currency != "USD"
        {
            return Err(invalid("execution attribution or currency mismatch"));
        }
        match event.kind.as_str() {
            "FILL" => {
                let quantity = decimal(&event.quantity)?;
                let price = decimal(&event.price)?;
                let fee = decimal(&event.fee)?;
                let filled = add(decimal(&self.filled_quantity)?, quantity)?;
                if event.trade_id.as_ref().is_none_or(|id| id.is_empty())
                    || quantity <= Decimal::ZERO
                    || price <= Decimal::ZERO
                    || fee < Decimal::ZERO
                    || filled > decimal(&self.admission.quantity)?
                    || (self.admission.order_side == "BUY"
                        && price > decimal(&self.admission.price)?)
                    || (self.admission.order_side == "SELL"
                        && price < decimal(&self.admission.price)?)
                {
                    return Err(invalid("invalid fill identity, quantity, price, or fee"));
                }
                self.filled_quantity = filled.to_string();
                self.filled_notional =
                    add(decimal(&self.filled_notional)?, mul(quantity, price)?)?.to_string();
                self.fees = add(decimal(&self.fees)?, fee)?.to_string();
                self.remaining_quantity =
                    sub(decimal(&self.admission.quantity)?, filled)?.to_string();
                if decimal(&self.remaining_quantity)? == Decimal::ZERO {
                    self.status = "FILLED".into();
                } else if !self.is_terminal() {
                    self.status = "PARTIALLY_FILLED".into();
                }
            }
            "FEE" => {
                let fee = decimal(&event.fee)?;
                if fee < Decimal::ZERO {
                    return Err(invalid("negative late fee"));
                }
                self.fees = add(decimal(&self.fees)?, fee)?.to_string();
            }
            "CANCELED" | "REJECTED" | "EXPIRED" => {
                if event.kind == "REJECTED" && decimal(&self.filled_quantity)? == Decimal::ZERO {
                    self.fee_buffer = "0".into();
                }
                if self.status != "FILLED" {
                    self.status.clone_from(&event.kind);
                }
                self.cancel_pending = false;
            }
            "SUBMITTED" => {
                if !self.is_terminal() && decimal(&self.filled_quantity)? == Decimal::ZERO {
                    self.status = "SUBMITTED".into();
                }
            }
            "ACCEPTED" => {
                if !self.is_terminal() && decimal(&self.filled_quantity)? == Decimal::ZERO {
                    self.status = "ACCEPTED".into();
                }
            }
            "CANCEL_REJECTED" => {
                self.cancel_pending = false;
            }
            _ => {
                return Err(invalid(
                    "unsupported execution fact; reconciliation required",
                ));
            }
        }
        self.version = version;
        self.recalculate()
    }

    pub(crate) fn recalculate(&mut self) -> Result<(), JournalError> {
        let buffer = sub(decimal(&self.fee_buffer)?, decimal(&self.fees)?)?.max(Decimal::ZERO);
        let remaining = if self.is_terminal() {
            Decimal::ZERO
        } else {
            decimal(&self.remaining_quantity)?
        };
        self.reserved_cash = if self.admission.order_side == "BUY" {
            add(mul(remaining, decimal(&self.admission.price)?)?, buffer)?.to_string()
        } else {
            buffer.to_string()
        };
        self.reserved_quantity = if self.admission.order_side == "SELL" {
            remaining.to_string()
        } else {
            "0".into()
        };
        Ok(())
    }
}

pub(crate) fn decimal(value: &str) -> Result<Decimal, JournalError> {
    value
        .parse()
        .map_err(|e| invalid(&format!("invalid decimal: {e}")))
}

pub(crate) fn add(left: Decimal, right: Decimal) -> Result<Decimal, JournalError> {
    left.checked_add(right)
        .ok_or_else(|| invalid("decimal addition overflow"))
}

pub(crate) fn sub(left: Decimal, right: Decimal) -> Result<Decimal, JournalError> {
    left.checked_sub(right)
        .ok_or_else(|| invalid("decimal subtraction overflow"))
}

pub(crate) fn mul(left: Decimal, right: Decimal) -> Result<Decimal, JournalError> {
    left.checked_mul(right)
        .ok_or_else(|| invalid("decimal multiplication overflow"))
}

pub(crate) fn invalid(reason: &str) -> JournalError {
    JournalError::Encoding(reason.into())
}
