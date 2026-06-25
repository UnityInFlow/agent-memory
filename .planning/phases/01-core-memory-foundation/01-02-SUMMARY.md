---
phase: 01-core-memory-foundation
plan: 02
subsystem: search-retrieval
tags: [rust, sqlite, fts5, bm25, decay, rmcp, mcp, search, forget, recency-bump]

# Dependency graph
requires:
  - 01-01 walking skeleton (Store trait, SqliteStore, MemoryService, rmcp stdio server, FTS5 mirror + triggers)
provides:
  - Store::search (FTS5 MATCH + negated-bm25 × inline-recomputed-decay ranking)
  - Store::forget (parameterized DELETE, rows-affected → deleted/not-found)
  - Store::bump_access (writer-lane recency bump)
  - decay.rs RankWeights + apply_decay on-read surfacing helper
  - MemoryService::search (empty-not-error, on-read decay recompute, fire-and-forget recency bump) + MemoryService::forget
  - memory_search + memory_forget rmcp tools — completes the four-tool Phase-1 surface
  - registered `exp()` SQLite scalar fn (bundled SQLite lacks SQLITE_ENABLE_MATH_FUNCTIONS)
affects: [01-03 decay-ttl-sweep]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "recompute-on-read decay inline in the FTS5 ORDER BY (negated bm25 × exp-decay blend) — recency bump immediately re-ranks without waiting for a sweep"
    - "register exp() as a deterministic per-connection scalar function (bundled SQLite has no math functions)"
    - "fire-and-forget recency bump on a detached spawn_blocking task — a bump failure never fails the search"
    - "Ok(false) from forget maps to a successful tool result (not_found), never a JSON-RPC error"

key-files:
  created:
    - crates/agent-memory-core/tests/decay.rs
  modified:
    - crates/agent-memory-core/src/store/mod.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory-core/src/decay.rs
    - crates/agent-memory/src/mcp.rs
    - crates/agent-memory/tests/tools.rs
    - Cargo.toml

key-decisions:
  - "Ranked search recomputes decay INLINE in the SQL ORDER BY from last_accessed at `now` (recompute-on-read, Open Question 3) instead of the materialised decay_score column — so a recency bump re-ranks immediately, before any Plan-03 sweep runs"
  - "Registered a custom `exp()` SQLite scalar function because the bundled SQLite is not compiled with SQLITE_ENABLE_MATH_FUNCTIONS (search failed with 'no such function: exp'); enabled the rusqlite `functions` feature"
  - "Recency bump is fire-and-forget on a detached spawn_blocking task; a bump failure is swallowed so it never fails the user's search (Open Question 2 recommendation)"
  - "memory_forget returns a {id, deleted, reason?} JSON status object; Ok(false) is a successful not-found result, never an Err (MCP-04 / D-06)"

patterns-established:
  - "exp-decay blend computed in SQL via the negated-bm25 sign rule and a CASE picking the pinned (6x) half-life for DECISION/ARCHITECTURE/CONSTRAINT"
  - "bump_access builds a parameterized IN-list (?2,?3,…) with bound ids — no string-built id list"

requirements-completed: [MCP-02, MCP-04, SEARCH-01, STORE-03]

# Metrics
duration: 30min
completed: 2026-06-25
---

# Phase 1 Plan 02: Keyword Search + Forget + Decay Surfacing Summary

**FTS5 keyword `memory_search` ranked by a negated-bm25 × on-read-recomputed-decay blend (decay surfaced on every result, recency-bumped on retrieval) plus delete-by-id `memory_forget` with clean not-found — completing the four-tool offline MCP surface (store → search → forget over stdio).**

## Performance

- **Duration:** ~30 min
- **Completed:** 2026-06-25
- **Tasks:** 2 (both `auto`; Task 1 `tdd`)
- **Files modified/created:** 7 (1 created, 6 modified)

