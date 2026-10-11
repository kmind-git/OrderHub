// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

//! Rust counterpart of `scripts/ci/check_test_network.py`.

use std::{collections::BTreeSet, fs, net::IpAddr, path::Path};

use anyhow::Context;
use regex::Regex;

struct Rules {
    calls: Regex,
    sockets: Regex,
    live: Regex,
    fork: Regex,
    test_module: Regex,
}

impl Rules {
    fn new() -> anyhow::Result<Self> {
        Ok(Self {
            calls: Regex::new(
                r#"\b(?:get|post|put|patch|delete|head|options|urlopen|connect(?:_async(?:_with_config)?|_ex)?|create_connection)\s*\(\s*(?:\(\s*)?[&rfb]*["'](?P<address>(?:https?|wss?)://[^"'\s]+)"#,
            )?,
            sockets: Regex::new(
                r#"\b(?:connect|connect_ex|create_connection)\s*\(\s*(?:\(\s*)?[&]*["'](?P<address>[^"'\s]*[.:][^"'\s]*)"#,
            )?,
            live: Regex::new(
                r#"["'][A-Z_]*(?:RUN_LIVE|LIVE_TEST|RUN_NETWORK_TEST|FORK_TESTS)[A-Z_]*["']"#,
            )?,
            fork: Regex::new(
                r#"--fork-(?:url|rpc-url)\b|\bfork_(?:url|rpc_url)\s*\(|["'][A-Z_]*FORK_RPC_URL[A-Z_]*["']"#,
            )?,
            test_module: Regex::new(
                r"(?m)^\s*(?:pub\s+)?mod\s+(?:tests?|test_\w+|\w+_tests)\s*\{",
            )?,
        })
    }

    fn findings(&self, text: &str) -> Vec<(usize, &'static str)> {
        let text = text
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") || trimmed.starts_with('#') {
                    ""
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut found = BTreeSet::new();
        for pattern in [&self.calls, &self.sockets] {
            for capture in pattern.captures_iter(&text) {
                if !local_address(&capture["address"]) {
                    let start = capture.get(0).unwrap().start();
                    let line = text[..start].bytes().filter(|byte| *byte == b'\n').count() + 1;
                    found.insert((line, "non-local network call"));
                }
            }
        }
        for (rule, pattern) in [
            ("live-test switch", &self.live),
            ("fork-RPC option", &self.fork),
        ] {
            for matched in pattern.find_iter(&text) {
                let line = text[..matched.start()]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1;
                found.insert((line, rule));
            }
        }
        found.into_iter().collect()
    }
}

fn local_address(address: &str) -> bool {
    let host =
        if address.matches(':').count() > 1 && !address.contains("://") && !address.contains('[') {
            address.to_string()
        } else {
            let authority = address
                .split_once("://")
                .map_or(address, |(_, value)| value)
                .split(['/', '?', '#'])
                .next()
                .unwrap_or_default();
            let host_port = authority.rsplit('@').next().unwrap_or_default();
            if let Some(bracketed) = host_port.strip_prefix('[') {
                let Some((host, _)) = bracketed.split_once(']') else {
                    return false;
                };
                let Ok(ip) = host.parse::<std::net::Ipv6Addr>() else {
                    return false;
                };
                return ip
                    .to_ipv4_mapped()
                    .map_or(ip.is_loopback(), |ip| ip.is_loopback());
            }
            if host_port.contains(['[', ']']) {
                return false;
            }
            host_port
                .split(':')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
        };
    if host.is_empty() || host.contains('{') {
        return true;
    }
    if ["localhost", "example.com", "example.net", "example.org"].contains(&host.as_str())
        || [
            ".localhost",
            ".invalid",
            ".test",
            ".example",
            ".example.com",
            ".example.net",
            ".example.org",
        ]
        .iter()
        .any(|suffix| host.ends_with(suffix))
    {
        return true;
    }
    match host.trim_matches(['[', ']']).parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => ip.is_loopback(),
        Ok(IpAddr::V6(ip)) => ip
            .to_ipv4_mapped()
            .map_or(ip.is_loopback(), |ip| ip.is_loopback()),
        Err(_) => false,
    }
}

pub(super) fn check_repository(root: &Path) -> anyhow::Result<Vec<String>> {
    let rules = Rules::new()?;
    let mut paths = Vec::new();
    collect_files(&root.join("crates"), "rs", &mut paths)?;
    collect_files(&root.join("python/tests"), "py", &mut paths)?;
    paths.sort();
    let mut errors = Vec::new();
    for path in paths {
        let relative = path.strip_prefix(root)?;
        if relative
            .components()
            .any(|part| part.as_os_str() == "test_data")
        {
            continue;
        }
        let text = fs::read_to_string(&path).with_context(|| format!("Read {}", path.display()))?;
        let mut offset = 0;
        let mut selected = text.as_str();
        if path.extension().is_some_and(|extension| extension == "rs")
            && !relative
                .components()
                .any(|part| part.as_os_str() == "tests")
            && path.file_name().is_none_or(|name| name != "tests.rs")
        {
            let Some(module) = rules.test_module.find(&text) else {
                continue;
            };
            offset = text[..module.start()]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count();
            selected = &text[module.start()..];
        }
        for (line, rule) in rules.findings(selected) {
            errors.push(format!("{}:{}: {rule}", relative.display(), line + offset));
        }
    }
    Ok(errors)
}

fn collect_files(
    dir: &Path,
    extension: &str,
    paths: &mut Vec<std::path::PathBuf>,
) -> anyhow::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_files(&path, extension, paths)?;
        } else if path.extension().is_some_and(|value| value == extension) {
            paths.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    fn literal_network_destinations_and_opt_ins_are_reported() {
        let rules = Rules::new().unwrap();
        assert_eq!(
            rules.findings(include_str!("../../test_data/network_patterns.txt")),
            vec![
                (2, "non-local network call"),
                (3, "live-test switch"),
                (4, "fork-RPC option")
            ],
        );
    }

    #[rstest]
    fn loopback_documentation_and_dynamic_addresses_remain_allowed() {
        for address in [
            "http://localhost:9000",
            "127.0.0.2:6379",
            "::1",
            "http://[::1]:80",
            "http://[::ffff:127.0.0.1]:80",
            "https://example.com",
            "https://host.invalid",
            "https://api.example.net",
            "https://{host}",
            "http://127.0.0.1:{port}{path}",
        ] {
            assert!(local_address(address), "{address}");
        }
        for address in [
            "https://api.exchange.org",
            "192.168.1.1:80",
            "http://[",
            "::2",
            "http://2130706433",
        ] {
            assert!(!local_address(address), "{address}");
        }
    }
}
