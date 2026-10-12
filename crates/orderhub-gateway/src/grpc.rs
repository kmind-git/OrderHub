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

//! gRPC service over the admission bridge (M1).
//!
//! Threading model per the design: the bridge owns non-`Send` kernel state
//! (`Rc<RefCell<Cache>>`), so all four RPCs exchange plain value objects with
//! a dedicated core thread through a channel. The tonic handlers themselves
//! only await reply channels and never touch kernel state. WatchEvents reads
//! the journal (which is `Send + Sync`) directly.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::SyncSender as CoreSender;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use nautilus_common::cache::Cache;
use nautilus_common::runner::{TimeEventMessage, TimeEventSender, replace_time_event_sender};
use nautilus_model::identifiers::{InstrumentId, StrategyId};
use tokio::sync::oneshot;
use tonic::Request as RpcRequest;
use tonic::{Request, Response, Status, transport::Server};

use crate::bridge::{BridgeError, LimitOrderRequest, OrderHubBridge};
use crate::journal::{BusinessJournal, JournalEntry, JournalOutcome};
use orderhub_proto::orderhub::v1::order_hub_server::{OrderHub, OrderHubServer};
use orderhub_proto::orderhub::v1::{
    AdmissionEvent, AdmissionResponse, CancelOrderRequest, GetStateRequest, GetStateResponse,
    InstrumentInfo, JournalEvent, OrderView, OutcomeEvent, ProgressEvent, SubmitOrderRequest,
    WatchEventsRequest,
};

use nautilus_model::enums::{OrderSide, TimeInForce};
use nautilus_model::types::{Price, Quantity};

/// Snapshot payload returned by the core thread for GetState.
#[derive(Debug)]
struct CoreSnapshot {
    inputs_ready: bool,
}

/// Wiring the spawned core thread owns: the bridge, the cache, the deferred
/// execution-event receiver, and the local FIX exchange pump (when attached).
pub type CoreState = (
    OrderHubBridge,
    Rc<RefCell<Cache>>,
    Option<crate::exec_wiring::ExecEventRx>,
    Option<crate::local_client::LocalPump>,
);

const CORE_TIMER_CAPACITY: usize = 4_096;

#[derive(Debug)]
struct CoreTimeSender {
    tx: mpsc::SyncSender<TimeEventMessage>,
    gate: Arc<crate::readiness::ReadinessGate>,
}

impl TimeEventSender for CoreTimeSender {
    fn send(&self, message: TimeEventMessage) {
        if self.tx.try_send(message).is_err() {
            self.gate.set(
                crate::readiness::Readiness::Halted,
                "core timer queue full or stopped",
            );
        }
    }
}

enum CoreRequest {
    Submit {
        queued_at: std::time::Instant,
        _permit: tokio::sync::OwnedSemaphorePermit,
        request: LimitOrderRequest,
        reply: oneshot::Sender<Result<crate::bridge::AdmissionReceipt, BridgeError>>,
    },
    Cancel {
        submitter_id: String,
        strategies: Vec<String>,
        request_id: String,
        client_order_id: String,
        reply: oneshot::Sender<Result<crate::bridge::AdmissionReceipt, BridgeError>>,
    },
    Snapshot {
        reply: oneshot::Sender<CoreSnapshot>,
    },
    Shutdown {
        directory: std::path::PathBuf,
        identity: crate::journal::RecoveryIdentity,
        reply: oneshot::Sender<Result<std::path::PathBuf, String>>,
    },
}

/// Handle to the core thread owning the bridge, cache, and quota ledger.
#[derive(Debug, Clone)]
pub struct CoreHandle {
    tx: CoreSender<CoreRequest>,
    control_tx: CoreSender<CoreRequest>,
    query_tx: CoreSender<CoreRequest>,
    journal: Arc<BusinessJournal>,
    registry: Arc<crate::auth::SubmitterRegistry>,
    gate: std::sync::Arc<crate::readiness::ReadinessGate>,
    admission_limits: Arc<std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>>,
}

impl CoreHandle {
    /// Spawns the core thread over an already-wired bridge and cache.
    ///
    /// # Panics
    ///
    /// Panics if the core thread cannot be spawned.
    #[allow(clippy::type_complexity)]
    pub fn spawn(
        setup: impl FnOnce() -> CoreState + Send + 'static,
        journal: Arc<BusinessJournal>,
        registry: Arc<crate::auth::SubmitterRegistry>,
        gate: std::sync::Arc<crate::readiness::ReadinessGate>,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel::<CoreRequest>(1_024);
        let (control_tx, control_rx) = mpsc::sync_channel::<CoreRequest>(128);
        let (query_tx, query_rx) = mpsc::sync_channel::<CoreRequest>(128);
        let timer_gate = Arc::clone(&gate);
        std::thread::Builder::new()
            .name("orderhub-core".to_string())
            .spawn(move || {
                let (time_tx, time_rx) = mpsc::sync_channel(CORE_TIMER_CAPACITY);
                replace_time_event_sender(Arc::new(CoreTimeSender {
                    tx: time_tx,
                    gate: timer_gate,
                }));
                let (bridge, cache, exec_rx, local_pump) = setup();
                core_thread(
                    &bridge,
                    &cache,
                    exec_rx,
                    local_pump,
                    &CoreReceivers {
                        submit: rx,
                        cancel: control_rx,
                        query: query_rx,
                    },
                    &time_rx,
                );
            })
            .expect("failed to spawn core thread");
        Self {
            tx,
            control_tx,
            query_tx,
            journal,
            admission_limits: Arc::new(
                registry
                    .submitter_ids()
                    .into_iter()
                    .map(|id| (id, Arc::new(tokio::sync::Semaphore::new(64))))
                    .collect(),
            ),
            registry,
            gate,
        }
    }

