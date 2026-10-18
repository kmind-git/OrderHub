// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

//! Strict single-session configuration for the local FIX 4.2 initiator.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

const MAX_CONFIG_BYTES: u64 = 65_536;
const KEYS: &[&str] = &[
    "ConnectionType",
    "BeginString",
    "SenderCompID",
    "TargetCompID",
    "SocketConnectHost",
    "SocketConnectPort",
    "HeartBtInt",
    "Logging",
    "FileLogPath",
    "FileLogMaxBytes",
    "FileLogBackups",
];

/// Effective FIX session and diagnostic log settings.
#[derive(Debug, Clone, Serialize)]
pub struct FixSessionConfig {
    pub begin_string: String,
    pub sender_comp_id: String,
    pub target_comp_id: String,
    pub socket_connect_host: String,
    pub socket_connect_port: u16,
    pub heartbeat_seconds: u32,
    pub logging: FixLogConfig,
}

/// Bounded file retention for the two FIX diagnostic logs.
#[derive(Debug, Clone, Serialize)]
pub struct FixLogConfig {
    pub enabled: bool,
    pub directory: PathBuf,
    pub max_bytes: u64,
    pub backups: u32,
}

impl FixSessionConfig {
    /// Reads one `[SESSION]`, inheriting explicitly supplied `[DEFAULT]` settings.
    ///
    /// Relative FileLogPath values are resolved against the configuration file.
    ///
    /// # Errors
    /// Returns file errors or invalid, duplicate, unknown, or unsupported settings.
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = normalized_path(path.as_ref())?;
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()?.join(path)
        };
        let mut text = String::new();
        std::fs::File::open(&path)?
            .take(MAX_CONFIG_BYTES + 1)
            .read_to_string(&mut text)?;
        anyhow::ensure!(
            text.len() as u64 <= MAX_CONFIG_BYTES,
            "FIX config exceeds 64 KiB"
        );
        Self::parse(&text, path.parent().unwrap_or(Path::new(".")))
    }

    fn parse(text: &str, directory: &Path) -> anyhow::Result<Self> {
        let mut defaults = BTreeMap::new();
        let mut session = BTreeMap::new();
        let mut section = "";
        let (mut saw_default, mut saw_session) = (false, false);
        for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(['#', ';']) {
                continue;
            }
            match line {
                "[DEFAULT]" => {
                    anyhow::ensure!(!saw_default && !saw_session, "duplicate or late [DEFAULT]");
                    saw_default = true;
                    section = "DEFAULT";
                    continue;
                }
                "[SESSION]" => {
                    anyhow::ensure!(!saw_session, "only one FIX [SESSION] is supported");
                    saw_session = true;
                    section = "SESSION";
                    continue;
                }
                _ => {}
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("invalid FIX config line {}", index + 1))?;
            let (key, value) = (key.trim(), value.trim());
            anyhow::ensure!(KEYS.contains(&key), "unsupported FIX setting {key}");
            anyhow::ensure!(
                !value.is_empty() && !value.chars().any(char::is_control),
                "invalid FIX setting {key}"
            );
            let fields = match section {
                "DEFAULT" => &mut defaults,
                "SESSION" => &mut session,
                _ => anyhow::bail!("FIX settings require [DEFAULT] or [SESSION]"),
            };
            anyhow::ensure!(
                fields.insert(key, value).is_none(),
                "duplicate FIX setting {key}"
            );
        }
        anyhow::ensure!(saw_session, "missing FIX [SESSION]");
        defaults.extend(session);
        let required = |key| {
            defaults
                .get(key)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("missing FIX setting {key}"))
        };
        anyhow::ensure!(
            required("ConnectionType")? == "initiator",
            "ConnectionType must be initiator"
        );
        let enabled = match defaults.get("Logging").copied().unwrap_or("Y") {
            "Y" => true,
            "N" => false,
            _ => anyhow::bail!("Logging must be Y or N"),
        };
        let log_path = if enabled {
            required("FileLogPath")?
        } else {
            defaults.get("FileLogPath").copied().unwrap_or("logs/fix")
        };
        let log_path = normalized_path(Path::new(log_path))?;
        let config = Self {
            begin_string: required("BeginString")?.to_string(),
            sender_comp_id: required("SenderCompID")?.to_string(),
            target_comp_id: required("TargetCompID")?.to_string(),
            socket_connect_host: required("SocketConnectHost")?.to_string(),
            socket_connect_port: required("SocketConnectPort")?.parse()?,
            heartbeat_seconds: required("HeartBtInt")?.parse()?,
            logging: FixLogConfig {
                enabled,
                directory: if log_path.is_absolute() {
                    log_path
                } else {
                    directory.join(log_path)
                },
                max_bytes: defaults
                    .get("FileLogMaxBytes")
                    .copied()
                    .unwrap_or("10485760")
                    .parse()?,
                backups: defaults
                    .get("FileLogBackups")
                    .copied()
                    .unwrap_or("5")
                    .parse()?,
            },
        };
        config.validate()?;
        Ok(config)
    }

    /// Validates the supported wire profile and safe log naming.
    ///
    /// # Errors
    /// Returns an error for invalid session values or retention limits.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.begin_string == "FIX.4.2",
            "only BeginString=FIX.4.2 is supported"
        );
        for id in [&self.sender_comp_id, &self.target_comp_id] {
            anyhow::ensure!(
                !id.is_empty()
                    && id.len() <= 64
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
                "CompID must contain 1-64 ASCII letters, digits, dots, underscores or hyphens"
            );
        }
        anyhow::ensure!(
            self.sender_comp_id != self.target_comp_id,
            "SenderCompID and TargetCompID must differ"
        );
        anyhow::ensure!(
            !self.socket_connect_host.is_empty()
                && self.socket_connect_host.len() <= 255
                && self
                    .socket_connect_host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-:%".contains(&b)),
            "invalid SocketConnectHost; use a hostname, IPv4 or unbracketed IPv6 address"
        );
        anyhow::ensure!(
            self.socket_connect_port > 0,
            "SocketConnectPort must be positive"
        );
        anyhow::ensure!(
            (1..=3600).contains(&self.heartbeat_seconds),
            "HeartBtInt must be between 1 and 3600 seconds"
        );
        anyhow::ensure!(
            (65_536..=1_073_741_824).contains(&self.logging.max_bytes),
            "FileLogMaxBytes must be between 65536 and 1073741824"
        );
        anyhow::ensure!(
            (1..=20).contains(&self.logging.backups),
            "FileLogBackups must be between 1 and 20"
        );
        Ok(())
    }

    /// Returns a socket address with IPv6 brackets where needed.
    #[must_use]
    pub fn address(&self) -> String {
        if self.socket_connect_host.contains(':') {
            format!(
                "[{}]:{}",
                self.socket_connect_host, self.socket_connect_port
            )
        } else {
            format!("{}:{}", self.socket_connect_host, self.socket_connect_port)
        }
    }

    /// Returns the filename prefix for this FIX session.
    #[must_use]
    pub fn log_prefix(&self) -> String {
        format!(
            "{}-{}-{}",
            self.begin_string, self.sender_comp_id, self.target_comp_id
        )
    }
}

