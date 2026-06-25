# Phase 1: Core Memory Foundation - Research

**Researched:** 2026-06-24
**Domain:** Local-first Rust MCP stdio daemon — embedded SQLite (WAL + single-writer) store, FTS5 keyword search, exponential decay + TTL, no Ollama / no cloud
**Confidence:** HIGH

## Summary

Phase 1 is the **walking skeleton**: a Rust workspace (`agent-memory-core` lib + `agent-memory` bin) that serves an MCP stdio server exposing four tools (`memory_store`, `memory_search`, `memory_list`, `memory_forget`) over an embedded SQLite store. No Ollama, no embeddings, no REST, no release tooling — those are Phase 2. The project research (STACK/ARCHITECTURE/PITFALLS) already settled the macro stack; this document supplies the **exact Phase-1 implementation shapes** the planner needs: the `rmcp 1.8` tool/stdio wiring, the FTS5 trigger-synced mirror, the migration mechanism, the `memories` schema honoring D-01..D-09, the deterministic decay formula, and the test/coverage strategy including the stdout-purity gate.

The single most load-bearing fact verified this session: the canonical `rmcp 1.8` server is a `#[derive(Clone)]` struct holding a `tool_router: ToolRouter<Self>` field; tools are `#[tool(description=…)]` methods taking `Parameters<T>` where `T: serde::Deserialize + schemars::JsonSchema`; `#[tool_router]` on the impl block generates the dispatcher and JSON Schema; `#[tool_handler]` wires `ServerHandler`; and stdio is served by `Counter::new().serve(stdio()).await?` with `tracing_subscriber::fmt().with_writer(std::io::stderr).with_ansi(false)`. This is taken verbatim from the official `modelcontextprotocol/rust-sdk` examples (`counter_stdio.rs` + `common/counter.rs`). [VERIFIED: github.com/modelcontextprotocol/rust-sdk examples @ main]

**Primary recommendation:** Build domain + `SqliteStore` (migrations, WAL, single-writer, FTS5 triggers) first, then `MemoryService` + `DecayEngine` (injectable `Clock`), then the four `rmcp` tools over stdio — thinnest end-to-end slice being `store` + keyword `search`. Use `rusqlite_migration` for schema versioning, `cargo-llvm-cov` for coverage, and a piped-`initialize` integration test as the stdout-purity CI gate.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Default to a **single global database** at the OS data dir resolved via the `dirs` crate — `<data_dir>/agent-memory/memory.db` (`~/Library/Application Support/agent-memory/memory.db` on macOS, `$XDG_DATA_HOME/agent-memory/memory.db` on Linux). Create the directory if missing.
- **D-02:** Override precedence: `--db <path>` flag > `AGENT_MEMORY_DB` env var > global default.
- **D-03:** `scope` is a **logical column filter inside the one store**, NOT a separate DB file per project. One shared store; `scope` partitions logically.
- **D-04:** `memory_search` ranks by a **blend of FTS5/BM25 relevance and `decay_score`**; pinned types (D-08) float up; results expose both relevance and decay score. Exact blend formula is a planner/implementer tuning detail.
- **D-05:** **Rich tool schema with sensible defaults.** `memory_store({ content, type, tags?, source?, scope?, ttl? })` — only `content` and `type` required; rest default (tags=[], source/scope=NULL, ttl=none).
- **D-06:** `memory_search`/`memory_list` results return `{ id, content, type, tags, scope, decay_score, created_at, last_accessed }`. `memory_forget({ id })` returns a deleted/not-found result.
- **D-07:** Tool input schemas must be explicit JSON Schema (rmcp); invalid `type` rejected with a clean error, never a panic.
- **D-08:** **Pin high-value types.** DECISION, ARCHITECTURE, CONSTRAINT decay much slower than (or exempt from) normal decay; TODO, ERROR, PATTERN decay at the normal rate.
- **D-09:** Ship a **sensible default half-life (~30 days)**; both half-life and pinned-type set are **configurable** (env/config), defaults must be good out of the box. Decay only down-ranks — never deletes.

### Claude's Discretion
- Crate/workspace mechanics, exact dependency versions, migration library, connection-pool approach, precise decay/ranking math, config-file format, coverage tool.

### Deferred Ideas (OUT OF SCOPE)
- Hybrid (keyword+semantic) RRF ranking fusion — v2 (SEARCH-04); Phase 1 blend is keyword-relevance × decay only.
- Per-runtime / per-project DB auto-detection beyond explicit `--db`/env — later UX.
- `memory_update` / relation tools — v2 (MCP-06).
- Semantic search / Ollama / `sqlite-vec` vec0 — Phase 2. **No embedding column in Phase 1.**
- REST API, GSD STATE.md import, prebuilt binaries + Homebrew — Phase 2.
- Windows support — deferred to v2 (DIST-03); path code need not be Windows-safe in Phase 1.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| STORE-01 | Typed memory storage (6 types) in SQLite | `memories` schema below; `MemoryType` enum with `TryFrom<&str>` for clean rejection (D-07) |
| STORE-02 | Durable local persistence across restart, zero cloud | `rusqlite` `bundled` + WAL; default path via `dirs` (D-01/02); offline by construction (no network crate on the store path) |
| STORE-03 | Exponential decay scoring, surfaced in results | `DecayEngine` + injectable `Clock`; formula below; per-type pinning (D-08) |
| STORE-04 | TTL expiry separate from decay; decay never deletes | `expires_at` column + sweep `DELETE`; decay is rank-only |
| MCP-01 | `memory_store` tool returns id | `#[tool]` method → `MemoryService::store` → returns id (D-05) |
| MCP-02 | `memory_search` tool (FTS5 keyword) | FTS5 mirror + bm25 × decay blend (D-04); empty result, not error, on no-match |
| MCP-03 | `memory_list` tool with filters | `MemoryService::list` with type/tag/scope/limit, newest-first |
| MCP-04 | `memory_forget` tool deletes by id | `MemoryService::forget` → deleted/not-found result (D-06) |
| MCP-05 | stdio JSON-RPC purity (logs → stderr) | `rmcp` `serve(stdio())` + `tracing` to stderr; piped-`initialize` CI test |
| SEARCH-01 | Keyword search works without Ollama | FTS5 is built into bundled SQLite; no network dependency |
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| MCP tool dispatch / JSON-RPC framing | Interface (bin, `rmcp`) | — | Transport concern; thin adapter, no business logic |
| Store/forget/list/search business logic | Domain/Service (`agent-memory-core` lib) | — | Reused by all transports + tests; `thiserror` typed errors |
| Decay scoring + TTL sweep | Domain (`DecayEngine` in lib) | — | Pure math + injectable `Clock`; deterministic, transport-agnostic |
| SQL / schema / WAL / FTS5 / migrations | Adapter (`SqliteStore` in lib, behind `Store` trait) | — | Single place that knows SQLite; swappable for unit tests |
| DB path resolution + dir creation | Interface (bin) → passed to lib as config | — | `dirs`/env/flag precedence is a CLI/edge concern (D-01/02) |
| Logging | Cross-cutting → **stderr only** | — | stdout is the JSON-RPC channel (MCP-05) |

