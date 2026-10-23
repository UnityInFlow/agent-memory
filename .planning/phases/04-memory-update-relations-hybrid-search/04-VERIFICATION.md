---
phase: 04-memory-update-relations-hybrid-search
verified: 2026-10-23T18:52:00Z
status: gaps_found
score: 13/15 must-haves verified
behavior_unverified: 0
overrides_applied: 0
gaps:
  - truth: "A malformed MATCH query is a 400/invalid_params in hybrid mode exactly as in keyword mode — the keyword leg's InvalidQuery propagates (plan 04-04 must-have #3; Phase 3 taxonomy: client input is never a 500)"
    status: failed
    reason: >-
      map_fts_query_error only recognises the substrings "fts5: syntax error" and "unterminated string".
      FTS5 reports column-filter-shaped input (a hyphen or a colon: `sqlite-vec`, `auth: tokens`) as
      "no such column: <x>", which falls through to MemoryError::Sqlite, so the result is MCP -32603
      internal_error / REST 500 and the raw SQLite message ("sqlite error: no such column: vec") leaks to the
      client. Keyword mode has had this since v1.0, but this phase sends every Ollama-up search through the
      keyword leg. v0.0.1 answered these queries semantically; HEAD returns a server fault. Observed live on
      both v0.0.1 (/opt/homebrew/bin/agent-memory) and the HEAD binary with a real Ollama.
    artifacts:
      - path: "crates/agent-memory-core/src/store/sqlite.rs"
        issue: "map_fts_query_error (line ~333) marker list misses the step-time 'no such column' FTS5 parse failure; it is used by keyword_candidates (hybrid leg) and search (keyword path)"
      - path: "crates/agent-memory-core/tests/hybrid.rs"
        issue: "a_malformed_match_is_invalid_query_in_hybrid_mode_exactly_as_in_keyword_mode covers only '\"unbalanced', 'AND', 'what is the plan?', with no hyphen or colon case"
    missing:
      - "Classify the step-time FTS5 'no such column' failure (the SQL is static, so at step time it can only come from the MATCH string) as MemoryError::InvalidQuery, or resolve it through the sanitisation option in the decision item below"
      - "Add 'sqlite-vec' and 'auth: tokens' to the malformed-query table test for both hybrid and keyword modes, plus a REST and MCP assertion that the tier is 400 / invalid_params with no 'sqlite error' text in the body"
human_verification:
  - test: "Decide the contract for natural-language queries when Ollama is up. Run memory_search with 'what vector store do we use?' (or any query containing ? , ' - :) against a store holding 'We chose sqlite-vec for local vector similarity search'"
    expected: "Developer decision. Today it returns invalid_params (400). v0.0.1 returned the right memory semantically. This was a deliberate planner decision (T-04-15), documented in README 'Behavioral changes in v0.1.0' item 3, but it cuts against the phase outcome 'I find the right memory even when the phrasing differs'. Options: (a) accept it as is and record an override; (b) sanitise or quote the keyword leg's input (e.g. tokenise into quoted OR'd terms) so natural-language punctuation can never fail the leg, which also fixes the gap above; (c) serve the KNN leg alone on a keyword-leg parse error (the plan rejected this as mode-probing)"
    why_human: "Two locked contracts conflict: the plan's T-04-15 classification parity against the roadmap goal's paraphrase-recall outcome. This is a product decision, not a code defect a grep can settle"
---

# Phase 4: Memory Update, Relations & Hybrid Search — Verification Report

**Phase Goal:** As an agent, I want to update existing memories, link related ones, and get hybrid keyword+semantic recall, so that my memory stays current and connected and I find the right memory even when the phrasing differs.
**Verified:** 2026-10-23T18:52:00Z
**Status:** gaps_found
**Re-verification:** No, initial verification

Evidence sources:
- Code read at HEAD `bff047f`, with a clean working tree under `crates/`.
- The phase test binaries (`update`, `relations`, `hybrid`, `migration_hygiene`, `import`): 56 passed, 0 failed, 1 ignored (the one-shot fixture generator).
- Orchestrator gates: fmt, clippy `-D warnings`, build, and `cargo test --workspace` (170 passed / 0 failed / 2 ignored).
- **Live sessions** of the HEAD binary over MCP stdio against a real local Ollama (`nomic-embed-text`) and against an unreachable URL.
- A **real v0.0.1 database** written by the installed Homebrew `agent-memory 0.0.1`, then upgraded by HEAD.

