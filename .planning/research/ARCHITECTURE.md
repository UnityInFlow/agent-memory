# Architecture Research

**Domain:** Single-binary Rust daemon — persistent agent memory (MCP server + REST API over embedded SQLite, with local Ollama embeddings and a decay/TTL background job)
**Researched:** 2026-06-24
**Confidence:** HIGH (stack pieces are all current, verified against official docs; one MEDIUM area noted below)

## Standard Architecture

### System Overview

```
┌──────────────────────────────────────────────────────────────────────┐
│                         INTERFACE LAYER (bin)                          │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────────────────┐   │
│  │ MCP / stdio  │   │ MCP / HTTP   │   │ REST API (axum)          │   │
│  │ (rmcp)       │   │ (rmcp+axum)  │   │ store/search/list/forget │   │
│  └──────┬───────┘   └──────┬───────┘   └────────────┬─────────────┘   │
│         │   CLI: import / serve / forget …          │                 │
│  ┌──────┴────────────────────────────────────────────────────────┐   │
│  │                  Tool handlers (thin adapters)                 │   │
│  └──────────────────────────────┬─────────────────────────────────┘   │
├─────────────────────────────────┼──────────────────────────────────────┤
│                       DOMAIN / SERVICE LAYER (lib)                     │
│  ┌───────────────┐ ┌───────────────┐ ┌──────────────┐ ┌────────────┐ │
│  │ MemoryService │ │ SearchService │ │ DecayEngine  │ │ ImportSvc  │ │
│  │ store/forget  │ │ semantic+kw   │ │ score + TTL  │ │ STATE.md   │ │
│  └───────┬───────┘ └───────┬───────┘ └──────┬───────┘ └─────┬──────┘ │
│          │                 │                │                │        │
│  ┌───────┴─────────────────┴────────────────┴────────────────┴────┐  │
│  │  Domain types: Memory, MemoryType, DecayScore, Embedding, Error  │  │
│  └──────────────────────────────────────────────────────────────────┘ │
├────────────────────────────────┬──────────────────────┬───────────────┤
│                          ADAPTERS                       │               │
│  ┌──────────────────────────┐  ┌───────────────────┐  ┌──────────────┐ │
│  │ SqliteStore (rusqlite +  │  │ OllamaClient      │  │ Background    │ │
│  │ r2d2 pool, WAL)          │  │ (reqwest /api/embed)│ │ tokio task    │ │
│  │ memories + vec0 table    │  │ degrades→keyword  │  │ (interval)    │ │
│  └──────────────────────────┘  └───────────────────┘  └──────────────┘ │
└──────────────────────────────────────────────────────────────────────┘
                 │                          │
            SQLite file                 Ollama daemon
         (WAL, sqlite-vec)             (localhost:11434)
```

### Component Responsibilities

| Component | Responsibility | Implementation |
|-----------|----------------|----------------|
| `agent-memory` (bin) | CLI arg parsing, transport wiring, runtime bootstrap, shutdown | `clap` (derive) + `tokio::main` |
| MCP server | Map `memory_store/search/list/forget` MCP tools to service calls | `rmcp` (official SDK), `transport-io` + `transport-streamable-http-server` |
| REST API | Same four operations over HTTP/JSON for non-MCP clients | `axum` (shares runtime/router with MCP-over-HTTP) |
| `MemoryService` | CRUD + lifecycle: validate, embed-on-store, persist, forget | lib module, returns `Result<T, MemoryError>` |
| `SearchService` | Semantic (vector) search with keyword fallback; rank by similarity × decay | lib module |
| `DecayEngine` | Compute decay score; background refresh + on-read recency bump; TTL sweep | `tokio::time::interval` task |
| `ImportService` | Parse GSD STATE.md → `Memory` records | lib module + parser submodule |
| `SqliteStore` | All SQL; owns connection pool; schema migrations; vec0 table | `rusqlite` (bundled) + `r2d2`/`r2d2_sqlite` |
| `OllamaClient` | Embed text via local Ollama; health check; timeout/degrade | `reqwest` JSON client |

