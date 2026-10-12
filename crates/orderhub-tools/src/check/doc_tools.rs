// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// SPDX-License-Identifier: LGPL-3.0-only

use regex::Regex;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn cells(row: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut start = 0;
    let bytes = row.as_bytes();
    for (index, value) in bytes.iter().enumerate() {
        if *value == b'|' && (index == 0 || bytes[index - 1] != b'\\') {
            result.push(row[start..index].to_string());
            start = index + 1;
        }
    }
    result.push(row[start..].trim_end().to_string());
    if result.len() < 2 {
        return Vec::new();
    }
    result.remove(0);
    result.pop();
    result
}

fn width(value: &str, display_width: bool) -> usize {
    if display_width {
        unicode_width::UnicodeWidthStr::width(value)
    } else {
        value.encode_utf16().count()
    }
}

pub(super) fn normalize(source: &str, display_width: bool) -> String {
    let mut lines: Vec<_> = source.split('\n').map(str::to_string).collect();
    let fence_re = Regex::new(r"^\s{0,3}(`{3,}|~{3,})\s*(.*)$").expect("fence pattern");
    let delimiter = Regex::new(r"^\|(?:\s*:?-+:?\s*\|)+$").expect("table delimiter");
    let mut fence: Option<String> = None;
    let mut index = 0;
    while index < lines.len() {
        if let Some(c) = fence_re.captures(&lines[index]) {
            let marker = c[1].to_string();
            if let Some(open) = &fence {
                if marker.starts_with(open.chars().next().expect("fence"))
                    && marker.len() >= open.len()
                    && c[2].is_empty()
                {
                    fence = None;
                }
            } else {
                fence = Some(marker);
            }
            index += 1;
            continue;
        }
        if fence.is_some() || !lines[index].starts_with('|') {
            index += 1;
            continue;
        }
        let start = index;
        while index < lines.len() && lines[index].starts_with('|') {
            index += 1;
        }
        if index - start < 2 || !delimiter.is_match(lines[start + 1].trim_end()) {
            continue;
        }
        let grid: Vec<_> = lines[start..index].iter().map(|l| cells(l)).collect();
        let columns = grid[0].len();
        if columns == 0 || grid.iter().any(|r| r.len() != columns) {
            continue;
        }
        let widths: Vec<_> = (0..columns)
            .map(|c| {
                let marker = grid[1][c].trim();
                let colons =
                    usize::from(marker.starts_with(':')) + usize::from(marker.ends_with(':'));
                let max = grid
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != 1)
                    .map(|(_, r)| width(r[c].trim(), display_width))
                    .max()
                    .unwrap_or(0);
                max.max(3 + colons) + 2
            })
            .collect();
        for (row, items) in grid.iter().enumerate() {
            let mut rendered = Vec::new();
            for (c, cell) in items.iter().enumerate() {
                let content = cell.trim();
                let width = widths[c];
                if row == 1 {
                    let left = content.starts_with(':');
                    let right = content.ends_with(':');
                    rendered.push(format!(
                        " {}{}{} ",
                        if left { ":" } else { "" },
                        "-".repeat(width - 2 - usize::from(left) - usize::from(right)),
                        if right { ":" } else { "" }
                    ));
                } else {
                    let padding = width - 2 - self::width(content, display_width);
                    let leading = cell.len() - cell.trim_start_matches(' ').len();
                    let trailing = cell.len() - cell.trim_end_matches(' ').len();
                    let left = if leading > 1 && trailing > 1 {
                        padding / 2
                    } else if leading > 1 {
                        padding
                    } else {
                        0
                    };
                    rendered.push(format!(
                        " {}{content}{} ",
                        " ".repeat(left),
                        " ".repeat(padding - left)
                    ));
                }
            }
            lines[start + row] = format!("|{}|", rendered.join("|"));
        }
    }
    lines.join("\n")
}

pub(super) fn tables(paths: &[PathBuf], display_width: bool) -> anyhow::Result<Vec<String>> {
    let mut changed = Vec::new();
    let mut paths = paths.to_vec();
    paths.sort();
    for path in paths
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
    {
        // Match text-mode reads in the retired formatter. Preserve original bytes if no
        // table changes; normalize newlines only when the formatter rewrites a file.
        let source = fs::read_to_string(path)?
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let result = normalize(&source, display_width);
        if result != source {
            fs::write(path, result)?;
            changed.push(format!(
                "{}: normalized table column widths",
                path.display()
            ));
        }
    }
    Ok(changed)
}

pub(super) fn links(root: &Path, output: &Path) -> anyhow::Result<Vec<String>> {
    let paths = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            "crates/*.rs",
            ":(glob)crates/**/*.rs",
        ])
        .current_dir(root)
        .output()?;
    anyhow::ensure!(paths.status.success(), "git source inventory failed");
    let doc = Regex::new(r"^\s*//[/!]")?;
    let constant = Regex::new(r"\b(?:const|static)\s+[A-Z0-9_]*DOCS_URL\b")?;
    let url = Regex::new(r#"https?://[^\s<>()\[\]"'`\\]+"#)?;
    let mut links = BTreeMap::new();
    let mut sources = std::str::from_utf8(&paths.stdout)?
        .split('\0')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    sources.sort_unstable();
    sources.dedup();
    for source in sources {
        let path = root.join(source);
        if !path.is_file() {
            continue;
        }
        let mut in_constant = false;
        for (index, line) in String::from_utf8_lossy(&fs::read(path)?)
            .lines()
            .enumerate()
        {
            if constant.is_match(line) {
                in_constant = true;
            }
            if in_constant || doc.is_match(line) {
                for found in url.find_iter(line) {
                    links
                        .entry(
                            found
                                .as_str()
                                .trim_end_matches(['.', ',', ';', ':'])
                                .to_string(),
                        )
                        .or_insert((source.to_string(), index + 1));
                }
            }
            if in_constant && line.contains(';') {
                in_constant = false;
            }
        }
    }
    let mut ordered = links.into_iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| a.1.cmp(&b.1));
    let mut content="# Doc comment links\n\nGenerated by `orderhub-check doc-links`; do not edit or commit.\n\n".to_string();
    for (url, (source, line)) in ordered {
        content.push_str(&format!("- [{source}:{line}]({url})\n"));
    }
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, content)?;
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    #[rstest]
    fn tables_preserve_fences_and_utf16_width() {
        let s = "| a | 中😀 |\n| :- | -: |\n| b | x |\n\n```\n|a|b|\n|-|-|\n```\n";
        let n = normalize(s, false);
        assert!(n.starts_with("| a    | 中😀  |\n| :--- | ---: |\n| b    | x    |\n"));
        assert!(n.contains("```\n|a|b|\n|-|-|\n```"));
        assert_eq!(normalize(&n, false), n);
        let display = normalize(s, true);
        assert!(display.starts_with("| a    | 中😀 |\n"));
        assert_eq!(normalize(&display, true), display);
    }
}
