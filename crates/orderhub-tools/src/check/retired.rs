// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

use regex::Regex;
use std::{collections::BTreeSet, fs, path::Path};

fn python_tools(path: &Path, errors: &mut Vec<String>, invocation: &Regex) -> anyhow::Result<()> {
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if path.is_dir() {
            python_tools(&path, errors, invocation)?;
        } else if path.extension().is_some_and(|e| e == "py" || e == "pyi") {
            errors.push(format!("retired Python tool remains: {}", path.display()));
        } else if path.extension().is_some_and(|e| e == "sh" || e == "bash")
            && invocation.is_match(&fs::read_to_string(&path)?)
        {
            errors.push(format!("Python invocation remains: {}", path.display()));
        }
    }
    Ok(())
}

fn walk(path: &Path, errors: &mut Vec<String>, pattern: &Regex) -> anyhow::Result<()> {
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "python") {
                errors.push(format!("retired binding directory: {}", path.display()));
            } else if !path.join("CACHEDIR.TAG").exists() {
                walk(&path, errors, pattern)?;
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            // This checker contains the forbidden spelling as policy data, not a binding.
            if path.ends_with("orderhub-tools/src/check/retired.rs") {
                continue;
            }
            let source = fs::read_to_string(&path)?;
            for (index, line) in source.lines().enumerate() {
                let lower = line.to_ascii_lowercase();
                let deferral =
                    lower.contains("cython") && (lower.contains("todo") || lower.contains("fixme"));
                if pattern.is_match(line) && (!line.trim_start().starts_with("//") || deferral) {
                    errors.push(format!(
                        "{}:{}: retired Python/plugin/example surface",
                        path.display(),
                        index + 1
                    ));
                }
            }
        } else if path
            .extension()
            .is_some_and(|e| matches!(e.to_str(), Some("pyx" | "pxd" | "pxi")))
            || path
                .file_name()
                .is_some_and(|n| n == "cbindgen_cython.toml")
        {
            errors.push(format!("retired Cython source remains: {}", path.display()));
        }
    }
    Ok(())
}
pub(super) fn check(root: &Path) -> anyhow::Result<Vec<String>> {
    let mut errors = Vec::new();
    let workspace = toml::from_str::<toml::Value>(&fs::read_to_string(root.join("Cargo.toml"))?)?;
    if let Some(dependencies) = workspace
        .get("workspace")
        .and_then(|v| v.get("dependencies"))
        .and_then(toml::Value::as_table)
    {
        for name in dependencies.keys().filter(|n| n.starts_with("pyo3")) {
            errors.push(format!("retired workspace dependency: {name}"));
        }
    }
    for name in [
        "examples",
        "crates/plugin",
        "crates/pyo3",
        "patches/pyo3-stub-gen",
    ] {
        if root.join(name).exists() {
            errors.push(format!("retired source tree remains: {name}"));
        }
    }
    if root.join("python/pyproject.toml").exists() || root.join("python/uv.lock").exists() {
        errors.push("retired Python environment configuration remains".into());
    }
    if root.join("python").is_dir() {
        for entry in fs::read_dir(root.join("python"))? {
            if entry?.file_name() != ".venv" {
                errors.push("retired Python source remains".into());
                break;
            }
        }
    }
    let invocation = Regex::new(r"\b(?:python3?|uv)\s+(?:-[A-Za-z]|run\b|sync\b)")?;
    for name in ["scripts", ".pre-commit-hooks"] {
        python_tools(&root.join(name), &mut errors, &invocation)?;
    }
    let pattern = Regex::new(
        r#"\b(?:pyo3|pyo3_stub_gen)::|feature\s*=\s*"(?:python|python-arrow|extension-module|plugin|examples)"|\b(?:pub\s+)?mod\s+python\s*;|cython-compat|(?i:TODO.*cython|FIXME.*cython|cython.*TODO|cython.*FIXME)"#,
    )?;
    walk(&root.join("crates"), &mut errors, &pattern)?;
    let mut ffi = BTreeSet::new();
    let mut ffi_dependencies = BTreeSet::new();
    let mut staticlibs = BTreeSet::new();
    let mut ffi_dirs = BTreeSet::new();
    let mut cbindgen = BTreeSet::new();
    for entry in fs::read_dir(root.join("crates"))? {
        let manifest = entry?.path().join("Cargo.toml");
        if manifest.is_file() {
            let value = toml::from_str::<toml::Value>(&fs::read_to_string(&manifest)?)?;
            for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(dependencies) = value.get(section).and_then(toml::Value::as_table) {
                    for name in dependencies.keys().filter(|n| n.starts_with("pyo3")) {
                        errors.push(format!("{}: retired dependency {name}", manifest.display()));
                    }
                }
            }
            let name = value["package"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if let Some(features) = value.get("features").and_then(toml::Value::as_table) {
                if features.contains_key("ffi") {
                    ffi.insert(name.clone());
                }
                if features
                    .values()
                    .filter_map(toml::Value::as_array)
                    .flatten()
                    .filter_map(toml::Value::as_str)
                    .any(|s| matches!(s, "nautilus-core/ffi" | "nautilus-model/ffi"))
                {
                    ffi_dependencies.insert(name.clone());
                }
                for name in [
                    "python",
                    "python-arrow",
                    "extension-module",
                    "plugin",
                    "examples",
                ] {
                    if features.contains_key(name) {
                        errors.push(format!("{}: retired feature {name}", manifest.display()));
                    }
                }
            }
            if value
                .get("lib")
                .and_then(|v| v.get("crate-type"))
                .and_then(toml::Value::as_array)
                .is_some_and(|a| a.iter().any(|v| v.as_str() == Some("staticlib")))
            {
                staticlibs.insert(name.clone());
            }
            let directory = manifest.parent().expect("manifest parent");
            if directory.join("src/ffi").is_dir() {
                ffi_dirs.insert(name.clone());
            }
            if directory.join("cbindgen.toml").is_file() {
                cbindgen.insert(name);
            }
        }
    }
    let expected = BTreeSet::from(["nautilus-core".to_string(), "nautilus-model".to_string()]);
    for (label, actual) in [
        ("ffi features", ffi),
        ("static libraries", staticlibs),
        ("ffi modules", ffi_dirs),
        ("cbindgen configs", cbindgen),
    ] {
        if actual != expected {
            errors.push(format!(
                "{label} must exist only in nautilus-core and nautilus-model"
            ));
        }
    }
    if ffi_dependencies != BTreeSet::from(["nautilus-model".to_string()]) {
        errors.push("only nautilus-model may enable another crate's ffi feature".into());
    }
    Ok(errors)
}
