---
phase: 04-memory-update-relations-hybrid-search
plan: 04
subsystem: search
tags: [rust, sqlite, fts5, sqlite-vec, rrf, hybrid-search, decay, rmcp, axum, mcp]

# Dependency graph
requires:
  - phase: 04-memory-update-relations-hybrid-search
    provides: "04-01: MAX_KNN_K = MAX_LIMIT * 4 (800), the 4x oversample at every limit. 04-02: update bumps decay, and the merge-patch schema hazard (default: null) to avoid. 04-03: expand_links on SearchArgs, plus the truncate -> bump returned ids -> expand order, with the flag captured before args move into the store"
provides:
  - "Store::keyword_candidates(args): bm25-ordered pure-relevance keyword leg (ORDER BY bm25 ASC, id ASC), oversampled limit*4 capped at MAX_KNN_K, same filters and map_fts_query_error routing as search"
  - "pub const RRF_K: f64 = 60.0 and pub fn rrf_fuse(&[Vec<i64>]) -> Vec<(i64, f64)> (1-based ranks, duplicate ids sum, total_cmp desc + ascending-id tie-break)"
  - "MemoryService::hybrid_candidates: concurrent legs via tokio::join!, keyword-first error propagation, RRF fusion, multiplicative post-fusion decay applied once"
  - "SearchMode::Hybrid (wire \"hybrid\"); SearchMode::Semantic removed (no producer left)"
  - "crates/agent-memory-core/tests/hybrid.rs: 19 tests (fusion goldens, oversample recall on both legs, SEARCH-05 goldens, the full degrade decision table, bump scope, hybrid expansion)"
  - "memory_search tool description and README search-mode table for 'hybrid'|'keyword'; 'Behavioral changes in v0.1.0' item 3 for the DIST-06 release notes"
affects: [05 export/import (search_mode values for any import/export tooling), 06 release notes (DIST-06 picks up README behavioral change 3), future SEARCH-06 request-level mode override]

# Actuals (#2632): chars/4 over the added lines of the realized diff (8903ee6..HEAD, code commits only)
actuals:
  tokens: 12977
  tasks: 2
  commits: 4

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Rank fusion over pure-relevance legs: each leg orders by relevance only, and decay is applied once, multiplicatively, after fusion"
    - "Concurrent blocking legs: two spawn_blocking futures under tokio::join!, with errors checked in a fixed order (the input-parsing leg first) so classification is deterministic"
    - "Degrade-path byte-compat test: call the public Store::search directly before the service search, then assert JSON equality of the whole envelope"
    - "Rank-controlled fixtures: query vector = axis 0, each row a chosen cosine similarity on a private axis, bm25 order set by term frequency and doc length"
    - "Leg-failure injection without data loss: ALTER TABLE ... RENAME the leg's table on a temp DB"

key-files:
  created:
    - crates/agent-memory-core/tests/hybrid.rs
  modified:
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory-core/src/store/mod.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory-core/tests/semantic.rs
    - crates/agent-memory-core/tests/update.rs
    - crates/agent-memory-core/tests/relations.rs
    - crates/agent-memory/src/mcp.rs
    - crates/agent-memory/src/rest/handlers.rs
    - crates/agent-memory/Cargo.toml
    - README.md

