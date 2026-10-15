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

//! Nautilus `ExecutionClient` over the local simulated exchange (FIX 4.2).
//!
//! The client sends orders via a [`FixWriter`] on the core thread; a
//! background reader thread delivers decoded events to a [`LocalPump`],
//! which converts them into `OrderEventAny` (via the common
//! `OrderEventFactory` and the cache) and forwards them through the
//! deferred execution-event channel, mirroring the sandbox client's
//! threading model.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use nautilus_common::cache::Cache;
use nautilus_common::clients::ExecutionClient;
use nautilus_common::factories::event::OrderEventFactory;
use nautilus_common::live::runner::try_get_exec_event_sender;
use nautilus_common::messages::ExecutionEvent;
use nautilus_common::messages::execution::{
    CancelOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
};
use nautilus_core::UnixNanos;
use nautilus_model::enums::AccountType;
use nautilus_model::enums::{LiquiditySide, OmsType, OrderSide};
use nautilus_model::identifiers::{
    AccountId, ClientId, ClientOrderId, TraderId, Venue, VenueOrderId,
};
use nautilus_model::instruments::Instrument;
use nautilus_model::orders::Order;
use nautilus_model::types::{Price, Quantity};

use rust_decimal::prelude::FromStr as _;

use crate::fix_config::FixSessionConfig;
use crate::fix_log::FixLog;
use crate::local_exchange::{ExecEvent, FixInitiator, FixWriter, Side};

/// Pump converting FIX reader events into deferred engine events.
pub struct LocalPump {
    reports: std::sync::mpsc::Receiver<ExecEvent>,
    factory: OrderEventFactory,
    cache: Rc<RefCell<Cache>>,
    gate: Rc<RefCell<Option<std::sync::Arc<crate::readiness::ReadinessGate>>>>,
    connected: Rc<Cell<bool>>,
    engine_owner: Option<Rc<RefCell<nautilus_execution::engine::ExecutionEngine>>>,
}

fn now_ns() -> UnixNanos {
    UnixNanos::from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or_default(),
    )
}

impl LocalPump {
    /// Returns live execution transport health, separate from business readiness.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.connected.get()
    }
    /// Installs the shared business failure gate.
    #[must_use]
    pub fn with_gate(self, gate: std::sync::Arc<crate::readiness::ReadinessGate>) -> Self {
        *self.gate.borrow_mut() = Some(gate);
        self
    }
    /// Drains all pending FIX events and forwards them into the deferred
    /// execution-event channel for the engine.
    pub fn pump(&mut self) {
        if !self.connected.get() {
            self.halt("execution writer unavailable; dispatch outcome unknown");
        }
        for _ in 0..1_024 {
            let event = match self.reports.try_recv() {
                Ok(event) => event,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.connected.set(false);
                    self.halt("FIX reader disconnected");
                    break;
                }
            };
            let Some(order_event) = self.translate(&event) else {
                if !matches!(event, ExecEvent::Session(_)) {
                    self.halt("invalid or unattributed execution report");
                }
                continue;
            };
            if let Some(sender) = try_get_exec_event_sender() {
                if sender.send(ExecutionEvent::Order(order_event)).is_err() {
                    self.halt("execution event queue full or stopped");
                }
            } else {
                self.halt("execution event sender unavailable");
            }
        }
    }

    fn halt(&self, reason: &str) {
        if let Some(gate) = self.gate.borrow().as_ref() {
            gate.set(crate::readiness::Readiness::Halted, reason);
        }
    }

    fn instrument_quote_currency(
        &self,
        order: &nautilus_model::orders::OrderAny,
    ) -> nautilus_model::types::Currency {
        let cache = self.cache.borrow();
        cache
            .instrument(&order.instrument_id())
            .map_or(nautilus_model::types::Currency::USD(), |instrument| {
                instrument.quote_currency()
            })
    }

    fn translate(&self, event: &ExecEvent) -> Option<nautilus_model::events::OrderEventAny> {
        let cl_ord_id = match event {
            ExecEvent::New { cl_ord_id, .. }
            | ExecEvent::Fill { cl_ord_id, .. }
            | ExecEvent::Canceled { cl_ord_id, .. }
            | ExecEvent::Rejected { cl_ord_id, .. } => cl_ord_id.clone(),
            ExecEvent::Session(_) => return None,
        };
        let client_order_id = ClientOrderId::new_checked(cl_ord_id.as_str()).ok()?;
        let order = {
            let cache = self.cache.borrow();
            cache.order(&client_order_id)?.clone()
        };
        let ts = UnixNanos::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or_default(),
        );
        let venue_order_id = match event {
            ExecEvent::New { order_id, .. }
            | ExecEvent::Fill { order_id, .. }
            | ExecEvent::Canceled { order_id, .. } => {
                VenueOrderId::new_checked(order_id.as_str()).ok()?
            }
            ExecEvent::Rejected { .. } => VenueOrderId::from("0"),
            ExecEvent::Session(_) => return None,
        };
        match event {
            ExecEvent::New { .. } => {
                Some(
                    self.factory
                        .generate_order_accepted(&order, venue_order_id, ts, ts),
                )
            }
            ExecEvent::Fill {
                exec_id,
                last_qty,
                last_px,
                ..
            } => {
                // The local exchange does not report liquidity flags; all
                // fills are treated as taker-side.
                Some(self.factory.generate_order_filled(
                    &order,
                    venue_order_id,
                    None,
                    nautilus_model::identifiers::TradeId::new_checked(exec_id.as_str()).ok()?,
                    last_qty.to_string().parse::<Quantity>().ok()?,
                    last_px.to_string().parse::<Price>().ok()?,
                    self.instrument_quote_currency(&order),
                    None,
                    LiquiditySide::Taker,
                    ts,
                    ts,
                ))
            }
            ExecEvent::Canceled { .. } => {
                Some(
                    self.factory
                        .generate_order_canceled(&order, Some(venue_order_id), ts, ts),
                )
            }
            ExecEvent::Rejected { reason, .. } => Some(
                self.factory
                    .generate_order_rejected(&order, reason, ts, ts, false),
            ),
            ExecEvent::Session(_) => None,
        }
    }
}

