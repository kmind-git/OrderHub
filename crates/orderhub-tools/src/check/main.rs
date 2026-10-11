// SPDX-License-Identifier: LGPL-3.0-only

//! Repository checks which do not require Python or a database service.

mod commit_message;
mod dependency_features;
mod doc_tools;
mod retired;
mod security;
mod test_network;
mod workspace;

use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    /// Repository to inspect.
    #[arg(long, default_value = ".", global = true)]
    root: PathBuf,
    /// Prints diagnostics as a JSON array.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Checks direct dependency features and lockfile consistency.
    DependencyFeatures,
    /// Detects literal external destinations and live opt-ins in tests.
    TestNetwork,
    /// Reconciles workspace members and the Rust test inventory.
    WorkspaceTests,
    /// Checks the local commit message policy; does not create a commit.
    CommitMessage {
        path: PathBuf,
        #[arg(long)]
        raw: bool,
    },
    /// Normalizes Markdown table padding; changed files return exit status 1.
    MarkdownTables {
        paths: Vec<PathBuf>,
        /// Aligns rendered Unicode width for MD060, including CJK and emoji.
        #[arg(long)]
        display_width: bool,
    },
    /// Generates a Markdown inventory of links in Rust documentation.
    DocLinks { output: PathBuf },
    /// Validates or runs the retained supply-chain audit policy.
    Security {
        #[arg(value_parser=["validate", "check-tools", "run"])]
        mode: String,
        #[arg(long)]
        plan: Option<PathBuf>,
    },
    /// Rejects reintroduction of retired binding/plugin/example source surfaces.
    RetiredSurfaces,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = cli
        .root
        .canonicalize()
        .map_err(anyhow::Error::from)
        .and_then(|root| match cli.command {
            Command::DependencyFeatures => dependency_features::check_repository(&root),
            Command::TestNetwork => test_network::check_repository(&root),
            Command::WorkspaceTests => workspace::check(&root),
            Command::CommitMessage { path, raw } => commit_message::check_file(&root, &path, raw),
            Command::MarkdownTables {
                paths,
                display_width,
            } => doc_tools::tables(&paths, display_width),
            Command::DocLinks { output } => doc_tools::links(&root, &output),
            Command::Security { mode, plan } => security::check(&root, &mode, plan.as_deref()),
            Command::RetiredSurfaces => retired::check(&root),
        });
    match result {
        Ok(violations) => {
            if cli.json {
                println!("{}", serde_json::json!(violations));
            } else if violations.is_empty() {
                println!("Checks passed");
            } else {
                for violation in &violations {
                    eprintln!("{violation}");
                }
            }
            if violations.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("Repository check failed: {e:#}");
            ExitCode::FAILURE
        }
    }
}
