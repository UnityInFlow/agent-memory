# Phase 1: Core Memory Foundation - Pattern Map

**Mapped:** 2026-06-25
**Files analyzed:** 14 new files (greenfield repo; no in-repo source)
**Analogs found:** 9 / 14 (5 files have no analog — rmcp server, SQLite store, FTS5, decay, migrations — covered by RESEARCH.md verbatim APIs)

> **Greenfield notice.** This repo (`10-agent-memory`) has no `Cargo.toml` or `*.rs` yet. All analogs below come from the **sibling UnityInFlow Rust repos** `07-mcp-hub` and `03-injection-scanner`, which are separate git repos on disk. Paths to those repos are absolute and read-only — do NOT edit them.
>
> **Critical caveat about mcp-hub and "MCP":** `07-mcp-hub` is an MCP **client / process supervisor** — it *spawns* MCP servers and speaks JSON-RPC *to* them over child pipes (`src/mcp/dispatcher.rs`, `src/mcp/protocol.rs`). agent-memory is the **opposite**: an MCP **server** built on the `rmcp` SDK's `#[tool_router]` macros. mcp-hub does NOT use `rmcp` and its hand-rolled JSON-RPC dispatcher is the inverse of what we need — **do not copy it for the server**. Use it only for CLI/runtime/logging/path-resolution patterns. The rmcp server wiring has **no in-ecosystem analog** — use RESEARCH.md Pattern 1 & 2 (verbatim from the official rust-sdk examples) instead.

---

## File Classification

| New File | Role | Data Flow | Closest Analog | Match Quality |
|----------|------|-----------|----------------|---------------|
| `Cargo.toml` (workspace root) | config | — | `07-mcp-hub/Cargo.toml` + `03-injection-scanner/Cargo.toml` | role-match (workspace adds `[workspace]`) |
| `crates/agent-memory-core/Cargo.toml` | config | — | `03-injection-scanner/Cargo.toml` (lib deps) | role-match |
| `crates/agent-memory/Cargo.toml` | config | — | `07-mcp-hub/Cargo.toml` (`[[bin]]`) | role-match |
| `crates/agent-memory-core/src/lib.rs` | module-root | — | `07-mcp-hub/src/lib.rs`, `03-injection-scanner/src/lib.rs` | exact |
| `crates/agent-memory-core/src/domain.rs` | model | transform (validation) | `03-injection-scanner/src/pattern.rs` (enum + `TryFrom`-style + `thiserror`) | role-match |
| `crates/agent-memory-core/src/clock.rs` | utility | transform | — | **no analog** (RESEARCH Pattern 5) |
| `crates/agent-memory-core/src/decay.rs` | service | transform (pure math) | — | **no analog** (RESEARCH "Decay formula") |
| `crates/agent-memory-core/src/service.rs` | service | CRUD | — partial (mcp-hub has no service-over-store layer) | **weak / no analog** |
| `crates/agent-memory-core/src/store/mod.rs` | adapter (trait) | CRUD | — | **no analog** |
| `crates/agent-memory-core/src/store/sqlite.rs` | adapter | CRUD + file-I/O | — | **no analog** (RESEARCH Pattern 3 & 5, schema) |
| `crates/agent-memory-core/src/store/migrations.rs` | adapter | file-I/O | — | **no analog** (RESEARCH Pattern 4) |
| `crates/agent-memory/src/main.rs` | bin entry | event-driven (stdio server) | `07-mcp-hub/src/main.rs` (tokio main + anyhow) + RESEARCH Pattern 2 | role-match (server side differs) |
| `crates/agent-memory/src/config.rs` | config resolver | transform | `07-mcp-hub/src/config.rs` `find_and_load_config` (`dirs::` + precedence) | exact |
| `crates/agent-memory/src/mcp.rs` | interface adapter | request-response | — | **no analog** (RESEARCH Pattern 1 — rmcp `#[tool_router]`) |
| `.github/workflows/ci.yml` | config | — | `03-injection-scanner/.github/workflows/ci.yml` | exact |
| `crates/agent-memory/tests/*.rs`, `core/tests/*.rs` | test | — | `03-injection-scanner/tests/cli_test.rs` (`Command`-spawn) | role-match |

