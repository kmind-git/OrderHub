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

//! OrderHub node binary.
//!
//! Assembles the service stack: an in-memory kernel (cache, risk engine,
//! execution engine) wired on the core thread to the local FIX exchange
//! client, the group-commit journal, and the tonic server.
//!
//! The local simulated exchange owns matching and execution reports. Connection
//! failure keeps the node HALTED while its diagnostic RPCs remain available.
//!
//! Configuration via environment:
//! - `ORDERHUB_ADDR`          listen address      (default `127.0.0.1:50051`)
//! - `ORDERHUB_JOURNAL`       journal file        (default `./orderhub-journal.redb`)
//! - `ORDERHUB_TOKEN`         shared auth token   (default `orderhub-dev-token`)
//! - `ORDERHUB_EXCHANGE_ADDR` FIX exchange address (default `127.0.0.1:5001`)
//! - `ORDERHUB_INPUT_STDIN`   `1` enables versioned local-fixture JSONL on stdin
//! - `ORDERHUB_CHECKPOINT_DIR` consistent recovery artifact directory
//! - `ORDERHUB_PERSISTENCE`   `async` (default) or `sync` acknowledgment policy

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use nautilus_common::cache::Cache;
use nautilus_common::clock::Clock;
use nautilus_common::live::clock::LiveClock;
use nautilus_common::live::runner::replace_exec_event_sender;
use nautilus_common::messages::ExecutionEvent;
use nautilus_common::runner::{SyncTradingCommandSender, replace_exec_cmd_sender};
use nautilus_model::identifiers::{AccountId, StrategyId, TraderId};
use nautilus_portfolio::Portfolio;
use nautilus_risk::engine::RiskEngine;
use nautilus_risk::engine::config::RiskEngineConfig;