**Tier-correctness note for the planner:** all four tools are *interface adapters* that call the *same* `MemoryService` methods. No SQL, no decay math, and no clock access may live inside the `rmcp` tool methods — that is Anti-Pattern 1 (see Architecture research). The tool method's only jobs are: deserialize `Parameters<T>`, call the service, map the `Result` to `CallToolResult`.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `rmcp` | `1.8` (`features = ["server", "transport-io", "macros"]`) | MCP stdio server + `#[tool]`/`#[tool_router]` macros | Official MCP Rust SDK; verified API shape this session [VERIFIED: crates.io 1.8.0 + github examples] |
| `rusqlite` | `0.40` (`features = ["bundled"]`) | Embedded SQLite; SQLite compiled into the binary | Zero system dependency; FTS5 included in bundled build [VERIFIED: crates.io 0.40.1] |
| `rusqlite_migration` | `2.6` | Versioned `user_version` migrations | Lightweight, no async/macros; right weight for single-file DB [VERIFIED: crates.io 2.6.0] |
| `r2d2` + `r2d2_sqlite` | `0.8` / `0.34` | Read connection pool (writer is separate, serialized) | Standard SQLite pool; read pool + single writer = no `SQLITE_BUSY` [VERIFIED: crates.io] |
| `tokio` | `1` (`features = ["full"]`) | Async runtime hosting the stdio server + sweep task | `rmcp` is tokio-native [VERIFIED: crates.io 1.52.3] |
| `serde` / `serde_json` | `1` / `1` | (De)serialize MCP payloads, tags, metadata | Required transitively by rmcp anyway [VERIFIED: crates.io] |
| `schemars` | `1.x` (use `rmcp::schemars` re-export — see pitfall) | JSON Schema derive for tool param structs (D-07) | rmcp generates tool schemas via schemars [VERIFIED: crates.io 1.2.1] |
| `thiserror` | `2` | Typed `MemoryError` in the **library** | Ecosystem rule: thiserror for libs [VERIFIED: crates.io 2.0.18] |
| `anyhow` | `1` | Errors at the **binary** edges (CLI/transport bootstrap) | Ecosystem rule: anyhow for bins [VERIFIED: crates.io 1.0.102] |
| `clap` | `4` (`features = ["derive", "env"]`) | CLI: `serve`, `--db`, env binding | Ecosystem standard [VERIFIED: crates.io 4.6.1] |
| `chrono` | `0.4` | UTC timestamps, decay/TTL deltas | Decay math needs wall-clock deltas (stored as i64 epoch) [VERIFIED: crates.io 0.4.45] |
| `dirs` | `6` | Default DB path resolution (D-01) | mcp-hub uses dirs; matches ecosystem [VERIFIED: crates.io 6.0.0] |
| `uuid` | `1` (`v4`) *(optional)* | Memory IDs **if** string IDs preferred over rowid | See "Open Questions" — INTEGER rowid is recommended for FTS5 join | [VERIFIED: crates.io 1.23.4] |
| `tracing` / `tracing-subscriber` | `0.1` / `0.3` (`env-filter`) | Structured logging **to stderr** | Observability; stderr writer is the stdout-purity mitigation [VERIFIED: crates.io] |

### Supporting / Dev
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `cargo-llvm-cov` | `0.8` | Coverage measurement for the >80% gate | **Recommended** over tarpaulin (see Alternatives) [VERIFIED: crates.io 0.8.7] |
| `tempfile` | `3` | Temp-file SQLite DB in integration tests | Restart-durability + store/search tests [ASSUMED — verify] |
| `assert_cmd` / `predicates` *(optional)* | `2` / `3` | Spawn the binary, pipe stdin, assert stdout | stdout-purity CI test harness [ASSUMED — verify] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `rusqlite_migration` | hand-rolled `PRAGMA user_version` match, or `refinery` | Hand-rolled is fewer deps but you re-implement ordering/idempotency; `refinery` is heavier (async, embedded migrations dir) and aimed at client/server DBs. `rusqlite_migration` is the sweet spot for a single embedded file — recommend it. |
| `cargo-llvm-cov` | `cargo-tarpaulin` | tarpaulin is Linux-x86_64-centric (ptrace) and flakier on macOS; `cargo-llvm-cov` uses LLVM source-based coverage, works on macOS + Linux (both Phase-1 targets), integrates with `cargo nextest`. Recommend `cargo-llvm-cov`. |
| INTEGER rowid id | `uuid` v4 TEXT id | FTS5 external-content tables require an INTEGER PRIMARY KEY (`content_rowid`). Using an INTEGER `id` lets `memories.id == fts.rowid` directly. A UUID would force a second indexed column. **Recommend INTEGER `id`** for Phase 1; expose it as the tool's id. |
| `r2d2` read pool | single shared connection behind a `Mutex` | A single mutexed connection is simpler and fully correct for v0.0.1's low concurrency, but serializes reads too. The read-pool + single-writer split is the documented standard; either is acceptable — pool is more future-proof. |

