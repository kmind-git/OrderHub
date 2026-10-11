// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

use anyhow::Context;
use regex::Regex;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn pattern(value: &str) -> Regex {
    Regex::new(value).expect("static commit policy expression")
}

pub(super) fn check(message: &str) -> Vec<String> {
    let lines: Vec<_> = message.lines().collect();
    let subject = lines.first().copied().unwrap_or_default();
    let mut errors = Vec::new();
    if subject.chars().count() < 10 {
        errors.push("subject must contain at least 10 characters".into());
    }
    if !subject.chars().next().is_some_and(char::is_uppercase) {
        errors.push("subject must start with a capitalized word".into());
    }
    if subject.ends_with('.') {
        errors.push("subject must not end with a period".into());
    }
    if pattern(r"(?i)^(?:build|chore|ci|docs|feat|fix|perf|refactor|revert|style|test)(?:\([^()\r\n]+\))?!?:").is_match(subject){errors.push("subject must not use Conventional Commits syntax".into());}
    if pattern(r"#\d+").is_match(subject) {
        errors.push("subject must not include an issue or pull request number".into());
    }
    if lines.iter().skip(1).any(|l| !l.trim().is_empty()) {
        if lines.get(1).is_some_and(|l| !l.trim().is_empty()) {
            errors.push("body must be separated from the subject by a blank line".into());
        }
        for (index, line) in lines.iter().enumerate().skip(1) {
            let length = line.chars().count();
            if length > 79 {
                errors.push(format!(
                    "line {} contains {length} characters; body lines must contain 79 or fewer",
                    index + 1
                ));
            }
        }
    }
    let phrase = pattern(
        r"(?i)\b(?:(?:authored|built|created|developed|generated|made|powered|produced|written)\s+(?:by|using|with)|(?:ai\W+)?assisted\s+(?:by|using|with)|thanks\s+to|with\s+(?:ai\s+)?assistance\s+from)\b",
    );
    let brand = pattern(
        r"(?i)\b(?:anthropic|chatgpt|claude(?:\s+code)?|codex|copilot|cursor|devin|gemini|github\s+copilot|google\s+ai|jules|openai|windsurf)\b",
    );
    let identity = pattern(
        r"(?i)(?:^|[^a-z0-9])(?:ai[^a-z0-9]+(?:agent|assistant|bot|model|tool)|anthropic|chatgpt|claude[^a-z0-9]+(?:agent|bot|code|haiku|opus|sonnet|v?\d+(?:\.\d+)*(?:[^a-z0-9]+(?:haiku|opus|sonnet))?)|codex|copilot|cursor[^a-z0-9]+(?:agent|bot|ai)|devin[^a-z0-9]+(?:agent|bot|ai)|gemini[^a-z0-9]+(?:agent|bot|code)|google[^a-z0-9]+ai|jules[^a-z0-9]+(?:agent|bot)|openai|windsurf[^a-z0-9]+(?:agent|bot|ai))(?:$|[^a-z0-9])",
    );
    let trailer = pattern(r"(?i)^\s*co-authored-by\s*:");
    let mut branded = std::collections::BTreeSet::new();
    for (index, line) in lines.iter().enumerate() {
        if phrase.is_match(line) && brand.is_match(line) {
            branded.insert(index);
        }
    }
    let mut start = 0;
    let mut paragraph = Vec::new();
    for (index, line) in lines.iter().copied().chain(std::iter::once("")).enumerate() {
        if !line.trim().is_empty() {
            if paragraph.is_empty() {
                start = index;
            }
            paragraph.push(line);
        } else if !paragraph.is_empty() {
            let joined = paragraph.join(" ");
            if !branded.range(start..index).next().is_some()
                && phrase.is_match(&joined)
                && brand.is_match(&joined)
            {
                branded.insert(start);
            }
            paragraph.clear();
        }
    }
    for (index, line) in lines.iter().enumerate() {
        let ai = trailer.is_match(line)
            && line.split_once(':').is_some_and(|(_, value)| {
                let name = value
                    .split('<')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_lowercase();
                [
                    "ai",
                    "anthropic",
                    "chatgpt",
                    "claude",
                    "codex",
                    "copilot",
                    "cursor",
                    "devin",
                    "gemini",
                    "jules",
                    "openai",
                    "windsurf",
                ]
                .contains(&name.as_str())
                    || identity.is_match(value)
            });
        if ai {
            errors.push(format!(
                "line {} contains an AI co-author trailer",
                index + 1
            ));
        } else if branded.contains(&index) {
            errors.push(format!(
                "line {} contains branded AI attribution",
                index + 1
            ));
        }
    }
    errors
}

pub(super) fn check_file(root: &Path, path: &Path, raw: bool) -> anyhow::Result<Vec<String>> {
    let message = fs::read_to_string(path)?;
    if raw {
        guidance(&message);
        return Ok(check(&message));
    }
    let scissors =
        pattern(r"^\s*[^\w\s]\s+------------------------ >8 ------------------------\s*$");
    let input = message
        .lines()
        .take_while(|l| !scissors.is_match(l))
        .collect::<Vec<_>>()
        .join("\n");
    let mut child = Command::new("git")
        .args(["stripspace", "--strip-comments"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Clean commit message")?;
    child
        .stdin
        .take()
        .context("git stdin")?
        .write_all(input.as_bytes())?;
    let output = child.wait_with_output()?;
    anyhow::ensure!(output.status.success(), "git stripspace failed");
    let message = std::str::from_utf8(&output.stdout)?;
    guidance(message);
    Ok(check(message))
}

fn guidance(message: &str) {
    let length = message.lines().next().unwrap_or_default().chars().count();
    if length > 60 {
        eprintln!(
            "WARNING: commit message: subject contains {length} characters; aim for 60 or fewer because this limit will be enforced in the future"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    #[rstest]
    fn meaningful_subjects_and_wrapped_attribution() {
        assert!(
            check("Improve local tool checks\n\nKeep business validation active.\n").is_empty()
        );
        assert_eq!(
            check("Improve local tool checks\n\nGenerated with\nCodex\n"),
            vec!["line 3 contains branded AI attribution"]
        );
        assert!(!check("fix: bad.").is_empty());
    }
}
