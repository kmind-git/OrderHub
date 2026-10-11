// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

//! Rust counterpart of the maintained dependency feature policy check.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Context;
use toml::Value;

const WORKSPACE_POLICIES: &[(&str, &[&str])] = &[
    ("criterion", &["cargo_bench_support"]),
    ("arrow", &["ipc"]),
    ("parquet", &["arrow"]),
    ("madsim", &["macros"]),
    ("nautilus-infrastructure", &[]),
    ("rstest", &[]),
];

fn check_benchmark_dependencies(manifests: &[(PathBuf, Value)]) -> Vec<String> {
    let mut errors = Vec::new();
    for (path, manifest) in manifests {
        let mut sections = dependency_sections(manifest);
        if let Some(dependencies) = manifest
            .get("workspace")
            .and_then(|w| w.get("dependencies"))
        {
            sections.push(("workspace.dependencies".into(), dependencies));
        }
        for (_, dependencies) in sections {
            let Some(dependencies) = dependencies.as_table() else {
                continue;
            };
            for (name, declaration) in dependencies {
                let package = declaration
                    .get("package")
                    .and_then(Value::as_str)
                    .unwrap_or(name);
                if package == "iai" || package.starts_with("codspeed") {
                    errors.push(format!(
                        "{}: {package} requires a retired native benchmark backend",
                        path.display()
                    ));
                }
                if name == "criterion" && package != "criterion" {
                    errors.push(format!(
                        "{}: criterion must use the native Rust package",
                        path.display()
                    ));
                }
            }
        }
    }
    errors
}

fn check_catalog_dependencies(manifests: &[(PathBuf, Value)]) -> Vec<String> {
    let mut errors = Vec::new();
    for (path, manifest) in manifests {
        if !path.starts_with("crates") {
            continue;
        }
        for (section, dependencies) in dependency_sections(manifest) {
            if let Some(declaration) = dependencies.get("parquet") {
                if path != Path::new("crates/orderhub-tools/Cargo.toml")
                    || section != "dependencies"
                {
                    errors.push(format!(
                        "{}: Parquet must remain in the optional offline converter",
                        path.display()
                    ));
                    continue;
                }
                errors.extend(check_policies(
                    path,
                    Some(dependencies),
                    &[("parquet", &["snap", "zstd"])],
                ));
                if declaration.get("optional").and_then(Value::as_bool) != Some(true) {
                    errors.push("orderhub-tools: Parquet must remain optional".into());
                }
            }
        }
    }
    errors
}
const TEST_SUPPORT_DEV_PACKAGES: &[&str] = &[
    "nautilus-common",
    "nautilus-data",
    "nautilus-execution",
    "nautilus-infrastructure",
    "orderhub-gateway",
    "orderhub-tools",
    "nautilus-portfolio",
    "nautilus-risk",
    "nautilus-testkit",
];

fn dependency_state(declaration: &Value) -> Result<(bool, BTreeSet<&str>), String> {
    if declaration.is_str() {
        return Ok((true, BTreeSet::new()));
    }
    let Some(table) = declaration.as_table() else {
        return Err(format!(
            "dependency declaration has unsupported type {}",
            declaration.type_str()
        ));
    };
    let mut features = BTreeSet::new();
    if let Some(value) = table.get("features") {
        let Some(values) = value.as_array() else {
            return Err("dependency features must be a list of strings".to_string());
        };
        for feature in values {
            let Some(feature) = feature.as_str() else {
                return Err("dependency features must be a list of strings".to_string());
            };
            features.insert(feature);
        }
    }
    let defaults = match table.get("default-features") {
        Some(value) => value
            .as_bool()
            .ok_or("default-features must be a boolean")?,
        None => true,
    };
    Ok((defaults, features))
}

fn describe_policy(defaults: bool, features: &BTreeSet<&str>) -> String {
    let defaults = if defaults { "enabled" } else { "disabled" };
    let features = features
        .iter()
        .map(|value| format!("'{value}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("defaults {defaults}, features [{features}]")
}

fn check_policies(
    path: &Path,
    dependencies: Option<&Value>,
    policies: &[(&str, &[&str])],
) -> Vec<String> {
    let Some(table) = dependencies.and_then(Value::as_table) else {
        return vec![format!(
            "{}: dependency table is missing or invalid",
            path.display()
        )];
    };
    let mut violations = Vec::new();
    for (dependency, features) in policies {
        let Some(declaration) = table.get(*dependency) else {
            violations.push(format!(
                "{}: dependency '{dependency}' is missing",
                path.display()
            ));
            continue;
        };
        match dependency_state(declaration) {
            Ok((defaults, actual)) => {
                let expected: BTreeSet<_> = features.iter().copied().collect();
                if defaults || actual != expected {
                    violations.push(format!(
                        "{}: dependency '{dependency}' must use {}, was {}",
                        path.display(),
                        describe_policy(false, &expected),
                        describe_policy(defaults, &actual),
                    ));
                }
            }
            Err(e) => violations.push(format!(
                "{}: dependency '{dependency}': {e}",
                path.display()
            )),
        }
    }
    violations
}

fn dependency_sections(data: &Value) -> Vec<(String, &Value)> {
    let mut sections = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = data.get(section) {
            sections.push((section.to_string(), table));
        }
    }
    if let Some(targets) = data.get("target").and_then(Value::as_table) {
        for (target, data) in targets {
            for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(table) = data.as_table().and_then(|table| table.get(section)) {
                    sections.push((format!("target.{target}.{section}"), table));
                }
            }
        }
    }
    sections
}

