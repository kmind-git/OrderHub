// SPDX-License-Identifier: LGPL-3.0-only

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use orderhub_tools::legacy::catalog::{migrate_parquet, verify_artifact};

#[derive(Debug, Parser)]
#[command(version, about = "Offline versioned OrderHub catalog conversion")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Parquet {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        destination: PathBuf,
        #[arg(long)]
        record_type: String,
    },
    Verify {
        #[arg(long)]
        artifact: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let summary = match Cli::parse().command {
        Command::Parquet {
            source,
            destination,
            record_type,
        } => migrate_parquet(&source, &destination, &record_type)?,
        Command::Verify { artifact } => verify_artifact(&artifact)?,
    };
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
