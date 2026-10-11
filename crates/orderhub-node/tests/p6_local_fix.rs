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

//! Opt-in real node / simulator FIX and HTTP/2 acceptance with owned processes and ports.

use orderhub_proto::orderhub::v1::get_state_request::Query;
use orderhub_proto::orderhub::v1::order_hub_client::OrderHubClient;
use orderhub_proto::orderhub::v1::{
    CancelOrderRequest, GetStateRequest, GetStateResponse, OrderLookup, SnapshotQuery,
    SubmitOrderRequest,
};
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command(path: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    for name in [
        "ORDERHUB_TLS_CERT",
        "ORDERHUB_TLS_KEY",
        "ORDERHUB_TLS_CA",
        "ORDERHUB_TLS_DOMAIN",
        "ORDERHUB_REDIS_ENABLED",
        "ORDERHUB_POSTGRES_ENABLED",
    ] {
        command.env_remove(name);
    }
    command
}

#[cfg(feature = "grpc-tls")]
fn pem(label: &str, der: &[u8]) -> String {
    use base64::Engine as _;
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        base64::engine::general_purpose::STANDARD.encode(der)
    )
}

fn node_command(directory: &Path) -> Command {
    let executable = std::env::var_os("ORDERHUB_TEST_NODE_EXE")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_orderhub-node").into());
    let node = command(executable);
    #[cfg(feature = "grpc-tls")]
    let mut node = node;
    #[cfg(feature = "grpc-tls")]
    {
        let cert = directory.join("tls-cert.pem");
        let key = directory.join("tls-key.pem");
        std::fs::write(
            &cert,
            pem(
                "CERTIFICATE",
                include_bytes!("../../infrastructure/test_data/tls/localhost.der"),
            ),
        )
        .unwrap();
        std::fs::write(
            &key,
            pem(
                "PRIVATE KEY",
                include_bytes!("../../infrastructure/test_data/tls/localhost.pk8"),
            ),
        )
        .unwrap();
        node.env("ORDERHUB_TLS_CERT", cert)
            .env("ORDERHUB_TLS_KEY", key);
    }
    #[cfg(not(feature = "grpc-tls"))]
    let _ = directory;
    node
}

fn port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn auth<T>(value: T) -> tonic::Request<T> {
    let mut request = tonic::Request::new(value);
    request
        .metadata_mut()
        .insert("authorization", "Bearer p6-isolated-token".parse().unwrap());
    request
}

fn start_node(directory: &Path, grpc: u16, fix: u16) -> (Process, Arc<AtomicBool>) {
    start_node_with_quote_refresh(directory, grpc, fix, true)
}

