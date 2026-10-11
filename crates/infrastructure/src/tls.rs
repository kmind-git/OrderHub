// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

//! The single approved rustls provider for retained `OrderHub` consumers.

use std::sync::{Arc, OnceLock};

use rustls::crypto::CryptoProvider;

static PROVIDER: OnceLock<Result<Arc<CryptoProvider>, String>> = OnceLock::new();

/// Installs the CPU checked provider once; another installed provider is rejected.
///
/// # Errors
/// Returns an error for unsupported CPUs or a process wide provider conflict.
pub fn ensure_provider() -> anyhow::Result<()> {
    provider().map(|_| ())
}

/// Returns the approved provider, installing it before any TLS operation.
///
/// # Errors
/// Returns an error for unsupported CPUs or a process wide provider conflict.
pub fn provider() -> anyhow::Result<Arc<CryptoProvider>> {
    PROVIDER
        .get_or_init(|| {
            required_cpu_features().map_err(|feature| {
                format!("OrderHub TLS requires the Graviola CPU feature {feature}")
            })?;
            let mut provider = rustls_graviola::default_provider();
            // Hybrid ML-KEM interoperability is outside this local profile.
            provider.kx_groups.retain(|group| {
                matches!(
                    group.name(),
                    rustls::NamedGroup::X25519
                        | rustls::NamedGroup::secp256r1
                        | rustls::NamedGroup::secp384r1
                )
            });
            provider.install_default().map_err(|_| {
                "OrderHub TLS provider conflict: another rustls provider is installed".to_string()
            })?;
            CryptoProvider::get_default()
                .cloned()
                .ok_or_else(|| "OrderHub TLS provider installation failed".to_string())
        })
        .as_ref()
        .cloned()
        .map_err(|message| anyhow::anyhow!("{message}"))
}

#[cfg(target_arch = "x86_64")]
fn required_cpu_features() -> Result<(), &'static str> {
    first_missing_cpu_feature(
        [
            std::is_x86_feature_detected!("aes"),
            std::is_x86_feature_detected!("ssse3"),
            std::is_x86_feature_detected!("avx"),
            std::is_x86_feature_detected!("avx2"),
            std::is_x86_feature_detected!("adx"),
            std::is_x86_feature_detected!("bmi2"),
            std::is_x86_feature_detected!("pclmulqdq"),
        ],
        ["aes", "ssse3", "avx", "avx2", "adx", "bmi2", "pclmulqdq"],
    )
}

#[cfg(target_arch = "aarch64")]
fn required_cpu_features() -> Result<(), &'static str> {
    first_missing_cpu_feature(
        [
            std::arch::is_aarch64_feature_detected!("aes"),
            std::arch::is_aarch64_feature_detected!("sha2"),
            std::arch::is_aarch64_feature_detected!("pmull"),
            std::arch::is_aarch64_feature_detected!("neon"),
        ],
        ["aes", "sha2", "pmull", "neon"],
    )
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn required_cpu_features() -> Result<(), &'static str> {
    Err("supported x86_64 or aarch64 architecture")
}

fn first_missing_cpu_feature<const N: usize>(
    available: [bool; N],
    names: [&'static str; N],
) -> Result<(), &'static str> {
    for (present, name) in available.into_iter().zip(names) {
        if !present {
            return Err(name);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn each_missing_cpu_feature_is_rejected() {
        let names = ["aes", "ssse3", "avx", "avx2", "adx", "bmi2", "pclmulqdq"];
        assert_eq!(super::first_missing_cpu_feature([true; 7], names), Ok(()));
        for (index, name) in names.into_iter().enumerate() {
            let mut available = [true; 7];
            available[index] = false;
            assert_eq!(
                super::first_missing_cpu_feature(available, names),
                Err(name)
            );
        }
    }

    #[test]
    fn approved_provider_is_shared_and_has_only_selected_groups() {
        let provider = super::provider().unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &provider,
            &super::provider().unwrap()
        ));
        assert!(std::sync::Arc::ptr_eq(
            &provider,
            rustls::crypto::CryptoProvider::get_default().unwrap()
        ));
        assert_eq!(provider.kx_groups.len(), 3);
    }
}
