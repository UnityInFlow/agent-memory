---
phase: 01-core-memory-foundation
plan: 01
subsystem: database
tags: [rust, sqlite, rusqlite, fts5, rmcp, mcp, tokio, thiserror, walking-skeleton]

# Dependency graph
requires: []
provides:
  - Cargo workspace (agent-memory-core lib + agent-memory bin)
  - MemoryType domain enum (6 types, UPPERCASE TryFrom, is_pinned), NewMemory/Memory/MemoryView, MemoryError (thiserror)
  - Injectable Clock trait (SystemClock + feature-gated TestClock) — Wave-0 test harness
  - Pure decay_score() + DecayConfig (30-day default) — stub for the Plan 03 sweep
  - SQLite store: schema + 4 indexes + FTS5 mirror + 3 sync triggers, WAL + single-writer + r2d2 read pool, parameterized insert/list
  - MemoryService.store/list over spawn_blocking
  - rmcp stdio MCP server exposing memory_store + memory_list with stdout-purity
  - DB-path resolution (flag > AGENT_MEMORY_DB > dirs::data_dir), 0700 dir create
  - Wave-0 integration test harness (tests/store.rs, tests/stdio_purity.rs, tests/tools.rs)
affects: [01-02 search-forget, 01-03 decay-ttl-sweep]