## Accomplishments
- `Store` trait gained `search`, `forget`, `bump_access`; `SqliteStore` implements all three over the writer lane / read pool established in Plan 01.
- `search` runs the RESEARCH Pattern-3 FTS5 query with the **CRITICAL bm25 sign rule** (bm25 negated before blending) and recomputes decay **inline** from `last_accessed` so the recency bump re-ranks immediately — no dependence on the not-yet-built Plan-03 sweep.
- `decay.rs` gained `RankWeights` (configurable relevance/decay blend) and `apply_decay` (on-read surfacing of `decay_score` into each `MemoryView`, STORE-03).
- `MemoryService::search` returns `Ok(vec![])` on no match (never an error), recomputes decay on read, and fires a fire-and-forget recency bump; `MemoryService::forget` returns `bool` (deleted / not-found).
- `memory_search` + `memory_forget` rmcp tools are live — the four-tool Phase-1 surface (`memory_store`, `memory_list`, `memory_search`, `memory_forget`) is complete and callable over stdio entirely offline.
- `tests/decay.rs` (new, STORE-03): deterministic half-life curve, pinned > unpinned at equal age, just-accessed ranks above un-accessed with the surfaced recomputed `decay_score`, no-match → empty.
- `tests/tools.rs` extended (MCP-02/04, SEARCH-01): search returns ranked results each carrying a `decay_score` and empty-on-no-match; forget deletes (search + list both omit it, proving the FTS5 mirror stayed in sync via the delete trigger) and returns a clean not-found for unknown ids.

## Task Commits

1. **Task 1: FTS5 search + bm25×decay ranking + on-read decay surfacing + recency bump** — `510f22a` (feat; TDD — `tests/decay.rs` written and run RED before the search impl, then GREEN)
2. **Task 2: memory_search + memory_forget rmcp tools + stdio integration cases** — `ddf7a4f` (feat)

**Plan metadata:** _(this docs commit)_

## Files Created/Modified
- `crates/agent-memory-core/src/store/mod.rs` — `Store` trait: add `search`/`forget`/`bump_access`
- `crates/agent-memory-core/src/store/sqlite.rs` — `search` (negated-bm25 × inline exp-decay blend, CASE pinned half-life), `forget` (parameterized DELETE → rows-affected), `bump_access` (parameterized IN-list UPDATE on writer lane), registered `exp()` scalar fn
- `crates/agent-memory-core/src/service.rs` — `SearchArgs`; `search` (empty-not-error, on-read `apply_decay`, fire-and-forget bump); `forget`; `decay_cfg` now consumed
- `crates/agent-memory-core/src/decay.rs` — `RankWeights` + `apply_decay` on-read helper
- `crates/agent-memory/src/mcp.rs` — `SearchArgs`/`ForgetArgs` tool structs; `memory_search` + `memory_forget` thin adapters
- `crates/agent-memory/tests/tools.rs` — search + forget stdio cases; `call_tool`/`tool_views` helpers
- `crates/agent-memory-core/tests/decay.rs` — new STORE-03 decay-curve + ranking test
- `Cargo.toml` — enable rusqlite `functions` feature

## Decisions Made
- **Recompute decay inline in SQL `ORDER BY`** from `last_accessed` at `now` (recompute-on-read, RESEARCH Open Question 3) rather than ranking by the materialised `decay_score` column. Reason: the materialised column is only refreshed by the Plan-03 sweep, so ranking by it would ignore the just-applied recency bump and the just-accessed memory would not float up. Inline recompute makes the bump take effect immediately. The `search` trait method therefore takes `DecayConfig` (added to its signature).
- **Registered a custom `exp()` scalar function.** The bundled SQLite is not compiled with `SQLITE_ENABLE_MATH_FUNCTIONS`; the inline decay blend uses `exp()`, which initially failed with `no such function: exp`. Registered a deterministic, side-effect-free `exp(x)` on every connection (writer + pool) and enabled the rusqlite `functions` feature. Same pre-audited crate, additive feature.
- **Fire-and-forget recency bump** on a detached `spawn_blocking` task whose result is discarded — a bump failure must never fail the user's search (Open Question 2). The decay-test sleeps 50ms to let the detached bump land before asserting ranking.
- **`memory_forget` returns a JSON status object** (`{id, deleted, reason?}`); `Ok(false)` is a successful tool result with `deleted=false`, never a JSON-RPC error (MCP-04 / D-06).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `exp()` not available in bundled SQLite**
- **Found during:** Task 1 (first run of `tests/decay.rs` against the inline-decay search query)
- **Issue:** The inline decay-blend `ORDER BY` calls `exp()`, but the bundled SQLite was built without `SQLITE_ENABLE_MATH_FUNCTIONS` → runtime `SqlInputError: no such function: exp`.
- **Fix:** Registered a deterministic `exp(x)` scalar function on every connection via `create_scalar_function` (new `register_functions` + `prepare_connection`), and enabled the rusqlite `functions` feature in `Cargo.toml`. No new crate; same pre-audited rusqlite.
- **Files modified:** `crates/agent-memory-core/src/store/sqlite.rs`, `Cargo.toml`
- **Verification:** `tests/decay.rs` green; full workspace clippy/fmt/test clean.
- **Committed in:** `510f22a` (Task 1 commit)