key-decisions:
  - "RRF discretion (CONTEXT): 1-based ranks; legs fused in the order [KNN, keyword]; descending total_cmp with ascending-id tie-break, both in rrf_fuse and after the decay blend"
  - "Post-fusion decay is multiplicative (final = rrf_score x decay_score), applied once, pinned-aware; each view's wire decay_score is that same value"
  - "keyword_candidates computes its own oversample (limit*4, cap MAX_KNN_K), exactly like knn_search, instead of taking a k argument, so no bare k crosses a function boundary (Pitfall 2)"
  - "Leg errors: the keyword leg's result is checked before the KNN leg's. Only the keyword leg parses the query, so a malformed MATCH is always InvalidQuery in hybrid mode, the same classification as keyword mode (T-04-15)"
  - "The keyword leg adds m.id ASC as a bm25 tie-break, for deterministic ranks"
  - "rrf_fuse and RRF_K are pub so tests/hybrid.rs can unit-test them against hand-computed values"
  - "The bin crate's dev-dependency enables core's test-clock feature, so the in-process MCP/REST tests use the real FakeEmbedder. cargo tree confirms it is absent from the normal build graph"
  - "tests/rest.rs left untouched: it had no 'semantic' assertions, and the optional WR-03 harness ride-along was skipped because the suite stayed stable"

patterns-established:
  - "A new search leg must be a pure-relevance ordering, oversampled like knn_search, and must route MATCH errors through map_fts_query_error"

requirements-completed: [SEARCH-04, SEARCH-05]

coverage:
  - id: D1
    description: "rrf_fuse matches hand-computed RRF: rank 2 in both legs (2/62) beats rank 1 in one leg (1/61); duplicate ids sum; ties break by ascending id regardless of hash order; empty legs give []; RRF_K == 60"
    requirement: SEARCH-04
    verification:
      - kind: unit
        ref: "crates/agent-memory-core/tests/hybrid.rs#rrf_dual_leg_rank_two_beats_single_leg_rank_one / rrf_duplicate_ids_sum_their_contributions / rrf_ties_break_deterministically_by_ascending_id / rrf_of_empty_legs_is_empty / rrf_k_is_the_cormack_clarke_constant"
        status: pass
    human_judgment: false
  - id: D2
    description: "With the embedder up, a semantic-only hit and a keyword-only hit both surface in fused order, labeled search_mode hybrid (wire string \"hybrid\")"
    requirement: SEARCH-04
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/hybrid.rs#hybrid_surfaces_the_top_semantic_and_the_top_keyword_hit / hybrid_wire_label_is_lowercase_hybrid"
        status: pass
    human_judgment: false
  - id: D3
    description: "Both legs are oversampled (limit*4): a keyword hit at KNN rank 5 and a KNN hit at keyword rank 3 (limit 2) each enter fusion and win. Mutation-verified: an un-oversampled keyword leg fails the test"
    requirement: SEARCH-04
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/hybrid.rs#semantic_leg_is_oversampled_beyond_limit / keyword_leg_is_oversampled_beyond_limit"
        status: pass
    human_judgment: false
  - id: D4
    description: "SEARCH-05 golden: at exactly equal fused RRF score the fresh memory strictly outranks the stale one, in both orientations and against the id tie-break. A stale pinned DECISION keeps its pinned advantage, and the window is chosen so double decay, ignored pinning or an additive blend would each flip the order. Mutation-verified for all three"
    requirement: SEARCH-05
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/hybrid.rs#at_equal_fused_rank_the_fresh_memory_outranks_the_stale_one / a_stale_pinned_memory_keeps_its_pinned_advantage_and_decay_applies_once"
        status: pass
    human_judgment: false
  - id: D5
    description: "Emptiness is not a fallback: an empty corpus, or filters that exclude everything, return [] with search_mode still hybrid"
    requirement: SEARCH-04
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/hybrid.rs#empty_corpus_is_an_empty_hybrid_answer / filters_that_exclude_everything_stay_hybrid"
        status: pass
    human_judgment: false
  - id: D6
    description: "Degrade rows: an embed error and an empty embed batch both return the v1.0 keyword path, with the whole JSON envelope byte-identical to a direct Store::search plus on-read decay, mode keyword"
    requirement: SEARCH-04
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/hybrid.rs#embed_error_degrades_to_the_unchanged_v1_keyword_path / an_empty_embed_batch_degrades_to_the_unchanged_v1_keyword_path"
        status: pass
    human_judgment: false
  - id: D7
    description: "Error taxonomy in hybrid mode: a malformed MATCH (unbalanced quote, dangling AND, bare '?') is InvalidQuery exactly as in keyword mode, and REST 400 / MCP invalid_params with a live embedder. A broken FTS or vec leg propagates MemoryError::Sqlite and never serves the surviving leg. Mutation-verified: swallowing the keyword error fails both tests"
    requirement: SEARCH-04
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/hybrid.rs#a_malformed_match_is_invalid_query_in_hybrid_mode_exactly_as_in_keyword_mode / an_internal_error_in_either_leg_propagates_and_never_serves_the_other_leg"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/rest/handlers.rs#a_malformed_query_is_400_in_hybrid_mode_too_never_a_semantic_200"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/mcp.rs#memory_search_malformed_query_is_invalid_params_in_hybrid_mode"
        status: pass
    human_judgment: false
  - id: D8
    description: "Bump scope: 20 rows that are candidates in both legs, limit 5, so exactly the 5 returned rows are bumped (once each) and 15 are untouched. On the hybrid path the expanded neighbor is never bumped and the flag-less envelope has no related member. Mutation-verified: bumping all candidates fails both tests"
    requirement: SEARCH-05
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/hybrid.rs#only_the_returned_ids_are_bumped_never_the_fusion_candidates / hybrid_expansion_runs_after_the_bump_and_never_bumps_neighbors"
        status: pass
    human_judgment: false
  - id: D9
    description: "Transport wire contract: in-process REST and MCP search return \"hybrid\" with a live FakeEmbedder and \"keyword\" with the dead loopback embedder, with the identical two-key envelope. The memory_search description enumerates exactly 'hybrid'|'keyword' with k=60"
    requirement: SEARCH-04
    verification:
      - kind: integration
        ref: "crates/agent-memory/src/rest/handlers.rs#search_reports_hybrid_with_a_live_embedder_and_keyword_with_a_dead_one"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/mcp.rs#memory_search_reports_hybrid_when_the_embedder_is_up_and_keyword_when_down / memory_search_description_enumerates_exactly_hybrid_and_keyword"
        status: pass
    human_judgment: false
  - id: D10
    description: "README: search-mode table (hybrid/keyword, RRF k=60, sums, post-fusion decay), bump-scope, query-syntax and filter-bound notes, and Behavioral changes in v0.1.0 item 3 (semantic -> hybrid)"
    requirement: SEARCH-04
    verification: []
    human_judgment: true
    rationale: "Documentation clarity and accuracy is a reading judgment; no test asserts README prose"
  - id: D11
    description: "Live Ollama end-to-end: a paraphrased memory_search returns hybrid with sensible fused ordering; stopping Ollama degrades the same query to keyword with an unchanged envelope"
    requirement: SEARCH-04
    verification: []
    human_judgment: true
    rationale: "Needs a real local Ollama (the 04-VALIDATION.md manual-only row, end-of-phase human check); FakeEmbedder covers the logic in CI"

