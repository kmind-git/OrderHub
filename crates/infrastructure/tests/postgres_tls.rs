// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

#![cfg(feature = "postgres-tls")]

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use rustls::{
        ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    };
    use sqlx::{
        Connection, PgConnection,
        postgres::{PgConnectOptions, PgSslMode},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task::JoinHandle,
        time::timeout,
    };
    use tokio_rustls::TlsAcceptor;

    const CERT: &[u8] = include_bytes!("../test_data/tls/localhost.der");
    const KEY: &[u8] = include_bytes!("../test_data/tls/localhost.pk8");
    const WRONG_CERT: &[u8] = include_bytes!("../test_data/tls/wronghost.der");
    const WRONG_KEY: &[u8] = include_bytes!("../test_data/tls/wronghost.pk8");
    const SSL_REQUEST: [u8; 8] = [0, 0, 0, 8, 4, 210, 22, 47];

    #[derive(Clone, Copy)]
    enum ServerMode {
        RefuseTls,
        Tls(&'static [u8], &'static [u8]),
        Plain,
    }

    async fn fake_postgres(mode: ServerMode) -> (u16, JoinHandle<Result<bool, String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            let mut first = [0; 8];
            socket
                .read_exact(&mut first)
                .await
                .map_err(|e| e.to_string())?;

            if matches!(mode, ServerMode::Plain) {
                return Ok(first != SSL_REQUEST);
            }
            if first != SSL_REQUEST {
                return Err("SQLx did not send a PostgreSQL SSLRequest".to_owned());
            }
            if matches!(mode, ServerMode::RefuseTls) {
                socket.write_all(b"N").await.map_err(|e| e.to_string())?;
                return Ok(false);
            }

            let ServerMode::Tls(cert, key) = mode else {
                unreachable!();
            };
            socket.write_all(b"S").await.map_err(|e| e.to_string())?;
            let config = ServerConfig::builder_with_provider(
                nautilus_infrastructure::tls::provider().unwrap(),
            )
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
                    let mut startup = [0; 4];
                    let read = timeout(Duration::from_secs(5), stream.read(&mut startup))
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())?;
                    Ok(read > 0)
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

    fn options(port: u16, mode: PgSslMode, root: &[u8]) -> PgConnectOptions {
        let options = PgConnectOptions::new()
            .host("127.0.0.1")
            .port(port)
            .username("test")
            .ssl_mode(mode)
            .ssl_root_cert_from_pem(root_pem(root));
        let options = if matches!(mode, PgSslMode::VerifyFull) {
            nautilus_infrastructure::sql::pg::verified_tls_options(options, root_pem(root)).unwrap()
        } else {
            options
        };
        nautilus_infrastructure::sql::pg::validate_transport(&options).unwrap();
        options
    }

    async fn connect(options: &PgConnectOptions) -> String {
        let result = timeout(Duration::from_secs(5), PgConnection::connect_with(options))
            .await
            .expect("SQLx connection timed out");
        result
            .expect_err("fake server must not complete PostgreSQL authentication")
            .to_string()
    }

    #[tokio::test]
    async fn postgres_tls_provider_and_certificate_paths() {
        nautilus_infrastructure::tls::ensure_provider().unwrap();

        let (port, server) = fake_postgres(ServerMode::Tls(CERT, KEY)).await;
        let _ = connect(&options(port, PgSslMode::VerifyFull, CERT)).await;
        assert!(
            server.await.unwrap().unwrap(),
            "valid TLS handshake did not reach startup"
        );

        let (port, server) = fake_postgres(ServerMode::Tls(WRONG_CERT, WRONG_KEY)).await;
        let error = connect(&options(port, PgSslMode::VerifyFull, WRONG_CERT)).await;
        assert!(error.to_lowercase().contains("certificate"), "{error}");
        assert!(!server.await.unwrap().unwrap());

        let (port, server) = fake_postgres(ServerMode::RefuseTls).await;
        let error = connect(&options(port, PgSslMode::VerifyFull, CERT)).await;
        assert!(error.contains("does not support TLS"), "{error}");
        assert!(!server.await.unwrap().unwrap());

        let (port, server) = fake_postgres(ServerMode::Plain).await;
        let _ = connect(&options(port, PgSslMode::Disable, CERT)).await;
        assert!(
            server.await.unwrap().unwrap(),
            "plain mode sent an SSLRequest"
        );
    }
}