/// FIX execution client for the local simulated exchange.
#[derive(Debug)]
pub struct LocalExchangeClient {
    gate: Rc<RefCell<Option<std::sync::Arc<crate::readiness::ReadinessGate>>>>,
    writer: RefCell<Option<FixWriter>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    account_id: AccountId,
    venue: Venue,
    connected: Rc<Cell<bool>>,
    factory: OrderEventFactory,
}

impl Drop for LocalExchangeClient {
    fn drop(&mut self) {
        self.connected.set(false);
        if let Some(writer) = self.writer.borrow_mut().take() {
            let _ = writer.close();
        }
    }
}

impl LocalExchangeClient {
    /// Connects to the exchange and returns the pump for the core loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the FIX logon handshake fails.
    pub fn connect_new(
        addr: &str,
        trader_id: TraderId,
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        cache: Rc<RefCell<Cache>>,
    ) -> Result<(Self, LocalPump), crate::local_exchange::FixError> {
        let initiator = FixInitiator::connect(addr, "ORDERHUB", "GOX", 30)?;
        Ok(Self::from_initiator(
            initiator, trader_id, client_id, account_id, venue, cache,
        ))
    }

    /// Connects using effective FIX settings loaded by the node.
    ///
    /// # Errors
    /// Returns configuration, transport or logon errors.
    pub fn connect_configured(
        config: &FixSessionConfig,
        log: &FixLog,
        trader_id: TraderId,
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        cache: Rc<RefCell<Cache>>,
    ) -> Result<(Self, LocalPump), crate::local_exchange::FixError> {
        let initiator = FixInitiator::connect_configured(config, log)?;
        Ok(Self::from_initiator(
            initiator, trader_id, client_id, account_id, venue, cache,
        ))
    }