All scratch data lives in `/tmp/v04live-data` and `/tmp/v04real-data`. Nothing in the repo was modified.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC1: `memory_update` partial patch (content/tags/scope/TTL) changes the memory in place; a content update re-mirrors FTS and invalidates/re-embeds the vector in the same writer tx; an outage gives `embedding_status=0` plus sweep backfill; the old meaning stops matching; an unknown id gets a clean not-found | ✓ VERIFIED | `SqliteStore::update` (sqlite.rs:731-854) does one lock and one tx. It runs `UPDATE memories SET ...` in place (never DELETE+INSERT); the `memories_au` trigger re-mirrors FTS; `DELETE FROM vec_memories` plus a re-insert, or status 0, happens in the same tx. Tests pass: `content_update_re_embeds_so_the_old_meaning_stops_matching` (it uses 4 distractors and also checks that the FTS old word is gone), `embed_outage_during_update_falls_back_to_pending_and_sweep_backfills`, `updating_an_unknown_id_is_a_clean_not_found`. Live: the update returned the full view with `embedding_status:1`. A paraphrase query that ranked id 1 first fell to rank 3 after its content changed. id 999 returned `{"id":999,"reason":"not_found","updated":false}` |
| 2 | SC2: `memory_link`/`memory_unlink` handle flat typed relations; search and list can expand 1-hop; forget and TTL expiry cascade links with no orphans; no graph model | ✓ VERIFIED | 0003 `memory_links` has a PK of (from,to,kind), FK `ON DELETE CASCADE` on both columns, `CHECK from!=to`, and `idx_links_to`. `related()` is a single non-recursive 1-hop SQL query. Explicit orphan deletes exist in both `forget` and `sweep_expired` (lines 669, 956). There are 19 relations tests, including pragma-less orphan healing. Live: a link of 1→2 `supersedes` expanded with `superseded_by`/`in` on id 2. After `forget(1)`, the DB held 0 orphan edges |
| 3 | SC3: with Ollama available, search returns RRF-fused results (k=60, duplicate ids sum) labelled `hybrid`; with Ollama unreachable, it degrades to keyword exactly as v1.0 with an unchanged envelope | ✓ VERIFIED (fusion and degrade mechanics; see #15 for the query-class caveat) | `RRF_K = 60.0` and `rrf_fuse` (service.rs:78-105) use 1-based ranks with summed contributions. The goldens pass (2/62 beats 1/61, duplicate ids sum, id tie-break). Public `fn search` is **byte-identical** to the pre-phase `907161c` (diffed), and the service keyword arm differs only by opt-in expansion. Live with Ollama up: `search_mode: hybrid`, keys `[results, search_mode]`. Live with Ollama down on the same DB: `search_mode: keyword` with the same keys and one fallback warning per search |
| 4 | SC4: hybrid ranking is decay-aware (fresh beats stale at equal fused rank); access bumps go only to post-truncation returned ids | ✓ VERIFIED | Decay is multiplicative and applied once after fusion (service.rs:437-455). `at_equal_fused_rank_the_fresh_memory_outranks_the_stale_one` uses a genuine cross-rank tie (1/61 + 1/62 each), in both orientations, so id order cannot win. `only_the_returned_ids_are_bumped_never_the_fusion_candidates` uses 20 candidates in both legs at limit 5 and finds exactly the 5 returned rows bumped. `spawn_bump` is called after `take(limit)` and before expansion |
| 5 | SC5: upgrading a real v0.0.1 DB applies 0003 only after a pre-migration backup; 0001/0002 SQL provably unchanged; a newer-schema DB gets a friendly error, not a crash | ✓ VERIFIED | Probe, then refuse, then pragmas, then backup, then migrate (sqlite.rs:224-252). `git diff v0.0.1` on 0001/0002 is empty. The fixture is committed, and the divergence test compares normalized `sqlite_master`. **Live with the real v0.0.1 binary:** the DB written by Homebrew 0.0.1 was opened by HEAD and produced `real.db.backup-pre-0003-20261023` (user_version=2, 2 rows, no memory_links); the live DB went to user_version=3 with data intact, and hybrid search, link and update all worked. **Live too-new:** `user_version=99` gave exit 1, 0 bytes on stdout, and the D-13 message on stderr naming 99, 3 and the brew command. The DB's shasum was unchanged |
| 6 | Re-importing a `source: None` draft is idempotent (CR-01) | ✓ VERIFIED | `source IS ?1` (sqlite.rs:496); `.as_deref()` in service import; test `reimporting_a_sourceless_draft_is_idempotent` passes |
| 7 | A poisoned writer lock is 500/internal_error, never 404 (WR-02) | ✓ VERIFIED | 9 `Internal("writer lock poisoned")` sites, including the new link/unlink/update writers. `MemoryError::NotFound`/`ApiError::NotFound` have 0 references. Mapper tier unit tests pass (full suite) |
| 8 | Merge-patch semantics: absent = unchanged, null = clear (ttl/tags/scope), content null or empty = 400 | ✓ VERIFIED | `validate_patch` runs before the embed. Matrix tests at core (`tags_and_scope_patch_matrix_absent_null_value`, `ttl_patch_matrix_...`) and on both transports from raw JSON (mcp.rs:669/701/753, handlers.rs PATCH tests). Patch DTOs set `skip_serializing_if`, so the schema carries no `default: null` |
| 9 | The update response is the full post-update view plus `embedding_status`, and the update bumps `last_accessed` | ✓ VERIFIED | `UpdatedMemory` flattens `MemoryView` and adds `embedding_status`. The UPDATE statement folds in `last_accessed = now, access_count + 1`. Tests: `update_bumps_freshness_by_exactly_one_access`, `update_returns_the_full_post_update_view_with_embedding_status`. Live response included `embedding_status` |
| 10 | Link edge cases: a duplicate is an idempotent no-op; a self-link is 400; an unknown kind is 400 listing the allowed values; a missing endpoint is a not-found naming the id | ✓ VERIFIED | `INSERT OR IGNORE`; endpoint pre-check inside the tx (no FK-violation 500); `validate_link_endpoints` at the seam. Relations tests and MCP/REST transport tests pass |
| 11 | Expansion covers both directions with natural reverse labels and a cap of `MAX_EXPANDED_LINKS = 10`, most recent first | ✓ VERIFIED | `related()` uses `ORDER BY l.created_at DESC ... LIMIT ?2`. Tests: `expansion_is_capped_and_the_most_recent_edges_win`, `expansion_follows_one_edge_both_ways_with_natural_labels`. Live: `supersedes`/`out` and `superseded_by`/`in` |
| 12 | With `expand_links` absent, the search and list JSON is byte-identical to v1.0 (no `related` key) | ✓ VERIFIED | `#[serde(skip_serializing_if = "Option::is_none")]` on `related`. Golden test `flag_less_envelopes_are_byte_identical_to_v1_and_never_mention_related`. Live: flag-less results carried no `related` |
| 13 | Expanded neighbors never receive access bumps | ✓ VERIFIED | Expansion runs after `spawn_bump(returned ids)`. Tests `expanded_neighbors_are_never_access_bumped` and `hybrid_expansion_runs_after_the_bump_and_never_bumps_neighbors` pass |
| 14 | A malformed MATCH is 400/invalid_params in hybrid mode exactly as in keyword mode (plan 04-04 must-have) | ✗ FAILED | Holds for the `fts5: syntax error` / `unterminated string` class only. **Live HEAD with Ollama up:** `memory_search {"query":"sqlite-vec"}` returned `-32603 internal_error "sqlite error: no such column: vec"`. The REST mapper sends `MemoryError::Sqlite` to 500. Classification parity with keyword mode holds (both modes give 500), but the 400 tier does not, and the SQL detail leaks. **Live v0.0.1 with Ollama up:** the same query returned the right memory (`search_mode: semantic`), so on the Ollama-up path this is a regression |
| 15 | Goal outcome: "I find the right memory even when the phrasing differs" (derived from the user story's so-that clause) | ? UNCERTAIN (decision needed) | Paraphrases without punctuation work live (for example, "which datastore keeps client information" finds "Postgres is our primary relational database for customer records" at rank 1 with no keyword overlap). Paraphrases with ordinary punctuation fail: `what vector store do we use?` gives invalid_params at HEAD, while v0.0.1 found the right memory. This is deliberate (T-04-15) and documented in the README, but it conflicts with the outcome clause. See Human Verification |

**Score:** 13/15 truths verified (0 present-but-behavior-unverified, 1 failed, 1 uncertain)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/agent-memory-core/sql/0003_relations.sql` | memory_links schema | ✓ VERIFIED | PK(from,to,kind), 2× ON DELETE CASCADE, CHECK, WITHOUT ROWID, idx_links_to |
| `crates/agent-memory-core/src/store/migrations.rs` | 0003 in chain + `LATEST_SCHEMA_VERSION` | ✓ VERIFIED | `= 3`, with a test that the const equals what `to_latest` writes |
| `crates/agent-memory-core/tests/migration_hygiene.rs` | divergence/backup/too-new tests | ✓ VERIFIED | 357 lines, 7 tests plus an ignored generator; the generator refuses to overwrite an existing fixture |
| `crates/agent-memory-core/tests/fixtures/v0.0.1.db` | committed fixture | ✓ VERIFIED | git-tracked, user_version=2, unmodified in the working tree |
| `crates/agent-memory-core/src/domain.rs` | Internal, SchemaTooNew, MemoryPatch, UpdatedMemory, LinkKind, RelatedLink, MAX_EXPANDED_LINKS | ✓ VERIFIED | all present and used |
| `crates/agent-memory-core/tests/update.rs` | MCP-06 tests | ✓ VERIFIED | 674 lines, 9 tests |
| `crates/agent-memory/src/serde_util.rs` | explicit-null helper | ✓ VERIFIED | used by UpdateArgs and UpdateBody |
| `crates/agent-memory-core/tests/relations.rs` | MCP-07 tests | ✓ VERIFIED | 737 lines, 19 tests |
| `crates/agent-memory-core/tests/hybrid.rs` | SEARCH-04/05 tests | ✓ VERIFIED (coverage hole: no hyphen or colon query) | 719 lines, 19 tests |
| `crates/agent-memory-core/src/store/sqlite.rs` | update / link / related / keyword_candidates / backup | ✓ VERIFIED (except the `map_fts_query_error` gap) | see truths 1, 2, 5, 14 |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| migrations.rs | 0003_relations.sql | `M::up(include_str!)` | ✓ WIRED | line 24 |
| sqlite.rs open() | rusqlite Backup API | `run_to_completion` | ✓ WIRED | line 181; feature `backup` on the pinned 0.39 |
| sqlite.rs open() | PRAGMA user_version | probe before pragmas and migrations | ✓ WIRED | line 233 |
| service.rs import | Store::exists | `draft.source.as_deref()` | ✓ WIRED | line 338 |
| mcp.rs | MemoryService::update/link/unlink | tool arms | ✓ WIRED | mcp.rs:374/400/438; exercised live |
| rest/mod.rs | update/link/unlink handlers | PATCH `/api/memories/{id}`, POST `/api/links`, DELETE `/api/links/{from}/{to}/{kind}` | ✓ WIRED | rest/mod.rs:77-85 |
| service.rs | Store::related | `expand_related` after the bump | ✓ WIRED | search (both arms) and list |
| sqlite.rs forget/sweep | memory_links | belt-and-braces orphan DELETE | ✓ WIRED | lines 669, 956 |
| service.rs | Store::keyword_candidates + knn_search | `tokio::join!`, keyword error checked first | ✓ WIRED | lines 423-428 |
| service.rs | decay_score | post-fusion multiplicative blend | ✓ WIRED | line 443 |
| sqlite.rs keyword_candidates | map_fts_query_error | InvalidQuery propagation | ⚠️ PARTIAL | wired, but the mapper misclassifies the `no such column` FTS5 failure (gap 1) |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `SearchOutcome.results` (hybrid) | fused views | `vec_memories` KNN plus `memories_fts` bm25, read pool | yes (live: real rows ranked) | ✓ FLOWING |
| `MemoryView.related` | RelatedLink list | `memory_links JOIN memories` | yes (live) | ✓ FLOWING |
| `UpdatedMemory` | post-update row | re-read inside the writer tx | yes (live) | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Phase test binaries | `cargo test -p agent-memory-core --test update --test relations --test hybrid --test migration_hygiene --test import` | 56 passed, 0 failed, 1 ignored | ✓ PASS |
| Live hybrid with a real Ollama (executor human-check, part 1) | MCP stdio `serve` against `/tmp/v04live-data/live.db`, Ollama at localhost:11434 | paraphrase gives `search_mode: hybrid`, correct memory at rank 1 | ✓ PASS |
| Live degrade (executor human-check, part 2) | same DB, `--ollama-url http://127.0.0.1:9` | `search_mode: keyword`, envelope keys identical, one fallback WARN per search | ✓ PASS |
| Real v0.0.1 upgrade | Homebrew 0.0.1 writes 2 rows, then HEAD opens | backup user_version=2 with 2 rows; live DB user_version=3; link/update/hybrid OK | ✓ PASS |
| Too-new refusal through the binary | `--db future.db` (user_version=99) | exit 1, empty stdout, D-13 message on stderr, shasum unchanged | ✓ PASS |
| Hyphenated query, Ollama up | `memory_search {"query":"sqlite-vec"}` | `-32603 sqlite error: no such column: vec` (v0.0.1: correct semantic hit) | ✗ FAIL |
| `?` query, Ollama up | `memory_search {"query":"what vector store do we use?"}` | `-32602 invalid_params` (v0.0.1: correct semantic hit) | ? decision |
| Normal build excludes test-clock | `cargo tree -p agent-memory -e normal -i agent-memory-core -f '{p} {f}'` | no features | ✓ PASS |

The executor's end-of-phase manual check (`<human-check>` in plan 04-04 Task 2) was performed by the verifier against a real local Ollama. The hybrid and degrade observations above satisfy it.

### Probe Execution

None declared. No `scripts/*/tests/probe-*.sh` exists, and no phase artifact references a probe.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| STORE-05 | 04-01 | 0003 with migration hygiene (frozen SQL, backup, newer-DB error) | ✓ SATISFIED | truth 5, live real-v0.0.1 upgrade |
| MCP-06 | 04-02 | memory_update, same-tx re-mirror and re-embed, outage fallback, not-found | ✓ SATISFIED | truths 1, 8, 9 |
| MCP-07 | 04-03 | link/unlink, 1-hop expansion, cascade, no graph model | ✓ SATISFIED | truths 2, 10-13 |
| SEARCH-04 | 04-04 | RRF k=60 hybrid, keyword degrade unchanged | ⚠️ SATISFIED WITH GAP | fusion and degrade verified (truth 3); hybrid now exposes the pre-existing 500 misclassification (truth 14) and the punctuation decision (truth 15) |
| SEARCH-05 | 04-04 | post-fusion decay, bumps on returned ids only | ✓ SATISFIED | truth 4 |

There are no orphaned requirements. REQUIREMENTS.md maps exactly these 5 IDs to Phase 4, and each is claimed by a plan.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| crates/agent-memory-core/src/store/sqlite.rs | ~333 | FTS5 error classifier matches message substrings and misses `no such column` | 🛑 Blocker | client input reaches the 500 tier with SQL detail on the now-default Ollama-up path (gap 1) |

The scan of all 19 phase-modified files found no unreferenced TBD/FIXME/XXX markers, no TODO/HACK/placeholder text, and no `unwrap()`/`expect()` in non-test source.

### Human Verification Required

#### 1. Natural-language punctuation in hybrid queries (decision)

**Test:** With Ollama up, run `memory_search` with `what vector store do we use?` against a store containing "We chose sqlite-vec for local vector similarity search".
**Expected:** A developer decision on the contract. Today the query returns invalid_params; v0.0.1 found the memory. Option (b), sanitising or quoting the keyword leg's input, would restore paraphrase recall and also close gap 1. Option (a) needs an override entry.
**Why human:** Plan decision T-04-15 (identical classification in every mode, so the mode cannot be probed) conflicts with the roadmap outcome clause. That is a product call.

### Gaps Summary

The phase is substantively delivered. Update, relations, RRF fusion, decay blending, bump scoping and migration hygiene are all real, wired, test-locked, and confirmed live. That includes an upgrade of a database written by the real v0.0.1 binary and a live hybrid-to-keyword degrade against a real Ollama.

One gap blocks a clean pass, and it shares a root cause with the decision item. Hybrid search now always parses the query through FTS5, even when Ollama is up. Two consequences follow:

1. **(Gap, fixable in one line plus tests.)** Hyphen- or colon-shaped queries (`sqlite-vec`, `auth: tokens`) produce FTS5's `no such column` error. `map_fts_query_error` does not recognise it, so it surfaces as a 500 / internal_error with the SQLite message. In v0.0.1 these queries worked whenever Ollama was up.
2. **(Decision.)** Queries with `?`, `,` or `'` are classified correctly as client errors, but that means ordinary paraphrased questions no longer return results with Ollama up.

Sanitising the keyword leg's input would resolve both at once. If the developer judges gap 1 pre-existing and out of scope, add this override to the frontmatter:

```yaml
overrides:
  - must_have: "A malformed MATCH query is a 400/invalid_params in hybrid mode exactly as in keyword mode"
    reason: "no-such-column misclassification is pre-existing v1.0 keyword-path behavior; tracked for a follow-up"
    accepted_by: "<name>"
    accepted_at: "<ISO timestamp>"
```

---

_Verified: 2026-10-23T18:52:00Z_
_Verifier: Claude (gsd-verifier)_