**Installation:**
```bash
cargo add rmcp --features server,transport-io,macros
cargo add rusqlite --features bundled
cargo add rusqlite_migration
cargo add r2d2 r2d2_sqlite
cargo add tokio --features full
cargo add serde --features derive
cargo add serde_json
cargo add thiserror@2          # library crate
cargo add anyhow               # binary crate
cargo add clap --features derive,env
cargo add chrono dirs
cargo add tracing tracing-subscriber --features tracing-subscriber/env-filter
# dev:
cargo add --dev tempfile
cargo install cargo-llvm-cov
```

**schemars note:** Do **not** add `schemars` as a direct dependency with an independent version unless needed — `rmcp` re-exports the exact `schemars` it was compiled against (use `rmcp::schemars`). A mismatched direct `schemars` version is a known source of "trait `JsonSchema` not satisfied" errors. If you import `schemars::JsonSchema` directly, pin it to whatever major version `rmcp 1.8` depends on (verify with `cargo tree -p schemars`). [VERIFIED: rmcp re-exports schemars — confirmed in example imports `model::*` + derive usage; CITED: rmcp README]

## Package Legitimacy Audit

All Phase-1 crates verified via `gsd-tools query package-legitimacy check --ecosystem crates` this session — all `OK`.

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| rmcp | crates.io | since 2025-03 | 571k/wk | github.com/modelcontextprotocol/rust-sdk | OK | Approved |
| rusqlite | crates.io | since 2014 | 1.7M/wk | github.com/rusqlite/rusqlite | OK | Approved |
| rusqlite_migration | crates.io | since 2020 | 114k/wk | github.com/cljoly/rusqlite_migration | OK | Approved |
| r2d2 | crates.io | since 2014 | 337k/wk | github.com/sfackler/r2d2 | OK | Approved |
| r2d2_sqlite | crates.io | since 2015 | 77k/wk | github.com/ivanceras/r2d2-sqlite | OK | Approved |
| clap | crates.io | since 2015 | 14.5M/wk | github.com/clap-rs/clap | OK | Approved |
| tokio | crates.io | since 2016 | 13.7M/wk | github.com/tokio-rs/tokio | OK | Approved |
| serde / serde_json | crates.io | since 2014/15 | 16M+/wk | github.com/serde-rs/* | OK | Approved |
| schemars | crates.io | mature | 8.5M/wk | github.com/GREsau/schemars | OK | Approved (prefer rmcp re-export) |
| thiserror / anyhow | crates.io | mature | high | github.com/dtolnay/* | OK | Approved |
| chrono / dirs / tracing / tracing-subscriber | crates.io | mature | high | (official repos) | OK | Approved |
| cargo-llvm-cov | crates.io | since 2021 | high | github.com/taiki-e/cargo-llvm-cov | OK | Approved (dev tool) |

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none
**No postinstall scripts** (crates have no postinstall concept; nonetheless none flagged).

## Architecture Patterns

### System Architecture Diagram (Phase 1 — stdio only, no embeddings/REST)

```
   MCP client (Claude Code / Cursor)
           │  JSON-RPC over stdio (stdin/stdout)
           ▼
 ┌──────────────────────────────────────────────┐
 │  agent-memory (bin)                            │
 │  main(): clap → resolve DB path → tracing→STDERR
 │          → MemoryMcp::new(state).serve(stdio())│
 │  ┌──────────────────────────────────────────┐ │
 │  │ #[tool_router] MemoryMcp  (thin adapters) │ │
 │  │  memory_store / search / list / forget    │ │
 │  └───────────────────┬──────────────────────┘ │
 └──────────────────────┼─────────────────────────┘
                        │ calls (Arc<AppState>)
                        ▼
 ┌──────────────────────────────────────────────┐
 │  agent-memory-core (lib)                       │
 │  MemoryService ── DecayEngine(Clock) ──┐       │
 │       │                                 │       │
 │       ▼            Store trait          ▼       │
 │  SqliteStore ◄───────────────── TTL sweep task  │
 │   writer lane (1 conn, serialized)              │
 │   read pool (r2d2)                              │
 └──────────────────────┬─────────────────────────┘
                        │
            ┌───────────▼────────────┐
            │  SQLite file (WAL)      │
            │  memories  +  memories_fts (FTS5, trigger-synced)
            └─────────────────────────┘

  Background: tokio::time::interval → DecayEngine.sweep()
              (recompute decay_score materialization + DELETE expired)
  NO network. NO embedding column. NO Ollama. (Phase 2 adds those.)
```

### Recommended Project Structure
```
agent-memory/                      # workspace root
├── Cargo.toml                     # [workspace] members
├── crates/
│   ├── agent-memory-core/         # library — thiserror, no anyhow
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── domain.rs          # Memory, MemoryType (TryFrom), MemoryError
│   │   │   ├── clock.rs           # Clock trait + SystemClock + TestClock
│   │   │   ├── decay.rs           # decay formula + DecayEngine + pin policy
│   │   │   ├── service.rs         # MemoryService (store/list/forget/search)
│   │   │   └── store/
│   │   │       ├── mod.rs         # Store trait
│   │   │       ├── sqlite.rs      # SqliteStore: pool, writer lane, queries
│   │   │       └── migrations.rs  # rusqlite_migration M::up steps
│   │   └── tests/                 # integration tests over temp-file SQLite
│   └── agent-memory/              # binary — anyhow at edges
│       └── src/
│           ├── main.rs            # clap CLI, tokio::main, tracing→stderr
│           ├── config.rs          # DB path resolution (flag>env>default)
│           └── mcp.rs             # MemoryMcp: #[tool_router] 4 tools
└── .github/workflows/ci.yml       # self-hosted runners; fmt+clippy+test+stdout-purity
```

### Pattern 1: rmcp 1.8 stdio server with the four tools

**What:** A `#[derive(Clone)]` struct holds `tool_router: ToolRouter<Self>` and an `Arc<AppState>`. Each tool is a `#[tool]` method taking `Parameters<T>`; `#[tool_router]` generates dispatch + JSON Schema; `#[tool_handler]` implements `ServerHandler`.

```rust
// Source: github.com/modelcontextprotocol/rust-sdk examples/servers/src/common/counter.rs (rmcp 1.8) [VERIFIED]
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    tool, tool_handler, tool_router,
};
use rmcp::schemars; // prefer the re-export — avoids a schemars version split

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct StoreArgs {
    /// The memory content to persist.
    pub content: String,
    /// One of: DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT.
    pub r#type: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,        // None => NULL => global (D-03/D-05)
    #[serde(default)]
    pub ttl_secs: Option<i64>,        // None => no TTL (D-05)
}

#[derive(Clone)]
pub struct MemoryMcp {
    state: std::sync::Arc<AppState>,
    tool_router: ToolRouter<MemoryMcp>,
}

#[tool_router]
impl MemoryMcp {
    pub fn new(state: std::sync::Arc<AppState>) -> Self {
        Self { state, tool_router: Self::tool_router() }
    }

    #[tool(description = "Store a typed memory; returns its id. Only content and type are required.")]
    async fn memory_store(
        &self,
        Parameters(args): Parameters<StoreArgs>,
    ) -> Result<CallToolResult, McpError> {
        // Validate type → clean error, never panic (D-07)
        let id = self.state.service.store(args).await
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(id.to_string())]))
    }

    // memory_search / memory_list / memory_forget follow the same shape:
    // deserialize Parameters<T> → call self.state.service.<op> → map Result.
}

#[tool_handler]
impl ServerHandler for MemoryMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder().enable_tools().build(),
        )
        .with_server_info(Implementation::from_build_env())
        .with_instructions("Persistent typed agent memory: store/search/list/forget.")
    }
}
```
> The exact `ServerInfo::new(...).with_*` builder chain and `McpError::invalid_params` constructor are from the rmcp 1.8 example; confirm the precise constructor names against `cargo doc` when wiring (the macro/model surface is stable across 1.8 but method names occasionally shift between minor versions). [VERIFIED for shape; CITED: rmcp README for builders]

### Pattern 2: stdio entrypoint with stderr-only logging (MCP-05)

```rust
// Source: github.com/modelcontextprotocol/rust-sdk examples/servers/src/counter_stdio.rs [VERIFIED verbatim]
use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with_writer(std::io::stderr)   // ← stdout stays pure JSON-RPC
        .with_ansi(false)               // ← no ANSI codes leaking
        .init();

    let state = build_app_state(/* resolved db path */)?; // anyhow at the edge
    let service = MemoryMcp::new(state).serve(stdio()).await
        .inspect_err(|e| tracing::error!("serving error: {e:?}"))?;
    service.waiting().await?;
    Ok(())
}
```
Also install a **panic hook to stderr** before `serve` so a panic backtrace never lands on stdout:
```rust
std::panic::set_hook(Box::new(|info| eprintln!("PANIC: {info}")));
```

### Pattern 3: FTS5 external-content mirror, trigger-synced

**What:** `memories_fts` is an `fts5` virtual table over `memories.content`, kept in sync by AFTER INSERT/UPDATE/DELETE triggers. Keyword search runs `MATCH` against it, ordered by `bm25()` blended with `decay_score`.

```sql
-- Source: sqlite.org/fts5.html external-content + triggers pattern [VERIFIED: sqlite.org]
CREATE VIRTUAL TABLE memories_fts USING fts5(
    content,
    content='memories',
    content_rowid='id'
);

CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, content) VALUES (new.id, new.content);
END;
CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content) VALUES('delete', old.id, old.content);
END;
CREATE TRIGGER memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content) VALUES('delete', old.id, old.content);
    INSERT INTO memories_fts(rowid, content) VALUES (new.id, new.content);
END;
```
**Search query (bm25 × decay blend, D-04):**
```sql
-- bm25() returns SMALLER = better; negate it so larger = better, then blend with decay_score.
SELECT m.id, m.content, m.mem_type, m.tags, m.scope,
       m.decay_score, m.created_at, m.last_accessed,
       bm25(memories_fts) AS bm25_raw
FROM memories_fts
JOIN memories m ON m.id = memories_fts.rowid
WHERE memories_fts MATCH ?1
  AND (?2 IS NULL OR m.mem_type = ?2)
  AND (?3 IS NULL OR m.scope = ?3)
ORDER BY ( (-bm25(memories_fts)) * ?4 + m.decay_score * ?5 ) DESC   -- weights configurable
LIMIT ?6;
```
> CRITICAL sign rule: `bm25()` returns numerically **smaller values for better matches** — you must negate it (or sort ascending on bm25 alone) before combining with `decay_score` (where larger = fresher). Getting the sign wrong silently inverts ranking. [VERIFIED: sqlite.org/fts5.html] Pinned types (D-08) get a constant additive boost in the `ORDER BY` expression or via a higher `base_weight` feeding `decay_score`.

### Pattern 4: Migrations via rusqlite_migration

```rust
// Source: rusqlite_migration docs (user_version-based) [CITED: docs.rs/rusqlite_migration]
use rusqlite_migration::{Migrations, M};

pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../sql/0001_init.sql")),   // memories + indexes + fts5 + triggers
        // Phase 2 will append M::up("ALTER TABLE memories ADD COLUMN embedding ...") etc.
    ])
}
// at startup, on the WRITER connection, before serving:
// migrations().to_latest(&mut conn)?;
```
Run PRAGMAs (`journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=5000`) on **every** connection as it is created (pool customizer + writer init), not inside a migration — `journal_mode` is a connection-level pragma.

### Pattern 5: Single-writer lane + injectable Clock

**What:** One owned write connection (behind a `Mutex` or fed by an `mpsc` channel) handles all writes — stores, recency bumps, decay-score materialization, TTL deletes. A `r2d2` read pool handles concurrent reads. All `rusqlite` work runs inside `tokio::task::spawn_blocking` (rusqlite is synchronous — Pitfall 2).

```rust
pub trait Clock: Send + Sync {
    fn now(&self) -> i64; // UTC unix epoch seconds
}
pub struct SystemClock;
impl Clock for SystemClock { fn now(&self) -> i64 { chrono::Utc::now().timestamp() } }

