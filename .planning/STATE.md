---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: Awaiting next milestone
stopped_at: Phase 02 complete (UAT closed, security verified, 25/25 threats closed) — milestone v1.0 ready to archive
last_updated: "2026-07-12T08:12:08.051Z"
last_activity: 2026-07-12 — Milestone v1.0 completed and archived
progress:
  total_phases: 2
  completed_phases: 2
  total_plans: 8
  completed_plans: 8
  percent: 100
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-07-12)

**Core value:** An agent can persist a structured memory and retrieve the right one later — across sessions and tools — over a standard MCP interface, with no cloud.
**Current focus:** Milestone v1.0 complete — archive and plan next milestone

## Current Position

Phase: Milestone v1.0 complete
Plan: —
Status: Awaiting next milestone
Last activity: 2026-07-12 — Milestone v1.0 completed and archived

## Performance Metrics

**Velocity:**

- Total plans completed: 11
- Average duration: ~28 min
- Total execution time: ~1.4 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 1 | 3 | - | - |
| 02 | 5 | - | - |

**Recent Trend:**

- Last 5 plans: —
- Trend: —

*Updated after each plan completion*
| Phase 02 P01 | 26 min | 3 tasks | 20 files |
| Phase 02 P02 | 10 min | 2 tasks | 8 files |
| Phase 02 P03 | 40 min | 2 tasks | 9 files |
| Phase 02 P04 | 1h 46m | 3 tasks | 7 files |
| Phase 02 P05 | 8 min | 2 tasks | 6 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [Roadmap]: 2-phase vertical split — Phase 1 ships a usable local memory tool with no Ollama; Phase 2 layers semantic search + interop + release.
- [Roadmap]: Keyword/FTS5 search (SEARCH-01) ships in Phase 1 before and without semantic search; Ollama is a pure Phase 2 enhancement with graceful fallback.
- [Roadmap]: Windows binaries deferred to v2 (DIST-03) per the mcp-hub `cfg(unix)` cross-compile precedent — v1 DIST-01 targets macOS + Linux only.
- [01-01]: Pinned `rusqlite 0.39` + `rusqlite_migration 2.5` (not research-suggested 0.40/2.6) so the whole graph shares one `libsqlite3-sys` (0.37) with `r2d2_sqlite 0.34` — 0.40/2.6 cause a `links = "sqlite3"` resolver conflict. Same audited crates, compatible pins.
- [01-01]: `test-clock` feature enabled for tests via a self dev-dependency so verify commands need no `--features`; production consumers never get the feature.
- [01-01]: rmcp 1.8 specifics — `ServerInfo` is `#[non_exhaustive]` (build from `Default` + field set); use `#[tool_handler(router = self.tool_router)]` to avoid a dead-code warning under `clippy -D warnings`.
- [01-02]: `memory_search` recomputes decay **inline** in the SQL `ORDER BY` from `last_accessed` (recompute-on-read, Open Question 3) instead of the materialised `decay_score` column — so a recency bump re-ranks immediately, independent of the Plan-03 sweep. The `Store::search` signature takes `DecayConfig`.
- [01-02]: Registered a custom `exp()` SQLite scalar fn + enabled the rusqlite `functions` feature — the bundled SQLite lacks `SQLITE_ENABLE_MATH_FUNCTIONS` (inline decay blend failed with `no such function: exp`).
- [01-02]: Recency bump is fire-and-forget on a detached `spawn_blocking` task (Open Question 2); `memory_forget` returns `Ok(false)` as a clean not-found tool result, never a JSON-RPC error.
- [01-03]: TTL `sweep_expired` is the ONLY delete; `materialize_decay` is UPDATE-only — `DecayEngine` runs delete-then-rescore so decay can never remove a row (STORE-04). Materialized `decay_score` uses the same `exp`/half-life/`CASE` math as on-read search, so the two agree (STORE-03).
- [01-03]: Background sweep is a detached hourly `tokio::time::interval` task spawned before `serve(stdio())`, logging `SweepReport` to stderr only (MCP-05 holds). `cargo-llvm-cov` absent locally → the `>80%` coverage gate is CI-enforced in `ci.yml` (self-hosted `[arc-runner-unityinflow, orangepi]`, never `ubuntu-latest`), not run locally.
- [Phase 02-01]: insert_embedding uses DELETE+INSERT in one writer TX (not INSERT OR REPLACE) — conflict-resolution clauses are not reliably supported on SQLite virtual tables; identical semantics, strictly safer
- [Phase 02-01]: SearchOutcome {search_mode, results} is the shared search envelope for MCP and the 02-02 REST API — one serde struct keeps the wire shape stable across transports (RESEARCH Open Question 3)
- [Phase 02-01]: Local darwin zigbuild canary PASSED (aarch64-apple-darwin builds+runs incl. sqlite-vec bundled C) — non-authoritative (darwin host, zig 0.16.0 vs pinned 0.14.1) but a strong positive prior for the 02-04 orangepi spike gate
- [Phase 02-02]: AppState carries Arc<dyn Embedder> and spawn_health_probe takes the trait object — one wiring shape for serve and serve-rest; REST /health reads embedder status without reaching into service internals
- [Phase 02-02]: REST forget 404 body reuses the exact mcp.rs not-found JSON ({id, deleted:false, reason:not_found}) instead of the generic ApiError shape — cross-transport body consistency
- [Phase 02-04]: chrono trimmed to default-features=false, features=[now] (UTC-only) — the default clock feature pulls iana-time-zone -> core-foundation-sys, un-linkable by zig darwin cross without a macOS SDK; Local time must never be reintroduced
- [Phase 02-04]: v0.0.1 ships without musl binaries: sqlite-vec.c uses BSD u_int*_t typedefs musl lacks; gnu covers Linux (musl was best-effort); fix upstream or CFLAGS shim in v2
- [Phase 02-04]: Coverage-bearing tests must run in-process: a SIGKILL'd spawned binary flushes no LLVM profile data — tests/rest.rs contributes 0%; the direct handler tests carry the 80% gate
- [Phase ?]: [02-05] map_fts_query_error classifies FTS5 parse failures at the store seam (markers: fts5: syntax error / unterminated string) as MemoryError::InvalidQuery -> REST 400 / MCP invalid_params; prepare errors stay internal
- [Phase ?]: [02-05] shared map_mcp_error gives MCP the same client/internal error split as REST map_memory_error; NotFound-as-error is internal_error, clean not-found stays the Ok(false) forget result