fn normalized_path(path: &Path) -> anyhow::Result<PathBuf> {
    let text = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("FIX configuration paths must be valid UTF-8"))?;
    Ok(PathBuf::from(text.replace('\\', "/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../test_data/fix_session.cfg");

    #[test]
    fn defaults_inheritance_override_and_portable_relative_log_path() {
        let text = format!("{EXAMPLE}\nHeartBtInt=7\nFileLogPath=custom\\fix\n");
        let config = FixSessionConfig::parse(&text, Path::new("instance/config")).unwrap();
        assert_eq!(config.heartbeat_seconds, 7);
        assert_eq!(config.address(), "127.0.0.1:5001");
        assert_eq!(
            config.logging.directory,
            Path::new("instance/config/custom/fix")
        );
        assert!(config.logging.enabled);
    }

    #[test]
    fn rejects_unknown_duplicate_multi_session_and_unsupported_protocol() {
        for text in [
            format!("{EXAMPLE}\nSenderCompID=duplicate"),
            format!("{EXAMPLE}\nPersistMessages=Y"),
            format!("{EXAMPLE}\n[SESSION]\nSenderCompID=second"),
            EXAMPLE.replace("FIX.4.2", "FIX.4.4"),
            EXAMPLE.replace("HeartBtInt=30", "HeartBtInt=0"),
            EXAMPLE.replace("ORDERHUB", "../ORDERHUB"),
            EXAMPLE.replace("SocketConnectPort=5001", "SocketConnectPort=0"),
            EXAMPLE.replace("FileLogBackups=5", "FileLogBackups=0"),
            EXAMPLE.replace("FileLogMaxBytes=10485760", "FileLogMaxBytes=1"),
        ] {
            assert!(
                FixSessionConfig::parse(&text, Path::new(".")).is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn required_values_and_ipv6_address_are_explicit() {
        let config =
            FixSessionConfig::parse(&EXAMPLE.replace("127.0.0.1", "::1"), Path::new(".")).unwrap();
        assert_eq!(config.address(), "[::1]:5001");
        assert!(
            FixSessionConfig::parse(&EXAMPLE.replace("HeartBtInt=30", ""), Path::new(".")).is_err()
        );
    }
}