    fn from_initiator(
        initiator: FixInitiator,
        trader_id: TraderId,
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        cache: Rc<RefCell<Cache>>,
    ) -> (Self, LocalPump) {
        let (writer, reports, _reader) = initiator.split();
        let connected = Rc::new(Cell::new(true));
        let gate = Rc::new(RefCell::new(None));
        let client = Self {
            gate: Rc::clone(&gate),
            writer: RefCell::new(Some(writer)),
            cache: Rc::clone(&cache),
            client_id,
            account_id,
            venue,
            connected: Rc::clone(&connected),
            factory: OrderEventFactory::new(trader_id, account_id, AccountType::Cash, None),
        };
        let pump = LocalPump {
            reports,
            factory: OrderEventFactory::new(trader_id, account_id, AccountType::Cash, None),
            cache,
            gate,
            connected,
            engine_owner: None,
        };
        (client, pump)
    }

    fn submit(
        &self,
        client_order_id: &ClientOrderId,
        side: OrderSide,
        quantity: &Quantity,
        price: Option<Price>,
    ) -> anyhow::Result<()> {
        crate::exec_wiring::check_dispatch_allowed(client_order_id).map_err(anyhow::Error::msg)?;
        if self
            .gate
            .borrow()
            .as_ref()
            .is_some_and(|gate| !gate.is_admissible())
        {
            anyhow::bail!("business gate closed before execution dispatch");
        }
        let mut writer = self
            .writer
            .borrow_mut()
            .take()
            .ok_or_else(|| anyhow::anyhow!("FIX writer unavailable"))?;
        let symbol = {
            let cache = self.cache.borrow();
            cache
                .order(client_order_id)
                .map(|order| order.instrument_id().symbol.to_string())
                .unwrap_or_default()
        };
        let result = writer.submit_order(
            client_order_id.as_ref(),
            &symbol,
            match side {
                OrderSide::Buy => Side::Buy,
                OrderSide::Sell => Side::Sell,
            },
            rust_decimal::Decimal::from_str(&quantity.to_string())
                .map_err(|e| anyhow::anyhow!("{e}"))
                .map_err(|e| anyhow::anyhow!("{e}"))?,
            price.map(|px| rust_decimal::Decimal::from_str(&px.to_string()).unwrap_or_default()),
        );
        *self.writer.borrow_mut() = Some(writer);
        if result.is_err() {
            self.connected.set(false);
        }
        result.map_err(|e| anyhow::anyhow!("{e}"))?;
        // Venue-agnostic ack that the order left the gateway (the engine
        // expects Submitted before the venue's Accepted).
        let order = {
            let cache = self.cache.borrow();
            cache
                .order(client_order_id)
                .ok_or_else(|| anyhow::anyhow!("order missing from cache"))?
                .clone()
        };
        if let Some(sender) = try_get_exec_event_sender() {
            sender
                .send(ExecutionEvent::Order(
                    self.factory.generate_order_submitted(&order, now_ns()),
                ))
                .map_err(|e| {
                    self.connected.set(false);
                    anyhow::anyhow!("execution event queue unavailable: {e}")
                })?;
        }
        Ok(())
    }

    fn fresh_cancel_id(&self) -> String {
        format!("C-{}-{}", self.client_id, {
            // Unique per call within the process lifetime.
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or_default()
        })
    }
}

impl ExecutionClient for LocalExchangeClient {
    fn generate_account_state(
        &self,
        _balances: Vec<nautilus_model::types::AccountBalance>,
        _margins: Vec<nautilus_model::types::MarginBalance>,
        _reported: bool,
        _ts_event: UnixNanos,
        _info: Option<nautilus_core::Params>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("not supported by local exchange")
    }

    fn is_connected(&self) -> bool {
        self.connected.get()
    }

    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn account_id(&self) -> AccountId {
        self.account_id
    }

    fn venue(&self) -> Venue {
        self.venue
    }

    fn oms_type(&self) -> OmsType {
        OmsType::Netting
    }

