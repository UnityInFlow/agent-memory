---
phase: 02-semantic-search-interop-release
plan: 01
subsystem: search
tags: [rust, sqlite-vec, ollama, reqwest, bytemuck, embeddings, knn, fts5, mcp]

# Dependency graph
requires:
  - phase: 01 (foundation)
    provides: SqliteStore writer/read-pool, FTS5 bm25×decay search, MemoryService seams (Clock injection), rmcp stdio server, hourly sweep
provides:
  - Embedder trait (dyn-compatible boxed futures) + OllamaClient (POST /api/embed batch, GET /api/tags health) + FakeEmbedder test double
  - migration 0002: vec_memories vec0 sidecar (FLOAT[768] cosine), memories.embedding_status, meta model/dim pin
  - register_vec_extension (process-global, OnceLock, before Connection::open)
  - Store: insert(embedding Option), knn_search (bound blob + k, oversample≤200), insert_embedding, pending_embeddings; explicit vec deletes in forget/sweep_expired
  - SearchMode + SearchOutcome {search_mode, results} — the shared MCP/REST envelope
  - semantic search with similarity×decay blend and graceful FTS5 keyword fallback at the service seam
  - --ollama-url flag / AGENT_MEMORY_OLLAMA_URL env (default http://localhost:11434)
  - startup health probe (one stderr line) + sweep embedding backfill (128/tick)
  - .github/workflows/spike-cross-compile.yml (Pitfall 1 release gate, workflow_dispatch on orangepi)
affects: [02-02 REST API (reuses SearchOutcome), 02-03 GSD import (batch embed via store path), 02-04 release (spike workflow gate)]

# Tech tracking
tech-stack:
  added: [sqlite-vec 0.1.9, reqwest 0.12 (no default features, json), bytemuck 1, tracing (core)]
  patterns: ["Arc<dyn Embedder> injection mirroring Arc<dyn Clock>", "degrade decision at the service seam, never in transports", "KNN candidates in SQL, similarity×decay blend in Rust", "explicit vec0 deletes (virtual tables ignore triggers)"]

key-files:
  created:
    - crates/agent-memory-core/src/embed/mod.rs
    - crates/agent-memory-core/src/embed/ollama.rs
    - crates/agent-memory-core/sql/0002_embeddings.sql
    - crates/agent-memory-core/tests/semantic.rs
    - crates/agent-memory-core/tests/fallback.rs
    - .github/workflows/spike-cross-compile.yml
  modified:
    - Cargo.toml
    - crates/agent-memory-core/Cargo.toml
    - crates/agent-memory-core/src/lib.rs
    - crates/agent-memory-core/src/store/migrations.rs
    - crates/agent-memory-core/src/store/mod.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory/src/mcp.rs
    - crates/agent-memory/src/main.rs
    - crates/agent-memory/tests/tools.rs
    - crates/agent-memory/tests/stdio_purity.rs

key-decisions:
  - "insert_embedding uses DELETE+INSERT in one writer TX instead of INSERT OR REPLACE — conflict-resolution clauses are not reliably supported on virtual tables"
  - "decay.rs test helper injects FakeEmbedder::failing() (not the succeeding default) so the suite deterministically exercises the FTS5 keyword path it asserts"
  - "vec extension transmute goes through `as *const ()` (not `as usize`) — rustc function_casts_as_integer warning under -D warnings"
  - "Local darwin canary PASSED: cargo zigbuild --target aarch64-apple-darwin builds and runs with sqlite-vec's bundled C (zig 0.16.0 brew, cargo-zigbuild 0.23.0)"

patterns-established:
  - "Embedder seam: all Ollama-outage behavior is a typed EmbedError caught in MemoryService — transports never see the degrade decision"
  - "SearchOutcome {search_mode, results} is the ONE wire envelope for every search consumer (MCP now, REST in 02-02)"
  - "vec_memories sync is explicit Rust deletes under the writer lock — never triggers (vec0 ignores them)"

requirements-completed: [SEARCH-02, SEARCH-03]

# Metrics
duration: 26min
completed: 2026-07-02
---

# Phase 02 Plan 01: Semantic Search with Graceful Keyword Fallback Summary

**Ollama (nomic-embed-text) semantic search over a sqlite-vec vec0 sidecar with similarity×decay blending, degrading loudly-but-gracefully to the Phase-1 FTS5 keyword path whenever the embedder is unreachable — every result carries `search_mode`**

## Performance

- **Duration:** 26 min
- **Started:** 2026-07-02T15:49:53Z
- **Completed:** 2026-07-02T16:15:25Z
- **Tasks:** 3
- **Files modified:** 20

## Accomplishments

- SEARCH-02 proven twice: deterministic FakeEmbedder golden set (CI gate) AND the `#[ignore]`d live-Ollama test — both rank the RLS memory first for "database access control rules" with zero keyword overlap (live run passed on this machine, Ollama 0.31.x + nomic-embed-text)
- SEARCH-03 kill-test green: with `FakeEmbedder::failing()`, store succeeds (row lands `embedding_status = 0`, no fake vector) and search returns non-empty FTS5 results with `search_mode: "keyword"` — never an error, never empty-because-of-outage
- vec_memories can never drift from memories: explicit deletes in `forget` + `sweep_expired` (same writer TX), verified down to the raw vec row count after forget
- MCP-05 regression holds: stdio purity test now runs with the embedder ACTIVE against a dead Ollama URL while the degrade warning fires
- Sweep backfill: pending embeddings (status 0) embed in one batch call per tick (≤128), so installing Ollama later makes old memories semantically searchable
- Release de-risk: spike-cross-compile.yml exists (dispatch deferred to 02-04 — no git remote yet), and the local darwin canary already builds+runs the aarch64-apple-darwin binary including sqlite3.c + sqlite-vec.c under zig

## Task Commits

Each task was committed atomically:

1. **Task 1: Embedding contract + OllamaClient + migration 0002 + failing suites** - `ffab061` (test — TDD RED, confirmed compile failure against the target API)
2. **Task 2: Semantic store/search slice — KNN + blend + fallback + search_mode** - `be67aa2` (feat — TDD GREEN, semantic + fallback suites pass)
3. **Task 3: Degrade visibility, sweep backfill, stdout-purity regression, spike workflow** - `c6efc10` (feat)

**TDD gate compliance:** RED (`ffab061`, test) precedes GREEN (`be67aa2`, feat); no refactor commit needed.

## Files Created/Modified

- `crates/agent-memory-core/src/embed/mod.rs` - Embedder trait (dyn-compatible BoxFuture methods), EmbedError, EmbedderHealth, FakeEmbedder (test-clock-gated), EMBEDDING_DIM
- `crates/agent-memory-core/src/embed/ollama.rs` - OllamaClient: /api/embed batch POST + /api/tags health, 10s timeout, count/dimension guards
- `crates/agent-memory-core/sql/0002_embeddings.sql` - embedding_status column, vec_memories vec0 (FLOAT[768] cosine), meta model/dim pin; explicitly NO triggers
- `crates/agent-memory-core/src/store/sqlite.rs` - register_vec_extension (OnceLock + SAFETY-commented transmute before Connection::open), vec0 smoke check, transactional insert with vector, knn_search CTE, insert_embedding, pending_embeddings, explicit vec deletes
- `crates/agent-memory-core/src/service.rs` - SearchMode/SearchOutcome, Arc<dyn Embedder> (4-arg new), best-effort embed on store, semantic→keyword fallback seam, sweep backfill
- `crates/agent-memory/src/mcp.rs` - memory_search serializes the whole envelope; description updated
- `crates/agent-memory/src/main.rs` - global --ollama-url flag (env AGENT_MEMORY_OLLAMA_URL), OllamaClient wiring, one-line startup health probe
- `crates/agent-memory-core/tests/{semantic,fallback}.rs` - SEARCH-02 golden set + live ignored test; SEARCH-03 kill-tests
- `crates/agent-memory/tests/{tools,stdio_purity}.rs` - dead-port Ollama env; keyword-envelope assertions; purity with degrade warning firing
- `.github/workflows/spike-cross-compile.yml` - workflow_dispatch, [orangepi], zig 0.14.1 + cargo-zigbuild 0.23.0 pins, 6 loud triples, host-arch-aware smoke

## Decisions Made

- `insert_embedding` = DELETE + INSERT inside one writer transaction (not the plan's literal `INSERT OR REPLACE`): conflict-resolution clauses are not reliably supported on SQLite virtual tables; semantics are identical and strictly safer
- Blend ordering uses `f64::total_cmp` (descending) — no NaN panics, no clippy unwrap
- `MemoryService::new(store, clock, embedder, decay_cfg)` — embedder injected 3rd, mirroring the clock (the plan's literal signature)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] decay.rs helper uses `FakeEmbedder::failing()` instead of the plan's "default succeeding" embedder**
- **Found during:** Task 2 (test helper updates)
- **Issue:** The plan said all three legacy helpers gain a succeeding hash-vector FakeEmbedder. With a SUCCEEDING embedder, search runs the semantic path, where hash-seeded vectors make KNN return ALL rows — breaking decay.rs's keyword-semantics assertions (`query "two" matches exactly one row`, `no-match returns empty`). Those tests verify SEARCH-01 FTS5 ranking specifically.
- **Fix:** decay.rs injects `FakeEmbedder::failing()` (deterministically routes search to the keyword path, store still succeeds — itself a SEARCH-03 exercise) and asserts `search_mode == Keyword`. store.rs/ttl.rs keep the succeeding default per plan (ttl.rs searches now additionally exercise vec-sync on sweep).
- **Files modified:** crates/agent-memory-core/tests/decay.rs
- **Verification:** full workspace suite green
- **Committed in:** be67aa2 (Task 2 commit)

**2. [Rule 1 - Bug] vec-init transmute cast via `as *const ()` instead of the research snippet's `as usize`**
- **Found during:** Task 1 (register_vec_extension)
- **Issue:** rustc warns `function_casts_as_integer` on `sqlite3_vec_init as usize`, which fails the clippy/-D warnings gate
- **Fix:** cast through `as *const ()` per the compiler's own suggestion; SAFETY comment unchanged
- **Files modified:** crates/agent-memory-core/src/store/sqlite.rs
- **Verification:** `cargo clippy --workspace --all-targets -- -D warnings` clean
- **Committed in:** ffab061 (Task 1 commit)

**3. [Rule 1 - Bug] Stale "offline-by-construction" header comment in tests/store.rs**
- **Found during:** Task 2
- **Issue:** The comment claimed core has NO reqwest/HTTP dependency — false once reqwest landed in Task 1
- **Fix:** Reworded: core now carries reqwest for the embedder, but these tests inject FakeEmbedder so no network call occurs
- **Files modified:** crates/agent-memory-core/tests/store.rs
- **Verification:** comment accuracy (no behavior change)
- **Committed in:** be67aa2 (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (3 × Rule 1)
**Impact on plan:** All corrections necessary for a green `-D warnings` build and truthful tests. No scope creep; interface contracts implemented exactly as specified.

## Issues Encountered

None — the plan's research (registration pattern, /api/embed shapes, KNN binding) was accurate on first implementation; semantic/fallback suites went GREEN on the first run after Task 2.

## Local Darwin Canary (for plan 02-04's spike gate)

**Outcome: PASSED (non-authoritative).** `brew install zig` (0.16.0) + cargo-zigbuild 0.23.0, then `cargo zigbuild --release --locked --target aarch64-apple-darwin -p agent-memory` compiled the full graph — including bundled sqlite3.c AND sqlite-vec.c — and the resulting Mach-O arm64 binary runs `agent-memory --version` successfully. Caveats: run on a darwin arm64 host with zig 0.16.0 (not the pinned 0.14.1) so it does NOT prove the Linux→darwin cross-compile; the authoritative environment remains the orangepi runner via the spike workflow (dispatch in 02-04 after the repo is published). Signal value: the bundled C sources themselves compile cleanly under zig cc for the darwin target.

## User Setup Required

None - no external service configuration required. (Ollama + `ollama pull nomic-embed-text` is optional: the tool fully works in keyword mode without it, and the startup log says exactly what to do.)

## Next Phase Readiness

- `SearchOutcome` envelope is ready for the REST adapter (plan 02-02) — serialize it as-is
- Store path embeds batches already (`/api/embed` array input) — the GSD import (02-03) reuses it directly
- spike-cross-compile.yml is ready to dispatch the moment 02-04 publishes the repo; local canary is a strong positive prior
- No blockers

---
*Phase: 02-semantic-search-interop-release*
*Completed: 2026-07-02*

## Self-Check: PASSED

- All key created files exist on disk
- Task commits ffab061 / be67aa2 / c6efc10 present in git log
- Plan-level verification re-run green: workspace tests (41 passed), clippy -D warnings, fmt --check, semantic/fallback suites, stdio_purity with dead Ollama URL, live --ignored Ollama test
