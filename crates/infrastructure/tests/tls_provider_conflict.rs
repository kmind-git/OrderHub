// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

#![cfg(feature = "tls")]

#[test]
fn an_external_provider_installation_is_rejected() {
    // This binary owns its process global rustls state independently of other tests.
    rustls_graviola::default_provider()
        .install_default()
        .unwrap();
    let error = nautilus_infrastructure::tls::ensure_provider().unwrap_err();
    assert!(error.to_string().contains("provider conflict"), "{error}");
}