    /// The shared readiness gate.
    #[must_use]
    pub fn gate(&self) -> std::sync::Arc<crate::readiness::ReadinessGate> {
        std::sync::Arc::clone(&self.gate)
    }

    /// Closes new risk, drains reports, and publishes the final consistent checkpoint.
    ///
    /// # Errors
    /// Returns full control queue, stopped core, or checkpoint errors.
    pub async fn shutdown(
        &self,
        directory: std::path::PathBuf,
        identity: crate::journal::RecoveryIdentity,
    ) -> Result<std::path::PathBuf, String> {
        let (reply, receive) = oneshot::channel();
        self.control_tx
            .try_send(CoreRequest::Shutdown {
                directory,
                identity,
                reply,
            })
            .map_err(|e| format!("shutdown control unavailable: {e}"))?;
        receive
            .await
            .map_err(|e| format!("shutdown reply unavailable: {e}"))?
    }

    async fn submit(
        &self,
        request: LimitOrderRequest,
    ) -> Result<crate::bridge::AdmissionReceipt, BridgeError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let permit = self
            .admission_limits
            .get(&request.submitter_id)
            .ok_or(BridgeError::PermissionDenied)?
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                BridgeError::Capacity("submitter in-flight request limit reached".into())
            })?;
        self.tx
            .try_send(CoreRequest::Submit {
                queued_at: std::time::Instant::now(),
                _permit: permit,
                request,
                reply: reply_tx,
            })
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => {
                    BridgeError::Capacity("submission queue full".into())
                }
                mpsc::TrySendError::Disconnected(_) => {
                    BridgeError::Journal("core thread stopped".into())
                }
            })?;
        reply_rx
            .await
            .map_err(|_| BridgeError::Journal("core thread dropped reply".to_string()))?
    }

    async fn cancel(
        &self,
        submitter_id: String,
        strategies: Vec<String>,
        request_id: String,
        client_order_id: String,
    ) -> Result<crate::bridge::AdmissionReceipt, BridgeError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.control_tx
            .try_send(CoreRequest::Cancel {
                submitter_id,
                strategies,
                request_id,
                client_order_id,
                reply: reply_tx,
            })
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => BridgeError::Capacity("control queue full".into()),
                mpsc::TrySendError::Disconnected(_) => {
                    BridgeError::Journal("core thread stopped".into())
                }
            })?;
        reply_rx
            .await
            .map_err(|_| BridgeError::Journal("core thread dropped reply".to_string()))?
    }

    async fn snapshot(&self) -> Result<CoreSnapshot, Status> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.query_tx
            .try_send(CoreRequest::Snapshot { reply: reply_tx })
            .map_err(|_| Status::unavailable("core thread stopped"))?;
        reply_rx
            .await
            .map_err(|_| Status::unavailable("core thread dropped reply"))
    }
}

struct CoreReceivers {
    submit: mpsc::Receiver<CoreRequest>,
    cancel: mpsc::Receiver<CoreRequest>,
    query: mpsc::Receiver<CoreRequest>,
}

