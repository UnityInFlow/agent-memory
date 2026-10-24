---
phase: 04-memory-update-relations-hybrid-search
verified: 2026-10-23T20:19:46Z
status: passed
score: 28/28 must-haves verified
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 13/15
  gaps_closed:
    - "Truth #14: hyphen/colon queries (sqlite-vec, auth: tokens) no longer produce a 500 / internal_error with leaked SQLite text. They now return results in both modes. The original wording ('a malformed MATCH is 400 in hybrid mode') is superseded by user decision option-a, which is recorded in the 04-05 plan and in STATE.md [04-05]"
    - "Truth #15: the human-decision item is resolved by option-a. Natural-language and punctuated queries succeed in hybrid and keyword mode, identically with Ollama up or down. Verified live over MCP and REST"
  gaps_remaining: []
  regressions: []
  superseded_must_haves:
    - must_have: "A malformed MATCH query is a 400/invalid_params in hybrid mode exactly as in keyword mode (04-04 must-have #3)"
      superseded_by: "User decision option-a, 2026-10-23"
      recorded_in:
        - ".planning/phases/04-memory-update-relations-hybrid-search/04-05-PLAN.md section 'Locked user decision'"
        - ".planning/STATE.md decision [Phase 04] 04-05 (line 118)"
---

# Phase 4: Memory Update, Relations & Hybrid Search Verification Report

**Phase goal:** As an agent, I want to update existing memories, link related ones, and get hybrid keyword+semantic recall, so that my memory stays current and connected and I find the right memory even when the phrasing differs.
**Verified:** 2026-10-23T20:19:46Z
**Status:** passed
**Re-verification:** Yes. This follows gap-closure plans 04-05, 04-06 and 04-07. The previous run was gaps_found at 13/15.

## Evidence

- **Code:** read at HEAD `8b2c762`. The working tree under `crates/` and `README.md` is clean.
- **Build:** the binary was rebuilt from HEAD (`cargo build -p agent-memory`).
- **Tests I ran myself:**
  - phase test binaries `hybrid` 21, `backfill_race` 7, `relations` 27, `migration_hygiene` 8 (+1 ignored generator), `update` 9: all pass;
  - `agent-memory-core --lib store::sqlite`: 12 pass;
  - the stdio e2e `tools`: 6 pass;
  - the named MCP tests `memory_update_*` and `memory_search_storage_failure_is_internal_error_without_sqlite_text`: pass.
- **Orchestrator gates at HEAD:** fmt, clippy `-D warnings`, `cargo test --workspace` (211 passed / 0 failed / 2 ignored).
- **Live sessions:** the HEAD binary over MCP stdio and REST, against the real local Ollama (`nomic-embed-text`) and against an unreachable URL (`http://127.0.0.1:9`).
- **Scratch data:** all under `/tmp/v04re-data` and `/tmp/v04re-scripts`. Nothing in the repo was modified.

## Goal Achievement — User Flow Coverage (MVP mode)