    fn get_account(&self) -> Option<nautilus_model::accounts::AccountAny> {
        None
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.connected.set(false);
        *self.writer.borrow_mut() = None;
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.cache.borrow();
            cache
                .order(&cmd.client_order_id)
                .ok_or_else(|| anyhow::anyhow!("order missing from cache"))?
                .clone()
        };
        self.submit(
            &cmd.client_order_id,
            order.order_side(),
            &order.quantity(),
            order.price(),
        )
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        for client_order_id in &cmd.order_list.client_order_ids {
            let order = {
                let cache = self.cache.borrow();
                cache
                    .order(client_order_id)
                    .ok_or_else(|| anyhow::anyhow!("order missing from cache"))?
                    .clone()
            };
            self.submit(
                client_order_id,
                order.order_side(),
                &order.quantity(),
                order.price(),
            )?;
        }
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.cache.borrow();
            cache
                .order(&cmd.client_order_id)
                .ok_or_else(|| anyhow::anyhow!("order missing from cache"))?
                .clone()
        };
        let mut writer = self
            .writer
            .borrow_mut()
            .take()
            .ok_or_else(|| anyhow::anyhow!("FIX writer unavailable"))?;
        let symbol = order.instrument_id().symbol.to_string();
        let side = match order.order_side() {
            OrderSide::Buy => Side::Buy,
            OrderSide::Sell => Side::Sell,
        };
        let result = writer.cancel_order(
            &self.fresh_cancel_id(),
            cmd.client_order_id.as_ref(),
            &symbol,
            side,
        );
        *self.writer.borrow_mut() = Some(writer);
        if result.is_err() {
            self.connected.set(false);
        }
        result.map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        anyhow::bail!("not supported by local exchange")
    }

    fn query_order(&self, _cmd: QueryOrder) -> anyhow::Result<()> {
        anyhow::bail!("not supported by local exchange")
    }
}

/// Registers engine + local FIX client; mirrors `attach_sandbox_execution`.
///
/// Returns the pump the core loop must call alongside command draining.
///
/// # Panics
///
/// Panics if engine or client registration fails.
pub fn attach_local_execution(
    addr: &str,
    trader_id: TraderId,
    venue: Venue,
    account_id: &AccountId,
    clock: Rc<RefCell<dyn nautilus_common::clock::Clock>>,
    cache: &Rc<RefCell<Cache>>,
) -> LocalPump {
    try_attach_local_execution(addr, trader_id, venue, account_id, clock, cache)
        .expect("connect local exchange")
}

/// Registers the execution engine and reports connection failures to the lifecycle owner.
///
/// # Errors
/// Returns FIX logon or engine registration errors without leaving a live client behind.
pub fn try_attach_local_execution(
    addr: &str,
    trader_id: TraderId,
    venue: Venue,
    account_id: &AccountId,
    clock: Rc<RefCell<dyn nautilus_common::clock::Clock>>,
    cache: &Rc<RefCell<Cache>>,
) -> anyhow::Result<LocalPump> {
    let (client, pump) = LocalExchangeClient::connect_new(
        addr,
        trader_id,
        ClientId::from("GOX"),
        *account_id,
        venue,
        Rc::clone(cache),
    )?;
    register_local(client, pump, venue, clock, cache)
}

/// Registers the local execution client using file-configured FIX settings.
///
/// # Errors
/// Returns configuration, logon or engine registration errors.
pub fn try_attach_local_execution_configured(
    config: &FixSessionConfig,
    log: &FixLog,
    trader_id: TraderId,
    venue: Venue,
    account_id: &AccountId,
    clock: Rc<RefCell<dyn nautilus_common::clock::Clock>>,
    cache: &Rc<RefCell<Cache>>,
) -> anyhow::Result<LocalPump> {
    let (client, pump) = LocalExchangeClient::connect_configured(
        config,
        log,
        trader_id,
        ClientId::from("GOX"),
        *account_id,
        venue,
        Rc::clone(cache),
    )?;
    register_local(client, pump, venue, clock, cache)
}

fn register_local(
    client: LocalExchangeClient,
    mut pump: LocalPump,
    venue: Venue,
    clock: Rc<RefCell<dyn nautilus_common::clock::Clock>>,
    cache: &Rc<RefCell<Cache>>,
) -> anyhow::Result<LocalPump> {
    let engine = Rc::new(RefCell::new(
        nautilus_execution::engine::ExecutionEngine::new(clock, Rc::clone(cache), None),
    ));
    nautilus_execution::engine::ExecutionEngine::register_msgbus_handlers(&engine);
    let mut engine_ref = engine.borrow_mut();
    engine_ref.register_client(Box::new(client))?;
    engine_ref.register_venue_routing(ClientId::from("GOX"), venue)?;
    drop(engine_ref);

    pump.engine_owner = Some(engine);
    Ok(pump)
}
