---
phase: 02-semantic-search-interop-release
plan: 05
subsystem: search-error-taxonomy
tags: [rust, sqlite, fts5, error-mapping, mcp, rest, gap-closure]

# Dependency graph
requires:
  - phase: 02 plan 01
    provides: keyword/semantic search split, SearchOutcome envelope, FakeEmbedder::failing() fallback harness
  - phase: 02 plan 02
    provides: REST handlers + ApiError two-tier mapping, tests/rest.rs spawned-binary harness
provides:
  - "SqliteStore::search keyword path honors args.tag via bound ?10 predicate — degraded mode applies the same filters the semantic path does (closes verification gap 1 / CR-01)"
  - "MemoryError::InvalidQuery(String) — typed client-input error for unparseable FTS5 MATCH strings, detected at the store seam by map_fts_query_error"
  - "map_mcp_error(MemoryError) -> McpError — shared two-tier client/internal mapping used by all four MCP tool service calls (closes WR-04)"
  - "REST map_memory_error: InvalidQuery -> 400 BadRequest — malformed search input can never surface as a 500 (closes WR-05)"
affects: [02-VERIFICATION.md gap re-check, phase 02 completion]

# Tech tracking
tech-stack:
  added: []
  patterns: ["FTS5 query-parse errors classified at the store seam by message marker (fts5: syntax error / unterminated string) — the regression test defines correctness, the markers serve it", "error-tier parity: REST 400/404/500 and MCP invalid_params/internal_error derive from the same MemoryError variants on both transports"]

key-files:
  created: []
  modified:
    - crates/agent-memory-core/src/domain.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory-core/tests/fallback.rs
    - crates/agent-memory/src/rest/handlers.rs
    - crates/agent-memory/src/mcp.rs
    - crates/agent-memory/tests/rest.rs

key-decisions:
  - "map_fts_query_error matches BOTH 'fts5: syntax error' AND 'unterminated string' — the lone double-quote reproducer reports the latter (observed at RED time, exactly the contingency the plan scripted)"
  - "Only post-bind execution errors (statement step / row iteration) route through the helper; conn.prepare failures stay MemoryError::Sqlite (a missing fts5 module is internal, not client input)"
  - "NotFound-as-error maps to internal_error on MCP — the clean not-found remains the Ok(false) forget result, so agents never argument-repair a DB fault (T-02G-03)"
  - "RED states verified at execution time but committed together with GREEN per ecosystem CLAUDE.md 'Never commit failing tests' (CLAUDE.md precedence over per-step TDD commits)"

patterns-established:
  - "Degraded-mode fidelity: every filter the primary path honors must be bound in the fallback path's SQL too — locked by dead-embedder regression tests"

requirements-completed: [SEARCH-03, API-01]

# Metrics
duration: 8min
completed: 2026-07-03
---

# Phase 02 Plan 05: Gap Closure — Degraded-Mode Filter Fidelity + Bad-Input Error Taxonomy Summary

**Keyword-fallback search now binds the tag filter (bound ?10 predicate mirroring knn_search) and malformed FTS5 queries surface as MemoryError::InvalidQuery → REST 400 / MCP invalid_params via a shared two-tier error mapping — closing both 02-VERIFICATION.md gaps (CR-01, WR-05, WR-04)**

## Performance