**Boundary rule (enforces the anyhow/thiserror convention):** the **library crate uses `thiserror`** for typed `MemoryError`; the **binary crate uses `anyhow`** at the edges (CLI/transport bootstrap). Interfaces depend on services; services depend on adapters via traits (`Store`, `Embedder`) so tests can swap in-memory/fake impls.

## Recommended Project Structure

Cargo **workspace** with a library + a binary. This is the cleanest way to honor the `thiserror` (lib) vs `anyhow` (bin) split and to keep transports thin.

```
agent-memory/                  # workspace root
├── Cargo.toml                 # [workspace] members
├── crates/
│   ├── agent-memory-core/     # the library — no anyhow, thiserror only
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── domain/        # Memory, MemoryType, DecayScore, Embedding
│   │   │   │   ├── mod.rs
│   │   │   │   └── error.rs   # MemoryError (thiserror)
│   │   │   ├── service/       # MemoryService, SearchService, ImportService
│   │   │   ├── decay/         # decay math + DecayEngine task driver
│   │   │   ├── store/         # Store trait + SqliteStore impl, migrations, vec0
│   │   │   ├── embed/         # Embedder trait + OllamaClient
│   │   │   └── import/        # gsd_state.rs parser
│   │   └── tests/             # integration tests over a temp SQLite file
│   └── agent-memory/          # the binary — anyhow at the edges
│       └── src/
│           ├── main.rs        # clap CLI, tokio runtime, graceful shutdown
│           ├── mcp/           # rmcp tool handlers (4 tools) — thin adapters
│           ├── rest/          # axum router + handlers — thin adapters
│           └── serve.rs       # builds shared AppState, mounts MCP+REST
└── .github/workflows/         # CI (self-hosted runners), release (binaries)
```

### Structure Rationale

- **`-core` library, `-bin` binary:** required to obey the ecosystem rule (`thiserror` for libraries, `anyhow` for binaries). It also makes the daemon, the CLI `import`, and tests all link the same domain logic.
- **`store/` behind a `Store` trait:** lets `DecayEngine` and services be unit-tested without a real DB, and isolates the one place that knows about SQLite/vec0/WAL.
- **`embed/` behind an `Embedder` trait:** the graceful-degradation path (Ollama down → keyword search) lives at this seam; tests inject a fake embedder.
- **`mcp/` and `rest/` as thin adapters:** both call the *same* `MemoryService`. No business logic in transports — only (de)serialization and error mapping.

## Architectural Patterns

### Pattern 1: Shared `AppState`, two transports, one runtime

**What:** Build one `Arc<AppState>` holding the connection pool, the Ollama client, and config. Mount the `rmcp` streamable-HTTP service and the `axum` REST router on the **same** `tokio` runtime / port (or two ports). Stdio MCP is selected at startup for editor integration.
**When to use:** Whenever a daemon must speak more than one protocol over the same store — exactly this case.
**Trade-offs:** Single process = simple ops and one DB pool, but a panic in one handler can affect the process; mitigate with per-request error boundaries (rmcp/axum already isolate handler errors into responses).

```rust
struct AppState { pool: Pool<SqliteConnectionManager>, embedder: Arc<dyn Embedder>, cfg: Config }

// serve.rs (sketch)
let state = Arc::new(AppState { /* ... */ });
match args.transport {
    Transport::Stdio => rmcp::serve_stdio(MemoryMcp::new(state)).await?,
    Transport::Http  => {
        let mcp = rmcp_streamable_http_router(MemoryMcp::new(state.clone()));
        let app = axum::Router::new().nest("/mcp", mcp).merge(rest::router(state));
        axum::serve(listener, app).await?;
    }
}
```

### Pattern 2: vec0 sidecar table joined by rowid

