# Phase 2: Semantic Search, Interop & Release - Research

**Researched:** 2026-07-02
**Domain:** Rust local-first daemon — sqlite-vec semantic search + Ollama embeddings, axum REST mirror, GSD STATE.md import, cross-platform release (zigbuild + Homebrew tap)
**Confidence:** HIGH (crate compatibility verified against crates.io sparse index + docs.rs; Ollama API + vec0 syntax from official docs via Context7; release path grounded in proven in-ecosystem precedent; one LOW-confidence release risk explicitly flagged with a mandatory early spike)

## Summary

Phase 2 layers four independent capabilities onto the completed Phase 1 codebase (Cargo workspace: `agent-memory-core` lib + `agent-memory` bin, rusqlite 0.39 bundled, FTS5 search with bm25×decay blend, rmcp 1.8 stdio server, hourly TTL/decay sweep). Nothing in Phase 1 needs restructuring — semantic search is an additive migration + a new `Embedder` seam, REST is a second thin adapter over the existing `MemoryService`, import is a CLI subcommand reusing the store path, and release is CI work.

The critical compatibility question is settled: **sqlite-vec 0.1.9 (latest stable) has only a `cc` build-dependency** — rusqlite appears only as a dev-dep — so it cannot conflict with the Phase-1-pinned rusqlite 0.39 / libsqlite3-sys 0.37 graph. Registration must use rusqlite's `auto_extension::register_auto_extension` (available by default in 0.39), NOT the older raw-transmute snippet still shown in sqlite-vec's Rust docs (sqlite-vec issue #206). The single biggest phase risk is not code — it is **cross-compiling the bundled C (sqlite3.c + sqlite-vec.c) to apple-darwin from the Linux ARM64 `orangepi` runner** via cargo-zigbuild. DIST-01 requires macOS binaries, and the darwin C cross-compile has known failure modes (`TargetConditionals.h`, zig `-nostdinc`). This must be a Wave-1 spike, not a release-time discovery. Note: Phase 1 already ships bundled sqlite3.c, so this risk exists for DIST-01 with or without sqlite-vec; sqlite-vec adds one more C file compiled the same way.

**Primary recommendation:** Build in this order — (1) migration 0002 + Embedder trait + Ollama client + vec0 KNN search with keyword fallback, with the darwin cross-compile spike running in parallel from day one; (2) axum REST mirror as a separate `serve-rest` process sharing the same WAL SQLite file; (3) import parser; (4) hand-rolled zigbuild release workflow on `orangepi` (the proven injection-scanner/mcp-hub pattern — NOT cargo-dist) + hand-written Homebrew tap formula.

## Project Constraints (from CLAUDE.md)

Directives the planner MUST honor (from `./CLAUDE.md` and the ecosystem root CLAUDE.md):

- Rust stable, edition 2021; `clap` (derive), `serde`/`serde_json`, `tokio`, `reqwest`
- `anyhow` in the binary, `thiserror` in the library (Phase 1 already enforces this split)
- No `unwrap()`/`expect()` in production code — `?` or handled errors only
- Pattern match exhaustively — no catch-all `_` unless truly needed
- `cargo fmt` before every commit; `cargo clippy -- -D warnings` must pass
- Test coverage >80% on core logic before release (CI-enforced via `cargo llvm-cov --fail-under-lines 80`)
- Distribution: pre-built binaries macOS (arm64/x86_64) + Linux (x86_64/aarch64); **Windows deferred to v2** (locked roadmap decision, mcp-hub `cfg(unix)` precedent)
- Zero cloud dependency — SQLite embedded, Ollama local only
- No secrets committed; credentials via env vars
- CI: UnityInFlow org self-hosted runners ONLY — `runs-on: [arc-runner-unityinflow]` (X64) / `[orangepi]` (ARM64); **never `ubuntu-latest`**. Recurring blocker: Hetzner X64 fleet intermittently offline — plan release for orangepi-only serial builds with host-arch-aware smoke tests (STATE.md carry-forward)
- `README.md`, `CONTRIBUTING.md`, `LICENSE` (MIT), `.github/workflows/ci.yml` required before v0.0.1 (README + ci.yml exist; **CONTRIBUTING.md + LICENSE are still missing** — release phase must add them)
- Commit style: `feat:`/`fix:`/`test:`/`docs:`/`chore:` prefixes

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| SEARCH-02 | Semantic similarity search via local Ollama embeddings (`nomic-embed-text`) | sqlite-vec 0.1.9 static-link compatibility with rusqlite 0.39 verified; vec0 KNN syntax + cosine metric documented; Ollama `/api/embed` request/response shape documented; migration 0002 design; Pattern 1–3; Pitfalls 1–4 |
| SEARCH-03 | Graceful degradation to keyword search when Ollama unavailable | `Embedder` trait seam design (Pattern 2); Phase 1 FTS5 search path already exists and stays intact; fallback + mode-surfacing contract; Pitfall 2 |
| API-01 | REST API exposing store/search/list/forget for non-MCP clients | axum 0.8.9 (ecosystem pin) shared-state JSON handler pattern; separate-process-same-WAL-file topology validated; loopback-bind security rule; Pattern 4 |
| INTEROP-01 | Import memories from GSD STATE.md, idempotent re-run | Actual STATE.md structure inspected in this repo; section→type mapping table; (source, type, content) dedup key for idempotency; Pattern 5 |
| DIST-01 | Pre-built binaries: macOS arm64/x86_64, Linux x86_64/aarch64 | Proven in-ecosystem zigbuild-on-orangepi release pattern (injection-scanner v0.0.2 shipped darwin triples); darwin bundled-C cross-compile risk identified with mandatory Wave-1 spike + Plan B; host-arch-aware smoke tests |
| DIST-02 | Homebrew formula installs the binary | Custom tap pattern (`UnityInFlow/homebrew-tap`, `Formula/agent-memory.rb`) with per-arch `on_macos`/`Hardware::CPU.arm?` url+sha256 blocks documented |
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Embedding generation | External service (Ollama, localhost:11434) | Core lib (`embed/` adapter) | Model runs in Ollama; the lib owns only the HTTP call, timeout, and degrade decision |
| Vector KNN + ranking | Database/Storage (sqlite-vec vec0 in SQLite) | Core lib (blend in Rust) | KNN candidate selection in SQL; decay×similarity blend on the small candidate set in Rust |
| Keyword fallback search | Database/Storage (FTS5, Phase 1) | — | Already built; fallback must not duplicate it |
| Semantic-vs-keyword routing | Core lib (`SearchService`) | — | The degrade seam lives at the `Embedder` trait boundary, never in transports |
| REST endpoints | Binary (axum adapter, `rest/`) | Core lib (`MemoryService`) | Thin adapter; zero business logic in handlers (same rule as MCP tools) |
| STATE.md parsing → typed memories | Core lib (`import/`) | Binary (clap subcommand) | Parser is testable library code; CLI only wires file path + prints report |
| Import idempotency | Database/Storage (existence check before insert) | Core lib | Dedup key checked in SQL against the authoritative store |
| Cross-compile + release artifacts | CI/Release infra (orangepi + zigbuild) | — | No code tier involved; workflow + Homebrew tap repo |
| MCP stdio serving | Binary (rmcp, Phase 1) | — | Unchanged this phase |

## Standard Stack

### Core (existing — DO NOT change these pins)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rusqlite | **0.39** (bundled, functions) | Embedded SQLite | **Locked Phase-1 decision**: 0.40 pulls libsqlite3-sys 0.38 and conflicts with r2d2_sqlite 0.34 (`links = "sqlite3"`). Do not bump. [VERIFIED: Cargo.toml + 01-01-SUMMARY] |
| rmcp | 1.8 (server, transport-io, macros) | MCP stdio server | Unchanged this phase [VERIFIED: Cargo.toml] |
| tokio / clap / serde / thiserror / anyhow / chrono / dirs / tracing | as pinned | Phase-1 foundations | Unchanged [VERIFIED: Cargo.toml] |

### New this phase

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| sqlite-vec | **0.1.9** (latest stable; 0.1.10 is alpha-only) | vec0 virtual table, static-linked KNN | Only build-dep is `cc` (rusqlite is a dev-dep `^0.31`) → **no libsqlite3-sys conflict with the 0.39 pin** [VERIFIED: crates.io sparse index + package-legitimacy OK, 59k dl/wk, asg017/sqlite-vec] |
| reqwest | **0.12**, `default-features = false, features = ["json"]` | Ollama HTTP client | Localhost-only HTTP → **no TLS backend at all** (no rustls/ring/openssl C code — directly reduces the darwin cross-compile surface). 0.13.4 exists but 0.12 is the settled line per project STACK.md [VERIFIED: cargo search; CITED: .planning/research/STACK.md] |
| axum | **0.8** (current 0.8.9) | REST API | Ecosystem pin (mcp-hub uses axum 0.8); tower-native [VERIFIED: cargo search + package-legitimacy OK] |
| bytemuck | **1** (current 1.25.0) | `Vec<f32>` → `&[u8]` for vec0 BLOB binding | `cast_slice::<f32, u8>()` is safe, zero-copy, and avoids the zerocopy 0.7→0.8 API rename trap (sqlite-vec docs show the old `AsBytes` API) [VERIFIED: cargo search + package-legitimacy OK, 5.3M dl/wk] |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tower-http | 0.6 (mcp-hub pin; 0.7.0 exists) | timeout/CORS middleware on REST router | Optional — only if the REST router needs middleware; plain axum is sufficient for loopback MVP |
| cargo-zigbuild | 0.23.0 (tool, not dep) | Cross-compile all 6 triples from orangepi | Release workflow [VERIFIED: cargo search; proven in-ecosystem] |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| sqlite-vec KNN | Plain BLOB column + in-Rust brute-force cosine | **Documented Plan B** if sqlite-vec C fails darwin cross-compile. Same storage concept, O(n) scan in Rust — fine at local scale. Keep schema forward-compatible either way |
| bytemuck | zerocopy 0.8 (`IntoBytes`) | Matches sqlite-vec docs lineage, but 0.8 renamed the trait the docs use (`AsBytes` → `IntoBytes`) [ASSUMED]; bytemuck is simpler and equally standard |
| Separate `serve-rest` process | One process serving stdio + HTTP | Editors spawn one stdio process per session — multiple sessions would fight over the REST port. Separate process over the same WAL file is cleaner |
| Hand-rolled release workflow | cargo-dist 0.32.0 | cargo-dist is alive and supports custom runners + Homebrew generation, but its generated workflow defaults to banned `ubuntu-latest`/`macos-latest` and assumes native per-OS builds (org has no macOS runner). The hand-rolled zigbuild matrix is **already proven twice in this ecosystem** (injection-scanner v0.0.2 incl. darwin, mcp-hub v0.1.1). Use the known-good path; hand-write the formula |
| Hand-rolled STATE.md section scanner | pulldown-cmark 0.13.4 | GSD STATE.md is a controlled, machine-generated format — a tolerant line scanner (headings + bullets + fence-state tracking) is ~50 lines and adds no dependency. pulldown-cmark is legitimate (checked OK) if the planner prefers event-based parsing; not required |

**Installation:**
```bash
cargo add sqlite-vec@0.1.9
cargo add reqwest@0.12 --no-default-features --features json
cargo add axum@0.8
cargo add bytemuck@1
```

**Version verification (performed 2026-07-02):**
- `sqlite-vec 0.1.9` — latest stable (0.1.10-alpha.4 is prerelease); deps: `cc ^1.0` (build) only [VERIFIED: crates.io sparse index]
- `axum 0.8.9`, `reqwest 0.13.4` (pin 0.12 line), `tower-http 0.7.0` (pin 0.6), `bytemuck 1.25.0`, `cargo-zigbuild 0.23.0`, `cargo-dist 0.32.0` [VERIFIED: cargo search]
- rusqlite 0.39 `auto_extension` module available **by default** (gated out only under the `loadable_extension` feature) [CITED: docs.rs/rusqlite/0.39.0/rusqlite/auto_extension]

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| sqlite-vec | crates.io | 2 yrs (2024-05) | 59,883/wk | github.com/asg017/sqlite-vec | OK | Approved |
| axum | crates.io | 5 yrs | 7.2M/wk | github.com/tokio-rs/axum | OK | Approved |
| reqwest | crates.io | 9 yrs | 10.6M/wk | github.com/seanmonstar/reqwest | OK | Approved |
| tower-http | crates.io | 9 yrs | 7.6M/wk | github.com/tower-rs/tower-http | OK | Approved |
| tower | crates.io | 9 yrs | 10.5M/wk | github.com/tower-rs/tower | OK | Approved |
| bytemuck | crates.io | 6 yrs | 5.4M/wk | github.com/Lokathor/bytemuck | OK | Approved |
| zerocopy | crates.io | 7 yrs | 15.1M/wk | github.com/google/zerocopy | OK | Approved (alternative only) |
| pulldown-cmark | crates.io | 10 yrs | 2.6M/wk | github.com/raphlinus/pulldown-cmark | OK | Approved (optional alternative only) |

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Architecture Patterns

### System Architecture Diagram

```
 MCP client (Claude Code/Cursor)        non-MCP client (curl, scripts)      user shell
        │ spawns per session                     │ HTTP JSON                    │
        ▼                                        ▼                              ▼
┌────────────────────┐              ┌─────────────────────────┐   ┌──────────────────────────┐
│ agent-memory serve │              │ agent-memory serve-rest │   │ agent-memory import       │
│ (rmcp stdio, P1)   │              │ (axum, 127.0.0.1:port)  │   │  --from gsd-state <path>  │
│ + hourly sweep task│              │ POST /api/memories …    │   │ parse → dedup → store     │
└─────────┬──────────┘              └───────────┬─────────────┘   └────────────┬─────────────┘
          │            all three are thin adapters over            │
          ▼                                        ▼                            ▼
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│                        agent-memory-core :: MemoryService / SearchService                  │
│                                                                                            │
│  store(content,…) ──► Embedder.embed(content) ──ok──► insert memories + vec_memories (TX) │
│                              │ err (Ollama down)                                           │
│                              └──► insert memories only, embedding_status=0 (loud stderr)   │
│                                                                                            │
│  search(query) ──► Embedder.embed(query) ──ok──► vec0 KNN (MATCH, k) ─► join memories     │
│                              │                     └─► blend (1-dist)×decay ─► rank, bump │
│                              │ err ──► existing FTS5 bm25×decay path (Phase 1, unchanged) │
│                              └────────► result carries search_mode: semantic|keyword      │
│                                                                                            │
│  sweep tick (P1) ──► TTL delete ─► materialize decay ─► [NEW] backfill pending embeddings │
└───────────────┬───────────────────────────────────────────────────┬───────────────────────┘
                ▼                                                    ▼
   SQLite file (WAL — safe cross-process:                 Ollama daemon (localhost:11434)
   memories + memories_fts + vec_memories + meta)         POST /api/embed, GET /api/tags
```

### Recommended Project Structure (additions to existing workspace)

```
crates/
├── agent-memory-core/
│   └── src/
│       ├── embed/                # NEW: Embedder trait + OllamaClient + FakeEmbedder(test)
│       │   ├── mod.rs
│       │   └── ollama.rs
│       ├── import/               # NEW: gsd_state.rs tolerant parser → Vec<MemoryDraft>
│       │   ├── mod.rs
│       │   └── gsd_state.rs
│       ├── store/sqlite.rs       # EXTEND: insert_embedding, knn_search, pending_embeddings
│       ├── store/migrations.rs   # EXTEND: register migration 0002
│       ├── sql/0002_embeddings.sql  # NEW: vec_memories vec0 + embedding_status + meta table
│       └── service.rs            # EXTEND: SearchService semantic path + fallback + import fn
└── agent-memory/
    └── src/
        ├── rest/                 # NEW: axum router + handlers + ApiError (thin adapters)
        │   ├── mod.rs
        │   └── handlers.rs
        ├── main.rs               # EXTEND: ServeRest + Import subcommands
        └── mcp.rs                # minor: search result gains search_mode field
.github/workflows/release.yml     # NEW: zigbuild matrix on orangepi, 6 triples, SHA256SUMS
```

### Pattern 1: sqlite-vec registration (rusqlite 0.34+ API — do NOT copy old docs)

**What:** Register the statically-linked extension process-globally, once, BEFORE any `Connection::open` (including the migration/pool connections in `SqliteStore::open`).
**When to use:** At `SqliteStore::open` entry (or a `std::sync::Once` in store init).

```rust
// Source: sqlite-vec issue #206 (github.com/asg017/sqlite-vec/issues/206) +
// docs.rs/rusqlite/0.39.0 auto_extension. The snippet on alexgarcia.xyz/sqlite-vec/rust.html
// uses the PRE-0.34 raw sqlite3_auto_extension pattern and does not fit rusqlite 0.39.
use rusqlite::auto_extension::register_auto_extension;
use sqlite_vec::sqlite3_vec_init;

// SAFETY: sqlite3_vec_init is a valid SQLite extension entry point compiled in
// via the sqlite-vec crate's cc build; the transmute adapts its C signature to
// rusqlite's RawAutoExtension type (documented pattern, sqlite-vec #206).
unsafe {
    let raw: unsafe extern "C" fn(
        *mut rusqlite::ffi::sqlite3,
        *mut *mut std::os::raw::c_char,
        *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::os::raw::c_int = std::mem::transmute(sqlite3_vec_init as usize);
    register_auto_extension(raw)?;
}
```

Note: this requires an `unsafe` block with a `// SAFETY:` comment — consistent with CLAUDE.md (the no-`unwrap` rule is separate; `unsafe` FFI registration is unavoidable here and is the documented upstream pattern).

### Pattern 2: `Embedder` trait — the degrade seam (SEARCH-03 lives here)

**What:** A trait in core with `OllamaClient` (production) and `FakeEmbedder` (tests). All Ollama-down behavior is a typed error at this seam; `SearchService` catches it and routes to FTS5.
**When to use:** Store path (best-effort embed), search path (embed query), sweep backfill.

```rust
// Source: design derived from .planning/research/ARCHITECTURE.md seam + Ollama API docs
#[async_trait::async_trait]  // or hand-rolled async-fn-in-trait (Rust 1.75+, preferred: no new dep)
pub trait Embedder: Send + Sync {
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, EmbedError>;
    async fn health(&self) -> Result<EmbedderHealth, EmbedError>; // GET /api/tags: running? model pulled?
}
```

Rules the planner must encode:
- `memory_store` NEVER fails because Ollama is down — row inserted with `embedding_status = 0`, a one-line warning to stderr (loud, not silent — Pitfall 4).
- `memory_search` NEVER errors or returns empty because Ollama is down — falls back to the existing FTS5 path, result payload carries `search_mode: "semantic" | "keyword"` so agents/users see degraded state.
- Startup (serve/serve-rest): probe `GET /api/tags`; log one clear line: OK / not running / model missing (`ollama pull nomic-embed-text`). Never block startup on it.
- Dimension guard: on first successful embed, assert `len == 768` against the `meta` table (`embedding_model`, `embedding_dim`); mismatch → typed error + degrade, never a mixed-dim insert (Pitfall: model drift).

### Pattern 3: vec0 sidecar table + KNN, cosine metric declared at creation

**What:** Migration 0002 creates the vec0 table keyed by memory id; KNN in SQL, blend in Rust.

```sql
-- Source: alexgarcia.xyz/sqlite-vec/features/knn.html (via Context7)
-- sql/0002_embeddings.sql
ALTER TABLE memories ADD COLUMN embedding_status INTEGER NOT NULL DEFAULT 0; -- 0=none/pending, 1=present

CREATE VIRTUAL TABLE vec_memories USING vec0(
  memory_id INTEGER PRIMARY KEY,          -- == memories.id
  embedding FLOAT[768] distance_metric=cosine
);

CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
INSERT INTO meta(key, value) VALUES ('embedding_model', 'nomic-embed-text'),
                                    ('embedding_dim', '768');
```

```sql
-- KNN query (k= works on any SQLite version; cosine because declared at creation)
WITH knn AS (
  SELECT memory_id, distance
  FROM vec_memories
  WHERE embedding MATCH :query_vec AND k = :k     -- :k = min(limit * 4, 200) oversample
)
SELECT m.*, knn.distance
FROM knn JOIN memories m ON m.id = knn.memory_id;
-- final rank in Rust: score = w_rel * (1.0 - distance) + w_decay * decay(last_accessed, now)
-- reuse RankWeights + the existing decay math; then bump_access (Phase 1, unchanged)
```

Write path (store with embedding): one transaction — `INSERT memories` → `INSERT vec_memories(memory_id, embedding)` with `bytemuck::cast_slice(&vec_f32)` as the BLOB. `forget` and TTL sweep must also `DELETE FROM vec_memories WHERE memory_id = ?` (vec0 is a virtual table — **no triggers/foreign keys fire on it**; the delete must be explicit in `forget()` and `sweep_expired()`).

### Pattern 4: REST mirror as a separate process over the same WAL file

**What:** `agent-memory serve-rest --addr 127.0.0.1:7437` runs axum over the same `MemoryService`. MCP stdio processes (editor-spawned, per-session) and the REST daemon coexist on one SQLite file — WAL + `busy_timeout` (both set in Phase 1) make cross-process access safe.
**Endpoints (thin adapters, zero logic):**

| Method | Path | Maps to |
|--------|------|---------|
| POST | `/api/memories` | `MemoryService::store` |
| GET | `/api/memories?type=&tag=&scope=&limit=` | `MemoryService::list` |
| POST | `/api/search` | `SearchService::search` (same semantic→keyword fallback) |
| DELETE | `/api/memories/{id}` | `MemoryService::forget` (404 on not-found) |
| GET | `/health` | store ping + embedder health summary |

```rust
// Source: axum 0.8 docs (via Context7) — shared state + typed error mapping
let app = Router::new()
    .route("/api/memories", post(store_handler).get(list_handler))
    .route("/api/search", post(search_handler))
    .route("/api/memories/{id}", delete(forget_handler))   // axum 0.8 path syntax: {id}, not :id
    .route("/health", get(health_handler))
    .with_state(state.clone());
let listener = tokio::net::TcpListener::bind(addr).await?; // default 127.0.0.1:7437
axum::serve(listener, app).await?;
```

Security rule: default bind `127.0.0.1`; refuse a non-loopback `--addr` unless an explicit `--allow-remote` flag is passed (log a warning). No auth for loopback MVP (single-user local tool — documented, matches FEATURES.md anti-feature analysis).

### Pattern 5: GSD STATE.md import — tolerant scanner + idempotent store

**What:** `agent-memory import --from gsd-state <path>`. Parser lives in core (`import/gsd_state.rs`), CLI wires path + prints a report. Verified against this repo's actual `.planning/STATE.md` structure.

Section → type mapping (from the real STATE.md format):

| STATE.md section | Memory type | Notes |
|------------------|-------------|-------|
| `### Decisions` bullets | DECISION | primary payload |
| `### Blockers/Concerns` bullets | CONSTRAINT | active constraints on work |
| `### Pending Todos` bullets | TODO | skip "None yet." placeholder lines |
| `## Deferred Items` table rows | TODO | tag `deferred`; content = item + status columns |

- Every imported memory: `source = "gsd-state"`, tag `gsd`, scope = project dir name (or `--scope` flag).
- **Idempotency:** before insert, `SELECT 1 FROM memories WHERE source = 'gsd-state' AND mem_type = ?1 AND content = ?2` — skip if present. No schema change, no hash crate needed. Re-run → `imported: 0, skipped: N`. (Edited bullets import as new memories — acceptable per success criterion 4, which requires only no-duplication.)
- Parser is tolerant: unknown sections skipped; track fenced-code state so ``` blocks never masquerade as headings; malformed lines skipped with a count, never a hard error (PITFALLS.md integration gotcha).
- Import is a CLI subcommand, NOT the stdio server — writing the report to stdout is fine here (MCP-05 applies only to `serve`).
- Imported memories go through the normal store path → best-effort batch embed (`/api/embed` accepts an array input — one call for the whole import).

### Pattern 6: Release — hand-rolled zigbuild matrix on orangepi (proven pattern)

**What:** `release.yml` triggered on `v*` tags; `runs-on: [orangepi]`; installs zig from the **official tarball, host-arch-matched** (runners lack pip3 — mcp-hub 03-04 lesson); `cargo zigbuild --release --target <triple>` serially for:
`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`
Then: tarball per triple + `SHA256SUMS` + GitHub Release. **Host-arch-aware smoke test:** run `--version` only when target arch == runner arch (`Exec format error` otherwise — mcp-hub precedent).

**Homebrew (DIST-02):** new repo `UnityInFlow/homebrew-tap`, `Formula/agent-memory.rb` with `on_macos`/`on_linux` × `Hardware::CPU.arm?` blocks selecting the per-arch release tarball URL + sha256; `bin.install "agent-memory"`. Install: `brew install unityinflow/tap/agent-memory`. For v0.0.1, updating the 4 sha256 values manually after the release is acceptable; automation is a later nicety.

### Anti-Patterns to Avoid

- **Copying the sqlite-vec Rust-docs registration snippet verbatim:** it targets pre-0.34 rusqlite and won't compile/work against 0.39 — use the #206 `register_auto_extension` pattern (Pattern 1).
- **Bumping rusqlite to 0.40 "while we're at it":** re-breaks the `links = "sqlite3"` resolution deliberately fixed in Phase 1.
- **Hard dependency on Ollama:** store or search failing when Ollama is absent violates SEARCH-03 and the zero-dependency core value.
- **Silent zero-vector storage:** never insert a fake/zero embedding on embed failure — `embedding_status = 0` + loud stderr line.
- **Business logic in REST handlers:** handlers deserialize, call the service, map errors. Same rule that kept mcp.rs thin.
- **Trusting triggers to clean vec_memories:** vec0 is a virtual table; the Phase-1 FTS trigger approach does NOT extend to it — explicit deletes in `forget`/`sweep_expired`.
- **cargo-dist's generated workflow as-is:** emits banned `ubuntu-latest`/`macos-latest` runners.
- **Testing semantic ranking against live Ollama in CI:** runners may lack Ollama; use `FakeEmbedder` with deterministic vectors; a live test exists but is `#[ignore]`d for local dev machines.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Vector KNN in SQL | Custom distance UDF + ORDER BY scan | sqlite-vec vec0 `MATCH … k=` | C-optimized, exact KNN, metric declared once; hand SQL scan is slower and easy to get wrong |
| f32↔bytes casting | `unsafe` transmute of slices | `bytemuck::cast_slice` | Alignment/endianness handled; zero unsafe in our code |
| HTTP server plumbing | Hand-rolled hyper service | axum 0.8 | Ecosystem pin; extractors + IntoResponse error mapping |
| Cosine similarity math (primary path) | In-Rust cosine over all rows | vec0 with `distance_metric=cosine` | Declared once at table creation; consistent store/query convention (in-Rust cosine is Plan B only, if darwin C fails) |
| Cross-compile toolchains | Per-target Docker/cross images | cargo-zigbuild + zig tarball | Proven twice in this ecosystem from the single orangepi runner |
| MCP protocol | — | rmcp (Phase 1) | Unchanged |

**Key insight:** every "new" hard problem in this phase (KNN, embeddings API, cross-compile) already has a settled solution either upstream (sqlite-vec, Ollama `/api/embed`) or in-ecosystem (zigbuild-on-orangepi release recipe). The genuinely novel code is small: the Embedder seam, the blend/fallback logic, ~5 REST handlers, and a ~50-line parser.

## Common Pitfalls

### Pitfall 1: Darwin cross-compile of bundled C fails at release time
**What goes wrong:** `cargo zigbuild --target aarch64-apple-darwin` fails compiling sqlite3.c / sqlite-vec.c (`TargetConditionals.h` not found; zig cc passes `-nostdinc`) or linking (zigbuild #316 iconv regression with rust ≥1.82).
**Why it happens:** zig's darwin header set doesn't always cover what bundled C expects; discovered late because CI only builds Linux.
**How to avoid:** **Mandatory Wave-1 spike**: a `workflow_dispatch` job on orangepi that zigbuilds all 6 triples with sqlite-vec added, before any release work. Keeping reqwest TLS-free removes ring/openssl C from the equation.
**Warning signs:** `fatal error: 'TargetConditionals.h' file not found`; `not mach-o … for architecture arm64`.
**Plan B (pre-approved):** store embeddings in a plain BLOB column on `memories` and do in-Rust brute-force cosine — drops the sqlite-vec C entirely; schema stays forward-compatible. sqlite3.c itself must still cross-compile (DIST-01 needs darwin regardless — injection-scanner proved zigbuild→darwin works for Rust-only crates; the open question is specifically the bundled C).

### Pitfall 2: Fallback that "works" by returning empty
**What goes wrong:** Ollama down → embed error swallowed → KNN over nothing → `[]` returned. Success criterion 2 explicitly forbids this.
**Why it happens:** error mapped to empty result instead of routing to the FTS5 path.
**How to avoid:** fallback decision is made at the `Embedder` seam BEFORE any vec query; a dedicated test kills the fake embedder and asserts keyword results are returned with `search_mode: "keyword"`.
**Warning signs:** search returns empty when FTS5 would match; no `search_mode` in the payload.

### Pitfall 3: vec_memories drifts out of sync with memories
**What goes wrong:** `forget` or TTL sweep deletes from `memories` but orphaned vectors remain in `vec_memories` (or vice versa), so KNN returns ids that join to nothing.
**Why it happens:** vec0 virtual tables don't participate in triggers/FK cascades (the Phase-1 FTS mirror uses triggers — that mental model doesn't transfer).
**How to avoid:** explicit `DELETE FROM vec_memories WHERE memory_id = ?` in `forget()`; in `sweep_expired()` delete matching vector rows in the same writer transaction (or `WHERE memory_id NOT IN (SELECT id FROM memories)` cleanup per sweep). Test: forget a memory → semantic search never surfaces its id.
**Warning signs:** KNN rows with NULL joins; vec_memories row count > memories rows with `embedding_status=1`.

### Pitfall 4: Stdout purity regression via the new code paths
**What goes wrong:** the Ollama client, REST startup logs, or import report prints to stdout while `serve` (stdio MCP) is running → JSON-RPC corruption (MCP-05).
**Why it happens:** new modules added by people/agents unaware of the invariant.
**How to avoid:** all logging stays on the Phase-1 `tracing`→stderr subscriber; the existing `stdio_purity.rs` test keeps running with the embedder active (extend it to run with `AGENT_MEMORY_OLLAMA_URL` pointing at a dead port so the degrade warning fires during the test).
**Warning signs:** `Unexpected token` parse errors in MCP clients after Phase 2 lands.

### Pitfall 5: Blocking the runtime with embed calls or vec queries
**What goes wrong:** embedding HTTP (tens-hundreds ms) awaited fine, but the KNN rusqlite query run directly in an async handler blocks a worker.
**How to avoid:** same Phase-1 rule — every rusqlite call via `spawn_blocking`; reqwest is async natively. The embed call happens OUTSIDE `spawn_blocking` (async), the SQL inside it.
**Warning signs:** latency spikes during concurrent search + import.

### Pitfall 6: Model/dimension drift silently corrupting the corpus
**What goes wrong:** user swaps Ollama model or nomic updates; new 512-dim vectors error against `float[768]`, or worse, comparable-looking vectors from a different model version pollute ranking.
**How to avoid:** `meta` table stores `embedding_model`/`embedding_dim`; startup + first-embed assertion; mismatch → loud degrade to keyword with an actionable message. Re-embed/`reindex` is v2 — document it.
**Warning signs:** vec0 insert dimension errors; recall quality drops after `ollama pull`.

### Pitfall 7: Release smoke test executes a foreign-arch binary
**What goes wrong:** running the x86_64 binary's `--version` on the ARM64 orangepi → `Exec format error`, red release job.
**How to avoid:** host-arch-aware guard (documented mcp-hub 03-04 decision) — only exec when target arch == `uname -m` equivalent.

### Pitfall 8: nomic-embed-text context truncation surprises
**What goes wrong:** long memory content silently truncated at embed time (the locally pulled model reports `context_length: 2048`, not the advertised 8192) — embedding represents only a prefix.
**How to avoid:** acceptable for MVP (memories are short); note it; optionally log a warning when content length is large. `truncate` defaults to true in `/api/embed`.
**Warning signs:** poor recall specifically on very long memories. [VERIFIED: local /api/tags shows context_length 2048 for the pulled nomic-embed-text]

## Code Examples

### Ollama embed call (batch-capable, the modern endpoint)
```rust
// Source: Ollama API docs via Context7 (docs/api.md, capabilities/embeddings.mdx)
// POST http://localhost:11434/api/embed
#[derive(serde::Serialize)]
struct EmbedRequest<'a> { model: &'a str, input: &'a [String] }
#[derive(serde::Deserialize)]
struct EmbedResponse { embeddings: Vec<Vec<f32>> }

let resp: EmbedResponse = client
    .post(format!("{base}/api/embed"))          // base default: http://localhost:11434
    .json(&EmbedRequest { model: "nomic-embed-text", input })
    .timeout(std::time::Duration::from_secs(10))
    .send().await?
    .error_for_status()?                        // 404-ish error body when model not pulled
    .json().await?;
// Health/model presence: GET {base}/api/tags → { models: [{ name: "nomic-embed-text:latest", … }] }
// NEVER use legacy /api/embeddings (singular `prompt`) — deprecated shape.
```

### Inserting a vector (transaction with the memories insert)
```rust
// Source: alexgarcia.xyz/sqlite-vec (Context7) + bytemuck docs
let blob: &[u8] = bytemuck::cast_slice(&embedding); // Vec<f32> -> little-endian bytes, zero copy
tx.execute(
    "INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)",
    rusqlite::params![memory_id, blob],
)?;
tx.execute("UPDATE memories SET embedding_status = 1 WHERE id = ?1", [memory_id])?;
```

### KNN query binding
```rust
// Query vector binds the same way; k is a bound parameter.
let query_blob: &[u8] = bytemuck::cast_slice(&query_vec);
let mut stmt = conn.prepare(
    "WITH knn AS (SELECT memory_id, distance FROM vec_memories
                  WHERE embedding MATCH ?1 AND k = ?2)
     SELECT m.id, m.mem_type, m.content, m.tags, m.source, m.scope,
            m.base_weight, m.access_count, m.created_at, m.last_accessed,
            m.expires_at, knn.distance
     FROM knn JOIN memories m ON m.id = knn.memory_id",
)?;
```

### Homebrew tap formula skeleton (per-arch prebuilt binaries)
```ruby
# Source: docs.brew.sh/How-to-Create-and-Maintain-a-Tap + community tap patterns (web, MEDIUM)
# Repo: UnityInFlow/homebrew-tap, file: Formula/agent-memory.rb
class AgentMemory < Formula
  desc "Persistent typed agent memory: MCP stdio server over embedded SQLite"
  homepage "https://github.com/UnityInFlow/agent-memory"
  version "0.0.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/UnityInFlow/agent-memory/releases/download/v#{version}/agent-memory-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_ARM64_DARWIN"
    else
      url "https://github.com/UnityInFlow/agent-memory/releases/download/v#{version}/agent-memory-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_X86_64_DARWIN"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/UnityInFlow/agent-memory/releases/download/v#{version}/agent-memory-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_ARM64_LINUX"
    else
      url "https://github.com/UnityInFlow/agent-memory/releases/download/v#{version}/agent-memory-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_X86_64_LINUX"
    end
  end

  def install
    bin.install "agent-memory"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/agent-memory --version")
  end
end
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| sqlite-vss (Faiss-based) | sqlite-vec (pure C, static-link) | 2024 (vss deprecated) | Single-binary distribution possible |
| Raw `sqlite3_auto_extension` + transmute of `*const ()` | `rusqlite::auto_extension::register_auto_extension` + RawAutoExtension signature | rusqlite 0.34 (2024) | The sqlite-vec Rust docs page is stale; issue #206 has the working pattern |
| Ollama `/api/embeddings` (singular `prompt`) | `/api/embed` with `input` (string or array, batch) | Ollama 0.2.x era | Batch import embeds in one call |
| zerocopy 0.7 `AsBytes` (shown in sqlite-vec docs) | zerocopy 0.8 `IntoBytes` — or sidestep with bytemuck | 2024 | Docs-vs-crate drift; bytemuck avoids it [ASSUMED] |
| axum 0.7 `:id` path params | axum 0.8 `{id}` path syntax | axum 0.8 (Jan 2025) | Use `{id}` in routes |
| cargo-dist as default release tool | Alive (0.32.0) but org-incompatible defaults; ecosystem hand-rolled zigbuild recipe proven twice | in-ecosystem, 2026 | Hand-rolled release.yml + hand-written formula |

**Deprecated/outdated:**
- `sqlite-vss`: abandoned upstream — never consider it.
- Ollama `/api/embeddings`: legacy singular endpoint — `/api/embed` only.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | zerocopy 0.8 renamed `AsBytes` → `IntoBytes` (motivates the bytemuck choice) | Standard Stack / State of the Art | Low — bytemuck works regardless; only the rationale weakens |
| A2 | Ollama error body on missing model is JSON `{"error": "..."}` with non-200 status | Code Examples / Embedder | Low — `error_for_status()` catches it either way; message extraction may need adjusting at impl time |
| A3 | Cross-process SQLite WAL access (stdio server + REST daemon on one DB file) is safe given the Phase-1 `busy_timeout`/WAL PRAGMAs | Pattern 4 | Medium — if contention shows, fall back to advisory "run one server" doc note; SQLite's WAL multi-process support is core documented behavior, so risk is low in practice |
| A4 | zig's bundled darwin headers cover sqlite3.c/sqlite-vec.c compilation on current zig (the Wave-1 spike resolves this either way) | Pitfall 1 / Release | High if wrong AND Plan B ignored — that's why the spike is mandatory Wave 1 and Plan B (BLOB + in-Rust cosine) is pre-approved |
| A5 | REST default port 7437 is free/uncontested (arbitrary choice, flag-overridable) | Pattern 4 | Trivial — `--addr` flag exists |
| A6 | `k = limit*4 (cap 200)` oversample before decay re-rank is a sensible default | Pattern 3 | Low — tunable constant; golden-set test will surface a bad choice |

## Open Questions (RESOLVED)

1. **Should the hourly sweep also backfill pending embeddings (`embedding_status = 0`)?**
   - What we know: the sweep task exists (Phase 1) and 01-03-SUMMARY explicitly flagged it as "a place to also refresh embeddings"; `/api/embed` batches, so a backfill is one call per tick.
   - What's unclear: whether it's in-scope for MVP (no requirement demands it; SEARCH-02/03 criteria pass without it).
   - Recommendation: include it — it is small (~30 lines), and it completes the "install Ollama later and old memories become semantically searchable" story. Planner's call to cut if the plan runs hot.
   - **RESOLVED:** recommendation adopted — sweep backfill of pending embeddings is included in plan 02-01 Task 3.
2. **musl targets in DIST-01?**
   - What we know: DIST-01 names "Linux (x86_64/aarch64)" without libc flavor; injection-scanner/mcp-hub shipped gnu+musl pairs; the spec-ci-plugin consumer path cared about musl.
   - Recommendation: build all 6 triples (darwin×2, gnu×2, musl×2) — zigbuild makes musl nearly free and bundled SQLite statically links cleanly under musl. If a musl leg fails, ship gnu-only for Linux (requirement still satisfied).
   - **RESOLVED:** recommendation adopted — plan 02-04 builds all 6 triples with the 4 gnu/darwin legs required and musl legs best-effort (`continue-on-error`).
3. **Where does the search-mode indicator surface in the MCP tool result?**
   - What we know: result payloads are JSON built in mcp.rs; adding a top-level `search_mode` field is non-breaking for agents.
   - Recommendation: top-level field on the search tool result + same field in the REST response; one shared serde struct (FEATURES.md "stable JSON record shape").
   - **RESOLVED:** recommendation adopted — shared `SearchOutcome {search_mode, results}` envelope defined in plan 02-01 and reused by the REST response in plan 02-02.
4. **Live-Ollama integration test policy.**
   - What we know: dev machine has Ollama 0.31.1 + nomic-embed-text running; CI runners' Ollama status unknown.
   - Recommendation: deterministic `FakeEmbedder` tests are the CI gate; one `#[ignore]`d live test (`cargo test -- --ignored`) for local/manual verification against real Ollama.
   - **RESOLVED:** recommendation adopted — plan 02-01 makes `FakeEmbedder` tests the CI gate and adds one `#[ignore]`d live-Ollama test.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Ollama daemon | SEARCH-02 dev/manual verification | ✓ (running, reachable) | 0.31.1 | FakeEmbedder for tests; graceful keyword fallback is the product behavior anyway |
| `nomic-embed-text` model | SEARCH-02 | ✓ (pulled; reports context_length 2048) | 137M F16 | — |
| Rust toolchain | all | ✓ | rustc/cargo 1.94.1 | — |
| cargo-zigbuild | DIST-01 | ✓ (local) | 0.23.0 | install on runner in workflow |
| zig | DIST-01 cross-compile | ✗ local | — | installed in release workflow from **official tarball, host-arch-matched** (runners lack pip3 — mcp-hub 03-04 precedent); `brew install zig` locally if spiking on this Mac |
| cargo-llvm-cov | coverage gate | ✓ (now installed locally — was CI-only in Phase 1) | — | CI installs `--locked` |
| Homebrew | DIST-02 local verification | ✓ | /opt/homebrew | — |
| Hetzner X64 runners (`arc-runner-unityinflow`) | CI/release | ⚠ intermittently offline (recurring ecosystem blocker) | — | `orangepi` ARM64 leg — release workflow must target orangepi-only serial builds from the start |
| `UnityInFlow/homebrew-tap` repo | DIST-02 | ✗ (does not exist yet) | — | create during release plan (new public repo + Formula/agent-memory.rb) |

**Missing dependencies with no fallback:** none blocking — everything has a documented path.
**Missing dependencies with fallback:** zig (workflow-installed), X64 runners (orangepi serial), tap repo (created in-phase).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) + tokio::test; integration tests spawn the real binary via `CARGO_BIN_EXE_agent-memory` (Phase-1 pattern) |
| Config file | none needed — workspace `Cargo.toml`; `test-clock` feature auto-enabled via self dev-dependency |
| Quick run command | `cargo test -p agent-memory-core` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SEARCH-02 | Semantic ranking finds non-keyword matches; blend with decay; vec_memories sync on forget/sweep | integration (FakeEmbedder, deterministic vectors + golden set) | `cargo test -p agent-memory-core --test semantic` | ❌ Wave 0 |
| SEARCH-02 | Live Ollama end-to-end (manual/local only) | integration `#[ignore]` | `cargo test -p agent-memory-core --test semantic -- --ignored` | ❌ Wave 0 |
| SEARCH-03 | Embedder failure → keyword results (not error/empty), `search_mode: keyword` surfaced | integration (failing FakeEmbedder) | `cargo test -p agent-memory-core --test fallback` | ❌ Wave 0 |
| API-01 | REST store/list/search/forget against the same store; error status mapping; loopback bind | integration (axum `tower::ServiceExt::oneshot` or spawned listener) | `cargo test -p agent-memory --test rest` | ❌ Wave 0 |
| INTEROP-01 | Fixture STATE.md imports typed memories; re-run imports 0; malformed sections skipped | integration (fixture file) | `cargo test -p agent-memory-core --test import` | ❌ Wave 0 |
| MCP-05 regression | stdout stays pure JSON-RPC with embedder active (dead Ollama URL) | integration (existing, extend) | `cargo test -p agent-memory --test stdio_purity` | ✅ (extend) |
| DIST-01 | 6 triples build via zigbuild; host-arch smoke `--version` | CI workflow_dispatch spike, then release run | manual-only (needs orangepi runner) — justification: cross-compile requires the runner's zig toolchain | ❌ Wave 1 spike workflow |
| DIST-02 | `brew install unityinflow/tap/agent-memory` launches the server | checkpoint:human-verify | manual-only — justification: requires published GitHub Release + tap repo | — |