#[cfg(test)]
pub struct TestClock(pub std::sync::atomic::AtomicI64);
// advance() in tests to make decay deterministic
```

### Anti-Patterns to Avoid
- **Business logic in tool methods:** no SQL/decay/clock inside `#[tool]` fns — call `MemoryService`. (Architecture Anti-Pattern 1)
- **Blocking the runtime:** never call `rusqlite` directly in an `async` tool fn — wrap in `spawn_blocking`. (Pitfall 2)
- **Multiple writers / no WAL:** opens the door to `SQLITE_BUSY`. (Pitfall 11)
- **Any stdout write:** `println!`, `dbg!`, a banner, an un-hooked panic — all corrupt the JSON-RPC stream. (Pitfall 1)
- **Decay that deletes:** decay is rank-only; deletion is TTL or `memory_forget` exclusively. (Pitfall 7, STORE-04)
- **Local-time timestamps:** store UTC epoch i64 only. (Pitfall 8)

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON-RPC framing + tool dispatch + schema | Custom stdio loop | `rmcp` `#[tool_router]` + `serve(stdio())` | Protocol versioning, schema gen, message framing all handled |
| Keyword search / tokenization / ranking | LIKE scans + custom scoring | SQLite **FTS5** + `bm25()` | Built into bundled SQLite; proper inverted index + BM25 |
| Schema versioning | Ad-hoc `if table exists` checks | `rusqlite_migration` (`user_version`) | Ordered, idempotent, forward-compatible with Phase 2 ALTERs |
| Connection pooling | Hand-rolled pool | `r2d2` + `r2d2_sqlite` | Battle-tested; pairs with single-writer lane |
| JSON Schema for tool args | Hand-written schema JSON | `schemars::JsonSchema` derive (via rmcp) | rmcp wires derived schema to the tool automatically (D-07) |
| Coverage measurement | grep/manual | `cargo-llvm-cov` | LLVM source-based; macOS+Linux; nextest-compatible |
| Type-string validation | scattered `match` returning bool | `MemoryType: TryFrom<&str>` returning `Result` | Single source of truth; clean error (D-07), never panic |

**Key insight:** Nearly every "hard" part of this phase (search, schema migration, JSON-RPC, JSON Schema) is a solved problem in a bundled or first-party crate. The genuinely custom code is small: the decay formula, the bm25×decay blend, the single-writer lane, and the four thin tool adapters.

## Runtime State Inventory

> Greenfield phase — no rename/refactor/migration. Section omitted (no existing runtime state to inventory; repo has no `Cargo.toml` or `*.rs` yet, verified in SPEC.md Background).

## Common Pitfalls

(Full domain pitfalls are in `.planning/research/PITFALLS.md`; the Phase-1-relevant ones, condensed:)

### Pitfall 1: stdout pollution corrupts MCP stdio
**What goes wrong:** any non-JSON-RPC byte on stdout breaks the client (`Parse error`).
**How to avoid:** `tracing` writer = `std::io::stderr`, `with_ansi(false)`, panic hook → stderr, audit deps for banners.
**Warning signs:** works manually, client reports parse error on connect.
**Verification:** the piped-`initialize` CI test (MCP-05).

### Pitfall 2: blocking the tokio runtime with synchronous rusqlite
**What goes wrong:** `conn.query_row()` inside an `async` handler stalls worker threads under concurrency.
**How to avoid:** all DB work in `spawn_blocking` (or a dedicated blocking writer thread fed by a channel).
**Warning signs:** latency spikes when store+search overlap.

### Pitfall 7: decay deleting / conflating decay-TTL-forget
**What goes wrong:** aggressive decay buries high-value DECISION/ARCHITECTURE/CONSTRAINT; coupling decay to deletion loses data.
**How to avoid:** decay ranks only; pin types (D-08); "touch on access" recency bump; TTL/forget are the only deletion paths.

### Pitfall 8: non-deterministic decay / local-time timestamps
**What goes wrong:** `SystemTime::now()` inside the formula makes tests flaky; local time corrupts deltas.
**How to avoid:** injectable `Clock` trait; UTC epoch i64 everywhere; compute-on-read from timestamps.

### Pitfall 11: SQLite writer contention
**What goes wrong:** concurrent writes → `SQLITE_BUSY` / "database is locked".
**How to avoid:** WAL + `busy_timeout` + single serialized writer lane + read pool; checkpoint WAL periodically.

### Pitfall (FTS5-specific): bm25 sign + FTS5 not available
**What goes wrong:** (a) blending `bm25()` (smaller=better) additively with decay (larger=better) inverts ranking; (b) assuming FTS5 is compiled in.
**How to avoid:** negate bm25 before blending; `rusqlite` `bundled` compiles FTS5 in by default — confirm with a smoke query at migration time.

## Code Examples

### memory_search service (keyword + decay blend, empty-not-error)
```rust
// returns Ok(vec![]) on no match — MCP-02 acceptance: empty list, not error
pub async fn search(&self, args: SearchArgs) -> Result<Vec<MemoryView>, MemoryError> {
    let clock = self.clock.clone();
    let pool = self.read_pool.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(MemoryError::Pool)?;
        // run the bm25×decay query (Pattern 3); recompute decay_score on read from timestamps
        // bump last_accessed/access_count on returned ids via the WRITER lane (fire-and-forget)
        Ok(rows)
    }).await.map_err(MemoryError::Join)?
}
```

### MemoryType validation (D-07, clean error)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType { Decision, Pattern, Error, Todo, Architecture, Constraint }

