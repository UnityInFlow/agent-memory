---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: Hardening & Interop
status: planning
last_updated: "2026-07-12"
last_activity: 2026-07-12
progress:
  total_phases: 4
  completed_phases: 0
  total_plans: 0
  completed_plans: 0
  percent: 0
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-07-12)

**Core value:** An agent can persist a structured memory and retrieve the right one later — across sessions and tools — over a standard MCP interface, with no cloud.
**Current focus:** Milestone v1.1 roadmap created (Phases 3-6) — Phase 3 (API Hardening & Toolchain Spikes) ready to plan

## Current Position

Phase: 3 of 6 — API Hardening & Toolchain Spikes (first phase of v1.1)
Plan: —
Status: Ready to plan
Last activity: 2026-07-12 — v1.1 roadmap created: 4 coarse phases (3-6), 12/12 requirements mapped

Progress: [░░░░░░░░░░] 0% (0/4 v1.1 phases)

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

Last session: 2026-07-12
Stopped at: v1.1 roadmap created (Phases 3-6, 12/12 requirements mapped) — Phase 3 ready to plan
Resume file: None

## Operator Next Steps

- `/gsd-discuss-phase 3` to lock in Phase 3 decisions, then `/gsd-plan-phase 3`
