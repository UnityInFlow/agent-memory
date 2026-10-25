---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: Hardening & Interop
current_phase: 6
current_phase_name: Distribution & v0.1.0 Release
status: planning
stopped_at: Phase 5 complete, ready to plan Phase 6
last_updated: "2026-10-25T13:51:46.548Z"
last_activity: 2026-10-25
last_activity_desc: Phase 5 complete, transitioned to Phase 6
state_head: fe18db796014dc9d885859771ee089ce7ba895df
progress:
  total_phases: 4
  completed_phases: 3
  total_plans: 14
  completed_plans: 14
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-07-12)

**Core value:** An agent can persist a structured memory and retrieve the right one later — across sessions and tools — over a standard MCP interface, with no cloud.
**Current focus:** Phase 5 — Portable Export & Import

## Current Position

Phase: 6 — Distribution & v0.1.0 Release
Plan: Not started
Status: Ready to plan
Last activity: 2026-10-25 — Phase 5 complete, transitioned to Phase 6

Progress: [░░░░░░░░░░] 0% (0/4 v1.1 phases)

## Performance Metrics

**Velocity:**

- Total plans completed: 22
- Average duration: ~28 min
- Total execution time: ~1.4 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 1 | 3 | - | - |
| 02 | 5 | - | - |
| 03 | 2 | - | - |
| 04 | 7 | - | - |
| 5 | 5 | - | - |

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
| Phase 04 P07 | 12 min | 3 tasks | 7 files |
| Phase 05 P01 | 14 min | 3 tasks | 10 files |
| Phase 05 P02 | 13 min | 2 tasks | 8 files |
| Phase 05 P03 | 16min | 2 tasks | 6 files |
| Phase 05 P04 | 9 min | 2 tasks | 2 files |
| Phase 05 P05 | 6 min | 2 tasks | 2 files |

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
- [Phase 04]: 04-06: WR-04 resolved by user decision option-a (reject, 2026-10-23, quick task 261023-v6w) — memory_list with expand_links and no limit is a client error (MemoryError::InvalidArgument → REST 400 {error} / MCP invalid_params) whose message tells the caller to pass a limit; expand_links with a limit, a list without expand_links, and search (already bounded) are unchanged, so the 200 x 10 expansion bound now covers every expanded response. Supersedes the 04-06 doc-only deferral; closed, nothing left for Phase 5/6.
- [Phase 04]: 04-07: deny_unknown_fields added to UpdateArgs/UpdateBody only (new v0.1.0 surfaces, REVIEW WR-02); the [03-01] decision stands for the v0.0.1 DTOs, so the v0.1.0 release-notes delta for existing surfaces is unchanged. Over MCP, rmcp 1.8 reports the rejection as a tool error result (isError: true, "failed to deserialize parameters: unknown field ..."), not a JSON-RPC -32602; REST is a 400 {"error"}.
- [Phase 04]: 04-07: backup temp is pid-qualified and a process-wide BACKUP_LOCK serializes backups within one process, so same-process openers never share a temp; prune tolerates NotFound from a concurrent opener
- [Phase 04]: 04-07: WR-01 closed with verify-then-rename + tolerate-by-re-probe (no file lock, no new dep); cross-process exclusive lock stays deferred
- [Phase 05-01]: redact_storage_error(op, e) at the MemoryService seam on all nine methods (store, list, search, import, forget, link, unlink, update, sweep) via *_unredacted inner fns; transport mappers byte-identical; closes 04-REVIEW IN-01 (D-14)
- [Phase 05-01]: Store::exists -> Store::find_by_key(Option<i64>) with the same NULL-safe (source, mem_type, content) key (D-07); LinkOutcome::Linked { created: bool } from tx.changes() across 7 sites, storage-only not a wire change (D-12)
- [Phase 05-01]: ImportedMemory + Store::insert_imported (no embedding parameter, always embedding_status 0, last_accessed = now, created_at/access_count/expires_at preserved) behind one shared insert_row helper; importer skips expires_at <= now, sweep deletes < now (D-10, D-13)
- [Phase 5]: [Phase 05-02]: Format v1 frozen in export.rs as one strict tagged Record enum (record first, declaration order, deny_unknown_fields; header carries exported_rows/exported_links); wire order golden-tested; D-03 one-way rating recorded as pre-approved
- [Phase 5]: [Phase 05-02]: Store::export_rows streams every row from ONE pooled connection + ONE read transaction (rusqlite unchecked_transaction) — meta pin, counts, memories ORDER BY id, links ORDER BY from_id,to_id,kind — via a visitor, never a Vec; MemoryService::export flushes inside spawn_blocking and redacts with op export (tenth op string)
- [Phase 5]: [Phase 05-02]: Research correction: stdout export sink is BufWriter::new(std::io::stdout()) — StdoutLock is !Send and cannot be a Box<dyn Write + Send>; --output publishes via export_file::PendingExport ({target}.export-tmp-{pid} beside the target, create_new + 0600, sync_all then rename, one abort covers every failure)
- [Phase 5]: 05-03: jsonl importer is add-alongside beside gsd-state — two parser modules, two tolerance policies (strict vs tolerant), shared seam/dedup/writer lane; a third format forces an ImportSource enum
- [Phase 5]: 05-03: D-11 messages — HeaderProbe format/version are Option (non-header line 1 → 'first record must be the header'), serde's ' at line 1 column N' suffix stripped, InvalidArgument payload lifted before the 'line N:' prefix
- [Phase 5]: 05-03: import_jsonl runs passes 2+3 in ONE spawn_blocking hop with per-record writer transactions; parse_jsonl is the all-or-nothing gate; the jsonl path never calls the embedder
- [Phase 05]: 05-04: README Export & import section is one ## with ten ### subsections in plan order; sweep cadence documented as the shipped 'then hourly' (SWEEP_INTERVAL 3600 s); tests/cli.rs is realism-only (zero coverage by design, like tests/rest.rs) and pins the exact report strings
- [Phase 05]: 05-04: phase-end gates recorded — cargo llvm-cov 95.61% lines (floor 80), D-01 transport grep empty, sql/ + Cargo.lock + .github/ unchanged across all phase-5 commits
- [Phase 5]: [05-05]: Gap 1 (DIST-04 backstop truth 2.8) closed with a channel-ordered race test driving Store::export_rows directly — WAL snapshot isolation proven, no production change; every wait bounded by recv_timeout
- [Phase 5]: [05-05]: Gap 2 (INTEROP-02 live re-embed) closed by a recorded run of the real binary against local nomic-embed-text (05-05-realism.sh, exit 0: backfilled=3 on first sweep tick, search_mode hybrid, target rank 0) — stays a script, never a cargo test

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
| 261023-v6w | WR-04: reject `memory_list` with `expand_links` and no `limit` as a client error (MCP `invalid_params`, REST 400); user decision option (a) | 2026-10-23 | 969e891 | [261023-v6w-wr-04-reject-memory-list-expand-links-wi](./quick/261023-v6w-wr-04-reject-memory-list-expand-links-wi/) |

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

Last session: 2026-10-25T13:42:17.195Z
Stopped at: Phase 5 complete, ready to plan Phase 6
Resume file: None

## Operator Next Steps

- `/gsd-discuss-phase 3` to lock in Phase 3 decisions, then `/gsd-plan-phase 3`