# Metrics
duration: 11min
completed: 2026-10-23
status: complete
---

# Phase 4 Plan 04: Decay-aware hybrid search (RRF k=60) Summary

**With the embedder up, `memory_search` now fuses two pure-relevance rankings, KNN by cosine distance and FTS5 by bm25.** Fusion is Reciprocal Rank Fusion: k = 60, 1-based ranks, and an id found by both rankings sums both contributions. Each fused score is then multiplied by the memory's pinned-aware decay score, exactly once, and the result is reported as `search_mode: "hybrid"`. With the embedder down, search takes the v1.0 keyword path, and its output is byte-identical to a direct `Store::search`. A malformed MATCH is a 400 / `invalid_params` in both modes, and only the returned ids are ever bumped.

## Performance

- **Duration:** ~11 min
- **Started:** 2026-10-23T18:27:36Z
- **Completed:** 2026-10-23T18:38:55Z
- **Tasks:** 2 / 2
- **Files modified:** 11 (1 created, 10 modified)

## Accomplishments

- **Phase success criteria 3 and 4 hold.** An exact-keyword hit and a semantic paraphrase hit both surface. Agreement between the two rankings is rewarded (2/62 beats 1/61). At equal fused rank the fresher memory wins, and the envelope is unchanged in every mode.
- **Decay is applied once, by construction.** Both legs are pure relevance: `knn_search` as-is, and the new `keyword_candidates` with `ORDER BY bm25(memories_fts) ASC, m.id ASC` and no decay term. Decay enters only in `hybrid_candidates`, as `rrf_score × decay_score(...)`. The pinned golden test sits in a window where decay applied twice, pinning ignored, or an additive blend would each flip the order, and hand mutations of all three failed it.
- **Oversampling on both legs is proven.** Each leg fetches `limit × 4`, capped at `MAX_KNN_K` (800). Two fixtures place the winning row beyond `limit` in one leg; the oversampled leg brings it into fusion, and without oversampling it would lose the id tie-break.
- **The degrade decision table is test-locked row by row:**
  - fuse → hybrid;
  - both legs empty → `[]`, still hybrid;
  - embed error or empty batch → v1.0 keyword output, byte-identical JSON;
  - malformed MATCH → InvalidQuery, the same in both modes;
  - internal error in either leg → `Sqlite`, never the other leg's rows.
