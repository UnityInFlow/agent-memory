---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: verifying
stopped_at: Completed 02-03-PLAN.md
last_updated: "2026-07-03T07:57:15.551Z"
last_activity: 2026-07-02 -- Phase 02 execution started
progress:
  total_phases: 2
  completed_phases: 2
  total_plans: 7
  completed_plans: 7
  percent: 100
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-06-24)

**Core value:** An agent can persist a structured memory and retrieve the right one later — across sessions and tools — over a standard MCP interface, with no cloud.
**Current focus:** Phase 02 — semantic-search-interop-release

## Current Position

Phase: 02 (semantic-search-interop-release) — EXECUTING
Plan: 4 of 4
Status: Phase complete — ready for verification
Last activity: 2026-07-02 -- Phase 02 execution started

Progress: [██████████] 100% (Phase 1 plans)

## Performance Metrics

**Velocity:**

- Total plans completed: 6
- Average duration: ~28 min
- Total execution time: ~1.4 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 1 | 3 | - | - |

**Recent Trend:**

- Last 5 plans: —
- Trend: —

*Updated after each plan completion*
| Phase 02 P01 | 26 min | 3 tasks | 20 files |
| Phase 02 P02 | 10 min | 2 tasks | 8 files |
| Phase 02 P03 | 40 min | 2 tasks | 9 files |
| Phase 02 P04 | 1h 46m | 3 tasks | 7 files |

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

### Pending Todos

[From .planning/todos/pending/ — ideas captured during sessions]

None yet.

### Blockers/Concerns

Front-loaded Phase 1 pitfalls (must be resolved in the foundation, not retrofit):

- stdout purity: all logging to stderr or MCP stdio transport corrupts (Phase 1 hard gate, MCP-05).
- Blocking async loop: wrap synchronous rusqlite in `spawn_blocking` / single-writer lane (Phase 1 architectural rule).
- Decay must only re-rank, never delete; schema separates `decay_score` / `expires_at` / `last_accessed` (Phase 1 schema, STORE-03/04).
- Injectable `Clock` + UTC timestamps + WAL + write serialization from day one.
- `sqlite-vec` static-link + Windows support-vs-defer decision made in Phase 1 so `dirs`/path deps are gated consistently.

Recurring ecosystem blocker: Hetzner X64 self-hosted fleet intermittently offline — plan Phase 2 release matrix for orangepi-only serial builds with host-arch-aware smoke tests. Validate `sqlite-vec` C cross-compile on orangepi early.

## Deferred Items

Items acknowledged and carried forward:

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| Distribution | DIST-03 Windows binaries (cfg(unix) refactor) | Deferred to v2 | Roadmap (2026-06-24) |
| Search | SEARCH-04 hybrid RRF fusion | Deferred to v2 | Roadmap (2026-06-24) |
| Interface | MCP-06 memory_update / relation tools | Deferred to v2 | Roadmap (2026-06-24) |
| Distribution | DIST-04 portable export | Deferred to v2 | Roadmap (2026-06-24) |

## Session Continuity

Last session: 2026-07-03T07:57:01.205Z
Stopped at: Completed 02-03-PLAN.md
Resume file: None
