//! `agent-memory` — a persistent typed-memory MCP stdio server over embedded SQLite.
//!
//! stdout is the JSON-RPC channel and must stay pure (MCP-05): all logging goes to
//! stderr, ANSI is disabled, and a panic hook routes any residual panic to stderr.
//! `anyhow` is used at these binary edges; the core library uses `thiserror`.

mod config;
mod export_file;
mod mcp;
mod rest;
mod serde_util;

use std::net::SocketAddr;
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
    /// Run the HTTP REST mirror for non-MCP clients (same store, same logic).
    ServeRest {
        /// Address to bind. Loopback by default; a non-loopback address is
        /// refused unless --allow-remote is also passed (the API carries no
        /// auth — see the security note in the README).
        #[arg(long, default_value = "127.0.0.1:7437")]
        addr: String,

        /// Permit binding a non-loopback address. The REST API is
        /// unauthenticated: anyone who can reach it can read and write
        /// memories. A warning is logged when this is used.
        #[arg(long)]
        allow_remote: bool,
    },
    /// Import memories from an external file: a GSD STATE.md (INTEROP-01) or
    /// an agent-memory export (INTEROP-02). Re-running the same import is a
    /// no-op: duplicates are skipped on the exact (source, type, content) key.
    /// A jsonl import also recreates every link, lands every row pending
    /// embedding (the sweep re-embeds locally), skips rows already expired,
    /// and rejects a malformed file by line before anything is written.
    Import {
        /// Source format. Supported: gsd-state (a GSD .planning/STATE.md
        /// file), jsonl (an agent-memory export).
        #[arg(long)]
        from: String,

        /// Path to the file to import.
        path: PathBuf,

        /// gsd-state only: scope stamped on every imported memory. Default:
        /// the GSD project directory name — the parent of the nearest
        /// `.planning` component in the (canonicalized) path — or the current
        /// directory's name when the path has no `.planning` ancestor. A
        /// jsonl file carries each memory's own scope, so --scope is rejected.
        #[arg(long)]
        scope: Option<String>,
    },
    /// Export every memory and link as format-v1 JSONL (DIST-04): a header
    /// line, one record per memory, one record per link; embedding vectors are
    /// never included. Writes to stdout by default.
    Export {
        /// Write to PATH instead of stdout. The file is created private (0600
        /// on Unix) as a temp file in PATH's directory and renamed into place;
        /// an existing PATH is replaced; an interrupted export never leaves a
        /// partial PATH.
        #[arg(long)]
        output: Option<PathBuf>,
    },
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
        Command::ServeRest { addr, allow_remote } => {
            serve_rest(cli.db, cli.ollama_url, addr, allow_remote).await
        }
        Command::Import { from, path, scope } => {
            run_import(cli.db, cli.ollama_url, from, path, scope).await
        }
        Command::Export { output } => run_export(cli.db, cli.ollama_url, output).await,
    }
}