### Pending Todos

[From .planning/todos/pending/ — ideas captured during sessions]

None yet.

### Blockers/Concerns

None open for milestone v1.0 — all Phase 1 pitfalls (stdout purity, spawn_blocking writer lane, decay-never-deletes, injectable Clock/WAL) were resolved in the foundation and verified; the Phase 2 release shipped via the orangepi serial-build path (ecosystem OPS-01 standard).

Carried notes for v2 planning:

- musl Linux binaries blocked by sqlite-vec.c BSD typedefs (upstream fix or CFLAGS shim).
- REST boundary-value hardening (limit/ttl_secs extremes, tag LIKE substring over-match) recorded as warnings in 02-REVIEW.md — not release-blocking.

## Deferred Items

Items acknowledged and carried forward:

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| Distribution | DIST-03 Windows binaries (cfg(unix) refactor) | Deferred to v2 | Roadmap (2026-06-24) |
| Search | SEARCH-04 hybrid RRF fusion | Deferred to v2 | Roadmap (2026-06-24) |
| Interface | MCP-06 memory_update / relation tools | Deferred to v2 | Roadmap (2026-06-24) |
| Distribution | DIST-04 portable export | Deferred to v2 | Roadmap (2026-06-24) |

## Session Continuity

Last session: 2026-07-12
Stopped at: Phase 02 complete (UAT closed, security verified, 25/25 threats closed) — milestone v1.0 ready to archive
Resume file: None

## Operator Next Steps

- Start the next milestone with /gsd-new-milestone
