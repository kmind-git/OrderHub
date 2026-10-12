// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

use anyhow::Context;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use toml::Value;

#[derive(Debug)]
struct Step {
    tool: String,
    args: Vec<String>,
    report: bool,
}
fn path(root: &Path, value: &str, directory: bool) -> anyhow::Result<PathBuf> {
    let candidate = Path::new(value);
    anyhow::ensure!(
        !value.is_empty()
            && !candidate.is_absolute()
            && !value.contains('\\')
            && !candidate.components().any(|c| matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )),
        "audit paths must stay within the repository"
    );
    let result = root.join(candidate).canonicalize()?;
    anyhow::ensure!(
        result.starts_with(root),
        "audit path resolves outside repository"
    );
    anyhow::ensure!(
        if directory {
            result.is_dir()
        } else {
            result.is_file()
        },
        "audit path has the wrong file type"
    );
    Ok(result)
}
fn text<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .with_context(|| format!("{key} must be a string"))
}
fn relative_path(root: &Path, value: &str, directory: bool) -> anyhow::Result<String> {
    let resolved = path(root, value, directory)?;
    let relative = resolved
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/");
    Ok(if relative.is_empty() {
        ".".into()
    } else {
        relative
    })
}
fn boolean(value: &Value, key: &str, default: bool) -> anyhow::Result<bool> {
    value.get(key).map_or(Ok(default), |v| {
        v.as_bool()
            .with_context(|| format!("{key} must be boolean"))
    })
}
fn strings(value: &Value, key: &str, default: &[&str]) -> anyhow::Result<Vec<String>> {
    let Some(v) = value.get(key) else {
        return Ok(default.iter().map(|s| (*s).to_string()).collect());
    };
    let values: Vec<String> = v
        .as_array()
        .context("audit list must be an array")?
        .iter()
        .map(|s| {
            s.as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .context("audit list entries must be strings")
        })
        .collect::<anyhow::Result<_>>()?;
    anyhow::ensure!(
        values
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == values.len(),
        "audit list contains a duplicate"
    );
    Ok(values)
}
fn keys(value: &Value, allowed: &[&str]) -> anyhow::Result<()> {
    for key in value
        .as_table()
        .context("audit configuration must be table")?
        .keys()
    {
        anyhow::ensure!(
            allowed.contains(&key.as_str()),
            "unknown audit field: {key}"
        );
    }
    Ok(())
}
fn plan(root: &Path) -> anyhow::Result<Vec<Step>> {
    let advisory_pattern = regex::Regex::new(r"^RUSTSEC-\d{4}-\d{4}$")?;
    let config = toml::from_str::<Value>(&fs::read_to_string(root.join("security-audit.toml"))?)?;
    keys(&config, &["version", "cargo", "osv"])?;
    anyhow::ensure!(
        config.get("version").and_then(Value::as_integer) == Some(1),
        "unsupported audit policy version"
    );
    let empty = Value::Table(Default::default());
    let cargo = config.get("cargo").unwrap_or(&empty);
    keys(cargo, &["audit", "deny", "vet"])?;
    let mut result = Vec::new();
    for kind in ["audit", "deny", "vet"] {
        let Some(entries) = cargo.get(kind) else {
            continue;
        };
        for entry in entries
            .as_array()
            .context("cargo audit entries must be array")?
        {
            let mut args = vec![kind.to_string()];
            let mut report = false;
            match kind {
                "audit" => {
                    keys(entry, &["lockfile", "ignore", "deny", "report"])?;
                    let lock = relative_path(root, text(entry, "lockfile")?, false)?;
                    args.extend(["--color".into(), "never".into(), "--file".into(), lock]);
                    for id in strings(entry, "ignore", &[])? {
                        anyhow::ensure!(advisory_pattern.is_match(&id), "invalid advisory id");
                        args.extend(["--ignore".into(), id]);
                    }
                    for warning in strings(entry, "deny", &[])? {
                        anyhow::ensure!(
                            ["unmaintained", "unsound", "warnings", "yanked"]
                                .contains(&warning.as_str()),
                            "invalid audit warning"
                        );
                        args.extend(["--deny".into(), warning]);
                    }
                    report = boolean(entry, "report", false)?;
                }
                "deny" | "vet" => {
                    keys(
                        entry,
                        if kind == "deny" {
                            &["manifest", "config", "all-features", "checks"]
                        } else {
                            &["manifest", "store"]
                        },
                    )?;
                    let manifest = relative_path(root, text(entry, "manifest")?, false)?;
                    args.extend(["--manifest-path".into(), manifest]);
                    let key = if kind == "deny" { "config" } else { "store" };
                    if entry.get(key).is_some() {
                        let value = relative_path(root, text(entry, key)?, kind == "vet")?;
                        args.extend([
                            if kind == "deny" {
                                "--config"
                            } else {
                                "--store-path"
                            }
                            .into(),
                            value,
                        ]);
                    }
                    if kind == "deny" && boolean(entry, "all-features", false)? {
                        args.push("--all-features".into());
                    }
                    args.push("--locked".into());
                    if kind == "deny" {
                        args.push("check".into());
                        for check in strings(
                            entry,
                            "checks",
                            &["advisories", "licenses", "sources", "bans"],
                        )? {
                            anyhow::ensure!(
                                ["advisories", "licenses", "sources", "bans"]
                                    .contains(&check.as_str()),
                                "invalid deny check"
                            );
                            args.push(check);
                        }
                    }
                }
                _ => unreachable!(),
            }
            result.push(Step {
                tool: "cargo".into(),
                args,
                report,
            });
        }
    }
    if let Some(osv) = config.get("osv") {
        keys(osv, &["config", "lockfiles", "report"])?;
        let mut args = vec!["scan".into(), "source".into()];
        if osv.get("config").is_some() {
            let value = relative_path(root, text(osv, "config")?, false)?;
            args.push(format!("--config={value}"));
        }
        let locks = strings(osv, "lockfiles", &[])?;
        anyhow::ensure!(!locks.is_empty(), "OSV lockfiles missing");
        for lock in locks {
            let lock = relative_path(root, &lock, false)?;
            args.push(format!("--lockfile={lock}"));
        }
        result.push(Step {
            tool: "osv-scanner".into(),
            args,
            report: boolean(osv, "report", false)?,
        });
    }
    anyhow::ensure!(!result.is_empty(), "audit policy has no steps");
    Ok(result)
}