### Sampling Rate
- **Per task commit:** `cargo test -p agent-memory-core && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** full suite green + `cargo llvm-cov --workspace --fail-under-lines 80` (now runnable locally too) before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/agent-memory-core/src/embed/mod.rs` `FakeEmbedder` (deterministic, per-input seeded vectors) — shared fixture for SEARCH-02/03 tests
- [ ] `crates/agent-memory-core/tests/semantic.rs` — covers SEARCH-02
- [ ] `crates/agent-memory-core/tests/fallback.rs` — covers SEARCH-03
- [ ] `crates/agent-memory/tests/rest.rs` — covers API-01
- [ ] `crates/agent-memory-core/tests/import.rs` + `tests/fixtures/STATE.md` fixture — covers INTEROP-01
- [ ] `.github/workflows/spike-cross-compile.yml` (workflow_dispatch, orangepi) — de-risks DIST-01 before release work
- Framework install: none — cargo test is built-in

## Security Domain

### Applicable ASVS Categories (L1)

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no (loopback-only single-user local tool) | REST binds 127.0.0.1 by default; non-loopback bind requires explicit `--allow-remote` + logged warning |
| V3 Session Management | no | stateless REST, stdio MCP |
| V4 Access Control | yes (network exposure) | default loopback bind is the control; document "never expose publicly" |
| V5 Input Validation | yes | serde-typed request bodies; `MemoryType::try_from` validates before SQL (Phase-1 pattern); all SQL parameterized incl. vec MATCH blob + k; search `limit` capped (DEFAULT_SEARCH_LIMIT=50 exists); axum default body-size limit (~2MB) retained |
| V6 Cryptography | no crypto in-app | release integrity via SHA256SUMS on artifacts + per-arch sha256 in the Homebrew formula |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via query/tags/id | Tampering | Bound parameters everywhere (Phase-1 discipline extends to KNN + REST paths) |
| REST exposed on 0.0.0.0 → memory corpus readable/writable on LAN | Information Disclosure / Tampering | Loopback default + explicit opt-in flag; no auth needed at loopback for MVP |
| SSRF-style redirect of the embedder URL | Tampering | Ollama base URL from config/env only (`AGENT_MEMORY_OLLAMA_URL`, default localhost:11434); never from request payloads |
| Stored memory as prompt-injection carrier | Elevation via downstream agent | Content treated as data; never executed; composes with ecosystem injection-scanner (documented, no in-phase code) |
| DoS via huge store/search bodies | Denial of Service | axum body limit; content length sanity cap; k/limit caps on search |
| Supply chain (new crates) | Tampering | All new crates passed legitimacy audit (table above); versions pinned in workspace Cargo.toml |
| Release artifact tampering | Tampering | SHA256SUMS published with release; Homebrew formula pins sha256 per artifact |
| DB file world-readable | Information Disclosure | 0700 dir creation already in Phase 1 `config.rs` |

