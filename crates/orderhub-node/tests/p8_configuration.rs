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
    "ORDERHUB_FIX_CONFIG",
    "ORDERHUB_EXCHANGE_ADDR",
    "ORDERHUB_INPUT_STDIN",
    "ORDERHUB_CHECKPOINT_DIR",
    "ORDERHUB_PERSISTENCE",
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

fn valid_node_variables(directory: &std::path::Path) -> Vec<(&'static str, String)> {
    let path = directory.join("orderhub-fix.cfg");
    std::fs::write(
        &path,
        include_str!("../../../config/orderhub-fix.cfg").replace("../logs/fix", "fix-logs"),
    )
    .unwrap();
    let variables = vec![("ORDERHUB_FIX_CONFIG", path.to_string_lossy().into_owned())];
    #[cfg(feature = "grpc-tls")]
    {
        use base64::Engine as _;
        let mut variables = variables;
        for (name, label, bytes) in [
            (
                "ORDERHUB_TLS_CERT",
                "CERTIFICATE",
                include_bytes!("../../infrastructure/test_data/tls/localhost.der").as_slice(),
            ),
            (
                "ORDERHUB_TLS_KEY",
                "PRIVATE KEY",
                include_bytes!("../../infrastructure/test_data/tls/localhost.pk8").as_slice(),
            ),
        ] {
            let path = directory.join(name);
            std::fs::write(
                &path,
                format!(
                    "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ),
            )
            .unwrap();
            variables.push((name, path.to_string_lossy().into_owned()));
        }
        variables
    }
    #[cfg(not(feature = "grpc-tls"))]
    variables
}

#[test]
fn invalid_fix_settings_fail_before_journal_or_network_startup() {
    for invalid in [
        "HeartBtInt=0",
        "BeginString=FIX.4.4",
        "UnknownSetting=Y",
        "[SESSION]",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let variables = valid_node_variables(dir.path());
        let path = dir.path().join("orderhub-fix.cfg");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(path, format!("{text}\n{invalid}\n")).unwrap();
        let variables = variables
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect::<Vec<_>>();
        let output = run(env!("CARGO_BIN_EXE_orderhub-node"), &variables, dir.path());
        assert!(!output.status.success());
        assert!(!dir.path().join("orderhub-journal.redb").exists());
        assert!(!dir.path().join("fix-logs").exists());
    }
}

#[test]
fn missing_fix_file_and_retired_override_have_no_default_connection_fallback() {
    for legacy_override in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut variables = valid_node_variables(dir.path());
        if legacy_override {
            variables.push(("ORDERHUB_EXCHANGE_ADDR", "127.0.0.1:5001".into()));
        } else {
            std::fs::remove_file(dir.path().join("orderhub-fix.cfg")).unwrap();
        }
        let variables = variables
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect::<Vec<_>>();
        let output = run(env!("CARGO_BIN_EXE_orderhub-node"), &variables, dir.path());
        assert!(!output.status.success());
        if legacy_override {
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("ORDERHUB_EXCHANGE_ADDR is retired")
            );
        }
        assert!(!dir.path().join("orderhub-journal.redb").exists());
    }
}

#[test]
fn unusable_log_directory_fails_before_journal_creation() {
    let dir = tempfile::tempdir().unwrap();
    let variables = valid_node_variables(dir.path());
    std::fs::write(dir.path().join("fix-logs"), "not a directory").unwrap();
    let variables = variables
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect::<Vec<_>>();
    let output = run(env!("CARGO_BIN_EXE_orderhub-node"), &variables, dir.path());
    assert!(!output.status.success());
    assert!(!dir.path().join("orderhub-journal.redb").exists());
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