---

## Pattern Assignments

### `crates/agent-memory-core/src/lib.rs` (module-root)

**Analog:** `07-mcp-hub/src/lib.rs` (lines 1-12) and `03-injection-scanner/src/lib.rs` (lines 1-5) — both are a flat list of `pub mod` declarations, nothing else.

**Copy this shape** — declare every core module, no logic in lib.rs:
```rust
pub mod clock;
pub mod decay;
pub mod domain;
pub mod service;
pub mod store;
```
mcp-hub puts a nested module under its own dir (`pub mod mcp;` → `src/mcp/mod.rs`); mirror that for `store` (`pub mod store;` → `src/store/mod.rs` which itself re-exports `sqlite` and `migrations`).

---

### `crates/agent-memory-core/src/domain.rs` (model — `MemoryType`, `Memory`, `MemoryError`)

**Analog:** `03-injection-scanner/src/pattern.rs` — the closest pattern in the ecosystem for (a) a `serde`-tagged value enum, (b) a `Display` impl, and (c) a `thiserror` error enum.

**Enum + Display pattern** (`pattern.rs` lines 8-26) — copy for `MemoryType`'s string mapping. injection-scanner uses `#[serde(rename_all = "UPPERCASE")]` + a hand `Display`; agent-memory needs the same UPPERCASE wire form but ALSO a fallible parse (D-07), so pair this with `TryFrom<&str>` from RESEARCH "MemoryType validation":
```rust
// injection-scanner/src/pattern.rs:8-26 — UPPERCASE serde enum + Display
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity { Low, Medium, High, Critical }
impl std::fmt::Display for Severity { /* match -> write!(f, "LOW") ... */ }
```
For agent-memory, the `TryFrom<&str>` returning `Err(MemoryError::InvalidType(..))` (RESEARCH §"MemoryType validation (D-07)") is the load-bearing addition — it gives the "clean error, never panic" the analog's plain enum does not.

**thiserror error-enum pattern** (`pattern.rs` lines 116-127) — copy verbatim in shape for `MemoryError`:
```rust
// injection-scanner/src/pattern.rs:116-127
use thiserror::Error;
#[derive(Debug, Error)]
pub enum PatternError {
    #[error("Failed to parse pattern file: {0}")]
    ParseError(String),
    #[error("Invalid regex pattern '{pattern}' in {id}: {source}")]
    InvalidRegex { id: String, pattern: String, source: regex::Error },
}
```
This is the **exact ecosystem convention** for the library's `thiserror` enum (CLAUDE.md: thiserror for libs). For `MemoryError`, add variants the research calls for: `InvalidType(String)`, plus `#[from]`-wrapped `Sqlite(rusqlite::Error)`, `Pool(r2d2::Error)`, `Join(tokio::task::JoinError)`, `Migration(rusqlite_migration::Error)`. Use `#[error("...")]` messages and `#[from]`/`source` exactly as the analog does.

---

### `crates/agent-memory/src/config.rs` (DB-path resolution — D-01/D-02)

**Analog:** `07-mcp-hub/src/config.rs` `find_and_load_config` (lines 190-219) — **exact** match for the "flag > env/global default > local" precedence pattern and the `dirs::` data-dir resolution.

**Path precedence pattern** (`config.rs` lines 190-219): mcp-hub resolves explicit-path-first, then a `dirs::config_dir()`-derived global path, then local, with a clear `match` over the `Option` combinations:
```rust
// 07-mcp-hub/src/config.rs:190-219 (adapt: config_dir→data_dir, .toml→memory.db)
pub fn find_and_load_config(explicit_path: Option<&std::path::Path>) -> anyhow::Result<HubConfig> {
    if let Some(path) = explicit_path { return load_config(path); }
    let global_path = dirs::config_dir().map(|d| d.join("mcp-hub").join("mcp-hub.toml"));
    // ...
}
```
**For agent-memory adapt as:** `--db <flag>` (clap) → else `AGENT_MEMORY_DB` env → else `dirs::data_dir().join("agent-memory").join("memory.db")` (D-01 uses **data_dir**, not config_dir — `~/Library/Application Support` on macOS / `$XDG_DATA_HOME` on Linux). Note: mcp-hub depends on `dirs = "5"` (gated under `cfg(unix)` in its Cargo.toml line 71); RESEARCH pins `dirs = "6"` for this repo and does NOT gate it (Windows deferred but keep `dirs` centralized in this one file — Constraint). Also: **create the dir `0700`** and `mkdir -p` if missing (RESEARCH Security §V4) — the analog does not do this; add it.