**What:** Keep authoritative rows in a normal `memories` table; store vectors in a `sqlite-vec` `vec0` virtual table whose `rowid` equals `memories.id`. KNN query returns `(rowid, distance)`; join back to `memories`.
**When to use:** Semantic search with metadata — `vec0` does the ANN/brute-force distance, the relational table does typed filtering, decay, and TTL.
**Trade-offs:** Two writes per store (one per table) — wrap in a transaction. `sqlite-vec` is brute-force (no HNSW), which is *fine* for an agent's local memory (thousands to low-millions of rows); it is exact, not approximate.

```sql
CREATE VIRTUAL TABLE vec_memories USING vec0(embedding float[768]);
-- search:
SELECT m.id, m.content, m.decay_score, v.distance
FROM vec_memories v
JOIN memories m ON m.id = v.rowid
WHERE v.embedding MATCH ?1 AND k = ?2
ORDER BY v.distance;          -- re-rank with decay_score in Rust
```

### Pattern 3: Hybrid decay — background sweep + on-read recompute

**What:** Decay is *computed on read* from `last_accessed` + `created_at` (cheap, always correct), while a **background `tokio` interval task** periodically (e.g. hourly/daily) materializes `decay_score` into the column for cheap `ORDER BY` and runs the TTL sweep (`DELETE WHERE expires_at < now`). On every read/search hit, bump `last_accessed` and `access_count` (recency reinforcement).
**When to use:** Any "memories fade unless used" model. Compute-on-read guarantees correctness between sweeps; the sweep keeps the stored score usable for ranking/pruning.
**Trade-offs:** A write-on-read (recency bump) adds write load; batch these or make them async/fire-and-forget through the single writer to avoid contention.

```
score(t) = base_weight * exp(-λ * (now - last_accessed))   // λ derived from a half-life config
```

## Data Flow

### Store flow (write path)

```
client → MCP/REST handler → MemoryService.store(content, type, ttl?)
   → Embedder.embed(content)            [Ollama /api/embed; on error → embedding=None]
   → SqliteStore.insert (TX):
        INSERT memories(...)             → id
        INSERT vec_memories(rowid=id, embedding)   [only if embedding present]
   → return Memory { id, decay_score=1.0, ... }
```

### Search flow (read path)

```
client → handler → SearchService.search(query, type?, limit)
   → Embedder.embed(query)
        ├─ ok  → vec0 KNN (MATCH, k) → join memories → re-rank by distance × decay
        └─ err → keyword LIKE/FTS fallback over memories.content (degraded mode)
   → bump last_accessed + access_count on returned rows (async)
   → return ranked memories
```

### Background lifecycle flow

```
tokio interval (e.g. every 1h):
   DecayEngine.sweep():
     UPDATE memories SET decay_score = recompute(...)         (single writer)
     DELETE FROM memories WHERE expires_at IS NOT NULL AND expires_at < now()
     DELETE FROM vec_memories WHERE rowid NOT IN (SELECT id FROM memories)
```

### Import flow

```
CLI: agent-memory import --from gsd-state .planning/STATE.md
   → ImportService.parse(file) → Vec<Memory draft (type inferred from section)>
   → for each: MemoryService.store(...)   (reuses normal store path → embeds + persists)
```

## Concrete SQLite Schema

```sql
PRAGMA journal_mode = WAL;        -- concurrent readers + 1 writer
PRAGMA synchronous = NORMAL;      -- good durability/speed balance for WAL
PRAGMA foreign_keys = ON;

CREATE TABLE memories (
    id            INTEGER PRIMARY KEY,         -- == vec_memories.rowid
    mem_type      TEXT NOT NULL,               -- DECISION|PATTERN|ERROR|TODO|ARCHITECTURE|CONSTRAINT
    content       TEXT NOT NULL,
    metadata      TEXT,                        -- JSON blob (serde_json)
    embedding_ok  INTEGER NOT NULL DEFAULT 0,  -- 0 = no embedding (degraded), 1 = present
    decay_score   REAL NOT NULL DEFAULT 1.0,   -- materialized by sweep, recomputed on read
    base_weight   REAL NOT NULL DEFAULT 1.0,   -- per-type or per-import importance
    access_count  INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,            -- unix epoch (i64)
    last_accessed INTEGER NOT NULL,
    expires_at    INTEGER                      -- NULL = no TTL
);

CREATE INDEX idx_memories_type        ON memories(mem_type);
CREATE INDEX idx_memories_expires     ON memories(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX idx_memories_decay       ON memories(decay_score);

-- sqlite-vec sidecar (registered via sqlite3_auto_extension at startup)
CREATE VIRTUAL TABLE vec_memories USING vec0(embedding float[768]);  -- nomic-embed-text = 768-dim

-- optional keyword fallback (degraded mode / no-Ollama installs)
CREATE VIRTUAL TABLE memories_fts USING fts5(content, content='memories', content_rowid='id');
```