impl std::convert::TryFrom<&str> for MemoryType {
    type Error = MemoryError;
    fn try_from(s: &str) -> Result<Self, MemoryError> {
        match s {
            "DECISION" => Ok(Self::Decision),
            "PATTERN" => Ok(Self::Pattern),
            "ERROR" => Ok(Self::Error),
            "TODO" => Ok(Self::Todo),
            "ARCHITECTURE" => Ok(Self::Architecture),
            "CONSTRAINT" => Ok(Self::Constraint),
            other => Err(MemoryError::InvalidType(other.to_string())),
        }
    }
}
impl MemoryType {
    /// Pinned types decay slower (D-08).
    pub fn is_pinned(self) -> bool {
        matches!(self, Self::Decision | Self::Architecture | Self::Constraint)
    }
}
```

### Decay formula (deterministic, on-read, per-type pinning)
```rust
// score in (0,1]; pinned types use a longer half-life. (D-08/D-09)
pub fn decay_score(now: i64, last_accessed: i64, half_life_secs: f64, pinned: bool) -> f64 {
    let elapsed = (now - last_accessed).max(0) as f64;
    let hl = if pinned { half_life_secs * 6.0 } else { half_life_secs }; // pinned decays ~6x slower (tunable)
    // exp(-ln2 * elapsed / half_life): score halves every half_life
    (-std::f64::consts::LN_2 * elapsed / hl).exp()
}
// default half_life_secs = 30 days = 2_592_000 (D-09); configurable via env/config.
```
> The `6.0` pinned multiplier and the bm25/decay blend weights are tuning details left to the planner/implementer (D-04/D-09). The contract is: pinned > unpinned at equal age; just-accessed > un-accessed.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `transmute`-based sqlite extension registration | `RawAutoExtension` | rusqlite 0.34+ | Not relevant Phase 1 (no sqlite-vec yet); flagged for Phase 2 |
| rmcp 0.7/0.8 tool API | rmcp 1.x `#[tool_router]` + `Parameters<T>` + `#[tool_handler]` | rmcp 1.0 | Use the 1.8 macro shape verified here; do NOT copy pre-1.0 snippets |
| FTS5 `LIKE` fallback | native `fts5` + `bm25()` | long-stable | Use FTS5 from the start; it's in bundled SQLite |

**Deprecated/outdated:**
- Any rmcp example using `#[tool]` without `ToolRouter`/`#[tool_router]` (pre-1.0) — won't compile against 1.8.
- `sqlite-vss` — irrelevant to Phase 1 (no vectors), and dead even for Phase 2 (use `sqlite-vec`).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `tempfile` 3 + `assert_cmd`/`predicates` are the right dev-test crates | Supporting stack | LOW — swap for `std::process::Command` + `NamedTempFile`; no production impact |
| A2 | Pinned types decay ~6× slower via a half-life multiplier (the `6.0`) | Decay formula | LOW — D-08 only mandates "much slower"; exact factor is discretion (D-09) |
| A3 | bm25×decay additive blend with configurable weights satisfies D-04 | Pattern 3 | LOW — D-04 explicitly leaves the formula to the implementer; sign rule is the only hard constraint |
| A4 | INTEGER rowid `id` (not UUID) is the chosen id type | Stack/Alternatives | MEDIUM — FTS5 external content *requires* an INTEGER rowid; using UUID forces a parallel column. Recommend INTEGER; confirm with planner since tool returns this id to agents |
| A5 | `McpError::invalid_params(msg, None)` is the exact 1.8 constructor | Pattern 1 | LOW — shape verified; exact method name confirm via `cargo doc` at wiring time |
| A6 | Default half-life = 30 days (2,592,000 s) | Decay formula | LOW — directly from D-09 ("~30 days") |

**These are LOW/MEDIUM risk; all align with Claude's-Discretion areas in CONTEXT.md.** The only one worth a planner glance is A4 (id type), because the chosen id is returned to agents over the tool boundary.

## Open Questions (RESOLVED)

> All three are LOW/MEDIUM-risk discretion items; each recommendation was adopted into the Phase-1 plans (id=INTEGER in 0001_init.sql/01-01; fire-and-forget recency bump in service.rs/01-02; materialize-on-sweep + recompute-on-read in 01-02/01-03). No unresolved design fork remains for the executor.

1. **Memory id type: INTEGER rowid vs UUID string**
   - What we know: FTS5 external-content tables need an INTEGER `content_rowid`. INTEGER `id` lets `memories.id == memories_fts.rowid` with no extra column.
   - What's unclear: whether the product wants opaque/stable string ids in the tool contract (D-06 just says "id").
   - RESOLVED: use INTEGER `id`, return it as a number in tool results. Cheapest, FTS5-native. (A4) — adopted in 01-01 schema.

2. **Recency-bump write amplification**
   - What we know: bumping `last_accessed`/`access_count` on every search hit adds writes through the single writer.
   - What's unclear: whether to bump synchronously or debounce.
   - RESOLVED: fire-and-forget through the writer lane in Phase 1; debounce only if a stress test shows contention. (Pitfall 11) — adopted in 01-02 service.rs.

3. **Decay-score materialization vs pure on-read**
   - What we know: ARCHITECTURE research recommends hybrid (sweep materializes, read recomputes).
   - What's unclear: whether Phase 1 needs the materialized column at all for ranking, or can `ORDER BY` compute decay inline.
   - RESOLVED: keep the `decay_score` column (sweep materializes it for cheap `ORDER BY`), but the blend query recomputes-on-read for correctness between sweeps. Both honored in the schema below; adopted in 01-02/01-03.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain (cargo) | entire phase | assumed ✓ (ecosystem standard) | stable, edition 2021 | none — blocking |
| C compiler (for `rusqlite` bundled C) | `rusqlite` bundled build | assumed ✓ (cc on macOS/Linux dev + runners) | — | none — but standard on all targets; zigbuild covers cross-compile |
| `cargo-llvm-cov` | >80% coverage gate | ✗ (install) | 0.8 | `cargo-tarpaulin` (Linux) |
| Ollama | NOT required Phase 1 | n/a | — | n/a — Phase 1 is keyword-only by design (SEARCH-01) |
| Network | NOT used Phase 1 | n/a | — | offline by construction (durability test asserts this) |