---

### `crates/agent-memory/src/main.rs` (bin entry — tokio + anyhow + stderr logging)

**Analogs:** `07-mcp-hub/src/main.rs` (CLI parse + tokio runtime + anyhow `Context`) for the *binary scaffolding*; RESEARCH **Pattern 2** for the *rmcp serve + stderr logging* (no in-ecosystem analog for `serve(stdio())`).

**From mcp-hub `main.rs`:**
- `let cli = Cli::parse();` then dispatch over `cli.command` (line 29, 63). agent-memory's CLI is much smaller (a `serve` subcommand + `--db`), but the parse-then-match shape is the same.
- `anyhow::Result<()>` return + `.context("...")` at the edges (lines 28, 65) — ecosystem rule: anyhow in the binary.
- mcp-hub uses a *manual* `tokio::runtime::Builder` because it forks before Tokio (lines 51-56). **agent-memory does NOT fork** — use the simpler `#[tokio::main]` from RESEARCH Pattern 2 instead.

**From mcp-hub `output.rs` `configure_tracing` (lines 139-150)** — the **stderr-logging pattern is directly reusable** and is the MCP-05 stdout-purity foundation:
```rust
// 07-mcp-hub/src/output.rs:139-150
tracing_subscriber::fmt()
    .with_env_filter(filter)
    .with_writer(std::io::stderr)   // ← the load-bearing line for MCP-05
    .init();
```
agent-memory should add `.with_ansi(false)` and a `std::panic::set_hook(... eprintln! ...)` panic hook (RESEARCH Pattern 2) — mcp-hub omits both because its stdout is not a protocol channel; for an rmcp stdio server they are mandatory. The rest of the entrypoint (`MemoryMcp::new(state).serve(stdio()).await?; service.waiting().await?;`) is RESEARCH Pattern 2 verbatim — **no analog**.

---

### `crates/agent-memory/src/mcp.rs` (rmcp `#[tool_router]` — the 4 tools)

**Analog: NONE.** mcp-hub's `src/mcp/dispatcher.rs` is a hand-rolled JSON-RPC *client* dispatcher (it owns a child's stdout and routes responses by id) — the **inverse** of an rmcp server and must not be copied. There is no `rmcp`/`ToolRouter`/`#[tool]` usage anywhere in the ecosystem (`grep rmcp` → 0 matches).

**Use RESEARCH Pattern 1 verbatim** (taken from the official `modelcontextprotocol/rust-sdk` 1.8 `common/counter.rs`): a `#[derive(Clone)]` `MemoryMcp { state: Arc<AppState>, tool_router: ToolRouter<Self> }`; `#[tool_router] impl` with four `#[tool(description=…)]` async methods each taking `Parameters<T>`; `#[tool_handler] impl ServerHandler`. The one cross-cutting rule from RESEARCH's Architecture map: **tool methods contain NO SQL / decay / clock** — they only deserialize `Parameters<T>`, call `self.state.service.<op>`, and map `Result → CallToolResult` (map `Err` via `McpError::invalid_params(e.to_string(), None)`). Tool arg structs derive `serde::Deserialize + rmcp::schemars::JsonSchema` (use the **re-export** `rmcp::schemars` — RESEARCH schemars note).

---

### `crates/agent-memory-core/src/{clock,decay,service}.rs` and `store/{mod,sqlite,migrations}.rs`

**Analog: NONE in the ecosystem.** No sibling repo embeds SQLite, uses FTS5, has a decay/scoring engine, or a `Store` trait. Use the RESEARCH document's verbatim sections:

