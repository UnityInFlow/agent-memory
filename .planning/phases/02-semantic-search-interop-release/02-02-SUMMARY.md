---
phase: 02-semantic-search-interop-release
plan: 02
subsystem: api
tags: [rust, axum, rest, http, loopback-guard, mcp-parity]

# Dependency graph
requires:
  - phase: 02 plan 01
    provides: SearchOutcome {search_mode, results} shared envelope, Arc<dyn Embedder> seam, OllamaClient, MemoryService wiring
  - phase: 01 (foundation)
    provides: MemoryService, SqliteStore (WAL + busy_timeout — cross-process safe), AppState, spawn_sweep_task
provides:
  - "`agent-memory serve-rest --addr 127.0.0.1:7437 [--allow-remote]` — axum 0.8 HTTP mirror of store/list/search/forget + /health over the SAME MemoryService and SQLite file as the MCP stdio server"
  - rest::build_router (axum 0.8 `{id}` path syntax), rest::ApiError (400/404/500 IntoResponse), rest::ensure_bind_allowed loopback guard (T-02-10)
  - rest::handlers — 5 thin handlers, serde-only DTOs mirroring the MCP arg structs field-for-field
  - AppState.embedder (Arc<dyn Embedder>) — embedder status surfaced on GET /health
  - tests/rest.rs — API-01 end-to-end proof against the spawned binary over real HTTP
affects: [02-03 GSD import (same store path), 02-04 release/README (loopback security note, curl examples)]

# Tech tracking
tech-stack:
  added: [axum 0.8 (workspace pin), reqwest as binary dev-dependency (TLS-free test client)]
  patterns: ["REST handlers copy the mcp.rs thin-adapter shape: validate wire type up front, one service call, two-tier error map", "SearchOutcome serialized as-is over HTTP — one wire envelope for every transport", "loopback-by-default bind; non-loopback requires explicit --allow-remote + logged warning"]

key-files:
  created:
    - crates/agent-memory/src/rest/mod.rs
    - crates/agent-memory/src/rest/handlers.rs
    - crates/agent-memory/tests/rest.rs
  modified:
    - Cargo.toml
    - crates/agent-memory/Cargo.toml
    - crates/agent-memory/src/main.rs
    - crates/agent-memory/src/mcp.rs

key-decisions:
  - "spawn_health_probe signature widened from Arc<OllamaClient> to Arc<dyn Embedder> so serve() and serve_rest() share one wiring shape and AppState carries the trait object directly"
  - "AppState.embedder field added in mcp.rs (where the struct is defined) rather than main.rs — the plan anticipated the plumbing; the struct's home file was the only correct place"
  - "serve-rest runs its own spawn_sweep_task: the REST daemon is long-running and must expire TTLs / backfill embeddings exactly like the stdio server"
  - "forget 404 body reuses the exact mcp.rs not-found JSON ({id, deleted:false, reason:not_found}) instead of the generic ApiError shape — cross-transport consistency beats uniform error bodies"

patterns-established:
  - "REST is a mirror, never a fork: zero business logic in handlers, DTO field names identical to MCP tool args, same service structs, same envelope"
  - "Bind-address security is a typed precondition (ensure_bind_allowed) checked before TcpListener::bind, unit-tested including the flag name in the error"

requirements-completed: [API-01]

# Metrics
duration: 10min
completed: 2026-07-02
---

# Phase 02 Plan 02: REST Mirror for Non-MCP Clients Summary

**axum 0.8 `serve-rest` daemon mirroring the four memory operations + /health as thin adapters over the same MemoryService and WAL SQLite file the MCP stdio server uses, loopback-bound by default with an explicit `--allow-remote` escape hatch**

## Performance

- **Duration:** 10 min
- **Started:** 2026-07-02T16:19:17Z
- **Completed:** 2026-07-02T16:28:05Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments

- API-01 proven end-to-end against the spawned real binary over real HTTP: store 201 with numeric id, invalid type 400 with `{"error"}` body (never 500), list with type filter, search carrying the shared `{search_mode:"keyword", results}` envelope (dead-Ollama env forces keyword mode deterministically), delete 200 then 404 on the same id with the exact mcp.rs not-found body, health 200 `{"status":"ok","embedder":"unreachable"}`
- Zero divergence risk by construction: handlers contain no SQL (grep-verified), build the exact `NewMemory`/`ServiceListArgs`/`ServiceSearchArgs` structs mcp.rs builds, and serialize `SearchOutcome` as-is — REST and MCP speak one dialect
- T-02-10 mitigated: `ensure_bind_allowed` refuses a non-loopback `--addr` without `--allow-remote` (unit-tested including the flag name in the error message, plus a spawned-binary integration test asserting non-zero exit and the flag named on stderr); `--allow-remote` with a non-loopback bind logs a warning naming the exposure
- `--addr 127.0.0.1:0` support: the `REST listening on {local_addr}` info line is emitted after bind, so tests (and scripts) can parse the resolved ephemeral port
- Full gate green: rest suite GREEN on first run after implementation, workspace 46 tests + 1 ignored, `clippy --all-targets -D warnings` clean, `fmt --check` clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Failing end-to-end REST test (spawned binary, real HTTP)** - `60b6eaa` (test — TDD RED, confirmed: serve-rest was an unknown subcommand, stdio_purity/tools stayed green)
2. **Task 2: axum adapter + ServeRest wiring + loopback guard** - `8412550` (feat — TDD GREEN)