**Missing dependencies with no fallback:** none blocking — Rust + cc are assumed present (ecosystem already ships 5 Rust tools). `cargo-llvm-cov` is a one-line install on the runner.
**Note:** No `sqlite-vec` / no C cross-compile risk in Phase 1 because there is no embedding/vec0 table yet — that risk (PITFALLS Pitfall 3) lands in Phase 2/release. Phase 1's only C is bundled SQLite, which `rusqlite` already ships and the ecosystem cross-compiles via zigbuild.

## Concrete `memories` Schema (Phase 1 — no embedding column)

```sql
-- 0001_init.sql  (run via rusqlite_migration M::up; PRAGMAs set per-connection, not here)
CREATE TABLE memories (
    id            INTEGER PRIMARY KEY,          -- rowid; == memories_fts.rowid
    mem_type      TEXT NOT NULL,                -- DECISION|PATTERN|ERROR|TODO|ARCHITECTURE|CONSTRAINT
    content       TEXT NOT NULL,
    tags          TEXT NOT NULL DEFAULT '[]',   -- JSON array (serde_json)
    source        TEXT,                         -- NULL allowed (D-05)
    scope         TEXT,                         -- NULL = global (D-03/D-05)
    base_weight   REAL NOT NULL DEFAULT 1.0,    -- per-type importance (pinning feeds this)
    decay_score   REAL NOT NULL DEFAULT 1.0,    -- materialized by sweep; recomputed on read
    access_count  INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,             -- UTC unix epoch (i64)
    last_accessed INTEGER NOT NULL,             -- UTC unix epoch (i64)
    expires_at    INTEGER                       -- NULL = no TTL (STORE-04)
    -- NO embedding column in Phase 1; added via Phase 2 migration.
);

CREATE INDEX idx_memories_type    ON memories(mem_type);
CREATE INDEX idx_memories_scope   ON memories(scope);
CREATE INDEX idx_memories_expires ON memories(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX idx_memories_decay   ON memories(decay_score);

CREATE VIRTUAL TABLE memories_fts USING fts5(content, content='memories', content_rowid='id');
-- + the three triggers from Pattern 3
```

## Validation Architecture