| Story step | Expected | Evidence | Status |
|---|---|---|---|
| Update an existing memory | Patch changes it in place; the old meaning stops matching; unknown or immutable fields are refused | Live: `memory_update {id:4, content}` returned the full view with `embedding_status:1`. `{type}`, `{ttl_sec}` and `{source}` were refused, naming the field, and the row was unchanged. Unknown id 999 gave a clean not_found | ✓ |
| Link related memories | Typed link; symmetric `relates_to`; 1-hop expansion; cascade on forget | Live: link(5,2) followed by link(2,5) `relates_to` produced one edge. Expansion listed it once from each side. unlink(2,5) removed it, and the response echoed the request order. A pair of directed `supersedes` edges (6→1 and 1→6) stayed two edges. forget(6) left 0 edges | ✓ |
| Find the right memory even when phrasing differs | Hybrid recall with punctuation-tolerant queries, Ollama up or down | Live, 25-query table (below): every query succeeded in both modes, with 0 errors and no SQLite text in any body or log | ✓ |

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC1: `memory_update` partial patch changes the memory in place. A content change re-mirrors FTS and re-embeds in the same writer tx. An embed outage gives status 0 plus backfill. The old meaning stops matching. An unknown id gets a clean not-found | ✓ VERIFIED (regression) | `update.rs` 9/9 pass. Live update returned `embedding_status:1`. Live id 999 returned `{"id":999,"reason":"not_found","updated":false}` on MCP, and 404 with the same body on REST |
| 2 | SC2: link/unlink handle flat typed relations; search/list can expand 1-hop; forget and TTL cascade with no orphans | ✓ VERIFIED (regression) | `relations.rs` 27/27 pass, including the forget/TTL cascade and orphan-healing tests. Live forget(6) left `memory_links` count = 0 |
| 3 | SC3: with Ollama up, RRF k=60 hybrid labelled `hybrid`; with Ollama down, keyword exactly as v1.0 with an unchanged envelope | ✓ VERIFIED (regression) | Live: up gave `search_mode:"hybrid"`, down gave `"keyword"`, both with keys `{search_mode, results}`. `hybrid.rs` RRF goldens pass |
| 4 | SC4: decay-aware hybrid ranking; bumps go only to post-truncation returned ids | ✓ VERIFIED (regression) | `hybrid.rs` passes (equal-fused-rank fresh-vs-stale, returned-ids-only bump). The service still bumps after `take(limit)` and before expansion |
| 5 | SC5: real v0.0.1 upgrade only after a pre-migration backup; 0001/0002 frozen; newer schema gives a friendly error | ✓ VERIFIED (regression) | `git diff v0.0.1 -- sql/0001* sql/0002*` is empty. The fixture upgrade live produced a backup with user_version=2 and 2 rows. Live `user_version=99`: exit 1, empty stdout, D-13 message on stderr naming 99/3 and the brew command, shasum unchanged |
| 6 | Re-importing a sourceless draft is idempotent (CR-01, old) | ✓ VERIFIED (regression) | `source IS ?1` unchanged; covered by the workspace suite |
| 7 | A poisoned writer lock is 500/internal, never 404 | ✓ VERIFIED (regression) | `Internal("writer lock poisoned")` at every writer, including the rewritten `insert_embedding`. Mapper tests pass |
| 8 | Merge-patch: absent = unchanged, null = clear, null or empty content = 400 | ✓ VERIFIED (regression) | MCP matrix tests (`memory_update_tags_and_scope_matrix_absent_null_value`, `..._ttl_matrix...`) pass |
| 9 | The update response is the full post-update view plus `embedding_status`; the update bumps freshness | ✓ VERIFIED (regression) | Live response carried `embedding_status` and a new `last_accessed` |
| 10 | Link edge cases: duplicate is a no-op, self-link is 400, unknown kind is 400, missing endpoint is a not-found naming the id | ✓ VERIFIED (regression) | Live: link(5,99) returned `missing_id:99`. REST link(9,1) returned 404 with `missing_id:9` (request's from_id first). REST unlink with `bogus` kind returned 400 listing the allowed kinds |
| 11 | Expansion goes both directions with natural reverse labels, a cap of 10, most recent first | ✓ VERIFIED (regression) | Live: `supersedes`/`out` and `superseded_by`/`in` |
| 12 | With the flag absent, the JSON is byte-identical to v1.0 (no `related` key) | ✓ VERIFIED (regression) | Live flag-less search results carried no `related` member |
| 13 | Expanded neighbours are never access-bumped | ✓ VERIFIED (regression) | relations/hybrid tests pass |
| 14 | **(previously FAILED)** Hyphenated or colon-bearing queries (`sqlite-vec`, `auth: tokens`, `re-embed`) succeed in both modes. Never a 500 / internal_error, and no SQLite text leaks. The original 400 wording is superseded by option-a | ✓ VERIFIED | `is_match_input_error` (sqlite.rs:519) now classifies the step-time `no such column` FTS5 class. `match_with_sanitized_retry` (sqlite.rs:569) is used by both `search` (:866) and `keyword_candidates` (:908). **Live MCP, Ollama up:** `sqlite-vec` → hybrid [1,…]; `auth: tokens` → hybrid [3,…]. **Ollama down:** `sqlite-vec` → keyword [1]; `auth: tokens` → keyword [3]. **Live REST:** 200 in both modes. `grep "sqlite error\|no such column"` over every serve log returned 0 |
| 15 | **(previously UNCERTAIN)** Goal outcome under option-a: natural-language queries with punctuation succeed in both modes and return the matching memory first | ✓ VERIFIED | **Live, both modes, correct memory at rank 1:** `what vector store do we use?` → 1, `don't retry` → 4, `auth, tokens` → 3, `C++` → 5, `how often do credentials get rotated?` → 3. Pure paraphrase with no keyword overlap: `which datastore keeps client information` → hybrid [2,…] (keyword mode returns [] as expected; the recall comes from the semantic leg) |
| 16 | Valid structured FTS5 syntax still runs exactly as written (raw first) | ✓ VERIFIED | `structured_fts5_syntax_still_runs_raw` passes. Live: `sqlite AND vec`, `"vector similarity"`, `vec*` → [1] in keyword mode. `content: sqlite` ran raw as a real column filter → [1] |
| 17 | A tokenless query: hybrid answers semantic-only, keyword mode returns an empty list, never an error | ✓ VERIFIED | `sanitize_fts_query` returns `None` → `Ok(vec![])`. Live: `""`, `"`, `?!`, `*`, `^` → hybrid returned 3 results (KNN only); keyword returned `[]` with HTTP 200 |
| 18 | Keyword-leg classification is identical with Ollama up or down (one shared helper) | ✓ VERIFIED | Both store paths call `match_with_sanitized_retry(&args.query, run)`. `queries_fts5_cannot_parse_succeed_identically_in_both_modes` passes. Live: no query errored in either mode |
| 19 | No MCP or REST search body ever carries raw SQLite text. A storage failure is a generic `SEARCH_STORAGE_FAILURE` (500 / internal_error), with the detail only in stderr | ✓ VERIFIED | `pub async fn search` → `.map_err(redact_search_error)` (service.rs:283). The redaction match has no catch-all arm and covers Sqlite/Pool/Join/Migration. Tests pass: `memory_search_storage_failure_is_internal_error_without_sqlite_text` (MCP, both modes, FTS table renamed away underneath a live server), the REST `search_storage_failure_is_500_without_sqlite_text`, and the hybrid.rs redaction rows. *A live induced-failure run was blocked by the project's DROP-statement hook, so the evidence here is test-based* |
| 20 | CR-03: a sweep backfill whose row changed, was forgotten, or was claimed by a racing sweep writes nothing. A failed racing update leaves the row pending for its NEW text | ✓ VERIFIED | `insert_embedding` (sqlite.rs:687) first runs a guarded `UPDATE … WHERE id=?1 AND embedding_status=0 AND content=?2` in the same tx as the vector write, and returns `false` with no write when 0 rows match. `service.sweep` passes the exact embedded `&content` (service.rs:698) and counts `skipped_stale`. `backfill_race.rs` 7/7 pass, deterministic: embedded update, unembedded update, forget, double backfill, gated-embedder mid-backfill update, and a failed update followed by the next sweep |
| 21 | `relates_to` is one symmetric relation: stored canonically, listed once per side, unlinkable from either end, with the request order echoed on the wire | ✓ VERIFIED | `canonical_endpoints`/`is_symmetric` (domain.rs:127-149). link stores `(min,max)`; unlink matches both orientations when `?4=1`. Live: link(5,2) and link(2,5) each echoed their own request order. Expansion showed id 2 once on 5's side. unlink(2,5) removed the edge (REST and MCP). Tests pass: `relates_to_linked_both_ways_is_one_relation_listed_once_from_each_side` and `relates_to_is_stored_with_the_smaller_id_first` |
| 22 | `supersedes`/`caused_by` keep directed identity; a missing endpoint is named in request order | ✓ VERIFIED | Live: 6→1 and 1→6 `supersedes` were both stored and both listed on id 6 (`supersedes` out, `superseded_by` in). unlink(1,6) removed only that one. REST link(9,1) named 9 |
| 23 | The `MAX_EXPANDED_LINKS` doc and the README state the real expanded bound (WR-04 doc correction) | ✓ VERIFIED | domain.rs:363-369 ("A list without a limit returns every row … deferred API-contract"), and a matching README note |
| 24 | `memory_update` / `PATCH` with an unknown or immutable field is rejected, naming the field, and nothing is written | ✓ VERIFIED (protocol-tier note) | `#[serde(deny_unknown_fields)]` on `UpdateArgs` (mcp.rs:169) and `UpdateBody` (handlers.rs:107). Live REST: `{type}`, `{ttl_sec}`, `{source,content}` → 400 `{"error":"… unknown field \`ttl_sec\`, expected one of …"}`, and row 2's content and type were unchanged. Live MCP: rejected as a tool error result (`isError:true`, "failed to deserialize parameters: unknown field `type` …") rather than a JSON-RPC -32602. That is rmcp 1.8's design; it is recorded in STATE.md [04-07] and the 04-07 SUMMARY. The intent (never silently drop a member, name the field, write nothing) holds |
| 25 | Malformed REST path or query params return a JSON `{"error"}` body with the extractor's status | ✓ VERIFIED | Live: `PATCH /api/memories/abc`, `DELETE /api/memories/abc`, `DELETE /api/links/x/2/relates_to`, `GET /api/memories?expand_links=1`, `?limit=abc` all returned 400 `application/json` `{"error":…}` |
| 26 | Two processes opening the same pre-0003 DB at once both start, and exactly one valid pre-migration backup with the OLD schema version survives | ✓ VERIFIED | `pre_migration_backup(&writer, path, db_version)` (sqlite.rs:409) writes a temp file, checks the version, renames, then prunes. `migrate_tolerating_concurrent_upgrade` re-probes the version on failure. **Live:** 3 rounds of two simultaneous `serve-rest` processes on a copy of the v0.0.1 fixture. Every round, both returned `/health` 200; the live DB reached uv=3; exactly one `m.db.backup-pre-0003-*` existed with uv=2, 2 rows and no `memory_links`. The log showed "another process completed the schema upgrade first". `two_concurrent_first_opens_of_a_v0_0_1_database_both_succeed_with_one_genuine_backup` and `a_real_lost_migration_race_is_absorbed` pass |
| 27 | A failed backup write never removes the previous valid backup (replace, then prune) | ✓ VERIFIED | Pruning happens only after a successful rename. Unit tests pass: `a_failed_backup_write_never_removes_the_previous_backup`, `a_backup_that_cannot_be_moved_into_place_keeps_the_previous_one`, `a_valid_backup_replaces_then_prunes_the_stale_one`, `a_backup_of_an_already_migrated_database_is_discarded_and_keeps_the_valid_one` |
| 28 | The README documents backup location, restore and the newer-schema refusal (IN-01) | ✓ VERIFIED | README `## Upgrading` (line 84): `memory.db.backup-pre-NNNN-YYYYMMDD`, the `cp` restore step, WAL/SHM notes, and the brew-upgrade message |

**Score:** 28/28 truths verified. None are present-but-behavior-unverified: every behaviour-dependent truth (20, 26, 27) has a passing deterministic test, and 26 was also exercised live.

### Live search decision table (MCP stdio, 25 queries, `limit: 3`)

| Query | Ollama up | Ollama down |
|---|---|---|
| `sqlite-vec` | hybrid [1,2,3] | keyword [1] |
| `auth: tokens` | hybrid [3,4,2] | keyword [3] |
| `re-embed` | hybrid [2,1,4] | keyword [] |
| `what vector store do we use?` | hybrid [1,2,5] | keyword [1] |
| `don't retry` | hybrid [4,3,6] | keyword [4] |
| `auth, tokens` | hybrid [3,4,2] | keyword [3] |
| `C++` | hybrid [5,2,1] | keyword [5] |
| `"unbalanced` / `AND` / `foo OR` / `NEAR(` / `(sqlite` | hybrid (all ok) | keyword [] / [5] / [] / [] / [1] |
| `?!` / `*` / `` / `"` / `^` (tokenless) | hybrid, semantic-only | keyword [] |
| `sqlite AND vec` / `"vector similarity"` / `vec*` | hybrid [1,…] | keyword [1] |
| `which datastore keeps client information` | hybrid [2,1,3] | keyword [] |
| `how often do credentials get rotated?` | hybrid [3,2,4] | keyword [3] |

There were 0 RPC errors and 0 tool errors. REST gave 200 for every query sampled in both modes.

### Required Artifacts (gap-closure plans)

| Artifact | Expected | Status | Details |
|---|---|---|---|
| `crates/agent-memory-core/src/store/sqlite.rs` | `fn match_with_sanitized_retry`, `embedding_status = 0 AND content = ?2`, `fn migrate_tolerating_concurrent_upgrade` | ✓ VERIFIED | All present and called from the right sites |
| `crates/agent-memory-core/src/service.rs` | `fn redact_search_error`, `SEARCH_STORAGE_FAILURE` | ✓ VERIFIED | Wired at `pub async fn search` |
| `crates/agent-memory-core/src/domain.rs` | `fn canonical_endpoints`, `fn is_symmetric` | ✓ VERIFIED | Exhaustive matches, no `_` arm |
| `crates/agent-memory-core/tests/hybrid.rs` | `queries_fts5_cannot_parse_succeed_identically_in_both_modes` | ✓ VERIFIED | Line 587, passes |
| `crates/agent-memory-core/tests/backfill_race.rs` | ≥120 lines, deterministic race tests | ✓ VERIFIED | 7 tests, pass |
| `crates/agent-memory/src/mcp.rs` | `deny_unknown_fields` on UpdateArgs | ✓ VERIFIED | Line 169 |
| `crates/agent-memory/src/rest/handlers.rs` | `deny_unknown_fields` on UpdateBody; PathRejection/QueryRejection mapping | ✓ VERIFIED | Lines 107, 234, 285, 316, 395 |
| `README.md` | "never rejected", `## Upgrading` | ✓ VERIFIED | Lines 205, 84 |

### Key Link Verification

| From | To | Via | Status |
|---|---|---|---|
| sqlite.rs `fn search` | `match_with_sanitized_retry` | keyword-degrade path | ✓ WIRED (:866) |
| sqlite.rs `fn keyword_candidates` | `match_with_sanitized_retry` | hybrid keyword leg | ✓ WIRED (:908) |
| service.rs `pub async fn search` | `redact_search_error` | `map_err` over both arms and expansion | ✓ WIRED (:283) |
| service.rs `sweep` | `Store::insert_embedding` | `insert_embedding(id, &content, vector)` | ✓ WIRED (:698) |
| sqlite.rs link/unlink | `canonical_endpoints` / `is_symmetric` | canonical storage; both-orientation unlink | ✓ WIRED (:977, :1002) |
| sqlite.rs `SqliteStore::open` | `pre_migration_backup(&writer, path, db_version)` | probed version passed in | ✓ WIRED (:409) |
| sqlite.rs `SqliteStore::open` | `migrate_tolerating_concurrent_upgrade` | wraps `to_latest` | ✓ WIRED |

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|---|---|---|---|---|
| Hybrid `SearchOutcome.results` | fused views | `vec_memories` KNN plus `memories_fts` bm25 (raw or sanitized) | yes (live) | ✓ FLOWING |
| Sweep backfill | vectors | Ollama batch embed, then guarded claim | yes (tests; live store gave status 1) | ✓ FLOWING |
| `MemoryView.related` | edges | `memory_links JOIN memories` | yes (live) | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|---|---|---|---|
| Phase test binaries | `cargo test -p agent-memory-core --test hybrid --test backfill_race --test relations --test migration_hygiene --test update` | 72 passed, 0 failed, 1 ignored | ✓ PASS |
| Store unit tests (backup, sanitizer, classifier) | `cargo test -p agent-memory-core --lib -- store::sqlite` | 12 passed | ✓ PASS |
| Stdio e2e | `cargo test -p agent-memory --test tools` | 6 passed | ✓ PASS |
| Live search table, Ollama up and down | MCP stdio driver, 25 queries × 2 modes | all succeed, no leaks | ✓ PASS |
| Live REST search and error bodies | `serve-rest` + curl | 200 on queries; 400 JSON on malformed path, query and unknown field | ✓ PASS |
| Live concurrent first open | 2 × `serve-rest` on a v0.0.1 fixture copy, 3 rounds | both up, one uv=2 backup | ✓ PASS |
| Live too-new refusal | `--db` with user_version=99 | exit 1, friendly message, file untouched | ✓ PASS |

### Probe Execution

None declared. There is no `scripts/*/tests/probe-*.sh`, and no phase artifact references a probe.

### Requirements Coverage

| Requirement | Source Plan | Status | Evidence |
|---|---|---|---|
| MCP-06 | 04-02, 04-06, 04-07 | ✓ SATISFIED | Truths 1, 8, 9, 20, 24 |
| MCP-07 | 04-03, 04-06, 04-07 | ✓ SATISFIED | Truths 2, 10-13, 21-23, 25 |
| STORE-05 | 04-01, 04-07 | ✓ SATISFIED | Truths 5, 26-28 |
| SEARCH-04 | 04-04, 04-05 | ✓ SATISFIED | Truths 3, 14-19. The previous gap is closed |
| SEARCH-05 | 04-04 | ✓ SATISFIED | Truth 4 |

No requirements are orphaned. REQUIREMENTS.md maps exactly these 5 IDs to Phase 4.

### Supersession Check (04-04 must-have #3)

The 04-04 must-have ("a malformed MATCH query is a 400/invalid_params in hybrid mode") is superseded, and the supersession is recorded explicitly in two places:

- `04-05-PLAN.md` § "Locked user decision": "…the 04-04 must-have 'A malformed MATCH query is a 400/invalid_params in hybrid mode' are **superseded** by this decision."
- `.planning/STATE.md` line 118 ([Phase 04] 04-05): "Supersedes the 04-04 decision that a malformed MATCH is InvalidQuery/400 in hybrid mode." The older line 117 is retained as history.

The code matches the decision. The service doc states "Search never returns `MemoryError::InvalidQuery`", and the README "Behavioral changes in v0.1.0" item 3 is rewritten for the forgiving leg.

### Deferred by Instruction (not gaps; confirmed recorded)

| Item | Recorded in |
|---|---|
| WR-04: unbounded `memory_list` with `expand_links` and no `limit` (behaviour unchanged; doc corrected) | 04-06-PLAN Deferred table; 04-06-SUMMARY "Deferred"; 04-07-SUMMARY "Deferred"; STATE.md [04-06] line 121 ("open API-contract question for Phase 5/6"); domain.rs:363-369 doc |
| WR-01 full cross-process exclusive lock | 04-07-PLAN Deferred table; 04-07-SUMMARY "Deferred"; STATE.md [04-07] line 124 |
| 03-REVIEW WR-01: filters applied after the KNN cut | 04-05/06/07-PLAN Deferred tables ("still deferred (not re-opened)"); 04-05-SUMMARY line 236; STATE.md [04-01] line 103 |
| 03-REVIEW WR-03: spawned REST test-harness hang | 04-05/06/07-PLAN Deferred tables; 04-05-SUMMARY line 236 |

The plans also record these deferrals, which are out of scope for this phase goal:

- internal-tier redaction on non-search surfaces;
- `deny_unknown_fields` on the v0.0.1 and link DTOs, per [03-01];
- stale `.backup-tmp-*` cleanup;
- IN-05 and IN-06 behaviour changes, which were documented instead.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|---|---|---|---|---|
| (gap-closure files, 14) | — | TBD/FIXME/XXX | none | Every "TODO" hit is the `TODO` memory-type domain value, not a debt marker |
| crates/agent-memory-core/src, crates/agent-memory/src | — | `unwrap()`/`expect()` before `#[cfg(test)]` | none | 0 hits |
| crates/agent-memory/src/mcp.rs, rest/handlers.rs | 40-44, 170-174 | Non-search internal errors still use `e.to_string()` | ℹ️ Info | Search paths are redacted (the decision's scope). Update/link/forget internal errors can still carry SQLite text on the loopback API. This deferral is recorded in the 04-05 plan |
| crates/agent-memory-core/src/store/sqlite.rs | 495-504 | FTS5 error classification matches message substrings | ℹ️ Info | Pinned by `every_bundled_fts5_parse_error_is_classified_as_match_input` against the bundled SQLite. A SQLite bump that rewords a message would fall to the redacted 500 tier, not leak, and the test would flag it |

### Human Verification Required

None. The previous decision item was resolved by the user (option-a) and is verified live.

### Gaps Summary

Both previous blockers are closed, and the code matches the user's option-a decision:

- **Gap #14:** the hyphen/colon 500 with leaked SQLite text is fixed.
- **Item #15:** punctuated natural-language recall is resolved.

How the fix works:

- **Shared retry helper.** One store helper, used by both the keyword-degrade path and the hybrid keyword leg, runs the raw FTS5 query first. On any FTS5 parse-class error, including the `no such column` class, it retries once with OR-ed quoted words. A tokenless query becomes an empty keyword leg.
- **Single redaction point.** Every search error is tiered at one point, so storage failures surface only as `SEARCH_STORAGE_FAILURE`.
- **Live confirmation.** All of this was confirmed on MCP and REST, with Ollama up and down.

The other two gap-closure plans also hold:

- **04-06:** the CR-03 conditional backfill and the symmetric `relates_to` behave as specified.
- **04-07:** the strict update DTOs, the uniform REST rejection bodies and the race-safe backup with tolerant concurrent open behave as specified. The concurrent first open was confirmed live across three rounds.

The regression checks on truths 1-13 found no regressions. The deferred items are each recorded explicitly.

One protocol-tier note (truth 24): over MCP, an unknown update field is reported as a tool error result rather than JSON-RPC `invalid_params`. This is rmcp's deserialization path and is recorded in STATE.md. The field is named and nothing is written.

---

_Verified: 2026-10-23T20:19:46Z_
_Verifier: Claude (gsd-verifier)_