pub(super) fn check(root: &Path, mode: &str, output: Option<&Path>) -> anyhow::Result<Vec<String>> {
    let steps = plan(root)?;
    if let Some(output) = output {
        fs::write(
            output,
            serde_json::to_vec_pretty(
                &steps
                    .iter()
                    .map(|s| serde_json::json!({"tool":s.tool,"args":s.args,"report":s.report}))
                    .collect::<Vec<_>>(),
            )?,
        )?;
    }
    if mode == "validate" {
        return Ok(Vec::new());
    }
    let catalog = toml::from_str::<Value>(&fs::read_to_string(
        root.join(".orderhub-engineering/tools.toml"),
    )?)?;
    let mut failures = Vec::new();
    let mut checked = std::collections::BTreeSet::new();
    let stable_pattern = regex::Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$")?;
    let local_catalog = root.join("tools.toml");
    let local = if local_catalog.is_file() {
        toml::from_str::<Value>(&fs::read_to_string(local_catalog)?)?
    } else {
        Value::Table(Default::default())
    };
    for step in &steps {
        let name = if step.tool == "cargo" {
            format!("cargo-{}", step.args[0])
        } else {
            step.tool.clone()
        };
        if !checked.insert(name.clone()) {
            continue;
        }
        anyhow::ensure!(
            local.get(&name).is_none(),
            "duplicate tool catalog entry: {name}"
        );
        let expected = text(
            catalog
                .get(&name)
                .context("tool version catalog entry missing")?,
            "version",
        )?;
        anyhow::ensure!(
            stable_pattern.is_match(expected),
            "tool pin must be a stable X.Y.Z release"
        );
        let mut command = Command::new(&step.tool);
        if step.tool == "cargo" {
            command.arg(&step.args[0]);
        }
        let version = command
            .arg("--version")
            .current_dir(root)
            .output()
            .with_context(|| format!("Run {name} --version"))?;
        let actual = format!(
            "{} {}",
            String::from_utf8_lossy(&version.stdout),
            String::from_utf8_lossy(&version.stderr)
        );
        if !version.status.success() || reported_version(&actual).as_deref() != Some(expected) {
            failures.push(format!(
                "{name}: expected version {expected}, received {}",
                actual.trim()
            ));
        }
    }
    if !failures.is_empty() || mode == "check-tools" {
        return Ok(failures);
    }
    for step in steps {
        let status = Command::new(&step.tool)
            .args(&step.args)
            .current_dir(root)
            .status()
            .with_context(|| format!("Start {} audit", step.tool))?;
        if !status.success() {
            failures.push(format!(
                "Audit failed ({status}): {} {}",
                step.tool,
                step.args.join(" ")
            ));
        }
    }
    Ok(failures)
}

fn reported_version(output: &str) -> Option<String> {
    let pattern = regex::Regex::new(
        r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?",
    ).expect("tool version pattern");
    pattern
        .find_iter(output)
        .find(|m| {
            let before = output[..m.start()].chars().next_back();
            let after = output[m.end()..].chars().next();
            !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '.')
                && !after.is_some_and(|c| c.is_ascii_alphanumeric() || ".+-".contains(c))
        })
        .map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    #[rstest]
    fn policy_keeps_cargo_gates_and_rejects_escaping_paths() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let steps = plan(&root).unwrap();
        assert_eq!(steps.len(), 4);
        assert!(steps.iter().take(3).all(|s| !s.report));
        assert!(path(&root, "../Cargo.lock", false).is_err());
        assert!(path(&root, "crates", false).is_err());
        assert!(path(&root, "Cargo.lock", true).is_err());
        assert_eq!(
            relative_path(&root, "./Cargo.lock", false).unwrap(),
            "Cargo.lock"
        );
        assert_eq!(
            reported_version("audit 1.2.3 (rustc 1.98.1)"),
            Some("1.2.3".into())
        );
        assert_eq!(reported_version("v1.2.3"), None);
        assert!(strings(&toml::from_str("x=['a','a']").unwrap(), "x", &[]).is_err());
    }
}
