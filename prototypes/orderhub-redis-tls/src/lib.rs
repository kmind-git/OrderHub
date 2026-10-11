//! Isolated Redis TLS provider probe for OrderHub P1.

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use nautilus_infrastructure::redis::{cache::RedisCacheConfig, create_redis_connection};
    use redis::{Client, TlsCertificates};
    use rustls::{
        ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    };
    use tokio::{io::AsyncReadExt, net::TcpListener, task::JoinHandle, time::timeout};
    use tokio_rustls::TlsAcceptor;

    const CERT: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/localhost.der");
    const KEY: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/localhost.pk8");
    const WRONG_CERT: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/wronghost.der");
    const WRONG_KEY: &[u8] = include_bytes!("../../orderhub-no-c-tls/fixtures/wronghost.pk8");

    #[derive(Clone, Copy)]
    enum ServerMode {
        Tls(&'static [u8], &'static [u8]),
        Plain,
    }

    async fn fake_redis(mode: ServerMode) -> (u16, JoinHandle<Result<bool, String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            if matches!(mode, ServerMode::Plain) {
                let mut first = [0];
                socket
                    .read_exact(&mut first)
                    .await
                    .map_err(|e| e.to_string())?;
                return Ok(first[0] == b'*');
            }

            let ServerMode::Tls(cert, key) = mode else {
                unreachable!();
            };
            let config =
                ServerConfig::builder_with_provider(Arc::new(rustls_graviola::default_provider()))
                    .with_safe_default_protocol_versions()
                    .unwrap()
                    .with_no_client_auth()
                    .with_single_cert(
                        vec![CertificateDer::from(cert.to_vec())],
                        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.to_vec())),
                    )
                    .map_err(|e| e.to_string())?;
            let acceptor = TlsAcceptor::from(Arc::new(config));
            match timeout(Duration::from_secs(5), acceptor.accept(socket)).await {
                Ok(Ok(mut stream)) => {
                    let mut first = [0];
                    let read = timeout(Duration::from_secs(5), stream.read(&mut first))
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())?;
                    Ok(read > 0 && first[0] == b'*')
                }
                Ok(Err(_)) => Ok(false),
                Err(e) => Err(e.to_string()),
            }
        });
        (port, task)
    }

    fn root_pem(cert: &[u8]) -> Vec<u8> {
        format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            STANDARD.encode(cert)
        )
        .into_bytes()
    }

    async fn connect(port: u16, tls: bool, root: &[u8]) -> String {
        let client = if tls {
            Client::build_with_tls(
                format!("rediss://127.0.0.1:{port}/"),
                TlsCertificates {
                    client_tls: None,
                    root_cert: Some(root_pem(root)),
                },
            )
            .unwrap()
        } else {
            Client::open(format!("redis://127.0.0.1:{port}/")).unwrap()
        };
        timeout(
            Duration::from_secs(5),
            client.get_multiplexed_async_connection(),
        )
        .await
        .expect("Redis connection timed out")
        .expect_err("fake server must not complete Redis connection")
        .to_string()
    }

    #[tokio::test]
    async fn redis_tls_and_plain_modes() {
        let (port, server) = fake_redis(ServerMode::Tls(CERT, KEY)).await;
        let config = RedisCacheConfig {
            host: Some("127.0.0.1".to_string()),
            port: Some(port),
            ssl: true,
            number_of_retries: 0,
            connection_timeout: 5,
            ..RedisCacheConfig::default()
        };
        let error = timeout(
            Duration::from_secs(5),
            create_redis_connection("tls-probe", &config),
        )
        .await
        .expect("production Redis TLS connection timed out")
        .expect_err("untrusted certificate must fail")
        .to_string();
        assert!(error.to_lowercase().contains("certificate"), "{error}");
        assert!(!server.await.unwrap().unwrap());

        let (port, server) = fake_redis(ServerMode::Tls(CERT, KEY)).await;
        let _ = connect(port, true, CERT).await;
        assert!(
            server.await.unwrap().unwrap(),
            "valid TLS did not reach Redis protocol"
        );

        let (port, server) = fake_redis(ServerMode::Tls(WRONG_CERT, WRONG_KEY)).await;
        let error = connect(port, true, WRONG_CERT).await;
        assert!(error.to_lowercase().contains("certificate"), "{error}");
        assert!(!server.await.unwrap().unwrap());

        let (port, server) = fake_redis(ServerMode::Plain).await;
        let _ = connect(port, false, CERT).await;
        assert!(
            server.await.unwrap().unwrap(),
            "plain mode did not send Redis protocol"
        );
    }
}
