// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

#![cfg(feature = "tls")]

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use rustls::{
        ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    };

    const ECDSA_CERT: &[u8] = include_bytes!("../test_data/tls/localhost.der");
    const ECDSA_KEY: &[u8] = include_bytes!("../test_data/tls/localhost.pk8");
    const EXPIRED_CERT: &[u8] = include_bytes!("../test_data/tls/expired.der");
    const EXPIRED_KEY: &[u8] = include_bytes!("../test_data/tls/expired.pk8");
    const RSA_CERT: &[u8] = include_bytes!("../test_data/tls/rsa.der");
    const RSA_KEY: &[u8] = include_bytes!("../test_data/tls/rsa.pk8");

    #[test]
    fn host_has_required_graviola_cpu_features() {
        nautilus_infrastructure::tls::ensure_provider().unwrap();
    }

    fn connections(
        cert: &[u8],
        key: &[u8],
        trusted: bool,
        host: &'static str,
        version: &'static rustls::SupportedProtocolVersion,
    ) -> (ClientConnection, ServerConnection) {
        let mut roots = RootCertStore::empty();
        if trusted {
            roots.add(CertificateDer::from(cert.to_vec())).unwrap();
        }

        let client =
            ClientConfig::builder_with_provider(nautilus_infrastructure::tls::provider().unwrap())
                .with_protocol_versions(&[version])
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth();
        let server =
            ServerConfig::builder_with_provider(nautilus_infrastructure::tls::provider().unwrap())
                .with_protocol_versions(&[version])
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from(cert.to_vec())],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.to_vec())),
                )
                .unwrap();

        (
            ClientConnection::new(Arc::new(client), ServerName::try_from(host).unwrap()).unwrap(),
            ServerConnection::new(Arc::new(server)).unwrap(),
        )
    }

    fn handshake(
        client: &mut ClientConnection,
        server: &mut ServerConnection,
    ) -> Result<(), String> {
        for _ in 0..20 {
            let mut client_bytes = Vec::new();
            client
                .write_tls(&mut client_bytes)
                .map_err(|e| e.to_string())?;
            if !client_bytes.is_empty() {
                server
                    .read_tls(&mut Cursor::new(client_bytes))
                    .map_err(|e| e.to_string())?;
                server.process_new_packets().map_err(|e| e.to_string())?;
            }

            let mut server_bytes = Vec::new();
            server
                .write_tls(&mut server_bytes)
                .map_err(|e| e.to_string())?;
            if !server_bytes.is_empty() {
                client
                    .read_tls(&mut Cursor::new(server_bytes))
                    .map_err(|e| e.to_string())?;
                client.process_new_packets().map_err(|e| e.to_string())?;
            }

            if !client.is_handshaking() && !server.is_handshaking() {
                return Ok(());
            }
        }
        Err("handshake did not complete".to_owned())
    }

    #[test]
    fn ecdsa_tls13_handshake() {
        let (mut client, mut server) = connections(
            ECDSA_CERT,
            ECDSA_KEY,
            true,
            "localhost",
            &rustls::version::TLS13,
        );
        handshake(&mut client, &mut server).unwrap();
        assert_eq!(
            client.protocol_version(),
            Some(rustls::ProtocolVersion::TLSv1_3)
        );
    }

    #[test]
    fn ecdsa_tls12_handshake() {
        let (mut client, mut server) = connections(
            ECDSA_CERT,
            ECDSA_KEY,
            true,
            "localhost",
            &rustls::version::TLS12,
        );
        handshake(&mut client, &mut server).unwrap();
        assert_eq!(
            client.protocol_version(),
            Some(rustls::ProtocolVersion::TLSv1_2)
        );
    }

    #[test]
    fn rsa_tls13_handshake() {
        let (mut client, mut server) = connections(
            RSA_CERT,
            RSA_KEY,
            true,
            "localhost",
            &rustls::version::TLS13,
        );
        handshake(&mut client, &mut server).unwrap();
    }

    #[test]
    fn rsa_tls12_handshake() {
        let (mut client, mut server) = connections(
            RSA_CERT,
            RSA_KEY,
            true,
            "localhost",
            &rustls::version::TLS12,
        );
        handshake(&mut client, &mut server).unwrap();
    }

    #[test]
    fn wrong_hostname_is_rejected() {
        let (mut client, mut server) = connections(
            ECDSA_CERT,
            ECDSA_KEY,
            true,
            "wrong.example",
            &rustls::version::TLS13,
        );
        let error = handshake(&mut client, &mut server).unwrap_err();
        assert!(error.contains("certificate"), "{error}");
    }

    #[test]
    fn untrusted_certificate_is_rejected() {
        let (mut client, mut server) = connections(
            ECDSA_CERT,
            ECDSA_KEY,
            false,
            "localhost",
            &rustls::version::TLS13,
        );
        let error = handshake(&mut client, &mut server).unwrap_err();
        assert!(error.contains("certificate"), "{error}");
    }

    #[test]
    fn expired_certificate_is_rejected() {
        let (mut client, mut server) = connections(
            EXPIRED_CERT,
            EXPIRED_KEY,
            true,
            "localhost",
            &rustls::version::TLS13,
        );
        let error = handshake(&mut client, &mut server).unwrap_err();
        assert!(error.contains("certificate"), "{error}");
    }
}
