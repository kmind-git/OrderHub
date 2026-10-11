//! Isolated tonic client and server TLS transport probe for OrderHub P1.

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, time::Duration};

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use http::{Request, Response, Version};
    use tokio::{net::TcpListener, sync::oneshot, time::timeout};
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{
        body::Body,
        transport::{Certificate, ClientTlsConfig, Endpoint, Identity, Server, ServerTlsConfig},
    };
    use tower::{ServiceExt, service_fn};

    const CERT: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/localhost.der");
    const KEY: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/localhost.pk8");
    const WRONG_CERT: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/wronghost.der");
    const WRONG_KEY: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/wronghost.pk8");

    fn pem(label: &str, der: &[u8]) -> String {
        format!(
            "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
            STANDARD.encode(der)
        )
    }

    async fn server(
        cert: &[u8],
        key: &[u8],
    ) -> (u16, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let identity = Identity::from_pem(pem("CERTIFICATE", cert), pem("PRIVATE KEY", key));
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let service = service_fn(|request: Request<Body>| async move {
                assert_eq!(request.version(), Version::HTTP_2);
                let response = Response::builder()
                    .status(200)
                    .header("content-type", "application/grpc")
                    .header("grpc-status", "12")
                    .body(Body::empty())
                    .unwrap();
                Ok::<_, Infallible>(response)
            });
            Server::builder()
                .tls_config(ServerTlsConfig::new().identity(identity))
                .unwrap()
                .serve_with_incoming_shutdown(service, TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        (port, shutdown_tx, task)
    }

    #[tokio::test]
    async fn tonic_tls_http2_and_wrong_host() {
        rustls_graviola::default_provider()
            .install_default()
            .unwrap();

        let (port, stop, task) = server(CERT, KEY).await;
        let tls = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(pem("CERTIFICATE", CERT)))
            .domain_name("localhost");
        let channel = timeout(
            Duration::from_secs(5),
            Endpoint::from_shared(format!("https://127.0.0.1:{port}"))
                .unwrap()
                .tls_config(tls)
                .unwrap()
                .connect(),
        )
        .await
        .unwrap()
        .unwrap();
        let response = timeout(
            Duration::from_secs(5),
            channel.oneshot(
                Request::builder()
                    .uri("/probe.Service/Call")
                    .header("content-type", "application/grpc")
                    .body(Body::empty())
                    .unwrap(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.version(), Version::HTTP_2);
        assert_eq!(response.headers()["grpc-status"], "12");
        stop.send(()).unwrap();
        task.await.unwrap();

        let (port, stop, task) = server(WRONG_CERT, WRONG_KEY).await;
        let tls = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(pem("CERTIFICATE", WRONG_CERT)))
            .domain_name("localhost");
        let connection = timeout(
            Duration::from_secs(5),
            Endpoint::from_shared(format!("https://127.0.0.1:{port}"))
                .unwrap()
                .tls_config(tls)
                .unwrap()
                .connect(),
        )
        .await
        .unwrap();
        assert!(connection.is_err());
        stop.send(()).unwrap();
        task.await.unwrap();
    }
}
