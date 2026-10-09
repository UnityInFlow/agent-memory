---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: Hardening & Interop
status: executing
stopped_at: Phase 4 context gathered
last_updated: "2026-07-13T13:42:43.486Z"
last_activity: 2026-10-09 -- Completed quick task 261009-hc1 (hosted release pipeline, closes #1)
progress:
  total_phases: 4
  completed_phases: 1
  total_plans: 2
  completed_plans: 2
  percent: 25
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-07-12)

**Core value:** An agent can persist a structured memory and retrieve the right one later — across sessions and tools — over a standard MCP interface, with no cloud.
**Current focus:** Phase 03 — api-hardening-toolchain-spikes

## Current Position

Phase: 4
Plan: Not started
Status: Ready to execute
Last activity: 2026-10-09 - Completed quick task 261009-hc1: hosted release pipeline with SLSA provenance (closes #1)

Progress: [░░░░░░░░░░] 0% (0/4 v1.1 phases)

## Performance Metrics

**Velocity:**

- Total plans completed: 13
- Average duration: ~28 min
- Total execution time: ~1.4 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 1 | 3 | - | - |
| 02 | 5 | - | - |
| 03 | 2 | - | - |

**Recent Trend:**

- Last 5 plans: —
- Trend: —

*Updated after each plan completion*
| Phase 02 P01 | 26 min | 3 tasks | 20 files |
| Phase 02 P02 | 10 min | 2 tasks | 8 files |
| Phase 02 P03 | 40 min | 2 tasks | 9 files |
| Phase 02 P04 | 1h 46m | 3 tasks | 7 files |
| Phase 02 P05 | 8 min | 2 tasks | 6 files |
| Phase 03 P01 | 12 min | 3 tasks | 11 files |
| Phase 03 P02 | 30 min | 3 tasks | 2 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [Roadmap v1.1]: 4 coarse phases (3-6) instead of 5 — hybrid RRF (SEARCH-04/05) runs as a parallel track inside Phase 4 rather than its own phase; it touches only `service.rs` and shares no files with relations beyond the error variants Phase 3 lands (research/SUMMARY.md consensus, granularity=coarse).
- [Roadmap v1.1]: Relations schema (MCP-07/STORE-05, Phase 4) must land BEFORE the export format freezes (DIST-04, Phase 5) — JSONL format v1 carries link edges from day one, avoiding a format v2 one phase later.
- [Roadmap v1.1]: Windows/musl cross-compile spikes pulled forward into Phase 3 (the milestone's only feasibility unknown) even though the release legs land in Phase 6; fallback ladder gnu → msvc → document-and-defer (mcp-hub HUB-V2 pattern, last resort).
- [Roadmap v1.1]: API hardening (API-02/03) is foundational — `MemoryError::InvalidArgument` added once at the `MemoryService` seam in Phase 3; all new v1.1 surfaces (update/link/export/import) route through it as they land.
- [Roadmap v1.1]: musl CFLAGS shim is cheap and independent — it belongs with distribution (Phase 6) and must never block core features.
- [01-01]: Pinned `rusqlite 0.39` + `rusqlite_migration 2.5` (not research-suggested 0.40/2.6) so the whole graph shares one `libsqlite3-sys` (0.37) with `r2d2_sqlite 0.34` — 0.40/2.6 cause a `links = "sqlite3"` resolver conflict. Same audited crates, compatible pins. (Pin triangle must not move in v1.1.)
- [Phase 02-01]: SearchOutcome {search_mode, results} is the shared search envelope for MCP and REST — one serde struct keeps the wire shape stable across transports (hybrid mode must not change the envelope shape).
- [Phase 02-04]: chrono trimmed to default-features=false, features=[now] (UTC-only) — the default clock feature pulls iana-time-zone -> core-foundation-sys, un-linkable by zig darwin cross; Local time must never be reintroduced.
- [Phase 02-04]: v0.0.1 shipped without musl binaries: sqlite-vec.c uses BSD u_int*_t typedefs musl lacks — CFLAGS shim scheduled as DIST-05 (Phase 6).
- [02-05]: Two-tier error taxonomy at the store seam (InvalidQuery → 400/invalid_params; internal → 500/internal_error) proven live at 4 layers — Phase 3 extends this same taxonomy with InvalidArgument.
- [03-01]: Bounds consts live in domain.rs beside MemoryError; MAX_KNN_K re-defined as crate::domain::MAX_LIMIT (single source of truth, D-03)
- [03-01]: deny_unknown_fields NOT added (discretion resolved) — keeps the v0.1.0 release-notes delta to exactly the two decided behavioral changes
- [03-01]: import() validates ttl per draft through the shared helper — the store() bypass is closed structurally for Phase 5 JSONL import
- [03-02]: Phase 6 toolchain verdict (D-11): all 4 spike legs GREEN in one dispatch — Windows = cargo-xwin 0.23.0 / x86_64-pc-windows-msvc (gnu mingw-w64 proven fallback); musl = zigbuild 0.23.0 + target-suffixed CFLAGS -Du_int*_t shim (both legs green, no DIST-05 blocker)
- [03-02]: CI revived on GitHub-hosted ubuntu-latest, secretless contents: read (D-12/D-10, ecosystem D-02 exception) — supersedes inner CLAUDE.md 'never ubuntu-latest' for this public repo; release.yml runner decision stays with Phase 6

### Pending Todos

[From .planning/todos/pending/ — ideas captured during sessions]

None yet.

### Blockers/Concerns

None open. The v1.0 carried notes are now scheduled in this milestone:

- musl Linux binaries (sqlite-vec BSD typedefs) → DIST-05, Phase 6 (CFLAGS shim spiked in Phase 3).
- REST boundary-value hardening (limit/ttl_secs extremes, tag LIKE over-match; 02-REVIEW WR-01/WR-02/WR-07) → API-02/API-03, Phase 3.

Watch items:

- Windows C-code cross-compile unproven for this workspace (sqlite3.c + sqlite-vec.c) — Phase 3 spike is the decision input; Windows runtime untestable in CI (presence smoke + best-effort label only).
- If a fixed sqlite-vec stable release (PR #199) ships mid-milestone: bump the pin and delete the CFLAGS shim in the same commit.

### Quick Tasks Completed

| # | Description | Date | Commit | Directory |
|---|-------------|------|--------|-----------|
| 261009-hc1 | Release pipeline on GitHub-hosted runners with SLSA provenance, semver-only trigger, all actions SHA-pinned (closes #1); resolves the release-runner decision Phase 3 D-12 deferred to Phase 6 | 2026-10-09 | 9810e31 | [261009-hc1-move-release-yml-to-github-hosted-runner](./quick/261009-hc1-move-release-yml-to-github-hosted-runner/) |

## Deferred Items

Items acknowledged and carried forward:

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| Distribution | DIST-03 Windows binaries | Picked up in v1.1 — Phase 6 (spiked Phase 3) | Roadmap (2026-06-24) |
| Search | SEARCH-04 hybrid RRF fusion | Picked up in v1.1 — Phase 4 | Roadmap (2026-06-24) |
| Interface | MCP-06 memory_update / relation tools | Picked up in v1.1 — Phase 4 | Roadmap (2026-06-24) |
| Distribution | DIST-04 portable export | Picked up in v1.1 — Phase 5 | Roadmap (2026-06-24) |
| Search | SEARCH-06 request-level `mode` override | Deferred (future) | Requirements v1.1 (2026-07-12) |
| Interface | MCP-08 LLM write-arbitration | Deferred (future — non-local) | Requirements v1.1 (2026-07-12) |

## Session Continuity

Last session: 2026-07-12T20:49:51.528Z
Stopped at: Phase 4 context gathered
Resume file: .planning/phases/04-memory-update-relations-hybrid-search/04-CONTEXT.md

## Operator Next Steps

- `/gsd-discuss-phase 3` to lock in Phase 3 decisions, then `/gsd-plan-phase 3`