| File | RESEARCH source to copy from |
|------|------------------------------|
| `clock.rs` | Pattern 5 — `Clock` trait + `SystemClock` + `#[cfg(test)] TestClock` |
| `decay.rs` | §"Decay formula" — `decay_score(now, last_accessed, half_life_secs, pinned)`; pinned ⇒ longer half-life (D-08/D-09); 30-day default = `2_592_000` |
| `service.rs` | §"memory_search service" — `spawn_blocking` wrapper, "empty list not error", recency-bump via writer lane |
| `store/mod.rs` | Architecture map — `Store` trait so `SqliteStore` is swappable in unit tests |
| `store/sqlite.rs` | Pattern 3 (FTS5 mirror + bm25×decay query, **bm25 sign rule**), Pattern 5 (WAL + single-writer + r2d2 read pool + per-connection PRAGMAs), §"Concrete memories Schema" |
| `store/migrations.rs` | Pattern 4 — `rusqlite_migration::{Migrations, M}` + `M::up(include_str!("../sql/0001_init.sql"))` |

For all of these, the **anti-patterns list in RESEARCH** ("no business logic in tool methods", "no blocking rusqlite in async — `spawn_blocking`", "single writer + WAL", "no stdout writes", "decay never deletes", "UTC epoch i64 only") is the checklist the planner should attach to each plan.

---

### `.github/workflows/ci.yml` (CI)

**Analog:** `03-injection-scanner/.github/workflows/ci.yml` — **exact** template for a Rust repo on the self-hosted runners.

**Copy verbatim** (injection-scanner `ci.yml` lines 1-23), then extend:
```yaml
jobs:
  build-and-test:
    strategy:
      matrix:
        runner: [arc-runner-unityinflow, orangepi]   # X64 + ARM64 (Hetzner-offline fallback)
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: clippy, rustfmt }
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo build
      - run: cargo test
```
**Phase-1 additions the analog lacks** (RESEARCH Validation §CI): a `cargo install cargo-llvm-cov` + `cargo llvm-cov --workspace --fail-under-lines 80` step, and the stdout-purity integration test runs under `cargo test --workspace`. Keep `runs-on: [arc-runner-unityinflow]` / `[orangepi]` — **never `ubuntu-latest`** (CLAUDE.md).

---

### Test files (`core/tests/*.rs`, `agent-memory/tests/*.rs`)

**Analog:** `03-injection-scanner/tests/cli_test.rs` — the pattern for **spawning the built binary and asserting on stdout/exit** (lines 1-37). This is the closest analog for the **stdout-purity test (MCP-05)** and the four-tools-over-stdio tests.