**2. [Rule 3 - Blocking] `search` needed the half-life config to recompute decay in SQL**
- **Found during:** Task 1 (the just-accessed memory did not rank above the un-accessed one because the materialised `decay_score` was still 1.0 for both pre-sweep)
- **Issue:** Ranking by the materialised column ignores the recency bump; the test's ranking assertion failed.
- **Fix:** Switched to recompute-on-read — `Store::search` now takes a `DecayConfig` and the `ORDER BY` computes decay inline from `last_accessed`/`now` with a `CASE` selecting the pinned (6×) half-life. Threaded `cfg` through `MemoryService::search`.
- **Files modified:** `crates/agent-memory-core/src/store/{mod,sqlite}.rs`, `crates/agent-memory-core/src/service.rs`
- **Verification:** `tests/decay.rs` ranking assertion green.
- **Committed in:** `510f22a` (Task 1 commit)

**Total deviations:** 2 auto-fixed (both blocking). Both are mechanical environment/wiring fixes; neither changed scope or behaviour — the bm25×decay contract, the four-tool surface, and the result shapes match the plan exactly.

## Threat Model Compliance
- **T-02-01 (SQL injection):** the FTS5 MATCH string, the forget id, and the bump id-list are all bound parameters — never concatenated into SQL.
- **T-02-02 (malformed FTS5 query DoS):** an invalid MATCH string surfaces as a typed `MemoryError::Sqlite`, mapped by the tool to `McpError::invalid_params` — no panic. A valid no-match query returns `Ok(vec![])`.
- **T-02-04 (unbounded result set):** `search` applies `DEFAULT_SEARCH_LIMIT = 50` when `limit` is omitted.
- No new security surface beyond the plan's threat register.

## Known Stubs
None. `memory_search`/`memory_forget` are fully wired end-to-end; the recency bump is a real writer-lane UPDATE; decay is surfaced from real timestamps. (The background decay/TTL **sweep** that materialises `decay_score` remains intentionally deferred to Plan 03 — but Plan-02 search does not depend on it, recomputing decay on read instead.)

## User Setup Required
None — no external service. Keyword search runs entirely offline (no Ollama, no network).

## Next Phase Readiness
- Four-tool MCP surface complete: `store` → `search` → `list` → `forget` all proven over real stdio.
- Plan 03 (decay/TTL sweep) inherits: `decay_score()` + `apply_decay` + `RankWeights`, `bump_access` on the writer lane, the `expires_at` column, and the `TestClock` harness. The sweep will materialise `decay_score` (cheaper `ORDER BY`); Plan-02 search already recomputes on read so correctness holds between sweeps.
- No blockers. Carry-forward (from Plan 01): CI workflow + `cargo-llvm-cov --fail-under-lines 80` coverage gate are Wave-0/phase-gate items, not yet landed.

## Self-Check: PASSED

`tests/decay.rs` exists on disk; both task commits (`510f22a`, `ddf7a4f`) are in git history. Full workspace verification green: `cargo build --workspace`, `cargo test --workspace` (lib 13, decay 4, store 4, bin-unit 2, stdio_purity 1, tools 4 = 28 tests), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` all clean. No `unwrap()`/`expect()` in production code (test-only occurrences excepted).

---
*Phase: 01-core-memory-foundation*
*Completed: 2026-06-25*