# Tech tracking
tech-stack:
  added: [rmcp 1.8, rusqlite 0.39 (bundled), rusqlite_migration 2.5, r2d2 0.8 + r2d2_sqlite 0.34, tokio 1, clap 4, thiserror 2, anyhow 1, chrono 0.4, dirs 6, tracing/tracing-subscriber, tempfile 3]
  patterns: [thiserror-in-lib/anyhow-at-edges, injectable Clock, single-writer + r2d2 read pool, spawn_blocking for sync rusqlite, FTS5 external-content mirror, stderr-only logging + panic hook for stdout purity, rmcp #[tool_router]/#[tool_handler]]

key-files:
  created:
    - Cargo.toml
    - crates/agent-memory-core/src/domain.rs
    - crates/agent-memory-core/src/clock.rs
    - crates/agent-memory-core/src/decay.rs
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory-core/src/store/migrations.rs
    - crates/agent-memory-core/sql/0001_init.sql
    - crates/agent-memory-core/tests/store.rs
    - crates/agent-memory/src/main.rs
    - crates/agent-memory/src/config.rs
    - crates/agent-memory/src/mcp.rs
    - crates/agent-memory/tests/stdio_purity.rs
    - crates/agent-memory/tests/tools.rs
  modified: []

key-decisions:
  - "Pinned rusqlite 0.39 + rusqlite_migration 2.5 (not research-suggested 0.40/2.6) to share one libsqlite3-sys 0.37 with r2d2_sqlite 0.34, avoiding a links=sqlite3 conflict"
  - "Enabled the test-clock feature for tests via a self dev-dependency so verify commands need no --features flag"
  - "Used #[tool_handler(router = self.tool_router)] (per rmcp's own test fixtures) to silence a false-positive dead-code warning under clippy -D warnings"
  - "Built ServerInfo from Default + field mutation because InitializeResult is #[non_exhaustive]"

patterns-established:
  - "Single-writer Mutex<Connection> + r2d2 read pool with a PragmaCustomizer applying WAL/synchronous/foreign_keys/busy_timeout per connection"
  - "All store SQL parameterized; NULL-bound parameters disable optional filters (no string-built SQL)"
  - "MemoryType::try_from validates the wire string before any SQL runs — clean error, never a panic (D-07)"
  - "Binary integration tests spawn the built binary via CARGO_BIN_EXE_agent-memory and drive real JSON-RPC over piped stdio"

requirements-completed: [STORE-01, STORE-02, MCP-01, MCP-03, MCP-05]

# Metrics
duration: 35min
completed: 2026-06-25
---

# Phase 1 Plan 01: Walking Skeleton Summary

**An rmcp stdio MCP server exposing memory_store + memory_list over a WAL+single-writer SQLite store (schema, migrations, FTS5 mirror) with stdout-purity, injectable Clock, and durable-across-restart persistence — the thin end-to-end slice.**

## Performance

- **Duration:** ~35 min
- **Started:** 2026-06-25T19:40:00Z (approx)
- **Completed:** 2026-06-25T19:50:00Z (approx)
- **Tasks:** 3
- **Files modified/created:** 17 (16 source/config + Cargo.lock)

## Accomplishments
- Cargo workspace with `agent-memory-core` (library, `thiserror`) and `agent-memory` (binary, `anyhow`) building clean on cargo/rustc 1.94.1.
- Full SQLite schema landed up front: `memories` table + 4 indexes + `memories_fts` FTS5 external-content mirror + 3 sync triggers, so Plan 02's search has no schema work. WAL + single serialized writer + r2d2 read pool with per-connection PRAGMAs.
- `MemoryType` (6 types, UPPERCASE `TryFrom`, `is_pinned`), `NewMemory`/`Memory`/`MemoryView`, and a `thiserror` `MemoryError` with `#[from]` variants.
- Injectable `Clock` (`SystemClock` + feature-gated `TestClock`) and a pure `decay_score()` + `DecayConfig` (30-day default) ready for the Plan 03 sweep.
- rmcp `#[tool_router]` server with `memory_store` + `memory_list`, stderr-only logging + panic hook for stdout purity, and clap CLI (`serve`, `--db`/`AGENT_MEMORY_DB`).
- Wave-0 test harness for Plans 02/03: `tests/store.rs` (durability/filters), `tests/stdio_purity.rs` (MCP-05 gate), `tests/tools.rs` (store→id→list over real stdio).

## Task Commits

Each task was committed atomically:

1. **Task 1: Workspace + domain types + Clock harness** - `e938cf8` (feat)
2. **Task 2: SQLite store + MemoryService + restart-durability test** - `a6b8df6` (feat; test+impl co-developed, TDD RED demonstrated by the unresolved-import failure before the store landed)
3. **Task 3: rmcp stdio server + config + stdio-purity/tools tests** - `89f645c` (feat)

**Plan metadata:** _(this docs commit)_

## Files Created/Modified
- `Cargo.toml` — workspace root, pinned `[workspace.dependencies]`
- `crates/agent-memory-core/src/domain.rs` — MemoryType/NewMemory/MemoryView/Memory/MemoryError
- `crates/agent-memory-core/src/clock.rs` — Clock trait + SystemClock + TestClock
- `crates/agent-memory-core/src/decay.rs` — decay_score() + DecayConfig stub
- `crates/agent-memory-core/src/service.rs` — MemoryService.store/list + ListArgs
- `crates/agent-memory-core/src/store/{mod,sqlite,migrations}.rs` — Store trait, SqliteStore, migrations
- `crates/agent-memory-core/sql/0001_init.sql` — schema + indexes + FTS5 + triggers
- `crates/agent-memory-core/tests/store.rs` — STORE-01/02 + filters/limit/order
- `crates/agent-memory/src/main.rs` — tokio main, clap, stderr logging + panic hook, serve(stdio())
- `crates/agent-memory/src/config.rs` — resolve_db_path + 0700 dir create
- `crates/agent-memory/src/mcp.rs` — MemoryMcp #[tool_router], memory_store + memory_list
- `crates/agent-memory/tests/stdio_purity.rs` — MCP-05 gate
- `crates/agent-memory/tests/tools.rs` — MCP-01/03 over piped stdio

## Decisions Made
- **rusqlite 0.39 + rusqlite_migration 2.5 instead of 0.40/2.6** — the research-suggested versions conflict: `r2d2_sqlite 0.34` pins `rusqlite ^0.39` (libsqlite3-sys 0.37) while `rusqlite_migration 2.6` and `rusqlite 0.40` want libsqlite3-sys 0.38, producing a `links = "sqlite3"` resolver error. Aligning all three on rusqlite 0.39 / libsqlite3-sys 0.37 resolves it with the same pre-audited crates.
- **`test-clock` enabled for tests via a self dev-dependency** (`agent-memory-core = { path = ".", features = ["test-clock"] }`) so `cargo test -p agent-memory-core --test store` works without `--features`, while production consumers never get the feature.
- **`#[tool_handler(router = self.tool_router)]`** — the explicit-router form (used in rmcp's own test fixtures) makes the macro reference the `tool_router` field, silencing a false-positive dead-code warning that would otherwise fail `clippy -D warnings`.
- **`ServerInfo` built from `Default` + field mutation** because `InitializeResult` is `#[non_exhaustive]` and cannot be struct-literal-constructed downstream.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Dependency version conflict on `libsqlite3-sys`**
- **Found during:** Task 1 (first `cargo build -p agent-memory-core`)
- **Issue:** The plan/research pinned `rusqlite 0.40` and `rusqlite_migration 2.6`, but `r2d2_sqlite 0.34` (latest) depends on `rusqlite ^0.39` → `libsqlite3-sys 0.37`. Two different `libsqlite3-sys` majors both `links = "sqlite3"`, which cargo refuses to resolve.
- **Fix:** Pinned `rusqlite = "0.39"` and `rusqlite_migration = "2.5"` (the 0.39-tracking release) so the whole graph shares libsqlite3-sys 0.37. Documented inline in `Cargo.toml`. Same audited crates, compatible version pins.
- **Files modified:** `Cargo.toml`
- **Verification:** `cargo build --workspace` clean; FTS5 still available (smoke-checked at store open + exercised by tests).
- **Committed in:** `e938cf8` (Task 1 commit)

**2. [Rule 3 - Blocking] `test-clock` feature not active for integration tests**
- **Found during:** Task 2 (first `cargo test -p agent-memory-core --test store`)
- **Issue:** `TestClock` is gated behind `#[cfg(any(test, feature = "test-clock"))]`; integration tests in `tests/` are a separate crate, so the lib's `#[cfg(test)]` did not apply and `TestClock` was "configured out". The plan's verify command omits `--features`.
- **Fix:** Added a self dev-dependency enabling `test-clock`, which activates it for test/example/bench builds only.
- **Files modified:** `crates/agent-memory-core/Cargo.toml`
- **Verification:** `cargo test -p agent-memory-core --test store` → 4 passed.
- **Committed in:** `a6b8df6` (Task 2 commit)

**3. [Rule 1 - Bug] Clippy `doc list item without indentation` in `sqlite.rs`**
- **Found during:** Task 2 (`cargo clippy --all-targets -- -D warnings`)
- **Issue:** A module doc line beginning with `+ \`busy_timeout\`` was parsed as a malformed Markdown list item, failing `-D warnings`.
- **Fix:** Reworded the doc comment to avoid a leading `+`.
- **Files modified:** `crates/agent-memory-core/src/store/sqlite.rs`
- **Verification:** `cargo clippy --all-targets -- -D warnings` clean.
- **Committed in:** `a6b8df6` (Task 2 commit)

**4. [Rule 3 - Blocking] `ServerInfo` struct-literal rejected (`#[non_exhaustive]`) + `tool_router` dead-code warning**
- **Found during:** Task 3 (`cargo build --workspace`)
- **Issue:** (a) `InitializeResult`/`ServerInfo` is `#[non_exhaustive]`, so the RESEARCH Pattern-1 struct literal (`ServerInfo { .., ..Default::default() }`) does not compile downstream. (b) The `tool_router` field appeared unused to dead-code analysis (used only by macro-generated dispatch), which would fail `clippy -D warnings`.
- **Fix:** Built `ServerInfo` from `Default` then set fields; switched to `#[tool_handler(router = self.tool_router)]` (rmcp's own test idiom).
- **Files modified:** `crates/agent-memory/src/mcp.rs`
- **Verification:** `cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` clean.
- **Committed in:** `89f645c` (Task 3 commit)

---

**Total deviations:** 4 auto-fixed (3 blocking, 1 bug)
**Impact on plan:** All four were mechanical build/lint/API-shape blockers from version pins and exact rmcp 1.8 surface; none changed scope or behavior. The architecture, schema, decay formula, and tool contract match the plan exactly.

## Issues Encountered
- macOS lacks GNU `timeout`; the manual stdio smoke test relied on stdin-EOF-driven shutdown instead (the integration tests do the same — drop stdin → server sees EOF → clean exit 0).

## Known Stubs
- `crates/agent-memory-core/src/decay.rs` — `decay_score()` and `DecayConfig` are implemented and unit-tested, but the **sweep engine** that materializes `decay_score` and the **search-time surfacing** are intentionally deferred to Plans 02/03 (per plan objective: "decay declared, body is a stub in this plan"). `decay_cfg` is held by `MemoryService` (marked `#[allow(dead_code)]`) awaiting the Plan 03 sweep. `base_weight` is materialized at insert (pinned → 2.0) but not yet read by ranking. These are documented future-plan work, not gaps in this plan's goal (the walking skeleton: store + list).

## User Setup Required
None - no external service configuration required. The server creates its own SQLite DB on first run.

## Next Phase Readiness
- Walking skeleton proven: `initialize` → `tools/call memory_store` → `tools/call memory_list` works over real stdio with durable SQLite; stdout is pure JSON-RPC.
- Plan 02 (memory_search / memory_forget) inherits: the FTS5 mirror + triggers (no schema work), the `Store` trait (add `search`/`forget` methods), the `MemoryService` spawn_blocking pattern, and `tests/tools.rs` (extend with search/forget cases).
- Plan 03 (decay/TTL sweep) inherits: `decay_score()` + `DecayConfig`, `TestClock`, the single-writer lane, and `expires_at`/`decay_score` columns.
- No blockers. Note for later: the self-hosted CI workflow (`.github/workflows/ci.yml`) and the `cargo-llvm-cov --fail-under-lines 80` coverage gate are Wave-0 items not in this plan's `files_modified` — track for a CI plan / phase gate.

## Self-Check: PASSED

All 14 created source/config files and the SUMMARY exist on disk; all 3 task commits (`e938cf8`, `a6b8df6`, `89f645c`) are present in git history. Full workspace verification green: `cargo build --workspace`, `cargo test --workspace` (domain 5, decay 5, migrations 1, store 4, config 2, stdio_purity 1, tools 2), `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo fmt --check` clean. No `unwrap()`/`expect()` in production code (all occurrences are inside `#[cfg(test)]` modules; sqlite.rs uses only infallible `unwrap_or_*`).

---
*Phase: 01-core-memory-foundation*
*Completed: 2026-06-25*