**Notes:**
- `vec0` is **brute-force exact KNN** (no HNSW) — appropriate and fast enough for local agent memory; revisit only at >~1M vectors.
- Vectors stored as 768 × `f32`; pass `Vec<f32>` to rusqlite as bytes via the `zerocopy` crate (no copy).
- nomic-embed-text supports Matryoshka truncation (64–768 dims) — pin to **768** for v0.0.1, make the column width a constant so it can be tuned later.

## Concurrency / Locking Model

- **WAL mode** → many concurrent readers + exactly one writer. This matches a daemon serving parallel MCP/REST reads with occasional writes.
- **Connection pool** (`r2d2_sqlite`): a small read pool plus a **single serialized writer**. Funnel all writes (store, recency bump, decay sweep, TTL delete) through one path so SQLite never returns `SQLITE_BUSY` from competing writers. Set `busy_timeout` as a safety net.
- The **decay sweep runs on the same writer lane** as stores — it does not need its own connection competing for the write lock.
- Recency bumps on read are the main contention risk; batch/debounce them or coalesce into the sweep if write pressure shows up.
- sqlite-vec extension is registered once at process start via `sqlite3_auto_extension` (rusqlite `bundled` feature) so every pooled connection has it.

## Scaling Considerations

| Scale | Adjustments |
|-------|-------------|
| 1 dev / 1 project (primary case) | Single file, WAL, brute-force vec search — nothing more needed |
| Many projects / 100k+ memories | Add per-project namespacing (column or separate DB file); ensure decay sweep stays incremental; FTS for cheap pre-filter before KNN |
| >1M vectors | sqlite-vec brute force gets slow; consider pre-filtering by type/recency, or evaluate an ANN-capable store (Turso/libsql, or partition vectors) |

### Scaling Priorities

1. **First bottleneck: write contention from on-read recency bumps** → batch/debounce through the single writer.
2. **Second bottleneck: brute-force KNN latency at large N** → pre-filter candidate set (type + decay/recency) before the `MATCH`, or cap k.

## Anti-Patterns

### Anti-Pattern 1: Business logic in the MCP/REST handlers
**What people do:** Implement decay/embedding/SQL inside the `rmcp` tool functions.
**Why it's wrong:** Duplicates logic across stdio, HTTP, and CLI; untestable without a transport.
**Do this instead:** Handlers are thin adapters that call `MemoryService`/`SearchService`.

### Anti-Pattern 2: Hard dependency on Ollama
**What people do:** Fail `store`/`search` when Ollama is unreachable.
**Why it's wrong:** Breaks the "zero cloud, just works" promise; Ollama may not be installed.
**Do this instead:** Embed-on-store is best-effort (`embedding_ok=0` on failure); search degrades to FTS/keyword. Surface a one-line "semantic search unavailable" notice.

### Anti-Pattern 3: Multiple writer connections / no WAL
**What people do:** Open a fresh connection per request and write from all of them.
**Why it's wrong:** `SQLITE_BUSY`, corrupt-looking lock errors under concurrency.
**Do this instead:** WAL + pooled readers + a single serialized writer lane.