- **The 04-03 carry-forward is honored.** The order is truncate, `spawn_bump(returned ids)`, then `expand_related`, and `expand_links` and `limit` are captured before `args` moves into the legs. The 20-row / limit-5 kill test bumps exactly 5 rows. On the hybrid path the expanded neighbor stays at `access_count 0`.
- **The wire and the docs agree.** The `memory_search` description reads `'hybrid'|'keyword'` with RRF k=60, the keyword degrade, FTS5 query syntax, the WR-01 filter-after-cut bound and returned-only bumps. The README has the search-mode table and "Behavioral changes in v0.1.0" item 3. No MCP param struct was touched, so the 04-02 `default: null` schema hazard does not arise.

## Task Commits

1. **Task 1: RRF fusion over two pure-relevance legs + SearchMode::Hybrid + post-fusion decay**: `024bcd4` (feat)
2. **Task 2: degrade decision table, error propagation, bump scope, wire/docs**: `7c63d0e` (test)

**Plan metadata:** this SUMMARY commit, then a `docs(04-04)` state/roadmap commit.

_TDD note:_
- Task 1's RED was observed before implementation: `tests/hybrid.rs` failed to compile with 8 errors (missing `rrf_fuse`, `RRF_K`, `SearchMode::Hybrid`).
- Task 2's tests are characterization locks over behaviour Task 1 had already implemented, so they passed on first run. They were proven to bite by hand mutation instead:
  - swallowing the keyword-leg error failed the InvalidQuery and internal-error tests;
  - bumping every fusion candidate failed both bump tests.
- Task 1 was also mutation-checked: double decay, an additive blend, ignored pinning and an un-oversampled keyword leg each failed a test.
- All mutations were reverted. RED was not committed separately, because CLAUDE.md and the operator rules forbid committing failing tests (same as 04-01 to 04-03).

## Files Created/Modified

- `crates/agent-memory-core/tests/hybrid.rs` (new, 719 lines, 19 tests):
  - RRF unit goldens;
  - e2e fusion and the wire label;
  - oversample recall on both legs;
  - the SEARCH-05 equal-rank and pinned goldens;
  - emptiness;
  - the degrade table (embed error, empty batch, malformed MATCH, broken FTS leg, broken vec leg);
  - bump scope and hybrid expansion.
- `crates/agent-memory-core/src/service.rs`:
  - `RRF_K` and `rrf_fuse`;
  - `SearchMode::Hybrid` replaces `Semantic`;
  - the embed-OK arm now calls `hybrid_candidates`, then truncates, bumps and expands;
  - doc updates.
  - The `other =>` keyword arm is untouched.