use orderhub_gateway::bridge::OrderHubBridge;
use orderhub_gateway::grpc::CoreHandle;
use orderhub_gateway::journal::{BusinessJournal, PersistenceMode};
use orderhub_gateway::worker::PersistenceWorker;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for variable in ["ORDERHUB_REDIS_ENABLED", "ORDERHUB_POSTGRES_ENABLED"] {
        match std::env::var(variable).as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("0" | "false") => {},
            _ => return Err(format!("{variable}: integration is retained and buildable; real service activation has not been accepted").into()),
        }
    }
    let addr: std::net::SocketAddr = std::env::var("ORDERHUB_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:50051".to_string())
        .parse()?;
    #[cfg(feature = "grpc-tls")]
    let (server, transport_identity) = {
        let cert = orderhub_gateway::transport::read_pem(
            std::env::var_os("ORDERHUB_TLS_CERT").ok_or("grpc-tls requires ORDERHUB_TLS_CERT")?,
        )?;
        let key = orderhub_gateway::transport::read_pem(
            std::env::var_os("ORDERHUB_TLS_KEY").ok_or("grpc-tls requires ORDERHUB_TLS_KEY")?,
        )?;
        let identity = format!("tls/{}", blake3::hash(&cert).to_hex());
        (
            orderhub_gateway::transport::tls_server(cert, key)?,
            identity,
        )
    };
    #[cfg(not(feature = "grpc-tls"))]
    let (server, transport_identity) = {
        if ["ORDERHUB_TLS_CERT", "ORDERHUB_TLS_KEY"]
            .iter()
            .any(|name| std::env::var_os(name).is_some())
        {
            return Err("TLS configuration requires a grpc-tls build".into());
        }
        orderhub_gateway::transport::require_loopback(addr)?;
        (
            tonic::transport::Server::builder(),
            "loopback-plaintext".to_string(),
        )
    };
    let journal_path = std::path::PathBuf::from(
        std::env::var("ORDERHUB_JOURNAL").unwrap_or_else(|_| "orderhub-journal.redb".to_string()),
    );
    let exchange_addr =
        std::env::var("ORDERHUB_EXCHANGE_ADDR").unwrap_or_else(|_| "127.0.0.1:5001".to_string());
    let input_stdin = std::env::var("ORDERHUB_INPUT_STDIN").is_ok_and(|value| value == "1");
    let checkpoint_dir = std::env::var_os("ORDERHUB_CHECKPOINT_DIR").map_or_else(
        || journal_path.with_extension("checkpoints"),
        std::path::PathBuf::from,
    );
    let persistence_setting = match std::env::var("ORDERHUB_PERSISTENCE") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "async".into(),
        Err(e) => return Err(e.into()),
    };
    let persistence_mode = PersistenceMode::parse(&persistence_setting)?;
    // M2: per-submitter credential registry (JSON file of credential list).
    let registry = match std::env::var("ORDERHUB_SUBMITTERS") {
        Ok(path) => {
            let text = std::fs::read_to_string(&path)?;
            Arc::new(serde_json::from_str::<
                orderhub_gateway::auth::SubmitterRegistry,
            >(&text)?)
        }
        Err(_) => Arc::new(orderhub_gateway::auth::SubmitterRegistry::new(vec![
            orderhub_gateway::auth::SubmitterCredentials {
                token: std::env::var("ORDERHUB_TOKEN")
                    .unwrap_or_else(|_| "orderhub-dev-token".into()),
                submitter_id: "trader-1".to_string(),
                strategies: vec!["ORDERHUB-S001".to_string()],
            },
        ])),
    };

    // Readiness lifecycle: BOOTING -> RESTORING -> RECONCILING -> READY.
    let gate = orderhub_gateway::readiness::ReadinessGate::new();
    let journal = match BusinessJournal::open_with_mode(&journal_path, persistence_mode) {
        Ok(journal) => {
            gate.set(orderhub_gateway::readiness::Readiness::Restoring, "");
            journal
        }
        Err(err) => {
            gate.set(
                orderhub_gateway::readiness::Readiness::Halted,
                &format!("journal open failed: {err}"),
            );
            return Err(Box::new(err));
        }
    };
    let journal = Arc::new(journal);
    println!(
        "orderhub-node: persistence={}; ACK durability follows this policy",
        persistence_mode.as_str()
    );
    let recovery = journal.verify_recovery()?;
    println!(
        "orderhub-node: verified recovery {}",
        serde_json::to_string(&recovery)?
    );
    let identity = orderhub_gateway::journal::RecoveryIdentity {
        instance_id: journal.instance_id()?,
        binary_hash: blake3::hash(&std::fs::read(std::env::current_exe()?)?)
            .to_hex()
            .to_string(),
        configuration_hash: blake3::hash(&serde_json::to_vec(&(
            &registry,
            &exchange_addr,
            input_stdin,
            addr,
            &journal_path,
            &checkpoint_dir,
            &transport_identity,
            persistence_mode.as_str(),
            "orderhub-local-v2/envelope-1/recovery-2/quotes-memory-only",
            orderhub_gateway::inputs::LOCAL_RISK_CONFIG_VERSION,
        ))?)
        .to_hex()
        .to_string(),
    };
    let worker = PersistenceWorker::start(Arc::clone(&journal));
    let recorder_worker = worker.clone();
    let gate_for_setup = gate.clone();
    let journal_for_setup = Arc::clone(&journal);
    let last_seq = journal.last_seq()?;
    let recovered = recovery.cursor;
    println!(
        "orderhub-node: journal={} epoch={} last_seq={} recovered_entries={recovered}",
        journal_path.display(),
        journal.epoch(),
        last_seq,
    );
    println!("orderhub-node: exchange={exchange_addr}");
    let core = CoreHandle::spawn(
        move || {
            core_setup(
                journal_for_setup,
                worker,
                recorder_worker,
                gate_for_setup,
                &exchange_addr,
                input_stdin,
            )
        },
        journal,
        registry,
        gate,
    );

    let runtime = tokio::runtime::Runtime::new()?;
    println!("orderhub-node: serving {transport_identity} gRPC on {addr}");
    let shutdown_core = core.clone();
    let shutdown_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let failure_signal = Arc::clone(&shutdown_failed);
    runtime.block_on(async move {
        orderhub_gateway::grpc::serve_with_transport(core, addr, server, async move {
            if let Err(e) = tokio::signal::ctrl_c().await {
                failure_signal.store(true, std::sync::atomic::Ordering::Release);
                eprintln!("shutdown signal failed: {e}");
            }
            match shutdown_core.shutdown(checkpoint_dir, identity).await {
                Ok(path) => println!("orderhub-node: stopped; checkpoint={}", path.display()),
                Err(e) => {
                    failure_signal.store(true, std::sync::atomic::Ordering::Release);
                    eprintln!("orderhub-node: shutdown incomplete: {e}");
                }
            }
        })
        .await
    })?;
    if shutdown_failed.load(std::sync::atomic::Ordering::Acquire) {
        return Err("node shutdown did not complete successfully; see preceding error".into());
    }
    Ok(())
}

