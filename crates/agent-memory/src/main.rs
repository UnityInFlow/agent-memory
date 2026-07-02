//! `agent-memory` — a persistent typed-memory MCP stdio server over embedded SQLite.
//!
//! stdout is the JSON-RPC channel and must stay pure (MCP-05): all logging goes to
//! stderr, ANSI is disabled, and a panic hook routes any residual panic to stderr.
//! `anyhow` is used at these binary edges; the core library uses `thiserror`.

mod config;
mod mcp;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use rmcp::transport::stdio;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use agent_memory_core::clock::{Clock, SystemClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::embed::ollama::OllamaClient;
use agent_memory_core::embed::{Embedder, EmbedderHealth};
use agent_memory_core::service::MemoryService;
use agent_memory_core::store::sqlite::SqliteStore;

/// How often the background lifecycle sweep runs (TTL expiry + decay
/// materialization). Hourly is a sane default — TTL is coarse-grained and decay
/// materialization only keeps ranking cheap between reads (search recomputes decay
/// on read regardless), so a tight interval buys nothing.
const SWEEP_INTERVAL: Duration = Duration::from_secs(3600);

use crate::config::resolve_db_path;
use crate::mcp::{AppState, MemoryMcp};

/// CLI for the agent-memory MCP server.
#[derive(Debug, Parser)]
#[command(name = "agent-memory", version, about)]
struct Cli {
    /// Path to the SQLite database. Overrides `AGENT_MEMORY_DB` and the default.
    #[arg(long, global = true, env = "AGENT_MEMORY_DB")]
    db: Option<PathBuf>,

    /// Ollama base URL for semantic embeddings. The URL comes ONLY from this
    /// flag/env, never from request payloads (T-02-02). When unreachable,
    /// search degrades to keyword mode and stores stay pending (SEARCH-03).
    #[arg(
        long,
        global = true,
        env = "AGENT_MEMORY_OLLAMA_URL",
        default_value = "http://localhost:11434"
    )]
    ollama_url: String,

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
        Command::Serve => serve(cli.db, cli.ollama_url).await,
    }
}

/// Resolve the DB path, open the store, and serve the MCP stdio transport.
async fn serve(db_flag: Option<PathBuf>, ollama_url: String) -> anyhow::Result<()> {
    let db_path = resolve_db_path(db_flag).context("resolving database path")?;
    let store = SqliteStore::open(&db_path)
        .with_context(|| format!("opening database at {}", db_path.display()))?;

    let embedder = Arc::new(OllamaClient::new(ollama_url.clone()));

    // Startup visibility (RESEARCH Pattern 2 rule 3): probe the embedder ONCE
    // on a detached task and log exactly one stderr line about semantic
    // availability. Serving NEVER blocks or exits on this probe.
    spawn_health_probe(embedder.clone(), ollama_url);

    let service = MemoryService::new(
        Arc::new(store),
        Arc::new(SystemClock),
        embedder,
        DecayConfig::default(),
    );
    let state = Arc::new(AppState { service });

    // Spawn the background lifecycle sweep BEFORE serving: on each tick it deletes
    // TTL-expired rows and materializes decay scores, logging the SweepReport to
    // STDERR only (NEVER stdout — MCP-05 must hold even with the sweep running).
    // The task holds a clone of the service + a real clock; it lives for the
    // process lifetime and is dropped when the runtime shuts down.
    spawn_sweep_task(state.service.clone(), Arc::new(SystemClock));

    let running = MemoryMcp::new(state)
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("serving error: {e:?}"))
        .context("starting MCP stdio server")?;

    running.waiting().await.context("server run loop")?;
    Ok(())
}

/// Spawn the detached one-shot embedder health probe.
///
/// Logs EXACTLY one stderr line via `tracing` (MCP-05 holds — nothing here
/// touches stdout): `Ready` → info; `ModelMissing` → warn with the actionable
/// `ollama pull` hint; `Unreachable` → warn naming the URL. The probe is fire-
/// and-forget: startup never blocks or exits on it (SEARCH-03 — the server is
/// fully usable in keyword mode without Ollama).
fn spawn_health_probe(embedder: Arc<OllamaClient>, ollama_url: String) {
    tokio::spawn(async move {
        match embedder.health().await {
            Ok(EmbedderHealth::Ready) => {
                tracing::info!("semantic search ready (nomic-embed-text)");
            }
            Ok(EmbedderHealth::ModelMissing) => {
                tracing::warn!(
                    "Ollama is running but the embedding model is missing — \
                     run: ollama pull nomic-embed-text (search falls back to keyword until then)"
                );
            }
            Ok(EmbedderHealth::Unreachable(_)) => {
                tracing::warn!("Ollama unreachable at {ollama_url} — search falls back to keyword");
            }
            Err(e) => {
                tracing::warn!("Ollama health probe failed ({e}) — search falls back to keyword");
            }
        }
    });
}

/// Spawn the detached background sweep loop on a `tokio::time::interval`.
///
/// Each tick runs `service.sweep(clock.now())` (TTL delete + decay
/// materialization) and logs the resulting `SweepReport` to stderr via `tracing`.
/// A sweep failure is logged and the loop continues — a transient DB hiccup must
/// not kill the lifecycle task. The first tick fires immediately; subsequent ticks
/// honor [`SWEEP_INTERVAL`]. Nothing here ever writes to stdout (MCP-05).
fn spawn_sweep_task(service: MemoryService, clock: Arc<dyn Clock>) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
        loop {
            ticker.tick().await;
            match service.sweep(clock.now()).await {
                Ok(report) => tracing::info!(
                    expired = report.expired,
                    rescored = report.rescored,
                    "lifecycle sweep complete"
                ),
                Err(e) => tracing::warn!("lifecycle sweep failed: {e}"),
            }
        }
    });
}