- **Duration:** ~8 min
- **Started:** 2026-07-03T11:45:29Z
- **Completed:** 2026-07-03T11:53:00Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- **Gap 1 (SEARCH-03 / CR-01) closed:** `SqliteStore::search` WHERE clause gains `AND (?10 IS NULL OR m.tags LIKE '%' || ?10 || '%')` with `args.tag` bound as the 10th `params![]` element — same shape as `knn_search`'s `?5` and `list`'s `?3`, parameterized, never formatted (T-02-01 preserved). Regression test `keyword_fallback_honors_tag_filter` proves: dead embedder + tag filter → `SearchMode::Keyword`, exactly 1 result, the alpha-tagged row only. RED confirmed first (both rows returned).
- **Gap 2 (API-01 / WR-05 + WR-04) closed:** `MemoryError::InvalidQuery(String)` variant added; `map_fts_query_error` at the store seam classifies post-bind FTS5 parse failures as client input; REST maps it to 400 with an `{"error": ...}` body (in-process handler test AND real-HTTP spawned-binary assertion both green); MCP gets the exhaustive shared `map_mcp_error` — `InvalidType | InvalidQuery` → invalid_params, `Sqlite | Pool | Join | Migration | NotFound` → internal_error — wired into all four tool service calls (grep count == 4).
- Stale sqlite.rs comment corrected: it now states the FTS5-parse mapping lives HERE at the store seam, making rest/mod.rs line 30's "bad input can never surface as a 500" contract factually true.
- Full gate green: fallback suite 4/4, agent-memory 20/20 (4 suites), workspace 66 passed / 0 failed, `clippy --workspace --all-targets -D warnings` clean, `cargo fmt --check` clean.
- Scope discipline held: WR-01/02/03/06/07/08/09 and IN-xx untouched; the new tag predicate keeps the existing LIKE-substring shape (exact-match semantics stays WR-07's concern).

## Task Commits

Each task was committed atomically:

1. **Task 1: Keyword fallback honors the tag filter (gap 1 / CR-01)** - `39be828` (feat)
2. **Task 2: Malformed FTS5 query → 400/invalid_params, shared two-tier MCP mapping (gap 2 / WR-05 + WR-04)** - `e491443` (feat)

**TDD compliance:** Both tasks ran RED→GREEN at execution time (Task 1: both rows returned before the fix; Task 2: `Err(Sqlite(... "unterminated string"))` before the variant/mapping). RED and GREEN were committed together per the ecosystem CLAUDE.md rule "Never commit failing tests" — see Deviations.

## Files Created/Modified

- `crates/agent-memory-core/src/domain.rs` - `InvalidQuery(String)` variant after `InvalidType`, thiserror message starting `invalid search query`
- `crates/agent-memory-core/src/store/sqlite.rs` - `map_fts_query_error` helper; `fn search` gains the `?10` tag predicate + `args.tag` bind; post-bind `rows?`/`row?` routed through the helper; stale lines-417-419 comment corrected
- `crates/agent-memory-core/tests/fallback.rs` - `keyword_fallback_honors_tag_filter` + `malformed_fts5_query_maps_to_invalid_query_in_keyword_mode` (both dead-embedder, no Ollama/network)
- `crates/agent-memory/src/rest/handlers.rs` - `InvalidQuery` → `BadRequest` arm in `map_memory_error`; in-process test `search_with_malformed_query_returns_400_never_500` (coverage-bearing per 02-04 decision)
- `crates/agent-memory/src/mcp.rs` - doc-commented `map_mcp_error`; four service-call sites now `.map_err(map_mcp_error)`; up-front `MemoryType::try_from` validations untouched; unit test `map_mcp_error_splits_client_and_internal_tiers`
- `crates/agent-memory/tests/rest.rs` - malformed-query 400 block appended inside `rest_end_to_end_store_list_search_forget_health` (real HTTP, reuses the dead-Ollama spawned server)

## Decisions Made

- Extended the FTS5-error marker check to `"unterminated string"`: the lone `"` reproducer reports `SqliteFailure(..., Some("unterminated string"))`, not `fts5: syntax error`. The plan explicitly scripted this contingency ("extend the helper's marker check to the observed FTS5 query-parse message — the test defines correctness").
- `conn.prepare` errors deliberately NOT routed through the helper — a missing fts5 module is an internal fault, per the plan's seam rule.
- mcp.rs `#[cfg(test)] mod tests` placed at end of file (after `impl ServerHandler`) — clippy `items_after_test_module` denies test mods before items.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] mcp.rs tests mod relocated below `impl ServerHandler`**
- **Found during:** Task 2 (clippy gate)
- **Issue:** Placing the new `#[cfg(test)] mod tests` between the tool impl and the `ServerHandler` impl trips clippy's `items_after_test_module` under `-D warnings`.
- **Fix:** Moved the tests mod to the end of the file; no behavioral change.
- **Files modified:** crates/agent-memory/src/mcp.rs
- **Verification:** `cargo clippy --workspace --all-targets -- -D warnings` clean
- **Committed in:** e491443 (Task 2 commit)

### CLAUDE.md-driven adjustments

**2. [CLAUDE.md precedence] TDD RED states not committed separately**
- The GSD tdd="true" flow calls for a `test(...)` RED commit before the `feat(...)` GREEN commit, but the ecosystem CLAUDE.md git convention says "Never commit failing tests." Per CLAUDE.md precedence, each task's RED was verified at execution time (failure output captured for the right reason) and committed together with its GREEN fix as one passing commit.

---

**Total deviations:** 1 auto-fixed (Rule 3) + 1 CLAUDE.md commit-granularity adjustment
**Impact on plan:** None on scope or interfaces — all interface contracts implemented exactly as specified.

## Issues Encountered

None beyond the two documented deviations. The plan's anticipated contingency (FTS5 message marker differing for the lone-quote case) occurred exactly as scripted and was resolved by the plan's own instruction.

## Known Stubs

None — both fixes wire real behavior end-to-end; no placeholder data paths introduced.

## Threat Flags

None — no new surface beyond the plan's threat model. T-02G-01 (raw internal error disclosure → controlled 400 message), T-02G-02 (bound parameterized tag predicate, no injection surface), and T-02G-03 (honest MCP error tiers) all mitigated as planned. Zero new dependencies (T-02G-SC accepted as planned).

## Authentication Gates

None encountered.

## User Setup Required

None.

## Next Phase Readiness

- Both 02-VERIFICATION.md partial truths are now observably true and regression-locked at core, handler, MCP-unit, and real-HTTP layers — phase 02 re-verification can proceed.
- All verification commands run with no Ollama and no network.
- No blockers.

---
*Phase: 02-semantic-search-interop-release*
*Completed: 2026-07-03*

## Self-Check: PASSED

- All 6 modified files exist on disk
- Task commits 39be828 (Task 1) and e491443 (Task 2) present in git log
- Verification re-run green: fallback 4/4, agent-memory 20/20, workspace 66 passed / 0 failed, clippy -D warnings clean, fmt --check clean
- Acceptance greps: "IS NULL OR m.tags LIKE" count == 2 in sqlite.rs; "map_err(map_mcp_error)" count == 4 in mcp.rs; InvalidQuery defined once, referenced in all three consumer files