fn start_node_with_quote_refresh(
    directory: &Path,
    grpc: u16,
    fix: u16,
    refresh_quotes: bool,
) -> (Process, Arc<AtomicBool>) {
    let definition_time = directory.join("definition-time.txt");
    let initial_time = if definition_time.exists() {
        std::fs::read_to_string(&definition_time)
            .unwrap()
            .parse::<u64>()
            .unwrap()
    } else {
        let value = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        std::fs::write(definition_time, value.to_string()).unwrap();
        value
    };
    let log = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(directory.join("node.log"))
        .unwrap();
    let mut child = node_command(directory)
        .env_remove("ORDERHUB_SUBMITTERS")
        .env("ORDERHUB_ADDR", format!("127.0.0.1:{grpc}"))
        .env("ORDERHUB_EXCHANGE_ADDR", format!("127.0.0.1:{fix}"))
        .env("ORDERHUB_JOURNAL", directory.join("journal.redb"))
        .env("ORDERHUB_CHECKPOINT_DIR", directory.join("checkpoints"))
        .env("ORDERHUB_TOKEN", "p6-isolated-token")
        .env("ORDERHUB_INPUT_STDIN", "1")
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::spawn(move || {
        let mut sequence = 0_u64;
        while !stopped.load(Ordering::Acquire) {
            sequence += 1;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;
            let mut facts: serde_json::Value = serde_json::from_str(include_str!(
                "../../orderhub-gateway/test_data/p4_inputs.json"
            ))
            .unwrap();
            for fact in facts.as_array_mut().unwrap() {
                if fact["kind"] == "quote" && !refresh_quotes && sequence > 1 {
                    continue;
                }
                fact["meta"]["event_time_ns"] = now.into();
                fact["meta"]["sequence"] = sequence.into();
                if fact["kind"] == "instrument" {
                    fact["effective_time_ns"] = initial_time.into();
                }
                if fact["kind"] == "status" {
                    fact["session_open_ns"] = (now - 60_000_000_000).into();
                    fact["session_close_ns"] = (now + 3_600_000_000_000).into();
                }
                if writeln!(input, "{fact}")
                    .and_then(|()| input.flush())
                    .is_err()
                {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });
    (Process(child), stop)
}

async fn connect(grpc: u16) -> OrderHubClient<tonic::transport::Channel> {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        #[cfg(feature = "grpc-tls")]
        let endpoint = orderhub_gateway::transport::tls_endpoint(
            format!("127.0.0.1:{grpc}").parse().unwrap(),
            pem(
                "CERTIFICATE",
                include_bytes!("../../infrastructure/test_data/tls/localhost.der"),
            )
            .into_bytes(),
            "localhost".into(),
        )
        .unwrap();
        #[cfg(not(feature = "grpc-tls"))]
        let endpoint = orderhub_gateway::transport::loopback_endpoint(
            format!("127.0.0.1:{grpc}").parse().unwrap(),
        )
        .unwrap();
        if let Ok(channel) = endpoint.connect().await {
            let client = OrderHubClient::new(channel);
            return client;
        }
        assert!(Instant::now() < until, "node did not start");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn snapshot(client: &mut OrderHubClient<tonic::transport::Channel>) -> GetStateResponse {
    client
        .get_state(auth(GetStateRequest {
            query: Some(Query::Snapshot(SnapshotQuery {
                page_size: 100,
                page_token: String::new(),
            })),
        }))
        .await
        .unwrap()
        .into_inner()
}

async fn assert_second_owner_blocked(directory: &Path, fix: u16) {
    let log_path = directory.join("second-owner.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut contender = Process(
        node_command(directory)
            .env_remove("ORDERHUB_SUBMITTERS")
            .env("ORDERHUB_ADDR", format!("127.0.0.1:{}", port()))
            .env("ORDERHUB_JOURNAL", directory.join("journal.redb"))
            .env("ORDERHUB_EXCHANGE_ADDR", format!("127.0.0.1:{fix}"))
            .env("ORDERHUB_INPUT_STDIN", "0")
            .env("ORDERHUB_TOKEN", "p6-isolated-token")
            .env(
                "ORDERHUB_CHECKPOINT_DIR",
                directory.join("second-owner-checkpoints"),
            )
            .current_dir(directory)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let until = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = contender.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < until,
            "second owner did not reject active database"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert!(!status.success());
    let message = std::fs::read_to_string(log_path)
        .unwrap()
        .to_lowercase()
        .replace(' ', "");
    assert!(
        message.contains("databasealreadyopen"),
        "wrong rejection: {message}"
    );
    println!("P6_SECOND_PROCESS owner_lock=DENIED original_owner=LIVE");
}

async fn wait_order(
    client: &mut OrderHubClient<tonic::transport::Channel>,
    epoch: &str,
    id: &str,
    status: &str,
) -> orderhub_proto::orderhub::v1::OrderView {
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        let state = client
            .get_state(auth(GetStateRequest {
                query: Some(Query::OrderLookup(OrderLookup {
                    client_order_ids: vec![id.into()],
                })),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(state.journal_epoch, epoch);
        if let Some(order) = state
            .active_orders
            .into_iter()
            .find(|order| order.client_order_id == id && order.status == status)
        {
            return order;
        }
        assert!(Instant::now() < until, "order {id} never reached {status}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn request(id: &str, side: &str, quantity: &str, epoch: &str) -> SubmitOrderRequest {
    SubmitOrderRequest {
        submitter_id: String::new(),
        request_id: id.into(),
        strategy_id: "ORDERHUB-S001".into(),
        instrument_id: "AAPL.GOX".into(),
        side: side.into(),
        quantity: quantity.into(),
        price: "153.22".into(),
        time_in_force: "DAY".into(),
        post_only: false,
        reduce_only: false,
        submit_before_ns: None,
        journal_epoch: epoch.into(),
    }
}

#[tokio::test]
#[ignore = "requires an explicitly supplied ORDERHUB_TEST_EXCHANGE_EXE simulator binary"]
async fn test_real_node_fix_partial_fill_cancel_disconnect_and_restart_stay_safe() {
    let exe =
        std::env::var_os("ORDERHUB_TEST_EXCHANGE_EXE").expect("explicit simulator executable");
    let root = std::env::temp_dir().join("orderhub-p6-local-fix");
    std::fs::create_dir_all(&root).unwrap();
    let dir = tempfile::tempdir_in(root).unwrap();
    let path = dir.keep();
    println!("P6_REAL_FIX_ARTIFACTS={}", path.display());
    let fix = port();
    let rest = port();
    let grpc = port();
    let config = path.join("fix.cfg");
    std::fs::write(&config, format!("[DEFAULT]\nConnectionType=acceptor\nBeginString=FIX.4.2\nHeartBtInt=30\nResetOnLogout=Y\nResetOnDisconnect=Y\nPersistMessages=N\nLogging=N\nDynamicSessions=N\n[SESSION]\nSenderCompID=GOX\nTargetCompID=ORDERHUB\nSocketAcceptPort={fix}\n")).unwrap();
    let instruments = path.join("instruments.txt");
    std::fs::write(&instruments, "1 AAPL\n").unwrap();
    let log = std::fs::File::create(path.join("exchange.log")).unwrap();
    let mut exchange = Process(
        command(&exe)
            .args(["--server", "-fix"])
            .arg(&config)
            .arg("-instruments")
            .arg(&instruments)
            .arg("-port")
            .arg(rest.to_string())
            .current_dir(&path)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let until = Instant::now() + Duration::from_secs(10);
    while std::net::TcpStream::connect(("127.0.0.1", fix)).is_err() {
        assert!(
            exchange.0.try_wait().unwrap().is_none(),
            "exchange failed: {}",
            std::fs::read_to_string(path.join("exchange.log")).unwrap()
        );
        assert!(Instant::now() < until, "exchange failed to bind");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let (mut node, inputs) = start_node_with_quote_refresh(&path, grpc, fix, false);
    let mut client = connect(grpc).await;
    let until = Instant::now() + Duration::from_secs(10);
    let initial = loop {
        let state = snapshot(&mut client).await;
        if state.writable {
            break state;
        }
        assert!(
            Instant::now() < until,
            "node not writable: {}",
            std::fs::read_to_string(path.join("node.log")).unwrap()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    tokio::time::sleep(Duration::from_millis(2_100)).await;
    let after_quote_loss = snapshot(&mut client).await;
    assert!(after_quote_loss.writable);
    assert!(!after_quote_loss.stale);
    let quote: serde_json::Value = after_quote_loss
        .inputs
        .iter()
        .map(|input| serde_json::from_str::<serde_json::Value>(&input.json).unwrap())
        .find(|input| input["kind"] == "quote")
        .expect("committed initial quote remains observable");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let quote_time = quote["meta"]["event_time_ns"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!(now - quote_time > 2_000_000_000);
    let epoch = initial.journal_epoch;
    let mut stream = client
        .watch_events(auth(orderhub_proto::orderhub::v1::WatchEventsRequest {
            journal_epoch: epoch.clone(),
            after: initial.state_cursor,
            resume_token: initial.resume_token,
        }))
        .await
        .unwrap()
        .into_inner();
    let observed_fills = Arc::new(AtomicUsize::new(0));
    let observed = observed_fills.clone();
    let watcher = tokio::spawn(async move {
        while let Ok(Some(event)) = stream.message().await {
            if let Some(orderhub_proto::orderhub::v1::journal_event::Kind::Execution(fact)) =
                event.kind
                && fact.kind == "FILL"
            {
                observed.fetch_add(1, Ordering::Release);
            }
        }
    });
    let mut buy_request = request("p6-buy", "BUY", "10", &epoch);
    buy_request.price = "170.00".into();
    let buy = client
        .submit_order(auth(buy_request.clone()))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(buy.result, "ADMITTED");
    wait_order(&mut client, &epoch, &buy.client_order_id, "ACCEPTED").await;
    let sell = client
        .submit_order(auth(request("p6-sell", "SELL", "4", &epoch)))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(sell.result, "ADMITTED");
    let partial = wait_order(
        &mut client,
        &epoch,
        &buy.client_order_id,
        "PARTIALLY_FILLED",
    )
    .await;
    assert_eq!(partial.filled_quantity, "4");
    wait_order(&mut client, &epoch, &sell.client_order_id, "FILLED").await;
    let cancel = client
        .cancel_order(auth(CancelOrderRequest {
            submitter_id: String::new(),
            request_id: "p6-cancel".into(),
            client_order_id: buy.client_order_id.clone(),
            journal_epoch: epoch.clone(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(cancel.result, "CANCEL_INTENT_ACCEPTED");
    wait_order(&mut client, &epoch, &buy.client_order_id, "CANCELED").await;
    let until = Instant::now() + Duration::from_secs(5);
    while observed_fills.load(Ordering::Acquire) < 2 {
        assert!(
            Instant::now() < until,
            "wire event stream omitted committed fills"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let retry = client
        .submit_order(auth(buy_request.clone()))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(retry.client_order_id, buy.client_order_id);
    assert!(retry.already_admitted);
    exchange.0.kill().unwrap();
    exchange.0.wait().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let state = snapshot(&mut client).await;
        if state.readiness == "HALTED" {
            assert!(!state.writable);
            break;
        }
        assert!(Instant::now() < until, "disconnect did not close gate");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    inputs.store(true, Ordering::Release);
    node.0.kill().unwrap();
    node.0.wait().unwrap();
    drop(client);
    watcher.abort();
    // Reconnect to a fresh isolated simulator with no historical query coverage
    let log = std::fs::OpenOptions::new()
        .append(true)
        .open(path.join("exchange.log"))
        .unwrap();
    let mut second_exchange = Process(
        command(&exe)
            .args(["--server", "-fix"])
            .arg(&config)
            .arg("-instruments")
            .arg(&instruments)
            .arg("-port")
            .arg(rest.to_string())
            .current_dir(&path)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let until = Instant::now() + Duration::from_secs(10);
    while std::net::TcpStream::connect(("127.0.0.1", fix)).is_err() {
        assert!(
            second_exchange.0.try_wait().unwrap().is_none(),
            "restarted exchange exited"
        );
        assert!(Instant::now() < until, "restarted exchange failed to bind");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let (_second_node, second_inputs) = start_node(&path, grpc, fix);
    let mut second = connect(grpc).await;
    let state = snapshot(&mut second).await;
    assert_eq!(state.journal_epoch, epoch);
    assert!(!state.writable);
    assert_eq!(state.readiness, "HALTED");
    let original = second
        .submit_order(auth(buy_request))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(original.client_order_id, buy.client_order_id);
    assert!(original.already_admitted);
    let terminal = wait_order(&mut second, &epoch, &buy.client_order_id, "CANCELED").await;
    assert_eq!(terminal.filled_quantity, "4");
    second_inputs.store(true, Ordering::Release);
    println!(
        "P6_REAL_FIX binary={} blake3={} fix={fix} grpc={grpc} partial=4 cancel=CANCELED restart=HALTED original_id={}",
        Path::new(&exe).display(),
        blake3::hash(&std::fs::read(&exe).unwrap()),
        buy.client_order_id
    );
    // Preserve owned logs, database, and configuration as run evidence
    println!("P6_REAL_FIX_ARTIFACTS={}", path.display());
}

#[tokio::test]
#[ignore = "requires an explicitly supplied ORDERHUB_TEST_EXCHANGE_EXE simulator binary"]
async fn test_real_node_killed_after_rpc_ack_retains_uncertain_order_and_original_result() {
    let exe =
        std::env::var_os("ORDERHUB_TEST_EXCHANGE_EXE").expect("explicit simulator executable");
    let root = std::env::temp_dir().join("orderhub-p6-local-fix");
    let path = tempfile::tempdir_in(root).unwrap().keep();
    println!("P6_RPC_CRASH_ARTIFACTS={}", path.display());
    let fix = port();
    let rest = port();
    let grpc = port();
    let config = path.join("fix.cfg");
    std::fs::write(&config, format!("[DEFAULT]\nConnectionType=acceptor\nBeginString=FIX.4.2\nHeartBtInt=30\nResetOnLogout=Y\nResetOnDisconnect=Y\nPersistMessages=N\nLogging=N\nDynamicSessions=N\n[SESSION]\nSenderCompID=GOX\nTargetCompID=ORDERHUB\nSocketAcceptPort={fix}\n")).unwrap();
    let instruments = path.join("instruments.txt");
    std::fs::write(&instruments, "1 AAPL\n").unwrap();
    let log = std::fs::File::create(path.join("exchange.log")).unwrap();
    let mut exchange = Process(
        command(&exe)
            .args(["--server", "-fix"])
            .arg(&config)
            .arg("-instruments")
            .arg(&instruments)
            .arg("-port")
            .arg(rest.to_string())
            .current_dir(&path)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let until = Instant::now() + Duration::from_secs(10);
    while std::net::TcpStream::connect(("127.0.0.1", fix)).is_err() {
        assert!(exchange.0.try_wait().unwrap().is_none(), "exchange exited");
        assert!(Instant::now() < until, "exchange did not bind");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let (mut node, inputs) = start_node(&path, grpc, fix);
    let mut client = connect(grpc).await;
    let until = Instant::now() + Duration::from_secs(10);
    let epoch = loop {
        let state = snapshot(&mut client).await;
        if state.writable {
            break state.journal_epoch;
        }
        assert!(Instant::now() < until, "node was not ready");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_second_owner_blocked(&path, fix).await;
    let response = client
        .submit_order(auth(request("crash-after-ack", "BUY", "3", &epoch)))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(response.result, "ADMITTED");
    node.0.kill().unwrap();
    node.0.wait().unwrap();
    inputs.store(true, Ordering::Release);
    drop(client);
    let journal =
        orderhub_gateway::journal::BusinessJournal::open(&path.join("journal.redb")).unwrap();
    let report = journal.verify_recovery().unwrap();
    assert_eq!(
        report.uncertain_orders,
        vec![response.client_order_id.clone()]
    );
    let sequence = orderhub_gateway::exec_wiring::seq_from_client_order_id(
        &nautilus_model::identifiers::ClientOrderId::from(response.client_order_id.as_str()),
    )
    .unwrap();
    let original = journal.order(sequence).unwrap().unwrap();
    assert_eq!(original.filled_quantity, "0");
    assert_eq!(
        original
            .reserved_cash
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        rust_decimal::Decimal::new(46066, 2)
    );
    drop(journal);
    let (_second_node, second_inputs) = start_node(&path, grpc, fix);
    let mut second = connect(grpc).await;
    let state = snapshot(&mut second).await;
    assert_eq!(state.readiness, "HALTED");
    assert!(!state.writable);
    let retry = second
        .submit_order(auth(request("crash-after-ack", "BUY", "3", &epoch)))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(retry.client_order_id, response.client_order_id);
    assert!(retry.already_admitted);
    let blocked = second
        .submit_order(auth(request("must-not-resend", "BUY", "1", &epoch)))
        .await
        .unwrap_err();
    assert_eq!(blocked.code(), tonic::Code::Unavailable);
    second_inputs.store(true, Ordering::Release);
    println!(
        "P6_RPC_ACK_KILL original_id={} retained_cash=460.66 restart=HALTED fresh_submit=UNAVAILABLE",
        response.client_order_id
    );
}
