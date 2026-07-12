---
phase: 03-api-hardening-toolchain-spikes
plan: 01
subsystem: api
tags: [rust, sqlite, rusqlite, json_each, thiserror, axum, rmcp, validation]

# Dependency graph
requires:
  - phase: 02 (v1.0 milestone)
    provides: two-tier error taxonomy (InvalidQuery -> 400/invalid_params vs internal tier) at both mappers; four-layer test template (core integration, in-process handler, MCP mapper unit, spawned-binary realism); MemoryService seam shared by MCP + REST
provides:
  - MemoryError::InvalidArgument(String) client-tier variant, mapped 400 (REST) / invalid_params (MCP)
  - pub bounds consts in domain.rs -- MIN_LIMIT=1, MAX_LIMIT=200, MIN_TTL_SECS=1, MAX_TTL_SECS=3_155_760_000
  - validate_limit/validate_ttl shared helpers; called at the top of store()/search()/list()/import()
  - exact case-sensitive tag matching via json_each equality at all three query paths (knn, keyword, list)
  - README bounds/exact-tag contract docs + "Behavioral changes in v0.1.0" section (feeds DIST-06 release notes)
affects: [phase-4 relations/update tools, phase-5 jsonl import/export, phase-6 release notes DIST-06]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - one-seam validation -- all transports inherit bounds checks from MemoryService method tops (D-06)
    - import() validates per-draft through the same helper (bypass closed, structural for future importers)
    - json_each equality EXISTS predicate with unchanged parameter indices/bind lists (injection posture preserved)

key-files:
  created:
    - crates/agent-memory-core/tests/validation.rs
  modified:
    - crates/agent-memory-core/src/domain.rs
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory/src/mcp.rs
    - crates/agent-memory/src/rest/handlers.rs
    - crates/agent-memory-core/tests/semantic.rs
    - crates/agent-memory-core/tests/fallback.rs
    - crates/agent-memory-core/tests/store.rs
    - crates/agent-memory/tests/rest.rs
    - README.md

key-decisions:
  - "Bounds consts live in domain.rs beside MemoryError; MAX_KNN_K re-defined as crate::domain::MAX_LIMIT (single source of truth, D-03)"
  - "deny_unknown_fields NOT added (discretion resolved in plan): keeps the v0.1.0 release-notes delta to exactly the two decided behavioral changes"
  - "service.rs calls validators fully qualified (crate::domain::validate_*) so the grep-count acceptance gates count only the 4 call sites"

patterns-established:
  - "Validation seam: new v1.1 surfaces (update/link/export/import) must call validate_* at the service method top, never in transports"
  - "Behavioral changes documented in README 'Behavioral changes in v0.1.0' as they land, not at release time"

requirements-completed: [API-02, API-03]

# Metrics
duration: 12min
completed: 2026-07-12
---

# Phase 3 Plan 01: Validation Seam + Exact Tag Matching Summary

**Out-of-bounds limit/ttl_secs now reject as descriptive 400/invalid_params at the shared MemoryService seam on all seven surfaces, and tag filters match whole tags exactly (case-sensitive) via json_each on all three query paths**

## Performance

- **Duration:** 12 min
- **Started:** 2026-07-12T13:50:48Z
- **Completed:** 2026-07-12T14:02:24Z
- **Tasks:** 3 (2 TDD)
- **Files modified:** 11

## Accomplishments

- API-02: `MemoryError::InvalidArgument` + pub bounds consts + two shared helpers; `store()`/`search()`/`list()`/`import()` all validate before any embed or store hop; the silent `.max(0)` clamp is gone; both exhaustive mappers route the new variant to the client tier. `ttl_secs=i64::MAX` overflow (Pitfall 1) and negative-limit-as-unlimited (`LIMIT -5`) are both unreachable now.
- API-03: all three `tags LIKE '%'||?||'%'` predicates replaced with `json_each` equality EXISTS — `tag=rust` no longer matches `rustling`, `Rust` no longer matches `rust`; parameter indices and `params![]` bind lists byte-identical (T-02-01 preserved); four agent-facing doc strings updated to the exact-tag contract.
- Test coverage across all four layers: boundary matrix at the core seam (valid boundaries included, per Pitfall 2), three in-process handler 400 tests, extended MCP mapper tier test, per-site exact-tag regression tests, real-HTTP realism assertions on the spawned binary. Workspace: 76 tests green, clippy `-D warnings` clean, fmt clean, coverage 86.56% lines (gate: 80).
- README documents the bounds, the D-05 message shape, exact-tag semantics, and a "Behavioral changes in v0.1.0" section for the Phase 6 release notes.