fn check_test_support(manifests: &[(PathBuf, Value)], expected: &[&str]) -> Vec<String> {
    let mut dev_packages = BTreeSet::new();
    let mut forwarders = BTreeSet::new();
    let mut testkit_has_support = false;
    let mut violations = Vec::new();
    for (path, data) in manifests {
        let Some(name) = data
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if let Some(features) = data.get("features").and_then(Value::as_table) {
            for (feature, values) in features {
                if values.as_array().is_some_and(|values| {
                    values.iter().any(|value| {
                        matches!(
                            value.as_str(),
                            Some("nautilus-model/test-support" | "nautilus-model?/test-support")
                        )
                    })
                }) {
                    forwarders.insert((name, feature.as_str()));
                }
            }
        }
        for (section, dependencies) in dependency_sections(data) {
            let Some(declaration) = dependencies
                .as_table()
                .and_then(|table| table.get("nautilus-model"))
            else {
                continue;
            };
            match dependency_state(declaration) {
                Ok((_, features)) if features.contains("test-support") => {
                    if section.ends_with("dev-dependencies") {
                        dev_packages.insert(name);
                    } else if name == "nautilus-testkit" && section == "dependencies" {
                        testkit_has_support = true;
                    } else {
                        violations.push(format!(
                            "{}: nautilus-model/test-support must be a dev dependency",
                            path.display()
                        ));
                    }
                }
                Ok(_) => {}
                Err(e) => violations.push(format!(
                    "{}: dependency 'nautilus-model': {e}",
                    path.display()
                )),
            }
        }
    }
    let expected: BTreeSet<_> = expected.iter().copied().collect();
    for name in dev_packages.difference(&expected) {
        violations.push(format!(
            "{name}: test-support dev dependency is not classified"
        ));
    }
    for name in expected.difference(&dev_packages) {
        violations.push(format!(
            "{name}: expected test-support dev dependency is missing"
        ));
    }
    if !testkit_has_support {
        violations.push("nautilus-testkit: normal dependency must enable test-support".to_string());
    }
    let expected_forwarders = BTreeSet::new();
    for (name, feature) in forwarders.difference(&expected_forwarders) {
        violations.push(format!(
            "{name}: feature '{feature}' must not forward nautilus-model/test-support"
        ));
    }
    for (name, feature) in expected_forwarders.difference(&forwarders) {
        violations.push(format!(
            "{name}: feature '{feature}' must forward nautilus-model/test-support"
        ));
    }
    violations
}

fn repository_paths(root: &Path, filename: &str) -> anyhow::Result<Vec<PathBuf>> {
    let output = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            filename,
        ])
        .arg(format!(":(glob)**/{filename}"))
        .output()
        .context("Run git ls-files")?;
    anyhow::ensure!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let paths = std::str::from_utf8(&output.stdout)?
        .split('\0')
        .filter(|raw| !raw.is_empty())
        .map(PathBuf::from)
        .filter(|path| root.join(path).is_file() && !path.starts_with("audit"))
        .collect::<BTreeSet<_>>();
    Ok(paths.into_iter().collect())
}

