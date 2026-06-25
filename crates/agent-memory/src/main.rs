//! `agent-memory` — a persistent typed-memory MCP stdio server over embedded SQLite.
//!
//! stdout is the JSON-RPC channel and must stay pure (MCP-05): all logging goes to
//! stderr, ANSI is disabled, and a panic hook routes any residual panic to stderr.
//! `anyhow` is used at these binary edges; the core library uses `thiserror`.

mod config;
mod mcp;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};
use rmcp::transport::stdio;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use agent_memory_core::clock::SystemClock;
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::service::MemoryService;
use agent_memory_core::store::sqlite::SqliteStore;

use crate::config::resolve_db_path;
use crate::mcp::{AppState, MemoryMcp};

/// CLI for the agent-memory MCP server.
#[derive(Debug, Parser)]
#[command(name = "agent-memory", version, about)]
struct Cli {
    /// Path to the SQLite database. Overrides `AGENT_MEMORY_DB` and the default.
    #[arg(long, global = true, env = "AGENT_MEMORY_DB")]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the MCP stdio server.
    Serve,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stderr-only logging + panic hook MUST be installed before anything writes,
    // so stdout stays a pure JSON-RPC channel (MCP-05).
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    std::panic::set_hook(Box::new(|info| {
        eprintln!("PANIC: {info}");
    }));

    let cli = Cli::parse();

    match cli.command {
        Command::Serve => serve(cli.db).await,
    }
}

/// Resolve the DB path, open the store, and serve the MCP stdio transport.
async fn serve(db_flag: Option<PathBuf>) -> anyhow::Result<()> {
    let db_path = resolve_db_path(db_flag).context("resolving database path")?;
    let store = SqliteStore::open(&db_path)
        .with_context(|| format!("opening database at {}", db_path.display()))?;

    let service = MemoryService::new(
        Arc::new(store),
        Arc::new(SystemClock),
        DecayConfig::default(),
    );
    let state = Arc::new(AppState { service });

    let running = MemoryMcp::new(state)
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("serving error: {e:?}"))
        .context("starting MCP stdio server")?;

    running.waiting().await.context("server run loop")?;
    Ok(())
}