## Sources

### Primary (HIGH confidence)
- crates.io sparse index (`index.crates.io`) — sqlite-vec 0.1.9 stable + dependency list (cc build-dep only); direct verification 2026-07-02
- `cargo search` live — axum 0.8.9, reqwest 0.13.4, tower-http 0.7.0, bytemuck 1.25.0, cargo-zigbuild 0.23.0, cargo-dist 0.32.0
- In-repo Phase 1 artifacts — `Cargo.toml` pins, `sql/0001_init.sql`, `store/mod.rs` Store trait, `main.rs` CLI/sweep wiring, `ci.yml`, 01-01/01-02/01-03 SUMMARYs
- Local environment probes — Ollama 0.31.1 running with nomic-embed-text pulled (context_length 2048); toolchain versions
- Ecosystem CLAUDE.md Decisions Log — zigbuild-on-orangepi release precedent (injection-scanner v0.0.2 darwin, mcp-hub v0.1.1), host-arch smoke rule, zig-from-tarball rule

### Secondary (MEDIUM confidence)
- sqlite-vec docs via Context7 (`/websites/alexgarcia_xyz_sqlite-vec`) — vec0 KNN syntax, distance_metric=cosine, rowid join pattern (registration snippet on that page is STALE for rusqlite ≥0.34)
- github.com/asg017/sqlite-vec/issues/206 (WebFetch) — working `register_auto_extension` pattern for rusqlite 0.34+
- docs.rs/rusqlite/0.39.0 auto_extension module — available by default (WebFetch)
- Ollama docs via Context7 (`/ollama/ollama`) — `/api/embed` request/response, batch input, `/api/tags`
- axum docs via Context7 — 0.8 state/handler/error patterns
- docs.brew.sh How-to-Create-and-Maintain-a-Tap + tap guides (WebSearch, cross-checked) — per-arch formula pattern

### Tertiary (LOW confidence — flagged for the Wave-1 spike)
- WebSearch: rusqlite #871/#1615 + cargo-zigbuild #316 — darwin bundled-C cross-compile failure modes (`TargetConditionals.h`, `-nostdinc`, iconv/rust≥1.82)
- WebSearch: cargo-dist maintenance status (alive; astral fork unofficial)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — every version verified live against the registry; the one compatibility landmine (rusqlite 0.39 vs sqlite-vec) checked at the dependency-graph level
- Architecture: HIGH — additive extension of an implemented, tested codebase; patterns come from official docs + Phase-1 established seams
- Semantic search / fallback design: HIGH — vec0 + `/api/embed` shapes from official docs; blend reuses existing decay machinery
- Release (darwin cross-compile): LOW→MEDIUM — the recipe is proven in-ecosystem, but bundled-C-to-darwin is unproven for THIS crate graph; mandatory Wave-1 spike + pre-approved Plan B bounds the risk
- Pitfalls: HIGH — grounded in upstream issues + this ecosystem's own decision log

**Research date:** 2026-07-02
**Valid until:** 2026-08-01 (stable domain; re-verify sqlite-vec/axum versions and Hetzner runner status at plan time if later)