## Task Commits

Each task was committed atomically (TDD tasks: test -> feat):

1. **Task 1: Reject out-of-bounds limit/ttl_secs at the MemoryService seam** — `04015de` (test, RED) + `ee43e06` (feat, GREEN)
2. **Task 2: Exact tag matching via json_each at all three predicate sites** — `6c2f1c1` (test, RED) + `18e2261` (feat, GREEN)
3. **Task 3: README contract docs + real-HTTP realism proof + phase coverage gate** — `1c9c346` (docs)

## Files Created/Modified

- `crates/agent-memory-core/src/domain.rs` — InvalidArgument variant, 4 pub bounds consts, validate_limit/validate_ttl helpers
- `crates/agent-memory-core/src/service.rs` — validation at the top of store/search/list/import; zero-clamp removed
- `crates/agent-memory-core/src/store/sqlite.rs` — 3 json_each predicates; MAX_KNN_K = crate::domain::MAX_LIMIT; DEFAULT_SEARCH_LIMIT promoted pub
- `crates/agent-memory/src/mcp.rs` — mapper client arm + exact-tag doc strings + extended tier test
- `crates/agent-memory/src/rest/handlers.rs` — mapper client arm + exact-tag doc strings + 3 in-process 400 tests
- `crates/agent-memory-core/tests/validation.rs` — new boundary-matrix suite (4 tests, dead-embedder harness)
- `crates/agent-memory-core/tests/semantic.rs` / `tests/fallback.rs` / `tests/store.rs` — per-site exact-tag regression tests
- `crates/agent-memory/tests/rest.rs` — 3 real-HTTP 400 realism assertions
- `README.md` — bounds tables, exact-tag wording, Behavioral changes in v0.1.0

## Decisions Made

- Fully qualified `crate::domain::validate_*` calls in service.rs (instead of `use` imports) so the plan's grep-count acceptance gates (`validate_ttl` == 2, `validate_limit` == 2) count exactly the call sites.
- `deny_unknown_fields` skipped per the plan's resolved discretion; case-folding not added (D-08 — no undecided behavior changes).
- Zero Cargo.toml changes — the rusqlite 0.39 pin triangle untouched (T-03-SC accepted disposition honored).

## Deviations from Plan

None functionally — plan executed as written. Two trivial adjustments:

1. The plan's verify command `cargo test -p agent-memory --lib` does not apply — the `agent-memory` crate is a binary with no lib target; ran `cargo test -p agent-memory --bins` (same tests, same coverage-bearing layer).
2. Comment wording in `service.rs` avoided the literal `.max(0)` and cited `MemoryError::InvalidArgument` so the plan's grep acceptance criteria (`grep -c "max(0)" == 0`; `grep -l InvalidArgument` lists service.rs) hold exactly.

## TDD Gate Compliance

Both TDD tasks completed the RED -> GREEN sequence with verified failing tests first:

- Task 1 RED (`04015de`): 2 core seam tests + 3 handler tests failed (out-of-bounds silently accepted/clamped); GREEN (`ee43e06`): all pass.
- Task 2 RED (`6c2f1c1`): all 3 tag tests failed (rustling over-matched on every path); GREEN (`18e2261`): all pass.
- No REFACTOR commits needed.

## Issues Encountered

None.

## Known Stubs

None — no placeholder values or unwired data paths introduced.

## Threat Flags

None — no new endpoints, auth paths, or schema changes; T-03-01..T-03-04 mitigations from the plan's threat model are implemented and test-locked (overflow unreachable, client-tier messages echo only client-sent values + public range, tag predicates stay bound-parameterized, bad input can no longer produce a 500 probe signal).

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 3 success criteria 1-3 are observably true and locked by tests; plan 03-02 (toolchain spikes + CI migration) can run independently (wave 1, no file overlap).
- Phase 4's update/link tools and Phase 5's JSONL importer inherit the validation seam structurally — `import()` already routes through `validate_ttl`.
- The "Behavioral changes in v0.1.0" README section is ready for DIST-06 release notes in Phase 6.

---
*Phase: 03-api-hardening-toolchain-spikes*
*Completed: 2026-07-12*

## Self-Check: PASSED

- All key files exist (validation.rs, domain.rs, README.md, SUMMARY.md)
- All 5 task commits present: 04015de, ee43e06, 6c2f1c1, 18e2261, 1c9c346
- Workspace suite 76 green; clippy -D warnings clean; fmt clean; coverage 86.56% lines (gate 80)