fn core_thread(
    bridge: &OrderHubBridge,
    cache: &Rc<RefCell<Cache>>,
    mut exec_rx: Option<crate::exec_wiring::ExecEventRx>,
    mut local_pump: Option<crate::local_client::LocalPump>,
    requests: &CoreReceivers,
    time_rx: &mpsc::Receiver<TimeEventMessage>,
) {
    loop {
        for message in time_rx.try_iter().take(CORE_TIMER_CAPACITY) {
            message.dispatch();
        }
        bridge.poll_inputs();
        // Keep the FIX reports flowing and flush deferred work while idle.
        if let Some(pump) = local_pump.as_mut() {
            pump.pump();
        }
        nautilus_common::runner::drain_trading_cmd_queue();
        crate::exec_wiring::pump_exec_events(&mut exec_rx);
        bridge.commit_health(
            local_pump
                .as_ref()
                .map(crate::local_client::LocalPump::is_connected),
        );
        for request in requests.cancel.try_iter().take(32) {
            if let CoreRequest::Shutdown {
                directory,
                identity,
                reply,
            } = request
            {
                let result = (|| {
                    bridge.transition(
                        crate::readiness::Readiness::Draining,
                        "graceful shutdown; new risk closed",
                    )?;
                    let until = std::time::Instant::now() + Duration::from_millis(250);
                    while std::time::Instant::now() < until {
                        if let Some(pump) = local_pump.as_mut() {
                            pump.pump();
                        }
                        nautilus_common::runner::drain_trading_cmd_queue();
                        crate::exec_wiring::pump_exec_events(&mut exec_rx);
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    bridge.transition(
                        crate::readiness::Readiness::Stopped,
                        "core stopped; unresolved execution occupancy retained",
                    )?;
                    bridge.finish_persistence_session()?;
                    bridge.checkpoint(&directory, identity)
                })()
                .map_err(|e: crate::journal::JournalError| e.to_string());
                if result.is_err() {
                    let _ = bridge.transition(
                        crate::readiness::Readiness::Halted,
                        "shutdown checkpoint failed",
                    );
                }
                let _ = reply.send(result);
                return;
            }
            handle_core_request(bridge, cache, request);
        }
        for request in requests.query.try_iter().take(8) {
            handle_core_request(bridge, cache, request);
        }
        match requests
            .submit
            .recv_timeout(std::time::Duration::from_millis(5))
        {
            Ok(request) => {
                handle_core_request(bridge, cache, request);
                // A submission or cancel may have queued commands and FIX
                // traffic; drain once more before waiting again.
                if let Some(pump) = local_pump.as_mut() {
                    pump.pump();
                }
                nautilus_common::runner::drain_trading_cmd_queue();
                crate::exec_wiring::pump_exec_events(&mut exec_rx);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn handle_core_request(bridge: &OrderHubBridge, _cache: &Rc<RefCell<Cache>>, request: CoreRequest) {
    {
        match request {
            CoreRequest::Submit {
                request,
                reply,
                queued_at,
                _permit,
            } => {
                let result = if queued_at.elapsed() > Duration::from_millis(250) {
                    Err(BridgeError::QueueExpired)
                } else {
                    bridge.submit_limit_order(&request)
                };
                let _ = reply.send(result);
            }
            CoreRequest::Cancel {
                submitter_id,
                strategies,
                request_id,
                client_order_id,
                reply,
            } => {
                let result = bridge
                    .authorize_cancel(&submitter_id, &client_order_id, &strategies)
                    .and_then(|()| {
                        bridge.cancel_order(&submitter_id, &request_id, &client_order_id)
                    });
                let _ = reply.send(result);
            }
            CoreRequest::Snapshot { reply } => {
                bridge.poll_inputs();
                let _ = reply.send(CoreSnapshot {
                    inputs_ready: bridge.inputs_ready(),
                });
            }
            CoreRequest::Shutdown { reply, .. } => {
                let _ = reply.send(Err("shutdown requires the reserved control queue".into()));
            }
        }
    }
}

/// gRPC service implementing the four OrderHub RPCs.
#[derive(Debug)]
pub struct OrderHubService {
    core: CoreHandle,
    snapshots: std::sync::Mutex<crate::views::SnapshotStore>,
    streams: Arc<tokio::sync::Semaphore>,
}

impl OrderHubService {
    fn storage_error(&self, e: crate::journal::JournalError) -> Status {
        self.core
            .gate
            .set(crate::readiness::Readiness::Halted, &e.to_string());
        storage_status(e)
    }
    /// Wraps a core handle as the tonic service.
    #[must_use]
    pub fn new(core: CoreHandle) -> Self {
        Self {
            core,
            snapshots: std::sync::Mutex::new(crate::views::SnapshotStore::default()),
            streams: Arc::new(tokio::sync::Semaphore::new(128)),
        }
    }

    /// Authenticates the bearer token to a submitter identity.
    fn authenticate<T>(
        &self,
        request: &RpcRequest<T>,
    ) -> Result<&crate::auth::SubmitterCredentials, Status> {
        let header = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        self.core
            .registry
            .authenticate_header(header)
            .ok_or_else(|| Status::permission_denied("invalid or missing token"))
    }

    /// Authenticates and verifies the request's strategy is authorized.
    fn authorize_strategy<T>(
        &self,
        request: &RpcRequest<T>,
        strategy_id: &str,
    ) -> Result<&crate::auth::SubmitterCredentials, Status> {
        let credentials = self.authenticate(request)?;
        if crate::auth::SubmitterRegistry::is_authorized(credentials, strategy_id) {
            Ok(credentials)
        } else {
            // Do not disclose whether the strategy exists for others.
            Err(Status::permission_denied(
                "strategy not authorized for this submitter",
            ))
        }
    }
}

/// Borrows the strategy id from a submit request before it is consumed.
fn req_ref_strategy(request: &RpcRequest<SubmitOrderRequest>) -> &str {
    request.get_ref().strategy_id.as_str()
}

#[tonic::async_trait]
impl OrderHub for OrderHubService {
    async fn submit_order(
        &self,
        request: Request<SubmitOrderRequest>,
    ) -> Result<Response<AdmissionResponse>, Status> {
        let credentials = self
            .authorize_strategy(&request, req_ref_strategy(&request))?
            .clone();
        let req = request.into_inner();
        check_epoch(&req.journal_epoch, self.core.journal.epoch())?;
        for value in [&req.request_id, &req.strategy_id, &req.instrument_id] {
            bounded_id(value)?;
        }
        bounded_decimal(&req.quantity)?;
        bounded_decimal(&req.price)?;
        let side = match req.side.as_str() {
            "BUY" => OrderSide::Buy,
            "SELL" => OrderSide::Sell,
            _ => return Err(Status::invalid_argument("invalid side")),
        };
        let time_in_force = match req.time_in_force.as_str() {
            "DAY" => TimeInForce::Day,
            "GTC" => TimeInForce::Gtc,
            "IOC" => TimeInForce::Ioc,
            "FOK" => TimeInForce::Fok,
            _ => return Err(Status::invalid_argument("invalid time in force")),
        };
        let normalized_quantity = req
            .quantity
            .parse::<rust_decimal::Decimal>()
            .map_err(|e| Status::invalid_argument(e.to_string()))?
            .normalize()
            .to_string();
        let normalized_price = req
            .price
            .parse::<rust_decimal::Decimal>()
            .map_err(|e| Status::invalid_argument(e.to_string()))?
            .normalize()
            .to_string();
        let request_id = req.request_id.clone();
        let result = self
            .core
            .submit(LimitOrderRequest {
                submitter_id: credentials.submitter_id.clone(),
                request_id: req.request_id,
                strategy_id: StrategyId::new_checked(&req.strategy_id)
                    .map_err(|e| Status::invalid_argument(e.to_string()))?,
                instrument_id: req
                    .instrument_id
                    .parse::<InstrumentId>()
                    .map_err(|e| Status::invalid_argument(e.to_string()))?,
                side,
                quantity: normalized_quantity
                    .parse::<Quantity>()
                    .map_err(Status::invalid_argument)?,
                price: normalized_price
                    .parse::<Price>()
                    .map_err(Status::invalid_argument)?,
                time_in_force,
                post_only: req.post_only,
                reduce_only: req.reduce_only,
                client_id: None,
                submit_before_ns: req.submit_before_ns,
            })
            .await;
        match result {
            Ok(receipt) => Ok(Response::new(receipt_response(receipt))),
            Err(BridgeError::Input(_)) => {
                let record = self
                    .core
                    .journal
                    .find_request(&credentials.submitter_id, "SubmitOrder", &request_id)
                    .map_err(|e| self.storage_error(e))?
                    .ok_or_else(|| Status::unavailable("request result unavailable"))?;
                Ok(Response::new(result_response(
                    record.admission.journal_epoch,
                    record.admission.seq,
                    &record.result,
                    false,
                )))
            }
            Err(e) => Err(bridge_status(e)),
        }
    }

    async fn cancel_order(
        &self,
        request: Request<CancelOrderRequest>,
    ) -> Result<Response<AdmissionResponse>, Status> {
        let credentials = self.authenticate(&request)?.clone();
        let req = request.into_inner();
        check_epoch(&req.journal_epoch, self.core.journal.epoch())?;
        bounded_id(&req.request_id)?;
        bounded_id(&req.client_order_id)?;
        let receipt = self
            .core
            .cancel(
                credentials.submitter_id,
                credentials.strategies,
                req.request_id,
                req.client_order_id,
            )
            .await
            .map_err(bridge_status)?;
        Ok(Response::new(receipt_response(receipt)))
    }

    async fn get_state(
        &self,
        request: Request<GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        use orderhub_proto::orderhub::v1::get_state_request::Query;
        let credentials = self.authenticate(&request)?.clone();
        let live = self.core.snapshot().await?;
        let epoch = self.core.journal.epoch();
        let key = self
            .core
            .journal
            .view_key()
            .map_err(|e| self.storage_error(e))?;
        let mut response = GetStateResponse {
            journal_epoch: epoch.to_string(),
            writable: self.core.gate.is_admissible() && live.inputs_ready,
            stale: !self.core.gate.is_admissible() || !live.inputs_ready,
            readiness: self.core.gate.get().as_str().into(),
            persistence_mode: self.core.journal.persistence_mode().as_str().into(),
            durable_cursor: self
                .core
                .journal
                .durable_cursor()
                .map_err(|e| self.storage_error(e))?
                .to_string(),
            ..Default::default()
        };
        let query = request
            .into_inner()
            .query
            .unwrap_or_else(|| Query::Snapshot(Default::default()));
        match query {
            Query::Snapshot(query) => {
                let page_size = if query.page_size == 0 {
                    crate::views::PAGE_MAXIMUM
                } else {
                    query.page_size as usize
                };
                if page_size > crate::views::PAGE_MAXIMUM {
                    return Err(Status::invalid_argument("page size exceeds 1000"));
                }
                let mut store = self
                    .snapshots
                    .lock()
                    .map_err(|_| Status::unavailable("snapshot store unavailable"))?;
                let (id, offset) = if query.page_token.is_empty() {
                    let snapshot = self
                        .core
                        .journal
                        .snapshot(
                            &credentials.submitter_id,
                            &credentials.strategies,
                            crate::views::SNAPSHOT_MAXIMUM,
                        )
                        .map_err(|e| self.storage_error(e))?;
                    (store.insert(snapshot, credentials.clone())?, 0)
                } else {
                    let token = crate::views::verify(&key, &query.page_token, epoch, &credentials)?;
                    if token.lease.is_empty() {
                        return Err(Status::invalid_argument("snapshot token required"));
                    }
                    (token.lease, token.offset)
                };
                let lease = store
                    .leases
                    .get(&id)
                    .ok_or_else(|| Status::out_of_range("RESYNC_REQUIRED"))?;
                if lease.expires_ms <= crate::views::now_ms()
                    || crate::views::scope(&lease.credentials) != crate::views::scope(&credentials)
                    || lease.credentials.submitter_id != credentials.submitter_id
                {
                    return Err(Status::out_of_range("RESYNC_REQUIRED"));
                }
                if offset > lease.data.orders.len() {
                    return Err(Status::invalid_argument("invalid page offset"));
                }
                let end = (offset + page_size).min(lease.data.orders.len());
                response.active_orders = lease.data.orders[offset..end]
                    .iter()
                    .map(order_view)
                    .collect();
                response.positions = lease.data.positions.clone();
                response.committed_health = lease.data.control.as_ref().map(control_view);
                response.accounts = lease.data.accounts.clone();
                response.inputs = lease
                    .data
                    .inputs
                    .iter()
                    .map(|event| orderhub_proto::orderhub::v1::InputFact {
                        json: public_input_json(event).expect("validated input serialization"),
                    })
                    .collect();
                response.instruments = lease
                    .data
                    .inputs
                    .iter()
                    .filter_map(instrument_view)
                    .collect();
                response.state_cursor = lease.data.cursor.to_string();
                response.snapshot_id = id.clone();
                response.resume_token = crate::views::issue(
                    &key,
                    epoch,
                    &credentials,
                    lease.data.cursor,
                    String::new(),
                    0,
                    None,
                );
                if end < lease.data.orders.len() {
                    response.next_page_token = crate::views::issue(
                        &key,
                        epoch,
                        &credentials,
                        lease.data.cursor,
                        id,
                        end,
                        Some(lease.expires_ms),
                    );
                } else {
                    store.leases.remove(&response.snapshot_id);
                }
            }
            Query::OrderLookup(query) => {
                if query.client_order_ids.is_empty()
                    || query.client_order_ids.len() > crate::views::PAGE_MAXIMUM
                {
                    return Err(Status::invalid_argument(
                        "order lookup requires 1..=1000 IDs",
                    ));
                }
                for id in &query.client_order_ids {
                    bounded_id(id)?;
                }
                let (cursor, orders) = self
                    .core
                    .journal
                    .lookup_orders(
                        &query.client_order_ids,
                        &credentials.submitter_id,
                        &credentials.strategies,
                    )
                    .map_err(|e| match e {
                        crate::journal::JournalError::Encoding(ref reason)
                            if reason == "order unavailable or unauthorized" =>
                        {
                            Status::permission_denied("order unavailable or unauthorized")
                        }
                        other => self.storage_error(other),
                    })?;
                response.active_orders = orders.iter().map(order_view).collect();
                response.state_cursor = cursor.to_string();
                response.resume_token =
                    crate::views::issue(&key, epoch, &credentials, cursor, String::new(), 0, None);
            }
            Query::RequestLookup(query) => {
                check_epoch(&query.journal_epoch, epoch)?;
                bounded_id(&query.request_id)?;
                if !matches!(query.rpc_method.as_str(), "SubmitOrder" | "CancelOrder") {
                    return Err(Status::invalid_argument("invalid RPC method"));
                }
                let record = self
                    .core
                    .journal
                    .find_request(
                        &credentials.submitter_id,
                        &query.rpc_method,
                        &query.request_id,
                    )
                    .map_err(|e| self.storage_error(e))?
                    .ok_or_else(|| Status::not_found("REQUEST_NOT_FOUND"))?;
                if !credentials
                    .strategies
                    .contains(&record.admission.strategy_id)
                {
                    return Err(Status::permission_denied(
                        "request unavailable or unauthorized",
                    ));
                }
                response.state_cursor = record.result.version.to_string();
                response.request_result = Some(result_response(
                    epoch,
                    record.admission.seq,
                    &record.result,
                    true,
                ));
            }
        }
        Ok(Response::new(response))
    }

    type WatchEventsStream = tokio_stream::wrappers::ReceiverStream<Result<JournalEvent, Status>>;

    async fn watch_events(
        &self,
        request: Request<WatchEventsRequest>,
    ) -> Result<Response<Self::WatchEventsStream>, Status> {
        let credentials = self.authenticate(&request)?.clone();
        let req = request.into_inner();
        let epoch = self.core.journal.epoch();
        check_epoch(&req.journal_epoch, epoch)?;
        let key = self
            .core
            .journal
            .view_key()
            .map_err(|e| self.storage_error(e))?;
        let token = crate::views::verify(&key, &req.resume_token, epoch, &credentials)?;
        if !token.lease.is_empty() {
            return Err(Status::invalid_argument("stream token required"));
        }
        let mut cursor = token.cursor;
        if req.after != cursor.to_string() {
            return Err(Status::invalid_argument(
                "cursor does not match signed token",
            ));
        }
        if cursor
            > self
                .core
                .journal
                .last_seq()
                .map_err(|e| self.storage_error(e))?
        {
            return Err(Status::out_of_range("RESYNC_REQUIRED"));
        }
        let permit = self
            .streams
            .clone()
            .try_acquire_owned()
            .map_err(|_| Status::resource_exhausted("stream limit reached"))?;
        let journal = Arc::clone(&self.core.journal);
        let gate = Arc::clone(&self.core.gate);
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        tokio::spawn(async move {
            let _permit = permit;
            loop {
                if tx.is_closed() {
                    break;
                }
                if gate.get() == crate::readiness::Readiness::Stopped {
                    let _ = tx
                        .send(Err(Status::unavailable(
                            "node stopped; resume from durable cursor after recovery",
                        )))
                        .await;
                    break;
                }
                if gate.get() == crate::readiness::Readiness::Halted {
                    let _ = tx
                        .send(Err(Status::unavailable(
                            "business storage or execution halted",
                        )))
                        .await;
                    break;
                }
                let entries = match journal.scan_page(cursor, 256) {
                    Ok(entries) => entries,
                    Err(e) => {
                        gate.set(crate::readiness::Readiness::Halted, &e.to_string());
                        let _ = tx.send(Err(storage_status(e))).await;
                        break;
                    }
                };
                for (seq, entry) in entries {
                    cursor = seq;
                    let kind = match visible_event(&journal, &credentials, entry) {
                        Ok(kind) => kind,
                        Err(e) => {
                            gate.set(crate::readiness::Readiness::Halted, &e.to_string());
                            let _ = tx.send(Err(storage_status(e))).await;
                            return;
                        }
                    };
                    let Some(kind) = kind else { continue };
                    let event = JournalEvent {
                        view: match journal.event_view(seq) {
                            Ok(view) => {
                                view.map(|(order, balance)| committed_view(&order, &balance))
                            }
                            Err(e) => {
                                gate.set(crate::readiness::Readiness::Halted, &e.to_string());
                                let _ = tx.send(Err(storage_status(e))).await;
                                return;
                            }
                        },
                        seq: seq.to_string(),
                        journal_epoch: epoch.to_string(),
                        event_id: format!("{epoch}:{seq}"),
                        resume_token: crate::views::issue(
                            &key,
                            epoch,
                            &credentials,
                            seq,
                            String::new(),
                            0,
                            None,
                        ),
                        kind: Some(kind),
                    };
                    if !stream_send(&tx, event).await {
                        return;
                    }
                }
                let progress = JournalEvent {
                    view: None,
                    seq: cursor.to_string(),
                    journal_epoch: epoch.to_string(),
                    event_id: String::new(),
                    resume_token: crate::views::issue(
                        &key,
                        epoch,
                        &credentials,
                        cursor,
                        String::new(),
                        0,
                        None,
                    ),
                    kind: Some(orderhub_proto::orderhub::v1::journal_event::Kind::Progress(
                        ProgressEvent {
                            state_cursor: cursor.to_string(),
                        },
                    )),
                };
                if !stream_send(&tx, progress).await {
                    break;
                }
                tokio::select! { () = tx.closed() => break, () = tokio::time::sleep(Duration::from_millis(50)) => {} }
            }
        });
        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }
}

fn bounded_id(value: &str) -> Result<(), Status> {
    if value.is_empty() || value.len() > 64 || value.chars().any(char::is_control) {
        return Err(Status::invalid_argument(
            "ID must contain 1..=64 bytes without control characters",
        ));
    }
    Ok(())
}

fn bounded_decimal(value: &str) -> Result<(), Status> {
    if value.is_empty()
        || value.len() > 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return Err(Status::invalid_argument("invalid decimal format or size"));
    }
    Ok(())
}

fn check_epoch(value: &str, epoch: u64) -> Result<(), Status> {
    if value != epoch.to_string() {
        return Err(Status::failed_precondition(
            "JOURNAL_EPOCH_MISMATCH; resynchronize GetState",
        ));
    }
    Ok(())
}

fn storage_status(e: crate::journal::JournalError) -> Status {
    let message = match e {
        crate::journal::JournalError::Storage(message) => {
            format!("journal storage error: {message}")
        }
        crate::journal::JournalError::Encoding(message) => {
            format!("journal encoding error: {message}")
        }
        crate::journal::JournalError::CommitUnknown(message) => {
            format!("journal commit result unknown: {message}")
        }
    };
    Status::unavailable(message)
}

fn bridge_status(e: BridgeError) -> Status {
    match e {
        BridgeError::Capacity(reason) => Status::resource_exhausted(reason),
        BridgeError::QueueExpired => Status::deadline_exceeded("queue expired before admission"),
        BridgeError::PermissionDenied => Status::permission_denied(e.to_string()),
        BridgeError::IdempotencyConflict { .. } => Status::already_exists("IDEMPOTENCY_CONFLICT"),
        BridgeError::Order(message) => Status::invalid_argument(message),
        other => Status::unavailable(other.to_string()),
    }
}

fn receipt_response(receipt: crate::bridge::AdmissionReceipt) -> AdmissionResponse {
    AdmissionResponse {
        journal_epoch: receipt.journal_epoch.to_string(),
        seq: receipt.seq.to_string(),
        client_order_id: if receipt.result == "REJECTED" {
            String::new()
        } else {
            receipt.client_order_id.to_string()
        },
        already_admitted: receipt.already_admitted,
        result: receipt.result,
        reason: receipt.reason,
    }
}

fn result_response(
    epoch: u64,
    seq: u64,
    result: &crate::ledger::RequestResult,
    retry: bool,
) -> AdmissionResponse {
    AdmissionResponse {
        journal_epoch: epoch.to_string(),
        seq: seq.to_string(),
        client_order_id: result.client_order_id.clone(),
        already_admitted: retry,
        result: result.result.clone(),
        reason: result.reason.clone(),
    }
}

fn order_view(order: &crate::ledger::BusinessOrder) -> OrderView {
    OrderView {
        client_order_id: order.client_order_id.clone(),
        strategy_id: order.admission.strategy_id.clone(),
        instrument_id: order.admission.instrument_id.clone(),
        side: order.admission.order_side.clone(),
        quantity: order.admission.quantity.clone(),
        price: order.admission.price.clone(),
        status: order.status.clone(),
        filled_quantity: order.filled_quantity.clone(),
        remaining_quantity: order.remaining_quantity.clone(),
        account_id: order.account_id.clone(),
        fees: order.fees.clone(),
        version: order.version.to_string(),
    }
}

fn control_view(
    control: &crate::ledger::ControlRecord,
) -> orderhub_proto::orderhub::v1::ControlFact {
    orderhub_proto::orderhub::v1::ControlFact {
        readiness: control.readiness.clone(),
        writable: control.writable,
        connected: control.connected,
        reason: control.reason.clone(),
    }
}

fn committed_view(
    order: &crate::ledger::BusinessOrder,
    balance: &crate::ledger::RiskBalance,
) -> orderhub_proto::orderhub::v1::CommittedView {
    use orderhub_proto::orderhub::v1::{AccountView, CommittedView, PositionView};
    CommittedView {
        order: Some(order_view(order)),
        position: Some(PositionView {
            strategy_id: order.admission.strategy_id.clone(),
            instrument_id: order.admission.instrument_id.clone(),
            quantity: balance.quantity.clone(),
            cost: balance.cost.clone(),
            reserved_quantity: balance.reserved_quantity.clone(),
        }),
        account: Some(AccountView {
            account_id: order.account_id.clone(),
            strategy_id: order.admission.strategy_id.clone(),
            reserved_cash: balance.reserved_cash.clone(),
            spent_cash: balance.spent_cash.clone(),
        }),
    }
}

fn public_input_json(event: &crate::inputs::InputEvent) -> Result<String, serde_json::Error> {
    fn normalize(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    if matches!(
                        key.as_str(),
                        "epoch" | "sequence" | "revision" | "instrument_revision"
                    ) || key.ends_with("_ns")
                    {
                        if let Some(number) = value.as_u64() {
                            *value = serde_json::Value::String(number.to_string());
                        }
                    } else {
                        normalize(value);
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    normalize(value);
                }
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(event)?;
    normalize(&mut value);
    serde_json::to_string(&value)
}

fn instrument_view(event: &crate::inputs::InputEvent) -> Option<InstrumentInfo> {
    match event {
        crate::inputs::InputEvent::Instrument {
            instrument_id,
            price_increment,
            size_increment,
            price_precision,
            ..
        } => Some(InstrumentInfo {
            instrument_id: instrument_id.clone(),
            price_increment: price_increment.clone(),
            size_increment: size_increment.clone(),
            price_precision: u32::from(*price_precision),
        }),
        _ => None,
    }
}

fn visible_event(
    journal: &BusinessJournal,
    credentials: &crate::auth::SubmitterCredentials,
    entry: JournalEntry,
) -> Result<Option<orderhub_proto::orderhub::v1::journal_event::Kind>, crate::journal::JournalError>
{
    use orderhub_proto::orderhub::v1::journal_event::Kind;
    let allowed = |record: &crate::journal::AdmissionRecord| {
        record.submitter_id == credentials.submitter_id
            && credentials.strategies.contains(&record.strategy_id)
    };
    Ok(match entry {
        JournalEntry::Control(control) => Some(Kind::Control(control_view(&control))),
        JournalEntry::Input { event, .. } => {
            if !event.is_persistent() {
                return Ok(None);
            }
            let json = if let crate::inputs::InputEvent::Account {
                meta,
                account_id,
                instrument_id,
                ownership,
                ..
            } = event.as_ref()
            {
                let ownership: Vec<_> = ownership
                    .iter()
                    .filter(|holding| credentials.strategies.contains(&holding.strategy_id))
                    .collect();
                let mut value = serde_json::json!({ "kind": "strategy_owned_input", "meta": { "source_id": meta.source_id, "sequence": meta.sequence.to_string(), "event_time_ns": meta.event_time_ns.to_string() }, "account_id": account_id, "instrument_id": instrument_id, "ownership": ownership });
                // This scoped input is not a total-account financial snapshot.
                if ownership.is_empty() {
                    value["ownership"] = serde_json::json!([]);
                }
                serde_json::to_string(&value)?
            } else {
                public_input_json(&event)?
            };
            Some(Kind::Input(orderhub_proto::orderhub::v1::InputFact {
                json,
            }))
        }
        JournalEntry::Admission(record) => {
            if allowed(&record) {
                Some(Kind::Admission(AdmissionEvent {
                    submitter_id: record.submitter_id,
                    request_id: record.request_id,
                    strategy_id: record.strategy_id,
                    instrument_id: record.instrument_id,
                    request_kind: record.request_kind,
                }))
            } else {
                None
            }
        }
        JournalEntry::Outcome {
            origin_seq,
            outcome,
        } => {
            let Some(JournalEntry::Admission(record)) = journal.entry(origin_seq)? else {
                return Err(crate::journal::JournalError::Encoding(
                    "outcome origin missing".into(),
                ));
            };
            if !allowed(&record) {
                return Ok(None);
            }
            Some(match outcome {
                JournalOutcome::Execution(event) => {
                    Kind::Execution(orderhub_proto::orderhub::v1::ExecutionFact {
                        event_id: event.event_id,
                        trade_id: event.trade_id.unwrap_or_default(),
                        client_order_id: event.client_order_id,
                        strategy_id: event.strategy_id,
                        account_id: event.account_id,
                        instrument_id: event.instrument_id,
                        kind: event.kind,
                        quantity: event.quantity,
                        price: event.price,
                        fee: event.fee,
                        currency: event.currency,
                        event_time_ns: event.ts_event_ns.to_string(),
                    })
                }
                outcome => {
                    let (kind, id, reason) = match outcome {
                        JournalOutcome::Admitted { client_order_id } => {
                            ("ADMITTED", client_order_id, String::new())
                        }
                        JournalOutcome::DispatchStarted { client_order_id } => {
                            ("DISPATCH_STARTED", client_order_id, String::new())
                        }
                        JournalOutcome::Denied { reason } => ("DENIED", String::new(), reason),
                        JournalOutcome::Canceled { client_order_id } => {
                            ("CANCELED", client_order_id, String::new())
                        }
                        JournalOutcome::Filled {
                            client_order_id, ..
                        } => ("FILLED", client_order_id, String::new()),
                        JournalOutcome::Execution(_) => unreachable!(),
                    };
                    Kind::Outcome(OutcomeEvent {
                        origin_seq: origin_seq.to_string(),
                        client_order_id: id,
                        outcome: kind.into(),
                        reason,
                    })
                }
            })
        }
    })
}

async fn stream_send(
    tx: &tokio::sync::mpsc::Sender<Result<JournalEvent, Status>>,
    event: JournalEvent,
) -> bool {
    match tokio::time::timeout(Duration::from_secs(5), tx.reserve_many(2)).await {
        Ok(Ok(mut permits)) => {
            permits
                .next()
                .expect("two reserved stream slots")
                .send(Ok(event));
            true
        }
        Ok(Err(_)) => false,
        Err(_) => {
            // Reserve an explicit error delivery opportunity once the slow client resumes
            let _ = tx.try_send(Err(Status::out_of_range("SLOW_CONSUMER; RESYNC_REQUIRED")));
            false
        }
    }
}

/// Serves the bounded gRPC endpoint.
///
/// # Errors
///
/// Returns an error if binding or serving fails.
pub async fn serve(
    core: CoreHandle,
    addr: std::net::SocketAddr,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    serve_until(core, addr, std::future::pending()).await
}

/// Serves the endpoint until its supplied graceful shutdown future completes.
///
/// # Errors
/// Returns binding or HTTP/2 serving errors.
pub async fn serve_until(
    core: CoreHandle,
    addr: std::net::SocketAddr,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    crate::transport::require_loopback(addr)?;
    serve_with_transport(core, addr, Server::builder(), shutdown).await
}

/// Serves the same bounded application and authorization layer over the selected transport.
///
/// # Errors
/// Returns binding or HTTP/2 serving errors. Callers must validate transport first.
pub async fn serve_with_transport(
    core: CoreHandle,
    addr: std::net::SocketAddr,
    mut server: Server,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    server
        .add_service(
            OrderHubServer::new(OrderHubService::new(core)).max_decoding_message_size(16 * 1024),
        )
        .serve_with_shutdown(addr, shutdown)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legacy_quote_events_are_not_exposed_in_the_persistent_stream() {
        let db = redb::Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap();
        let journal = BusinessJournal::from_database(db).unwrap();
        let credentials = crate::auth::SubmitterCredentials {
            token: "test".into(),
            submitter_id: "owner".into(),
            strategies: vec!["ORDERHUB-S001".into()],
        };
        let mut facts: Vec<crate::inputs::InputEvent> =
            serde_json::from_str(include_str!("../test_data/p4_inputs.json")).unwrap();
        let event = JournalEntry::Input {
            event: Box::new(facts.remove(1)),
            received_at_ns: 1_800_000_000_000_000_000,
        };
        assert!(
            visible_event(&journal, &credentials, event)
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_submitter_burst_is_bounded_and_cancel_keeps_reserved_capacity() {
        use crate::auth::{SubmitterCredentials, SubmitterRegistry};
        use crate::readiness::{Readiness, ReadinessGate};
        use nautilus_common::clock::VirtualClock;
        use nautilus_model::identifiers::TraderId;
        use rust_decimal::Decimal;
        let db = redb::Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap();
        let journal = Arc::new(BusinessJournal::from_database(db).unwrap());
        let record = crate::journal::AdmissionRecord {
            journal_epoch: 0,
            seq: 0,
            submitter_id: "owner".into(),
            request_id: "target".into(),
            request_kind: "SubmitOrder".into(),
            request_fingerprint: "target".into(),
            strategy_id: "STRAT-S001".into(),
            instrument_id: "AAPL.GOX".into(),
            order_side: "BUY".into(),
            quantity: "1".into(),
            price: "10".into(),
            time_in_force: "DAY".into(),
            post_only: false,
            reduce_only: false,
            submit_before_ns: None,
            ts_init_ns: 1,
        };
        let context = crate::ledger::RiskContext {
            account_id: "GOX-001".into(),
            strategy_budget: Decimal::from(100),
            account_budget: Decimal::from(100),
            owned_quantity: Decimal::ZERO,
            owned_cost: Decimal::ZERO,
            account_quantity: Decimal::ZERO,
            strategy_exposure_limit: Decimal::from(100),
            account_exposure_limit: Decimal::from(100),
            reference_price: Decimal::from(10),
            fee_buffer: Decimal::ONE,
            maximum_quantity: Decimal::from(10),
            maximum_notional: Decimal::from(100),
            risk_config_version: "test".into(),
        };
        let target = journal.admit_business(record, Some(context), None).unwrap();
        let id = journal.order(target.seq).unwrap().unwrap().client_order_id;
        let registry = Arc::new(SubmitterRegistry::new(vec![SubmitterCredentials {
            token: "token".into(),
            submitter_id: "owner".into(),
            strategies: vec!["STRAT-S001".into()],
        }]));
        let gate = ReadinessGate::new();
        gate.set(Readiness::Ready, "");
        let (release, blocked) = mpsc::channel();
        let journal_for_core = journal.clone();
        let core_gate = gate.clone();
        let core = CoreHandle::spawn(
            move || {
                blocked.recv().unwrap();
                let cache = Rc::new(RefCell::new(Cache::default()));
                let bridge = OrderHubBridge::new(
                    TraderId::from("ORDER-HUB"),
                    Rc::new(RefCell::new(VirtualClock::new())),
                    cache.clone(),
                    journal_for_core,
                )
                .with_gate(core_gate);
                (bridge, cache, None, None)
            },
            journal.clone(),
            registry,
            gate,
        );
        let request = LimitOrderRequest {
            submitter_id: "owner".into(),
            request_id: "burst".into(),
            strategy_id: StrategyId::from("STRAT-S001"),
            instrument_id: InstrumentId::from("AAPL.GOX"),
            side: OrderSide::Buy,
            quantity: Quantity::from("1"),
            price: Price::from("10"),
            time_in_force: TimeInForce::Day,
            post_only: false,
            reduce_only: false,
            client_id: None,
            submit_before_ns: None,
        };
        let mut submits = Vec::new();
        for index in 0..64 {
            let core = core.clone();
            let mut request = request.clone();
            request.request_id = format!("burst-{index}");
            submits.push(tokio::spawn(async move { core.submit(request).await }));
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while core.admission_limits["owner"].available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            core.submit(request).await,
            Err(BridgeError::Capacity(_))
        ));
        let mut query_replies = Vec::new();
        for _ in 0..128 {
            let (reply, receive) = oneshot::channel();
            core.query_tx
                .try_send(CoreRequest::Snapshot { reply })
                .unwrap();
            query_replies.push(receive);
        }
        let (reply, _) = oneshot::channel();
        assert!(matches!(
            core.query_tx.try_send(CoreRequest::Snapshot { reply }),
            Err(mpsc::TrySendError::Full(_))
        ));
        let cancel_core = core.clone();
        let cancel = tokio::spawn(async move {
            cancel_core
                .cancel(
                    "owner".into(),
                    vec!["STRAT-S001".into()],
                    "cancel".into(),
                    id,
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        release.send(()).unwrap();
        let receipt = tokio::time::timeout(Duration::from_secs(2), cancel)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(receipt.reason, "LOCAL_CANCELED");
        assert_eq!(
            journal.order(target.seq).unwrap().unwrap().reserved_cash,
            "0"
        );
        for submit in submits {
            assert!(matches!(
                submit.await.unwrap(),
                Err(BridgeError::QueueExpired)
            ));
        }
        assert!(
            journal
                .find_request("owner", "SubmitOrder", "burst-0")
                .unwrap()
                .is_none()
        );
    }
}
