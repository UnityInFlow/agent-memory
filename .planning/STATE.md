---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: Hardening & Interop
current_phase: 04
current_phase_name: Memory Update, Relations & Hybrid Search
status: executing
stopped_at: Completed 04-06-PLAN.md
last_updated: "2026-10-23T19:59:51.264Z"
last_activity: 2026-10-23
last_activity_desc: Completed 04-06 (gap wave 2, conditional backfill + symmetric relates_to)
state_head: c72146bc6296170116a8ed40749576c6af5c6f87
progress:
  total_phases: 4
  completed_phases: 1
  total_plans: 9
  completed_plans: 8
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-07-12)

**Core value:** An agent can persist a structured memory and retrieve the right one later — across sessions and tools — over a standard MCP interface, with no cloud.
**Current focus:** Phase 04 — Memory Update, Relations & Hybrid Search

## Current Position

Phase: 04 (Memory Update, Relations & Hybrid Search) — EXECUTING
Plan: 6 of 7 (gap closure: 04-05, 04-06 done; 04-07 next)
Status: Executing gap closure — 04-07 ready to execute
Last activity: 2026-10-23 — Completed 04-06 (CR-03 conditional sweep backfill, WR-03 symmetric relates_to, IN-05/IN-06/WR-04 docs)

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
**Per-Plan Metrics:**

| Plan | Duration | Tasks | Files |
|------|----------|-------|-------|
| Phase 04 P01 | 9 min | 2 tasks | 13 files |
| Phase 04 P02 | 10min | 2 tasks | 11 files |
| Phase 04 P03 | 10 min | 2 tasks | 9 files |
| Phase 04 P04 | 11 min | 2 tasks | 11 files |
| Phase 04 P05 | 10 min | 3 tasks | 12 files |
| Phase 04 P06 | 7 min | 3 tasks | 9 files |

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
- [Phase 04]: [04-01]: MemoryError::Internal(String) is the 500/internal_error tier for server faults (poisoned writer lock); MemoryError::NotFound and ApiError::NotFound deleted — not-found is data (Ok(false)/Ok(None)) only (WR-02)
- [Phase 04]: [04-01]: Store::exists takes Option<&str> with NULL-safe 'source IS ?1' — source:None re-imports are idempotent (CR-01)
- [Phase 04]: [04-01]: MAX_KNN_K = MAX_LIMIT * 4 (800) — candidate oversample cap, distinct from RRF k=60 (WR-01 partial; full filter-aware fix deferred)
- [Phase 04]: [04-01]: SqliteStore::open order is probe user_version (before any PRAGMA, so a refused DB is byte-untouched) -> SchemaTooNew refusal -> pragmas -> sibling online backup {db}.backup-pre-{NNNN}-{YYYYMMDD} (one per target version, failure aborts) -> to_latest; LATEST_SCHEMA_VERSION=3 must be bumped with every new migration
- [Phase 04]: [04-01]: tests/fixtures/v0.0.1.db is a committed binary fixture generated once from tag-identical 0001+0002 — never regenerate it; it is gitignored locally by *.db so stage with git add -f
- [Phase 04]: 04-02: memory_update returns a dedicated UpdatedMemory (flattened MemoryView + embedding_status); MemoryView untouched so list/search envelopes stay byte-identical (D-10)
- [Phase 04]: 04-02: an all-fields-absent patch is InvalidArgument, not a no-op; content null/empty rejected at the seam; TTL patches reuse validate_ttl (D-08)
- [Phase 04]: 04-02: update is UPDATE-in-place (never delete+insert), folds the D-09 freshness bump and decay_score = 1.0 into the same statement, and swaps the vector in the same writer tx
- [Phase 04]: 04-02: patch DTO fields carry skip_serializing_if so the MCP tool schema never advertises default: null (null means clear in a merge-patch)
- [Phase 04]: 04-02: PATCH /api/memories/{id} maps every Json extractor rejection to 400 {error}; other REST handlers keep axum defaults
- [Phase 04]: 04-03: Relations are flat directed edges (LinkKind relates_to|supersedes|caused_by, closed enum); duplicate link = idempotent Linked, self-link = InvalidArgument at the seam, missing endpoint = LinkOutcome::NotFound{missing_id} data (never an FK 500)
- [Phase 04]: 04-03: forget() and sweep_expired() each carry an explicit orphan-edge DELETE beside the FK ON DELETE CASCADE, proven by pragma-less-connection tests
- [Phase 04]: 04-03: expand_links attaches MemoryView.related (Option, skip_serializing_if) after truncation and after spawn_bump; MAX_EXPANDED_LINKS = 10 newest-first, snippet 160 chars; neighbors never bumped; flag-less envelope golden-tested byte-identical
- [Phase 04]: 04-03: Incoming edges read natural reverse labels (superseded_by, caused); RelatedLink.direction is out|in; REST POST /api/links + DELETE /api/links/{from_id}/{to_id}/{kind} with 200/404 and 400 on any bad body/kind
- [Phase 04]: 04-04: Hybrid search = RRF (k=60, 1-based ranks, duplicate ids sum, id tie-break) over two pure-relevance legs (KNN distance, bm25 ASC), each oversampled limit*4 capped at MAX_KNN_K; decay applied once, multiplicatively, post-fusion (SEARCH-04/05)
- [Phase 04]: 04-04: SearchMode::Semantic removed; embedder-up searches report search_mode "hybrid", embedder-down stays "keyword" byte-identical to v1.0 (README behavioral change 3)
- [Phase 04]: 04-04: Keyword-leg errors propagate first, so a malformed MATCH (incl. bare ?/,/' punctuation) is InvalidQuery/400 in hybrid as in keyword mode (T-04-15); follow-up candidate: sanitize natural-language queries for the keyword leg
- [Phase 04]: 04-05: T-04-15 revised by user decision option-a (2026-10-23) — search runs the raw FTS5 MATCH first, then one sanitized retry (word tokens quoted and OR-joined) on any FTS5 query-parse error, including the step-time `no such column` class from hyphens/colons; a tokenless query is an empty keyword leg (hybrid answers semantic-only, keyword mode returns empty); identical in both modes, so search never returns InvalidQuery; every search-path internal error is redacted to SEARCH_STORAGE_FAILURE (T-04-19). Supersedes the 04-04 decision that a malformed MATCH is InvalidQuery/400 in hybrid mode.
- [Phase 04]: 04-06: sweep backfill writes a vector only through a guarded claim (embedding_status = 0 AND content = embedded text) run first in the writer tx; Store::insert_embedding returns bool. Closes REVIEW CR-03, across processes too.
- [Phase 04]: 04-06: relates_to stored canonically as (min id, max id) and unlinked from either end; supersedes/caused_by stay directed (D-03). Storage-only: MCP/REST responses echo request ids; NotFound names the request's from_id first. No migration (REVIEW WR-03).
- [Phase 04]: 04-06: WR-04 documented only: the 200 x 10 expansion bound covers search and list-with-limit; unlimited expanded memory_list returns every row. Behaviour choice (reject / default limit / unbounded) is an open API-contract question for Phase 5/6.

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

Last session: 2026-10-23T19:59:51.214Z
Stopped at: Completed 04-06-PLAN.md
Resume file: None

## Operator Next Steps

- `/gsd-discuss-phase 3` to lock in Phase 3 decisions, then `/gsd-plan-phase 3`
