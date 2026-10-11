// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

//! Real entrypoint rejects unaccepted integrations and invalid transport before journal creation.

use std::process::{Command, Output};

const VARIABLES: &[&str] = &[
    "ORDERHUB_ADDR",
    "ORDERHUB_JOURNAL",
    "ORDERHUB_SUBMITTERS",
    "ORDERHUB_TOKEN",
    "ORDERHUB_TLS_CERT",
    "ORDERHUB_TLS_KEY",
    "ORDERHUB_TLS_CA",
    "ORDERHUB_TLS_DOMAIN",
    "ORDERHUB_REDIS_ENABLED",
    "ORDERHUB_POSTGRES_ENABLED",
];

fn run(binary: &str, variables: &[(&str, &str)], directory: &std::path::Path) -> Output {
    let mut command = Command::new(binary);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    for name in VARIABLES {
        command.env_remove(name);
    }
    for (name, value) in variables {
        command.env(name, value);
    }
    command.current_dir(directory).output().unwrap()
}

#[test]
fn unaccepted_integrations_fail_before_business_state_is_opened() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["ORDERHUB_REDIS_ENABLED", "ORDERHUB_POSTGRES_ENABLED"] {
        let result = run(
            env!("CARGO_BIN_EXE_orderhub-node"),
            &[(name, "1")],
            dir.path(),
        );
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            error.contains("real service activation has not been accepted"),
            "{error}"
        );
        assert!(!dir.path().join("orderhub-journal.redb").exists());
    }
}

#[test]
fn missing_or_unavailable_tls_does_not_fall_back_to_plaintext() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(feature = "grpc-tls")]
    let vars = [];
    #[cfg(not(feature = "grpc-tls"))]
    let vars = [("ORDERHUB_TLS_CERT", "missing.pem")];
    let result = run(env!("CARGO_BIN_EXE_orderhub-node"), &vars, dir.path());
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("grpc-tls"));
    assert!(!dir.path().join("orderhub-journal.redb").exists());
    #[cfg(feature = "grpc-tls")]
    let client_vars = [];
    #[cfg(not(feature = "grpc-tls"))]
    let client_vars = [("ORDERHUB_TLS_CA", "missing.pem")];
    let result = run(
        env!("CARGO_BIN_EXE_orderhub-client"),
        &client_vars,
        dir.path(),
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("grpc-tls"));
}

#[cfg(not(feature = "grpc-tls"))]
#[test]
fn base_build_rejects_non_loopback_listener() {
    let dir = tempfile::tempdir().unwrap();
    let result = run(
        env!("CARGO_BIN_EXE_orderhub-node"),
        &[("ORDERHUB_ADDR", "0.0.0.0:50051")],
        dir.path(),
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("loopback"));
    assert!(!dir.path().join("orderhub-journal.redb").exists());
}
