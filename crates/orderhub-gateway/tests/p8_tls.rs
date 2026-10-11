// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

#![cfg(feature = "grpc-tls")]

use std::{cell::RefCell, net::SocketAddr, rc::Rc, sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use nautilus_common::{cache::Cache, clock::VirtualClock};
use nautilus_model::identifiers::TraderId;
use orderhub_gateway::{
    auth::{SubmitterCredentials, SubmitterRegistry},
    bridge::OrderHubBridge,
    grpc::{CoreHandle, serve_with_transport},
    journal::BusinessJournal,
    readiness::ReadinessGate,
    transport::{tls_endpoint, tls_server},
};
use orderhub_proto::orderhub::v1::{GetStateRequest, order_hub_client::OrderHubClient};
use tokio::sync::oneshot;

const CERT: &[u8] = include_bytes!("../../infrastructure/test_data/tls/localhost.der");
const KEY: &[u8] = include_bytes!("../../infrastructure/test_data/tls/localhost.pk8");
const WRONG: &[u8] = include_bytes!("../../infrastructure/test_data/tls/wronghost.der");
const EXPIRED: &[u8] = include_bytes!("../../infrastructure/test_data/tls/expired.der");
const EXPIRED_KEY: &[u8] = include_bytes!("../../infrastructure/test_data/tls/expired.pk8");

fn pem(label: &str, data: &[u8]) -> Vec<u8> {
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        STANDARD.encode(data)
    )
    .into_bytes()
}

fn core() -> CoreHandle {
    let db = redb::Database::builder()
        .create_with_backend(redb::backends::InMemoryBackend::new())
        .unwrap();
    let journal = Arc::new(BusinessJournal::from_database(db).unwrap());
    let setup_journal = journal.clone();
    let registry = Arc::new(SubmitterRegistry::new(vec![SubmitterCredentials {
        token: "tls-owner".into(),
        submitter_id: "owner".into(),
        strategies: vec!["ORDERHUB-S001".into()],
    }]));
    CoreHandle::spawn(
        move || {
            let cache = Rc::new(RefCell::new(Cache::default()));
            let clock = Rc::new(RefCell::new(VirtualClock::new()));
            let bridge = OrderHubBridge::new(
                TraderId::from("ORDER-HUB"),
                clock,
                cache.clone(),
                setup_journal,
            );
            (bridge, cache, None, None)
        },
        journal,
        registry,
        ReadinessGate::new(),
    )
}

async fn server(
    cert: &[u8],
    key: &[u8],
) -> (SocketAddr, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let addr = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap();
    let server = tls_server(pem("CERTIFICATE", cert), pem("PRIVATE KEY", key)).unwrap();
    let (stop, stopped) = oneshot::channel();
    let core = core();
    let task = tokio::spawn(async move {
        serve_with_transport(core, addr, server, async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });
    // Await the listener, not an arbitrary sleep or a successful TLS result.
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while tokio::net::TcpStream::connect(addr).await.is_err() {
        assert!(std::time::Instant::now() < until);
        tokio::task::yield_now().await;
    }
    (addr, stop, task)
}

#[tokio::test]
async fn verified_tls_uses_the_real_service_and_keeps_bearer_authorization() {
    let (addr, stop, task) = server(CERT, KEY).await;
    let channel = tls_endpoint(addr, pem("CERTIFICATE", CERT), "localhost".into())
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = OrderHubClient::new(channel);
    assert_eq!(
        client
            .get_state(GetStateRequest::default())
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
    let mut request = tonic::Request::new(GetStateRequest::default());
    request
        .metadata_mut()
        .insert("authorization", "Bearer tls-owner".parse().unwrap());
    let state = client.get_state(request).await.unwrap().into_inner();
    assert!(!state.journal_epoch.is_empty());
    stop.send(()).unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn certificate_failures_and_plaintext_downgrade_are_rejected() {
    for (cert, key, root, name) in [
        (CERT, KEY, CERT, "wrong.example"),
        (CERT, KEY, WRONG, "localhost"),
        (EXPIRED, EXPIRED_KEY, EXPIRED, "localhost"),
    ] {
        let (addr, stop, task) = server(cert, key).await;
        let result = tls_endpoint(addr, pem("CERTIFICATE", root), name.into())
            .unwrap()
            .connect()
            .await;
        assert!(result.is_err(), "invalid certificate reached the service");
        stop.send(()).unwrap();
        task.await.unwrap();
    }
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let plaintext = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let (mut stream, _) = listener.accept().await.unwrap();
        stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
    });
    assert!(
        tls_endpoint(addr, pem("CERTIFICATE", CERT), "localhost".into())
            .unwrap()
            .connect()
            .await
            .is_err()
    );
    plaintext.await.unwrap();
}

#[test]
fn invalid_identity_is_rejected_before_serving() {
    assert!(tls_server(Vec::new(), Vec::new()).is_err());
    assert!(tls_server(pem("CERTIFICATE", CERT), pem("PRIVATE KEY", EXPIRED_KEY)).is_err());
    assert!(
        tls_endpoint(
            "127.0.0.1:1".parse().unwrap(),
            Vec::new(),
            "localhost".into()
        )
        .is_err()
    );
}
