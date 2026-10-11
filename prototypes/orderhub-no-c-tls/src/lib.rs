//! Isolated TLS backend probe for the OrderHub P1 gate.

#[cfg(target_arch = "x86_64")]
pub fn required_cpu_features() -> Result<(), &'static str> {
    first_missing_required_cpu_feature([
        std::is_x86_feature_detected!("aes"),
        std::is_x86_feature_detected!("ssse3"),
        std::is_x86_feature_detected!("avx"),
        std::is_x86_feature_detected!("avx2"),
        std::is_x86_feature_detected!("adx"),
        std::is_x86_feature_detected!("bmi2"),
        std::is_x86_feature_detected!("pclmulqdq"),
    ])
}

#[cfg(target_arch = "x86_64")]
fn first_missing_required_cpu_feature(available: [bool; 7]) -> Result<(), &'static str> {
    for (available, feature) in
        available
            .into_iter()
            .zip(["aes", "ssse3", "avx", "avx2", "adx", "bmi2", "pclmulqdq"])
    {
        if !available {
            return Err(feature);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use rustls::{
        ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    };

    const ECDSA_CERT: &[u8] = include_bytes!("../fixtures/localhost.der");
    const ECDSA_KEY: &[u8] = include_bytes!("../fixtures/localhost.pk8");
    const EXPIRED_CERT: &[u8] = include_bytes!("../fixtures/expired.der");
    const EXPIRED_KEY: &[u8] = include_bytes!("../fixtures/expired.pk8");
    const RSA_CERT: &[u8] = include_bytes!("../fixtures/rsa.der");
    const RSA_KEY: &[u8] = include_bytes!("../fixtures/rsa.pk8");

    #[test]
    fn host_has_required_graviola_cpu_features() {
        super::required_cpu_features().unwrap();
    }

    #[test]
    fn missing_required_graviola_cpu_feature_is_rejected() {
        let features = ["aes", "ssse3", "avx", "avx2", "adx", "bmi2", "pclmulqdq"];
        assert_eq!(super::first_missing_required_cpu_feature([true; 7]), Ok(()));

        for (index, feature) in features.into_iter().enumerate() {
            let mut available = [true; 7];
            available[index] = false;
            assert_eq!(
                super::first_missing_required_cpu_feature(available),
                Err(feature)
            );
        }
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
            ClientConfig::builder_with_provider(Arc::new(rustls_graviola::default_provider()))
                .with_protocol_versions(&[version])
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth();
        let server =
            ServerConfig::builder_with_provider(Arc::new(rustls_graviola::default_provider()))
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
