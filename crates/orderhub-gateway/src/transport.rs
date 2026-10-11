// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

//! Explicit loopback plaintext and verified TLS transports. No automatic fallback.

use std::net::SocketAddr;

/// Reads a certificate/key file with a one MiB limit, including concurrent growth.
///
/// # Errors
/// Returns file errors or rejects an oversized/empty configuration.
#[cfg(feature = "grpc-tls")]
pub fn read_pem(path: impl AsRef<std::path::Path>) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    let mut value = Vec::new();
    std::fs::File::open(path)?
        .take(1_048_577)
        .read_to_end(&mut value)?;
    anyhow::ensure!(
        !value.is_empty() && value.len() <= 1_048_576,
        "TLS PEM files must contain 1..=1048576 bytes"
    );
    Ok(value)
}

/// Validates the local plaintext listener/client boundary.
///
/// # Errors
/// Returns an error for any address outside loopback.
pub fn require_loopback(addr: SocketAddr) -> anyhow::Result<()> {
    anyhow::ensure!(
        addr.ip().is_loopback(),
        "plaintext gRPC is restricted to explicit loopback addresses"
    );
    Ok(())
}

/// Builds the explicit local plaintext endpoint with bounded connection time.
///
/// # Errors
/// Returns address or loopback policy errors.
pub fn loopback_endpoint(addr: SocketAddr) -> anyhow::Result<tonic::transport::Endpoint> {
    require_loopback(addr)?;
    Ok(
        tonic::transport::Endpoint::from_shared(format!("http://{addr}"))?
            .connect_timeout(std::time::Duration::from_secs(5)),
    )
}

/// Builds and validates a TLS server before listeners or business state are opened.
///
/// # Errors
/// Returns provider, certificate, key, or TLS configuration errors.
#[cfg(feature = "grpc-tls")]
pub fn tls_server(cert_pem: Vec<u8>, key_pem: Vec<u8>) -> anyhow::Result<tonic::transport::Server> {
    nautilus_infrastructure::tls::ensure_provider()?;
    let identity = tonic::transport::Identity::from_pem(cert_pem, key_pem);
    Ok(tonic::transport::Server::builder()
        .tls_config(tonic::transport::ServerTlsConfig::new().identity(identity))?)
}

/// Builds an HTTPS endpoint with an explicit trust bundle and verified DNS identity.
///
/// # Errors
/// Returns missing trust/identity, provider or TLS configuration errors.
#[cfg(feature = "grpc-tls")]
pub fn tls_endpoint(
    addr: SocketAddr,
    roots_pem: Vec<u8>,
    domain: String,
) -> anyhow::Result<tonic::transport::Endpoint> {
    anyhow::ensure!(
        !roots_pem.is_empty() && !domain.is_empty(),
        "gRPC TLS requires an explicit trust bundle and server identity"
    );
    nautilus_infrastructure::tls::ensure_provider()?;
    let tls = tonic::transport::ClientTlsConfig::new()
        .ca_certificate(tonic::transport::Certificate::from_pem(roots_pem))
        .domain_name(domain);
    Ok(
        tonic::transport::Endpoint::from_shared(format!("https://{addr}"))?
            .connect_timeout(std::time::Duration::from_secs(5))
            .tls_config(tls)?,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn plaintext_requires_loopback() {
        for addr in ["127.0.0.1:50051", "[::1]:50051"] {
            super::loopback_endpoint(addr.parse().unwrap()).unwrap();
        }
        for addr in ["0.0.0.0:50051", "192.0.2.1:50051", "[::]:50051"] {
            assert!(super::loopback_endpoint(addr.parse().unwrap()).is_err());
        }
    }
}
