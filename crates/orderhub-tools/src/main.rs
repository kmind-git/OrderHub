// SPDX-License-Identifier: LGPL-3.0-only

use std::{
    fmt::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use nautilus_infrastructure::sql::pg::{PostgresConnectOptions, connect_pg, init_postgres};
use sha2::{Digest, Sha256};

const SCHEMA_FILES: [&str; 4] = ["types.sql", "tables.sql", "functions.sql", "partitions.sql"];

#[derive(Debug, Parser)]
#[command(version, about = "OrderHub PostgreSQL schema maintenance")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Checks the packaged schema files without connecting to a database.
    SchemaCheck {
        /// Directory containing the four PostgreSQL schema files.
        #[arg(long)]
        schema: PathBuf,
    },
    /// Initializes or migrates an explicitly selected PostgreSQL database.
    Init {
        #[arg(long)]
        schema: PathBuf,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        database: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::SchemaCheck { schema } => {
            for (name, size, digest) in inspect_schema(&schema)? {
                println!("{name}\t{size}\t{digest}");
            }
        }
        Command::Init {
            schema,
            host,
            port,
            username,
            database,
        } => {
            inspect_schema(&schema)?;
            let schema = schema
                .canonicalize()
                .with_context(|| format!("Cannot resolve schema directory: {}", schema.display()))?
                .into_os_string()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Schema path is not valid UTF-8"))?;
            let options = connection_options(host, port, username, database)?;
            let pool = connect_pg(options.clone().into()).await?;
            init_postgres(&pool, options.database, options.password, Some(schema)).await?;
        }
    }
    Ok(())
}

fn connection_options(
    host: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    database: Option<String>,
) -> Result<PostgresConnectOptions> {
    let host = host
        .or_else(|| std::env::var("POSTGRES_HOST").ok())
        .unwrap_or_else(|| String::from("localhost"));
    let port = match port {
        Some(port) => port,
        None => std::env::var("POSTGRES_PORT")
            .ok()
            .map(|value| value.parse::<u16>().context("Invalid POSTGRES_PORT"))
            .transpose()?
            .unwrap_or(5432),
    };
    let username = username
        .or_else(|| std::env::var("POSTGRES_USERNAME").ok())
        .context("Supply --username or POSTGRES_USERNAME")?;
    let password = std::env::var("POSTGRES_PASSWORD").context("Supply POSTGRES_PASSWORD")?;
    let database = database
        .or_else(|| std::env::var("POSTGRES_DATABASE").ok())
        .context("Supply --database or POSTGRES_DATABASE")?;
    anyhow::ensure!(
        !username.is_empty(),
        "PostgreSQL username must not be empty"
    );
    anyhow::ensure!(!host.is_empty(), "PostgreSQL host must not be empty");
    anyhow::ensure!(port != 0, "PostgreSQL port must not be zero");
    anyhow::ensure!(
        !password.is_empty(),
        "PostgreSQL password must not be empty"
    );
    anyhow::ensure!(
        !database.is_empty(),
        "PostgreSQL database must not be empty"
    );
    Ok(PostgresConnectOptions::new(
        host, port, username, password, database,
    ))
}

fn inspect_schema(schema: &Path) -> Result<Vec<(&'static str, usize, String)>> {
    let mut files = Vec::with_capacity(SCHEMA_FILES.len());
    for name in SCHEMA_FILES {
        let path = schema.join(name);
        let data = std::fs::read(&path)
            .with_context(|| format!("Cannot read schema file: {}", path.display()))?;
        anyhow::ensure!(!data.is_empty(), "Schema file is empty: {}", path.display());
        std::str::from_utf8(&data)
            .with_context(|| format!("Schema file is not UTF-8: {}", path.display()))?;
        let mut digest = String::with_capacity(64);
        for byte in Sha256::digest(&data) {
            write!(&mut digest, "{byte:02x}").expect("writing to String cannot fail");
        }
        files.push((name, data.len(), digest));
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    fn parses_existing_database_init_arguments() {
        let cli = Cli::try_parse_from([
            "orderhub-db",
            "init",
            "--schema",
            "schema/sql",
            "--host",
            "localhost",
            "--port",
            "5432",
            "--username",
            "admin",
            "--database",
            "orderhub",
        ])
        .unwrap();
        assert!(matches!(cli.command, Command::Init { .. }));
    }

    #[rstest]
    fn rejects_password_on_command_line() {
        assert!(
            Cli::try_parse_from([
                "orderhub-db",
                "init",
                "--schema",
                "schema/sql",
                "--password",
                "secret",
            ])
            .is_err()
        );
    }

    #[rstest]
    fn current_schema_files_are_readable() {
        let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/sql");
        let files = inspect_schema(&schema).unwrap();
        assert_eq!(files.len(), SCHEMA_FILES.len());
    }

    #[rstest]
    fn missing_schema_is_rejected_before_connecting() {
        let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join("missing-schema");
        assert!(inspect_schema(&schema).is_err());
    }
}