pub(super) fn check_repository(root: &Path) -> anyhow::Result<Vec<String>> {
    let root = root.canonicalize().context("Resolve repository root")?;
    let mut manifests = Vec::new();
    for path in repository_paths(&root, "Cargo.toml")? {
        let source = fs::read_to_string(root.join(&path))
            .with_context(|| format!("Read {}", path.display()))?;
        let data = toml::from_str::<Value>(&source)
            .with_context(|| format!("Parse {}", path.display()))?;
        manifests.push((path, data));
    }
    let mut violations = Vec::new();
    if let Some((path, data)) = manifests
        .iter()
        .find(|(path, _)| path == Path::new("Cargo.toml"))
    {
        violations.extend(check_policies(
            path,
            data.get("workspace").and_then(|w| w.get("dependencies")),
            WORKSPACE_POLICIES,
        ));
    } else {
        violations.push("Cargo.toml: workspace manifest is missing".to_string());
    }
    violations.extend(check_test_support(&manifests, TEST_SUPPORT_DEV_PACKAGES));
    violations.extend(check_catalog_dependencies(&manifests));
    violations.extend(check_benchmark_dependencies(&manifests));
    let root_manifest = &manifests
        .iter()
        .find(|(path, _)| path == Path::new("Cargo.toml"))
        .context("workspace manifest")?
        .1;
    if root_manifest
        .get("patch")
        .and_then(|v| v.get("crates-io"))
        .and_then(|v| v.get("parquet"))
        .and_then(|v| v.get("path"))
        .and_then(Value::as_str)
        != Some("patches/parquet-59.3.0")
    {
        violations.push("Parquet must use the pure Rust reader patch".into());
    }
    if root_manifest
        .get("patch")
        .and_then(|v| v.get("crates-io"))
        .and_then(|v| v.get("criterion"))
        .and_then(|v| v.get("path"))
        .and_then(Value::as_str)
        != Some("patches/criterion-0.7.0")
    {
        violations.push("Criterion must use the patch that disables external plotting".into());
    }
    for lockfile in repository_paths(&root, "Cargo.lock")? {
        let manifest = lockfile.with_file_name("Cargo.toml");
        if !root.join(&manifest).is_file() {
            violations.push(format!(
                "{}: sibling Cargo.toml is missing",
                lockfile.display()
            ));
            continue;
        }
        let output = Command::new("cargo")
            .args([
                "metadata",
                "--locked",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(root.join(&manifest))
            .current_dir(&root)
            .output()
            .context("Run cargo metadata")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = stderr
                .trim()
                .lines()
                .next_back()
                .or_else(|| stdout.trim().lines().next())
                .unwrap_or("cargo metadata failed");
            violations.push(format!(
                "{}: cargo metadata --locked failed: {detail}",
                lockfile.display()
            ));
        }
    }
    Ok(violations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_benchmark_backends_cannot_be_reintroduced_by_alias_or_target() {
        for (section, declaration, expected) in [
            (
                "workspace.dependencies",
                "criterion = {version = '=0.7.0', default-features = false, features = ['cargo_bench_support']}",
                0,
            ),
            (
                "workspace.dependencies",
                "criterion = {package = 'codspeed-criterion-compat', version = '5'}",
                2,
            ),
            (
                "dev-dependencies",
                "counts = {package = 'iai', version = '0.1'}",
                1,
            ),
            ("target.'cfg(unix)'.dev-dependencies", "codspeed = '5'", 1),
        ] {
            let data = toml::from_str(&format!("[{section}]\n{declaration}")).unwrap();
            assert_eq!(
                check_benchmark_dependencies(&[(PathBuf::from("Cargo.toml"), data)]).len(),
                expected
            );
        }
    }

    #[test]
    fn catalog_backend_features_remain_optional_and_isolated() {
        for (path, features, optional, expected) in [
            (
                "crates/orderhub-tools/Cargo.toml",
                "'snap', 'zstd'",
                true,
                0,
            ),
            (
                "crates/orderhub-tools/Cargo.toml",
                "'snap', 'zstd', 'encryption'",
                true,
                1,
            ),
            (
                "crates/orderhub-tools/Cargo.toml",
                "'snap', 'zstd'",
                false,
                1,
            ),
            ("crates/orderhub-node/Cargo.toml", "'snap', 'zstd'", true, 1),
        ] {
            let manifest = toml::from_str(&format!("[dependencies]\nparquet = {{ default-features = false, features = [{features}], optional = {optional} }}")).unwrap();
            assert_eq!(
                check_catalog_dependencies(&[(PathBuf::from(path), manifest)]).len(),
                expected
            );
        }
    }
    use rstest::rstest;

    #[rstest]
    fn workspace_policy_matches_same_python_inputs() {
        for (source, expected_count) in [
            (
                r#"arrow = {default-features = false, features = ["ipc"]}"#,
                0,
            ),
            (
                r#"arrow = {default-features = false, features = ["ipc", "json"]}"#,
                1,
            ),
            (
                r#"arrow = {default-features = true, features = ["ipc"]}"#,
                1,
            ),
            (r#"arrow = "59.3""#, 1),
            (r#"arrow = {default-features = "false"}"#, 1),
            ("arrow = {features = [1]}", 1),
            ("", 1),
        ] {
            let value = toml::from_str::<Value>(source).unwrap();
            assert_eq!(
                check_policies(
                    Path::new("Cargo.toml"),
                    Some(&value),
                    &[("arrow", &["ipc"])]
                )
                .len(),
                expected_count
            );
        }
    }

    #[rstest]
    fn target_dev_dependencies_are_classified_and_normal_test_support_is_rejected() {
        let base = r#"
            [package]
            name = "nautilus-testkit"
            [dependencies.nautilus-model]
            features = ["test-support"]
        "#;
        for (section, expected_count) in [
            ("dev-dependencies", 0),
            ("target.'cfg(unix)'.dev-dependencies", 0),
            ("dependencies", 2),
        ] {
            let consumer = format!(
                "[package]\nname = 'nautilus-common'\n[{section}.nautilus-model]\nfeatures = ['test-support']"
            );
            let manifests = vec![
                (
                    PathBuf::from("testkit/Cargo.toml"),
                    toml::from_str(base).unwrap(),
                ),
                (
                    PathBuf::from("common/Cargo.toml"),
                    toml::from_str(&consumer).unwrap(),
                ),
            ];
            assert_eq!(
                check_test_support(&manifests, &["nautilus-common"]).len(),
                expected_count
            );
        }
    }
}