**Copy the spawn-the-binary helper shape** (`cli_test.rs` lines 1-29):
```rust
// 03-injection-scanner/tests/cli_test.rs:1-29
fn binary_path() -> String { format!("{}/target/debug/injection-scanner", env!("CARGO_MANIFEST_DIR")) }
let output = Command::new(binary_path()).args(["check", &fixture]).output().expect("...");
assert!(output.status.success());
let stdout = String::from_utf8_lossy(&output.stdout);
```
**For agent-memory, adapt to:** spawn `agent-memory serve`, **write `initialize`/`tools/call` JSON-RPC frames to stdin** (the analog only passes args, not piped stdin — this is the new part), then assert **every stdout line parses as JSON-RPC** while logs land on stderr (capture `output.stdout` and `output.stderr` separately). RESEARCH recommends `assert_cmd`/`predicates` + `tempfile` (mcp-hub's dev-deps already prove these in-ecosystem: `07-mcp-hub/Cargo.toml` lines 84-89). Core unit tests (`decay.rs`, `ttl.rs`, `store.rs`) use the injected `TestClock` + a `tempfile::NamedTempFile` DB (RESEARCH Validation §Harnesses) — no analog, but standard `#[tokio::test]`.

---

## Shared Patterns

### Stderr-only logging (MCP-05 foundation)
**Source:** `07-mcp-hub/src/output.rs` `configure_tracing` (lines 139-150) — `tracing_subscriber::fmt().with_writer(std::io::stderr).init()`.
**Apply to:** `crates/agent-memory/src/main.rs`. **Augment** with `.with_ansi(false)` + a panic hook to stderr (RESEARCH Pattern 2) — these are not in the analog but are mandatory for a stdio JSON-RPC server.

### anyhow-at-edges / thiserror-in-lib split
**Source:** `03-injection-scanner` uses `anyhow::Result` in `src/main.rs` (line 5) and `thiserror` `PatternError` in `src/pattern.rs` (lines 116-127). `07-mcp-hub` uses `anyhow::Context` throughout `main.rs`.
**Apply to:** binary crate (`agent-memory`) → `anyhow` + `.context(...)`; library crate (`agent-memory-core`) → `thiserror` `MemoryError`. This is the CLAUDE.md ecosystem rule, already followed by both analogs.

### clap derive CLI
**Source:** `07-mcp-hub/src/cli.rs` (lines 1-60, `#[derive(Parser)]` + `#[derive(Subcommand)]` + `#[arg(long, env = "...")]`) and `03-injection-scanner/src/main.rs` (lines 14-36, inline `Cli`/`Commands`).
**Apply to:** `agent-memory/src/main.rs` (or a small `cli` module). Note mcp-hub's `#[arg(long, global=true, env="NO_COLOR")]` (cli.rs line 13) is the exact pattern for binding `--db`/`AGENT_MEMORY_DB` via clap's `env` feature (D-02). agent-memory's CLI is far smaller — likely just `serve` + a global `--db <PATH>`.

### Module-per-file, nested dir module
**Source:** `07-mcp-hub/src/lib.rs` + `src/mcp/mod.rs` (a `pub mod mcp;` pointing at a directory with its own `mod.rs`).
**Apply to:** `agent-memory-core/src/store/` (mod.rs declares `pub mod sqlite; pub mod migrations;` and defines the `Store` trait).

### Self-hosted-runner CI
**Source:** `03-injection-scanner/.github/workflows/ci.yml` — matrix `[arc-runner-unityinflow, orangepi]`, `dtolnay/rust-toolchain@stable`, fmt-check → clippy -D warnings → build → test.
**Apply to:** `.github/workflows/ci.yml`. Extend with the `cargo-llvm-cov --fail-under-lines 80` coverage gate.

---

## No Analog Found

Files/concerns with no close match in the ecosystem (planner uses RESEARCH.md verbatim):

| File / Concern | Role | Data Flow | Reason — and RESEARCH source |
|----------------|------|-----------|------------------------------|
| `src/mcp.rs` (rmcp `#[tool_router]`, 4 tools) | interface | request-response | No `rmcp` anywhere in ecosystem; mcp-hub is an MCP *client*, its dispatcher is the inverse. → RESEARCH **Pattern 1** |
| rmcp `serve(stdio())` + panic hook | bin entry | event-driven | No rmcp server entrypoint exists. → RESEARCH **Pattern 2** |
| `store/sqlite.rs` (rusqlite bundled, WAL, r2d2, single-writer, `spawn_blocking`) | adapter | CRUD + file-I/O | No embedded-SQLite tool in ecosystem. → RESEARCH **Pattern 3, 5**, §Schema |
| `store/migrations.rs` (`rusqlite_migration`) | adapter | file-I/O | No migration code in ecosystem. → RESEARCH **Pattern 4** |
| `decay.rs` (exponential decay, per-type pinning) | service | transform | No scoring/decay logic exists. → RESEARCH §"Decay formula" |
| `clock.rs` (`Clock` trait + `TestClock`) | utility | transform | No injectable-clock pattern in ecosystem. → RESEARCH **Pattern 5** |
| FTS5 + bm25×decay ranking (in `sqlite.rs`) | adapter | transform | No full-text search in ecosystem. → RESEARCH **Pattern 3** (note the **bm25 sign rule**) |

---

## Metadata

**Analog search scope:**
- `/Users/jirihermann/Documents/workspace-1-ideas/unity-in-flow-ai/07-mcp-hub/src/**` (read: main.rs, cli.rs, lib.rs, output.rs, config.rs, mcp/dispatcher.rs, Cargo.toml)
- `/Users/jirihermann/Documents/workspace-1-ideas/unity-in-flow-ai/03-injection-scanner/{src,tests,.github}/**` (read: main.rs, lib.rs, pattern.rs, tests/cli_test.rs, .github/workflows/ci.yml, Cargo.toml)

**Files scanned (read in full or targeted):** 13
**Pattern extraction date:** 2026-06-25
**Greenfield:** target repo has no source; all analogs are cross-repo references, read-only.