### Anti-Pattern 4: Recompute-only or store-only decay
**What people do:** Either never materialize decay (every ranking does math) or only update on a timer (stale between sweeps).
**Why it's wrong:** Pure recompute makes `ORDER BY` expensive; pure timer gives wrong scores between runs.
**Do this instead:** Hybrid — materialize on sweep, correct on read.

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| Ollama | HTTP `POST /api/embed` (`reqwest`), model `nomic-embed-text` (768-dim) | Local only (localhost:11434). Use `/api/embed` (modern; `input` accepts array → batch). Timeout + health check; degrade gracefully. |
| MCP clients (Claude Code, Cursor) | `rmcp` stdio (editor spawns the binary) and streamable-HTTP | stdio is the default editor integration path. |
| GSD STATE.md | File parser in `import/` | Maps section headings → memory types; reuses the normal store path. |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| handlers ↔ services | direct function calls on `AppState` | no logic leak into transports |
| services ↔ store | `Store` trait | swappable for tests; SqliteStore is the only impl |
| services ↔ embedder | `Embedder` trait | degrade path lives here |
| DecayEngine ↔ store | writer lane | shares the single writer; runs on tokio interval |

## Build Order Implications (for roadmap phasing)

Dependencies flow strictly upward — **domain → store → service → transport → release**. Suggested order:

1. **Domain + schema + SqliteStore (no vectors yet).** `Memory`/`MemoryType`/`MemoryError`, migrations, WAL/pool, plain CRUD. *Everything depends on this.*
2. **MemoryService (store/list/forget) + decay/TTL.** Decay math, on-read recency, background sweep. Needs (1).
3. **MCP server (4 tools, stdio).** Thin adapters over (2). First user-facing surface; needs (2).
4. **Ollama embedder + semantic search.** `Embedder` trait, OllamaClient, vec0 table, KNN + keyword fallback, re-rank by decay. Needs (2) and (3) (search tool already wired to keyword in (2)/(3), now upgraded). *Semantic search is the latest-to-arrive feature and must not block MCP.*
5. **REST API (axum) + MCP-over-HTTP.** Same services, second transport. Needs (2)–(4).
6. **GSD STATE.md import.** Parser → reuses store path. Needs (2); independent of (4)/(5), can slot in parallel.
7. **Release: prebuilt binaries (self-hosted runners) + Homebrew, README/CONTRIBUTING/LICENSE/CI.** Needs all above + >80% core coverage.

**Key phasing rule:** keyword/FTS search ships *before* semantic search so the MCP `search` tool is functional without Ollama; semantic is a strict enhancement layered on the same query path.

## Sources

- [modelcontextprotocol/rust-sdk (rmcp) — official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk) — stdio + streamable-HTTP transports, axum integration (HIGH)
- [rmcp README — transport features](https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/README.md) (HIGH)
- [asg017/sqlite-vec](https://github.com/asg017/sqlite-vec) — vec0 virtual table, brute-force exact KNN, MIT/Apache, v0.1.0 stable (HIGH)
- [Using sqlite-vec in Rust — Alex Garcia](https://alexgarcia.xyz/sqlite-vec/rust.html) — `sqlite3_auto_extension` registration, rusqlite `bundled`, zerocopy `Vec<f32>` (HIGH)
- [sqlite-vec simple-rust demo](https://github.com/asg017/sqlite-vec/blob/main/examples/simple-rust/demo.rs) — `CREATE VIRTUAL TABLE … vec0`, `MATCH`/`k=`/`ORDER BY distance`, explicit rowid (HIGH)
- [Ollama nomic-embed-text](https://ollama.com/library/nomic-embed-text) — 768-dim, Matryoshka 64–768 (HIGH)
- [How to Build a stdio MCP Server in Rust — Shuttle](https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust) (MEDIUM)
- [How to Build a Streamable HTTP MCP Server in Rust — Shuttle](https://www.shuttle.dev/blog/2025/10/29/stream-http-mcp) (MEDIUM)

---
*Architecture research for: Rust agent-memory daemon (MCP + REST + SQLite + Ollama)*
*Researched: 2026-06-24*