> nyquist_validation is enabled (config.json `workflow.nyquist_validation: true`). This section is consumed to generate VALIDATION.md.

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` / `#[tokio::test]` + integration tests in `crates/agent-memory-core/tests/` and binary-level tests |
| Config file | none (cargo standard); `cargo-llvm-cov` config optional in `Cargo.toml` |
| Quick run command | `cargo test -p agent-memory-core` |
| Full suite command | `cargo test --workspace` |
| Coverage command | `cargo llvm-cov --workspace --fail-under-lines 80` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command / Harness | File Exists? |
|--------|----------|-----------|------------------------------|-------------|
| STORE-01 | Each of 6 types persists; invalid type → clean error (no panic) | unit | `MemoryType::try_from` + `service.store` over temp-file SQLite; assert `Err(InvalidType)` not panic | ❌ Wave 0 |
| STORE-02 | N memories survive process restart, offline | integration | Store N into a temp-file DB, drop store, reopen same path, assert all N returned; assert no network crate on path | ❌ Wave 0 |
| STORE-03 | Decay decreases for un-accessed; surfaced in results | unit | `TestClock` advanced by T; assert `decay_score(un-accessed) < decay_score(just-accessed)`; deterministic curve assert | ❌ Wave 0 |
| STORE-04 | TTL sweep removes expired; near-zero decay + no TTL still retrievable | integration | Insert with past `expires_at`; run sweep; assert gone. Insert low-decay no-TTL; assert still listed | ❌ Wave 0 |
| MCP-01 | `memory_store` returns valid id; row has fields; omit scope → NULL | integration | Pipe `tools/call memory_store` JSON-RPC to the binary's stdin; parse id from stdout; query DB asserts NULL scope | ❌ Wave 0 |
| MCP-02 | FTS5 keyword search ranks sensibly; no-match → empty list | integration | Store rows, `tools/call memory_search`; assert ordering + empty (not error) on no-match; Ollama absent (it isn't used) | ❌ Wave 0 |
| MCP-03 | `memory_list` filters + limit | integration | `tools/call memory_list` with each filter; assert exact rows + limit cap | ❌ Wave 0 |
| MCP-04 | `memory_forget` deletes; unknown id → clean not-found | integration | `tools/call memory_forget` known id (gone after) + unknown id (not-found, no error) | ❌ Wave 0 |
| MCP-05 | stdout is pure JSON-RPC; logs on stderr | integration (CI gate) | Spawn binary, pipe `initialize` to stdin, capture stdout+stderr separately; assert **every** stdout line parses as JSON-RPC and stderr has the log lines | ❌ Wave 0 |
| SEARCH-01 | Keyword search works with no Ollama | integration | Same as MCP-02; explicitly assert no localhost:11434 call (no reqwest on path in Phase 1) | ❌ Wave 0 |

### Harnesses
- **Injected `Clock`:** `TestClock` (advanceable) makes STORE-03/STORE-04 deterministic — no `SystemTime::now()` in the formula.
- **Temp-file SQLite:** `tempfile::NamedTempFile` (or a temp dir) gives real persistence semantics for restart tests; `:memory:` is NOT used for STORE-02 (must survive reopen).
- **Piped JSON-RPC:** spawn the built binary, write `initialize`/`tools/call` frames to stdin, read stdout — this is the real stdio transport, exercising MCP-01..05 end-to-end.

### Sampling Rate
- **Per task commit:** `cargo test -p agent-memory-core` (fast unit layer) + `cargo clippy -- -D warnings` + `cargo fmt --check`
- **Per wave merge:** `cargo test --workspace` (includes the stdio integration tests)
- **Phase gate:** `cargo llvm-cov --workspace --fail-under-lines 80` green + the stdout-purity test green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/agent-memory-core/src/clock.rs` — `Clock` trait + `TestClock` (blocks STORE-03/04 determinism)
- [ ] `crates/agent-memory-core/tests/store.rs` — persistence + restart (STORE-01/02)
- [ ] `crates/agent-memory-core/tests/decay.rs` — deterministic decay curve + pinning (STORE-03, D-08)
- [ ] `crates/agent-memory-core/tests/ttl.rs` — sweep removes expired, decay≠delete (STORE-04)
- [ ] `crates/agent-memory/tests/stdio_purity.rs` — piped `initialize`, stdout-JSON-RPC-only assertion (MCP-05) — the highest-priority gate
- [ ] `crates/agent-memory/tests/tools.rs` — the four tools over piped JSON-RPC (MCP-01..04, SEARCH-01)
- [ ] Coverage tool install on CI runner: `cargo install cargo-llvm-cov`
- [ ] `.github/workflows/ci.yml` — `runs-on: [arc-runner-unityinflow]`, steps: fmt-check, clippy -D warnings, test --workspace, llvm-cov fail-under 80, stdout-purity

## Project Constraints (from CLAUDE.md)

- Rust stable, **edition 2021**; no JVM/Python runtime dependency.
- `clap` (derive), `serde`/`serde_json`, `tokio`, **`anyhow` in binary / `thiserror` in library**.
- **No `unwrap()`/`expect()` in production code** — use `?`, handle errors. (A long-running daemon panic kills sessions.)
- **Exhaustive pattern matching** — no catch-all `_` unless justified.
- `cargo fmt` + `cargo clippy -- -D warnings` clean before every commit (enforced by active harness hooks once `Cargo.toml` lands).
- **>80% coverage on core logic** before phase completion.
- **No secrets committed**; all credentials via env (not relevant Phase 1 — no secrets).
- CI: **self-hosted runners only** — `runs-on: [arc-runner-unityinflow]` (X64) / `[orangepi]` (ARM64). **Never `ubuntu-latest`.** Hetzner X64 fleet is intermittently offline (ecosystem-wide blocker) — orangepi fallback may be needed even for Phase-1 CI.
- Distribution targets: macOS (arm64/x86_64) + Linux (x86_64/aarch64). **Windows deferred to v2** — keep platform deps (`dirs`) centralized; do not scatter `cfg(unix)`.

## Security Domain

> `security_enforcement` not explicitly false in config → included.

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | Local single-user stdio daemon; no auth surface in Phase 1 (no network listener) |
| V3 Session Management | no | No sessions |
| V4 Access Control | partial | DB file perms — create the data dir `0700` so other local users can't read the corpus (PITFALLS security table) |
| V5 Input Validation | yes | `MemoryType::try_from` rejects bad types (D-07); `Parameters<T>` + schemars validates tool args; SQL via parameterized rusqlite (never string-built) |
| V6 Cryptography | no | No crypto in Phase 1 |

### Known Threat Patterns for a local Rust SQLite MCP daemon
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via tool args | Tampering | Parameterized `rusqlite` queries only — never format SQL strings with user input |
| Stored memory as prompt-injection carrier | Tampering | Treat memory content as untrusted data; Phase 1 never executes/renders it (composes with injection-scanner later) |
| World-readable DB file | Information Disclosure | Create `<data_dir>/agent-memory/` with `0700` perms |
| Panic leaking to stdout (DoS of the client) | DoS | Panic hook → stderr; `?`/typed errors, no `unwrap()` |
| Secrets accidentally stored in memory content | Information Disclosure | Out of Phase-1 scope; document that storage is plaintext SQLite |

## Sources

### Primary (HIGH confidence)
- github.com/modelcontextprotocol/rust-sdk — `examples/servers/src/counter_stdio.rs` + `common/counter.rs` (rmcp 1.8 `#[tool_router]`/`#[tool_handler]`/`Parameters<T>`/`serve(stdio())`/stderr logging) — fetched verbatim via `gh api` this session
- sqlite.org/fts5.html — external-content table + INSERT/DELETE/UPDATE trigger sync pattern; `bm25()` semantics (smaller = better)
- crates.io (`cargo search`) — verified versions: rmcp 1.8.0, rusqlite 0.40.1, rusqlite_migration 2.6.0, r2d2 0.8.10, r2d2_sqlite 0.34.0, clap 4.6.1, tokio 1.52.3, schemars 1.2.1, thiserror 2.0.18, anyhow 1.0.102, chrono 0.4.45, dirs 6.0.0, cargo-llvm-cov 0.8.7
- `gsd-tools query package-legitimacy check --ecosystem crates …` — all crates verdict OK
- Project research (this milestone, HIGH): `.planning/research/STACK.md`, `ARCHITECTURE.md`, `PITFALLS.md`

### Secondary (MEDIUM confidence)
- docs.rs/rmcp 1.8 — `serve_server`, `ServerHandler`, transport `IntoTransport` (builder method names to confirm at wiring time)
- docs.rs/rusqlite_migration — `Migrations::new(vec![M::up(...)])` + `to_latest`

### Tertiary (LOW confidence)
- dev-test crate choices (`tempfile`/`assert_cmd`) — conventional, mark for confirmation (A1)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all versions verified on crates.io + legitimacy-checked; builds on settled project research
- rmcp tool/stdio API: HIGH — code fetched verbatim from official 1.8 examples this session
- FTS5 schema/triggers/bm25: HIGH — official sqlite.org docs; sign rule explicit
- Decay formula/blend: MEDIUM — math is standard; exact constants are discretion (D-04/D-09)
- Architecture/pitfalls: HIGH — inherited from project research, cross-checked against this phase's scope

**Research date:** 2026-06-24
**Valid until:** 2026-07-24 (rmcp is fast-moving across minors — re-verify the macro/builder surface before bumping past 1.8; ~30 days otherwise)
</content>
</invoke>