/// Resolve the DB path, open the store, and serve the MCP stdio transport.
async fn serve(db_flag: Option<PathBuf>, ollama_url: String) -> anyhow::Result<()> {
    let db_path = resolve_db_path(db_flag).context("resolving database path")?;
    let store = SqliteStore::open(&db_path)
        .with_context(|| format!("opening database at {}", db_path.display()))?;

    let embedder: Arc<dyn Embedder> = Arc::new(OllamaClient::new(ollama_url.clone()));

    // Startup visibility (RESEARCH Pattern 2 rule 3): probe the embedder ONCE
    // on a detached task and log exactly one stderr line about semantic
    // availability. Serving NEVER blocks or exits on this probe.
    spawn_health_probe(embedder.clone(), ollama_url);

    let service = MemoryService::new(
        Arc::new(store),
        Arc::new(SystemClock),
        embedder.clone(),
        DecayConfig::default(),
    );
    let state = Arc::new(AppState { service, embedder });

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

/// Resolve the DB path, open the store, and serve the axum REST mirror.
///
/// Same wiring as [`serve`] over the SAME SQLite file — WAL + busy_timeout
/// (Phase 1) make cross-process access safe, so editor-spawned MCP stdio
/// processes and this daemon coexist on one store (API-01, RESEARCH Pattern
/// 4). The lifecycle sweep runs here too: the REST daemon is long-running and
/// must expire TTLs and backfill embeddings like the stdio server does.
async fn serve_rest(
    db_flag: Option<PathBuf>,
    ollama_url: String,
    addr: String,
    allow_remote: bool,
) -> anyhow::Result<()> {
    let db_path = resolve_db_path(db_flag).context("resolving database path")?;
    let store = SqliteStore::open(&db_path)
        .with_context(|| format!("opening database at {}", db_path.display()))?;

    let embedder: Arc<dyn Embedder> = Arc::new(OllamaClient::new(ollama_url.clone()));
    spawn_health_probe(embedder.clone(), ollama_url);

    let service = MemoryService::new(
        Arc::new(store),
        Arc::new(SystemClock),
        embedder.clone(),
        DecayConfig::default(),
    );
    let state = Arc::new(AppState { service, embedder });

    spawn_sweep_task(state.service.clone(), Arc::new(SystemClock));

    let socket_addr: SocketAddr = addr
        .parse()
        .with_context(|| format!("parsing --addr {addr:?} as a socket address"))?;
    rest::ensure_bind_allowed(&socket_addr, allow_remote)?;
    if allow_remote && !socket_addr.ip().is_loopback() {
        tracing::warn!(
            "binding non-loopback address {socket_addr} — the REST API is \
             unauthenticated; anyone who can reach it can read and write memories"
        );
    }

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .with_context(|| format!("binding {socket_addr}"))?;
    // The bound address is logged AFTER binding so `--addr 127.0.0.1:0`
    // resolves to the real ephemeral port — tests parse it from this line.
    tracing::info!("REST listening on {}", listener.local_addr()?);

    axum::serve(listener, rest::build_router(state))
        .await
        .context("serving REST")?;
    Ok(())
}

/// Run a one-shot import: parse the file into typed records, wire the
/// store/embedder exactly like [`serve`], import idempotently, and print the
/// report to STDOUT. stdout is sanctioned here — this is a CLI subcommand, not
/// the MCP transport (MCP-05 applies only to `serve`).
///
/// Two formats, two tolerance policies (Phase 5 D-11):
///
/// - `gsd-state` (INTEROP-01): the tolerant STATE.md scraper; malformed
///   bullets are skip-counted and reported.
/// - `jsonl` (INTEROP-02): the strict format-v1 reader. The WHOLE file is
///   parsed and validated BEFORE the database is resolved or opened, so a
///   malformed file never creates or touches a database and the error names
///   the offending line. The embedder is constructed but never called: every
///   imported row lands pending and the sweep re-embeds it locally (D-13).
async fn run_import(
    db_flag: Option<PathBuf>,
    ollama_url: String,
    from: String,
    path: PathBuf,
    scope: Option<String>,
) -> anyhow::Result<()> {
    match from.as_str() {
        "gsd-state" => {
            let text = read_import_file(&path)?;
            let scope = scope.unwrap_or_else(|| default_scope_for(&path));
            let parsed = agent_memory_core::import::gsd_state::parse_gsd_state(&text, Some(scope));

            let service = open_service(db_flag, ollama_url)?;
            let report = service.import(parsed.drafts).await.context("importing")?;
            println!(
                "imported: {}, skipped duplicates: {}, malformed lines skipped: {}",
                report.imported, report.skipped_duplicates, parsed.skipped
            );
        }
        "jsonl" => {
            if scope.is_some() {
                anyhow::bail!(
                    "--scope applies only to --from gsd-state (a jsonl file carries each memory's own scope)"
                );
            }
            let text = read_import_file(&path)?;
            // Validate the whole file first (D-08 pass 1): a defect is
            // reported by line and the database is never even opened.
            let file = agent_memory_core::import::jsonl::parse_jsonl(&text)
                .with_context(|| format!("validating {}", path.display()))?;

            let service = open_service(db_flag, ollama_url)?;
            let report = service.import_jsonl(file).await.context("importing")?;
            println!(
                "imported: {}, skipped duplicates: {}, skipped expired: {}, links imported: {}, links skipped duplicates: {}, links dropped: {}",
                report.imported,
                report.skipped_duplicates,
                report.skipped_expired,
                report.links_imported,
                report.links_skipped_duplicates,
                report.links_dropped
            );
            let pending = report.imported;
            println!(
                "{pending} rows pending embedding — the sweep backfills them on the next serve/serve-rest start (128 per tick)"
            );
        }
        other => {
            anyhow::bail!(
                "unsupported --from value {other:?} (supported formats: gsd-state, jsonl)"
            );
        }
    }
    Ok(())
}

/// Read an import file to a string with the path in the error context.
fn read_import_file(path: &std::path::Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// Resolve the DB path, open the store and build the service with the real
/// clock and the Ollama embedder — the one-shot subcommand wiring shared by
/// both import formats.
fn open_service(db_flag: Option<PathBuf>, ollama_url: String) -> anyhow::Result<MemoryService> {
    let db_path = resolve_db_path(db_flag).context("resolving database path")?;
    let store = SqliteStore::open(&db_path)
        .with_context(|| format!("opening database at {}", db_path.display()))?;
    let embedder: Arc<dyn Embedder> = Arc::new(OllamaClient::new(ollama_url));
    Ok(MemoryService::new(
        Arc::new(store),
        Arc::new(SystemClock),
        embedder,
        DecayConfig::default(),
    ))
}

/// Run a one-shot export (DIST-04): wire the store/embedder exactly like
/// [`run_import`] (the embedder is constructed but never called — vectors do
/// not travel), then stream the format-v1 JSONL document to STDOUT or, with
/// `--output`, into a private temp file beside the target that is renamed
/// into place only after the whole stream is flushed and synced (D-02). The
/// report line goes to STDERR, always: stdout may BE the file here, so the
/// convention is the reverse of `import`'s.
///
/// The stdout sink is `BufWriter::new(std::io::stdout())`: the `Stdout`
/// handle is `Send`, whereas `StdoutLock` is not and cannot cross into the
/// service's blocking task. The service flushes the writer before returning,
/// so `commit`'s `sync_all` sees every byte.
async fn run_export(
    db_flag: Option<PathBuf>,
    ollama_url: String,
    output: Option<PathBuf>,
) -> anyhow::Result<()> {
    let db_path = resolve_db_path(db_flag).context("resolving database path")?;
    let store = SqliteStore::open(&db_path)
        .with_context(|| format!("opening database at {}", db_path.display()))?;
    let embedder: Arc<dyn Embedder> = Arc::new(OllamaClient::new(ollama_url));
    let service = MemoryService::new(
        Arc::new(store),
        Arc::new(SystemClock),
        embedder,
        DecayConfig::default(),
    );
    // The binary's own version, not the core crate's (Pitfall 9).
    let version = env!("CARGO_PKG_VERSION").to_string();

    let report = match output {
        None => {
            let sink: Box<dyn std::io::Write + Send> =
                Box::new(std::io::BufWriter::new(std::io::stdout()));
            service.export(sink, version).await.context("exporting")?
        }
        Some(path) => {
            let pending = export_file::begin(&path)
                .with_context(|| format!("creating export file beside {}", path.display()))?;
            // Everything that can fail between `begin` and `commit` runs under
            // one result, so exactly one abort covers every failure and the
            // temp never outlives an error.
            let streamed: anyhow::Result<_> = async {
                let file = pending
                    .writer()
                    .context("duplicating the export file handle")?;
                let sink: Box<dyn std::io::Write + Send> = Box::new(std::io::BufWriter::new(file));
                service.export(sink, version).await.context("exporting")
            }
            .await;
            match streamed {
                Ok(report) => {
                    pending.commit().with_context(|| {
                        format!("moving the export into place at {}", path.display())
                    })?;
                    report
                }
                Err(e) => {
                    pending.abort();
                    return Err(e);
                }
            }
        }
    };

    let (memories, links) = (report.memories, report.links);
    eprintln!("exported {memories} memories and {links} links");
    Ok(())
}

/// Default scope for an import when `--scope` is absent: the GSD project
/// directory name — the parent of the nearest `.planning` component in the
/// canonicalized path — else the current working directory's name.
fn default_scope_for(path: &std::path::Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    for ancestor in canonical.ancestors() {
        if ancestor.file_name().is_some_and(|name| name == ".planning") {
            if let Some(project) = ancestor.parent().and_then(|p| p.file_name()) {
                return project.to_string_lossy().into_owned();
            }
        }
    }
    std::env::current_dir()
        .ok()
        .and_then(|dir| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "default".to_string())
}

/// Spawn the detached one-shot embedder health probe.
///
/// Logs EXACTLY one stderr line via `tracing` (MCP-05 holds — nothing here
/// touches stdout): `Ready` → info; `ModelMissing` → warn with the actionable
/// `ollama pull` hint; `Unreachable` → warn naming the URL. The probe is fire-
/// and-forget: startup never blocks or exits on it (SEARCH-03 — the server is
/// fully usable in keyword mode without Ollama).
fn spawn_health_probe(embedder: Arc<dyn Embedder>, ollama_url: String) {
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
