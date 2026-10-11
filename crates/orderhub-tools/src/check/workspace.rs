// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

use anyhow::Context;
use regex::Regex;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn sources(
    directory: &Path,
    nested: &BTreeSet<PathBuf>,
    output: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            if !nested.contains(&path) && !path.join("CACHEDIR.TAG").exists() {
                sources(&path, nested, output)?;
            }
        } else if path.extension().is_some_and(|v| v == "rs") {
            output.push(path);
        }
    }
    Ok(())
}

pub(super) fn check(root: &Path) -> anyhow::Result<Vec<String>> {
    let output = Command::new("cargo")
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .context("Read Cargo workspace")?;
    anyhow::ensure!(
        output.status.success(),
        "cargo metadata: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let data: Value = serde_json::from_slice(&output.stdout)?;
    let ids: BTreeSet<_> = data["workspace_members"]
        .as_array()
        .context("workspace members")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let packages: BTreeMap<_, _> = data["packages"]
        .as_array()
        .context("packages")?
        .iter()
        .filter(|p| p["id"].as_str().is_some_and(|id| ids.contains(id)))
        .map(|p| (p["name"].as_str().expect("Cargo package name"), p))
        .collect();
    let roots: BTreeSet<_> = packages
        .values()
        .map(|p| {
            PathBuf::from(p["manifest_path"].as_str().expect("Cargo manifest"))
                .parent()
                .expect("manifest parent")
                .to_path_buf()
        })
        .collect();
    let make = fs::read_to_string(root.join("Makefile"))?;
    let logical = make.replace("\\\r\n", " ").replace("\\\n", " ");
    let mut counts = BTreeMap::new();
    let mut no_tests = Vec::new();
    for name in ["CORE_CRATES", "ADAPTER_CRATES", "NO_TEST_CRATES"] {
        let pattern = Regex::new(&format!(r"(?m)^{name}[ \t]*:?=[ \t]*([^\n]*)"))?;
        let captures = pattern
            .captures(&logical)
            .with_context(|| format!("Makefile does not define {name}"))?;
        for entry in captures[1].split_whitespace() {
            *counts.entry(entry.to_string()).or_insert(0) += 1;
            if name == "NO_TEST_CRATES" {
                no_tests.push(entry.to_string());
            }
        }
    }
    let mut errors = Vec::new();
    for (name, count) in &counts {
        if *count != 1 {
            errors.push(format!("workspace crate listed more than once: {name}"));
        }
        if !packages.contains_key(name.as_str()) {
            errors.push(format!("inventory entry not found in workspace: {name}"));
        }
    }
    for name in packages.keys() {
        if !counts.contains_key(*name) {
            errors.push(format!("workspace crate missing from inventories: {name}"));
        }
    }
    let tests = Regex::new(
        r"(?m)^[ \t]*#\s*\[\s*(?:(?:[A-Za-z_][A-Za-z0-9_]*)::)*(?:test|rstest|test_case)\b",
    )?;
    for name in no_tests {
        let Some(package) = packages.get(name.as_str()) else {
            continue;
        };
        for target in package["targets"].as_array().context("targets")? {
            if target["kind"]
                .as_array()
                .context("target kind")?
                .iter()
                .any(|v| v == "test")
            {
                errors.push(format!(
                    "no-test crate {name} defines test target: {}",
                    target["name"]
                ));
            }
        }
        let directory = Path::new(package["manifest_path"].as_str().context("manifest")?)
            .parent()
            .context("parent")?;
        let nested = roots
            .iter()
            .filter(|p| p.as_path() != directory)
            .cloned()
            .collect();
        let mut paths = Vec::new();
        sources(directory, &nested, &mut paths)?;
        for path in paths {
            if tests.is_match(&fs::read_to_string(&path)?) {
                errors.push(format!(
                    "no-test crate {name} contains Rust tests: {}",
                    path.strip_prefix(root)?.display()
                ));
            }
        }
    }
    Ok(errors)
}