- `crates/agent-memory-core/src/store/mod.rs`: the `keyword_candidates` trait method and its contract docs.
- `crates/agent-memory-core/src/store/sqlite.rs`: the `keyword_candidates` impl, a pure insertion after `fn search` (whose body is unchanged).
- `crates/agent-memory-core/tests/{semantic,update,relations}.rs`: `SearchMode::Semantic` → `SearchMode::Hybrid` (8 assertions).
- `crates/agent-memory/src/mcp.rs`: the `memory_search` description, `live_server` / `server_with`, and 3 tests.
- `crates/agent-memory/src/rest/handlers.rs`: `live_state` / `state_with` / `search_request`, and 2 tests.
- `crates/agent-memory/Cargo.toml`: the dev-dependency enabling core's `test-clock` feature.
- `README.md`: the feature blurb, the tools-table row, the `memory_search` mode table and notes, the Ollama section, the `memory_update` note, and behavioral change 3.

## Decisions Made

See `key-decisions` in the frontmatter. In short:
- ranks are 1-based, with an id tie-break everywhere;
- the decay blend is multiplicative and applied once;
- `keyword_candidates` owns its oversample;
- the keyword leg's error is checked first, for deterministic classification;
- the public `rrf_fuse` / `RRF_K` exist for tests;
- the `test-clock` dev feature supplies the real `FakeEmbedder` in transport tests.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical] Tests beyond the listed behaviours**
- **What was added:**
  - the empty-embed-batch degrade row;
  - a broken-leg internal-error row for each leg;
  - a hybrid-path test that expansion never bumps neighbors (the 04-03 carry-forward);
  - transport tests showing a malformed query is 400 / `invalid_params` with a live embedder;
  - a test locking the tool description's exact mode enumeration.
- **Why:** these rows of the RESEARCH decision table and the acceptance criteria (400 / `invalid_params` at both transports) were otherwise only implied.
- **Commit:** 7c63d0e

**2. [Rule 3 - Blocking] Test files outside the plan's file list**
- **Issue:** removing `SearchMode::Semantic` breaks every test that names it, including `tests/update.rs` and `tests/relations.rs`, which the plan did not list. The plan's "any transport tests" covered the intent.
- **Fix:** each assertion was changed to `SearchMode::Hybrid`. The result-content assertions were unchanged and still hold.
- **Commit:** 024bcd4

**3. [Rule 3 - Blocking] `crates/agent-memory/Cargo.toml` dev-dependency feature**
- **Issue:** the plan asks for a live, FakeEmbedder-backed state in the bin crate's in-process tests, but `FakeEmbedder` is gated behind core's `test-clock` feature.
- **Fix:** one `[dev-dependencies]` line re-declares the existing path dependency with `features = ["test-clock"]`. No new package. `cargo tree -e normal,features` shows `test-clock` absent from the normal graph.
- **Commit:** 7c63d0e

**4. [Plan wording] `keyword_candidates(args)`, not `(args, k)`**
- The artifacts list sketches a `k` argument. The action text says to mirror `knn_search`'s oversample expression, and Pitfall 2 forbids passing a bare `k`, so the method computes `limit*4` capped at `MAX_KNN_K` internally, exactly as `knn_search` does. "Identical parameter positions" was read as "the same bound-parameter filter block": positions are `?1..?5`, because the `search` positions `?4..?9` carry weights that this leg does not have.
- **Commit:** 024bcd4

**5. [Environment] A project hook blocks `DROP` in Bash commands**
- `.claude/hooks/pre-bash-rust.sh` rejected the command that wrote the broken-leg test, because the test SQL contained `DROP TABLE`. Rather than route around the guard, the test breaks each leg non-destructively with `ALTER TABLE <leg> RENAME TO <leg>_offline` on its temp DB.
- **Commit:** 7c63d0e

