//! Isolated HTTPS provider probe for OrderHub P1.

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use rustls::{
        ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::timeout,
    };
    use tokio_rustls::TlsAcceptor;

    const CERT: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/localhost.der");
    const KEY: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/localhost.pk8");
    const WRONG_CERT: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/wronghost.der");
    const WRONG_KEY: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/wronghost.pk8");

    async fn serve_once(
        cert: &'static [u8],
        key: &'static [u8],
    ) -> (u16, tokio::task::JoinHandle<bool>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let config =
                ServerConfig::builder_with_provider(Arc::new(rustls_graviola::default_provider()))
                    .with_safe_default_protocol_versions()
                    .unwrap()
                    .with_no_client_auth()
                    .with_single_cert(
                        vec![CertificateDer::from(cert.to_vec())],
                        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.to_vec())),
                    )
                    .unwrap();
            let Ok(mut stream) = TlsAcceptor::from(Arc::new(config)).accept(socket).await else {
                return false;
            };
            let mut request = [0u8; 4096];
            let Ok(n) = timeout(Duration::from_secs(5), stream.read(&mut request))
                .await
                .unwrap()
            else {
                return false;
            };
            let is_get = request[..n].starts_with(b"GET /probe HTTP/1.1");
            if is_get {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    )
                    .await
                    .unwrap();
            }
            is_get
        });
        (port, task)
    }

    #[tokio::test]
    async fn https_uses_custom_provider_and_rejects_wrong_host() {
        rustls_graviola::default_provider()
            .install_default()
            .unwrap();
        let cert = reqwest::Certificate::from_der(CERT).unwrap();
        let client = reqwest::Client::builder()
            .add_root_certificate(cert)
            .add_root_certificate(reqwest::Certificate::from_der(WRONG_CERT).unwrap())
            .no_proxy()
            .build()
            .unwrap();
        let (port, server) = serve_once(CERT, KEY).await;
        let response = client
            .get(format!("https://localhost:{port}/probe"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "ok");
        assert!(server.await.unwrap());

        let (port, server) = serve_once(WRONG_CERT, WRONG_KEY).await;
        let result = client
            .get(format!("https://localhost:{port}/probe"))
            .send()
            .await;
        assert!(result.is_err());
        assert!(!server.await.unwrap());
    }
}