**TDD gate compliance:** RED (`60b6eaa`, test) precedes GREEN (`8412550`, feat); no refactor commit needed.

## Files Created/Modified

- `crates/agent-memory/src/rest/mod.rs` - build_router (axum 0.8 `{id}` syntax), ApiError IntoResponse (400/404/500, `{"error"}` bodies), ensure_bind_allowed + 3 unit tests
- `crates/agent-memory/src/rest/handlers.rs` - StoreRequest/SearchRequest/ListQuery serde DTOs mirroring MCP args; store/list/search/forget/health handlers, two-tier MemoryError→ApiError map (InvalidType→400, NotFound→404, rest→500)
- `crates/agent-memory/tests/rest.rs` - RestServer spawn harness (port parsed from stderr, Drop-guard child reaping, stderr drain thread), full CRUD+error-status sequence test, separate bind-guard process test
- `crates/agent-memory/src/main.rs` - ServeRest subcommand (`--addr` default 127.0.0.1:7437, `--allow-remote`), serve_rest() copying serve()'s wiring incl. sweep task, post-bind listening log line
- `crates/agent-memory/src/mcp.rs` - AppState gains `pub embedder: Arc<dyn Embedder>` for REST /health
- `Cargo.toml` - axum 0.8 workspace pin with ecosystem-rationale comment
- `crates/agent-memory/Cargo.toml` - axum dependency; reqwest (TLS-free) dev-dependency

## Decisions Made

- `spawn_health_probe` now takes `Arc<dyn Embedder>` (was `Arc<OllamaClient>`) — health is a trait method, and both serve paths plus AppState want the trait object; one signature serves all three
- The forget 404 body is the mcp.rs not-found JSON verbatim rather than the generic `{"error"}` shape — the plan explicitly required cross-transport body consistency for this case
- serve-rest includes the startup embedder health probe (one stderr line) for wiring parity with serve — the daemon operator gets the same "keyword mode until you pull the model" visibility

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] AppState embedder field added in mcp.rs (not in the task's file list)**
- **Found during:** Task 2 (health_handler plumbing)
- **Issue:** The plan said "add the field in main.rs where AppState is constructed", but the `AppState` struct is *defined* in mcp.rs — the field cannot be added anywhere else. mcp.rs was not in the plan's files_modified list.
- **Fix:** Added `pub embedder: Arc<dyn Embedder>` to the struct in mcp.rs; both construction sites in main.rs populate it. Widened `spawn_health_probe` to `Arc<dyn Embedder>` so `serve()` compiles with the trait-object binding.
- **Files modified:** crates/agent-memory/src/mcp.rs, crates/agent-memory/src/main.rs
- **Verification:** full workspace suite + clippy -D warnings green; stdio_purity/tools regression suites unaffected
- **Committed in:** 8412550 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 × Rule 3)
**Impact on plan:** None on scope or interfaces — the plan itself anticipated the plumbing ("if plumbing the embedder into AppState is needed, add the field ... where AppState is constructed"); only the file location differed because that is where the struct lives.

## Issues Encountered

None — the rest suite went GREEN on the first run after Task 2, and the plan's interface contracts (route table, status mapping, listening-line protocol) were implemented exactly as specified.

## Known Stubs

None — every handler is wired to the real `MemoryService` against the real SQLite store; no placeholder data paths exist.

## Threat Flags

None — the new HTTP surface is exactly the one enumerated in the plan's threat model (T-02-10..13 dispositions applied: loopback guard implemented, serde-typed DTOs validate before the service, axum's default body limit untouched, loopback no-auth accepted by design).

## Authentication Gates

None encountered.

## User Setup Required

None — `agent-memory serve-rest` works out of the box on 127.0.0.1:7437; Ollama remains optional (health reports `"embedder":"unreachable"` and search serves keyword mode).

## Next Phase Readiness

- Plan 02-03 (GSD import) can proceed: the store path the import batches through is unchanged, and the REST daemon coexisting with import runs is safe (WAL, separate processes)
- Plan 02-04 (release/README) should document: the loopback-by-default security posture, `--allow-remote` semantics, and curl examples matching tests/rest.rs
- No blockers

---
*Phase: 02-semantic-search-interop-release*
*Completed: 2026-07-02*

## Self-Check: PASSED

- crates/agent-memory/src/rest/mod.rs (111 lines ≥ 60), rest/handlers.rs (215 ≥ 80), tests/rest.rs (264 ≥ 80) — all exist on disk
- Task commits 60b6eaa (test/RED) and 8412550 (feat/GREEN) present in git log
- Plan-level verification re-run green: `cargo test -p agent-memory --test rest` (2 passed), `cargo test --workspace` (46 passed, 1 ignored), `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo fmt --check` clean
- Acceptance greps: `{id}` route present, zero colon-form path params in rest/, zero SQL in rest/, default addr exactly 127.0.0.1:7437, no unwrap/expect outside #[cfg(test)]