---

**Total deviations:** 3 auto-fixed (1 missing-critical, 2 blocking), plus 2 wording/environment adaptations.
**Impact on plan:** no scope creep, no new crates, no workflow changes.

## Issues Encountered

- **User-facing consequence of the locked T-04-15 rule (please read).** FTS5 rejects bare punctuation: `what is the plan?`, `don't` and `a, b` are all `fts5: syntax error` (checked against SQLite directly). Because hybrid now runs the keyword leg on every query and must not swallow its `InvalidQuery`, a natural-language query with punctuation that v0.0.1 answered semantically while Ollama was up is now a 400 / `invalid_params`. This is what the plan, the must-haves and the threat model lock, and the MCP `query` param was already documented as "FTS5 MATCH syntax". It is recorded in the tool description and in README behavioral change 3, and a test locks it. If agents routinely send punctuated questions, a follow-up should decide whether to sanitize the keyword leg's input (for example, quote each token). That would be a contract change to discuss, not a ride-along.
- The plan's "one warn logged" for the degrade row is not asserted, because the core crate has no tracing-capture dev-dependency. The warn sits in the untouched keyword arm.
- One stale word remains: a comment in the untouched keyword arm still says "Same order as the semantic leg". It was kept so the arm stays byte-identical, as the plan requires.

## Verification Results

- `cargo test -p agent-memory-core --test hybrid`: **19 passed**.
- `cargo test -p agent-memory --bins`: **53 passed** (48 before this plan).
- `cargo test --workspace`: **170 passed / 0 failed / 2 ignored** (04-03 baseline: 146 / 0 / 2).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, with both crates re-checked.
- `cargo fmt --all --check`: clean.
- Acceptance greps:
  - `const RRF_K: f64 = 60.0` present;
  - `fn keyword_candidates` present;
  - `bm25(memories_fts) ASC` present, with no decay term in that ORDER BY;
  - `SearchMode::Semantic` count 0;
  - `'semantic'` in `mcp.rs` count 0;
  - `hybrid` in the `memory_search` description;
  - README behavioral change 3 present;
  - the mode table lists `"hybrid"` and `"keyword"`;
  - the `sqlite.rs` diff is one hunk inserted after `fn search` (`@@ -601,0 +602,43 @@`).
- Hand mutations (6), all reverted: double decay, additive blend, ignored pinning, un-oversampled keyword leg, swallowed keyword error, bumping all candidates. Each failed at least one test.

## User Setup Required

None. Optional end-of-phase manual check (the 04-VALIDATION.md manual-only row): with a local Ollama running, a paraphrased `memory_search` should report `"hybrid"`; stop Ollama and the same query should degrade to `"keyword"` with the same envelope.

## Next Phase Readiness

- **Phase 4:** all four plans are complete and the phase is ready for verification. The orchestrator marks it complete.
- **Phase 5 (export/import):** search is unaffected by import ids. Note that `search_mode` now has only two values.
- **Phase 6 (release notes, DIST-06):** README "Behavioral changes in v0.1.0" now has three items. Item 3 covers semantic → hybrid and the punctuation consequence.
- **Still open, outside this plan:**
  - WR-01: full filter-aware KNN. Documented as a bound, not fixed.
  - WR-03: the spawned-binary REST harness hang. Not touched; the suite was stable.
  - The punctuation-query consequence above, as a possible follow-up decision.

---
*Phase: 04-memory-update-relations-hybrid-search*
*Completed: 2026-10-23*

## Self-Check: PASSED

- FOUND: crates/agent-memory-core/tests/hybrid.rs
- FOUND: crates/agent-memory-core/src/service.rs
- FOUND: crates/agent-memory-core/src/store/sqlite.rs
- FOUND: crates/agent-memory/src/mcp.rs
- FOUND: README.md
- FOUND: commit 024bcd4
- FOUND: commit 7c63d0e