/// Builds the kernel on the core thread (see `grpc::CoreHandle::spawn`).
fn core_setup(
    journal: Arc<BusinessJournal>,
    worker: PersistenceWorker,
    recorder_worker: PersistenceWorker,
    gate: std::sync::Arc<orderhub_gateway::readiness::ReadinessGate>,
    exchange_addr: &str,
    input_stdin: bool,
) -> orderhub_gateway::grpc::CoreState {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(LiveClock::default()));
    let portfolio = Portfolio::new(Rc::clone(&clock), Rc::clone(&cache), None);
    let (input_sender, mut inputs) = orderhub_gateway::inputs::InputRuntime::channel(
        Rc::clone(&cache),
        portfolio.clone_shallow(),
        Rc::clone(&clock),
        "local-fixture-v1",
        vec![StrategyId::from("ORDERHUB-S001")],
        orderhub_gateway::inputs::INPUT_QUEUE_CAPACITY,
    )
    .expect("valid local input scope");
    let recovered = orderhub_gateway::recovery::restore_native(&journal, &mut inputs, &cache);
    gate.set(
        orderhub_gateway::readiness::Readiness::Reconciling,
        "local recovery verified; awaiting external evidence",
    );

    if input_stdin {
        println!("orderhub-node: stdin inputs are local fixture facts, not exchange account facts");
        std::thread::Builder::new()
            .name("orderhub-input".into())
            .spawn(move || {
                let stdin = std::io::stdin();

                if let Err(e) =
                    orderhub_gateway::inputs::read_jsonl_inputs(stdin.lock(), input_sender, || {
                        nautilus_core::time::get_atomic_clock_realtime().get_time_ns()
                    })
                {
                    eprintln!("orderhub-node: input adapter stopped: {e}");
                }
            })
            .expect("failed to spawn local input adapter");
    } else {
        println!("orderhub-node: no input source configured; new orders remain blocked");
        drop(input_sender);
    }
    let risk_engine = Rc::new(RefCell::new(RiskEngine::new(
        RiskEngineConfig::default(),
        portfolio,
        Rc::clone(&clock),
        Rc::clone(&cache),
    )));
    RiskEngine::register_msgbus_handlers(&risk_engine);
    // Deferred command dispatch (re-entrancy safe), matching the e2e wiring.
    replace_exec_cmd_sender(Arc::new(SyncTradingCommandSender));
    // Deferred execution events: the local FIX client's reports are forwarded
    // through this channel instead of re-entering the engine synchronously.
    let (exec_tx, exec_rx) = tokio::sync::mpsc::channel::<ExecutionEvent>(1_024);
    replace_exec_event_sender(exec_tx);

    let pump = if let Err(error) = &recovered {
        Err(format!("native restore failed: {error}"))
    } else {
        orderhub_gateway::local_client::try_attach_local_execution(
            exchange_addr,
            TraderId::from("ORDER-HUB"),
            nautilus_model::identifiers::Venue::from("GOX"),
            &AccountId::from("GOX-001"),
            Rc::clone(&clock),
            &cache,
        )
        .map(|pump| Some(pump.with_gate(Arc::clone(&gate))))
        .map_err(|e| e.to_string())
    };
    let pump = match pump {
        Ok(pump) => pump,
        Err(e) => {
            gate.set(orderhub_gateway::readiness::Readiness::Halted, &e);
            None
        }
    };
    orderhub_gateway::exec_wiring::attach_outcome_recorder_with_gate(
        recorder_worker,
        Arc::clone(&gate),
    );

    let quota = Rc::new(RefCell::new(orderhub_gateway::quota::QuotaLedger::new()));
    quota
        .borrow_mut()
        .set_budget("ORDERHUB-S001", rust_decimal::Decimal::from(1_000_000));
    // The bus handlers hold weak references; the node keeps the engine for
    // the process lifetime (a full LiveNode integration owns engines in its
    // trader state instead).
    let initial_readiness = recovered.as_ref().is_ok_and(|report| {
        report.business_orders == 0
            && !report.backup_restore_fenced
            && !report.unclean_async_restart
    });
    if !initial_readiness && gate.get() != orderhub_gateway::readiness::Readiness::Halted {
        gate.set(
            orderhub_gateway::readiness::Readiness::Halted,
            "local adapter lacks complete order/fill queries; external reconciliation required; original IDs and occupancy retained",
        );
    }
    let bridge = OrderHubBridge::new(
        TraderId::from("ORDER-HUB"),
        clock,
        Rc::clone(&cache),
        journal,
    )
    .with_worker(worker)
    .with_quota(quota)
    .with_gate(gate)
    .with_inputs(inputs)
    .with_runtime_owner(risk_engine);
    let bridge = if initial_readiness {
        bridge.with_initial_readiness()
    } else {
        bridge
    };
    (bridge, cache, Some(exec_rx), pump)
}
