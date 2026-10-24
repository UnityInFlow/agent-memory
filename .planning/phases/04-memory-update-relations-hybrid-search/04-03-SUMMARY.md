---
phase: 04-memory-update-relations-hybrid-search
plan: 03
subsystem: api
tags: [rust, sqlite, relations, fts5, sqlite-vec, serde, schemars, rmcp, axum, mcp]

# Dependency graph
requires:
  - phase: 04-memory-update-relations-hybrid-search
    provides: "04-01: migration 0003 memory_links (PK(from,to,kind), FK ON DELETE CASCADE, CHECK from!=to, idx_links_to), MemoryError::Internal for writer-lock sites, not-found as data. 04-02: UPDATE-in-place keeps ids immutable, so edges never re-attach"
provides:
  - "LinkKind closed enum (relates_to | supersedes | caused_by) with as_wire_str / reverse_label / TryFrom -> InvalidArgument (D-01)"
  - "LinkOutcome { Linked, NotFound { missing_id } }: a missing endpoint is data naming the id (D-02)"
  - "Store::link / Store::unlink / Store::related + SqliteStore impls"
  - "MemoryService::link (self-link rejected at the seam) / MemoryService::unlink"
  - "Belt-and-braces orphan-edge deletes inside forget() and sweep_expired() (T-04-12)"
  - "RelatedLink {id, kind, direction, type, snippet, tags} + LinkDirection + snippet() + RELATED_SNIPPET_CHARS = 160"
  - "pub const MAX_EXPANDED_LINKS: usize = 10 (D-07)"
  - "MemoryView.related: Option<Vec<RelatedLink>> skipped when None (byte-identical v1.0 envelope)"
  - "expand_links: bool on core SearchArgs/ListArgs and on all four transport surfaces"
  - "memory_link / memory_unlink MCP tools; POST /api/links + DELETE /api/links/{from_id}/{to_id}/{kind}"
affects: [04-04 hybrid RRF (expansion must stay orthogonal to the new hybrid search_mode), 05 export/import (JSONL format v1 must carry memory_links edges), 06 release notes]

# Actuals (#2632): chars/4 over the added lines of the realized diff
actuals:
  tokens: 20971
  tasks: 2
  commits: 4

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Closed-enum wire kinds: TryFrom<&str> returning InvalidArgument naming the allowed set; SQL binds only as_wire_str()"
    - "Endpoint existence pre-checked inside the writer tx, so a missing id is returned as data, never an FK-violation 500"
    - "Every deletion path carries an explicit orphan cleanup beside the FK cascade (the vec_memories idiom, now also for memory_links)"
    - "Optional response members: Option<T> + skip_serializing_if, locked by a golden byte-equality test of the old envelope"
    - "Read-only enrichment (expansion) runs after truncation and after spawn_bump, so the ids it touches are never bumped"

key-files:
  created:
    - crates/agent-memory-core/tests/relations.rs
  modified:
    - crates/agent-memory-core/src/domain.rs
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory-core/src/store/mod.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory/src/mcp.rs
    - crates/agent-memory/src/rest/handlers.rs
    - crates/agent-memory/src/rest/mod.rs
    - README.md

key-decisions:
  - "MAX_EXPANDED_LINKS = 10 (D-07 discretion); snippet = first 160 chars cut on a char boundary, with a trailing ellipsis only when cut (D-05 discretion)"
  - "related lives directly on MemoryView as Option<Vec<RelatedLink>> with skip_serializing_if (research Open Question 2). Flag on: always Some, [] when there are no edges. Flag off: None, so the member is absent"
  - "RelatedLink.direction is a LinkDirection enum serializing to out/in (same wire as the planned String, but typed); kind is the direction-aware natural label (superseded_by, caused, relates_to)"
  - "The link not-found body echoes from_id/to_id/kind as well as {linked:false, reason:not_found, missing_id}, mirroring forget's id echo. from_id is checked first, so a link with both ids missing names from_id"
  - "Unlink stays a thin wrapper. A self-pair (from==to) is a not-found (such an edge cannot exist), not an InvalidArgument"
  - "POST /api/links maps every JSON body rejection to 400 {error}, like PATCH. A non-numeric path id on DELETE /api/links keeps axum's default plain-text 400, like DELETE /api/memories"
  - "expand_links on REST search is a member of the existing POST /api/search JSON body (search has no query-param DTO). On REST list it is the ?expand_links= query param"

patterns-established:
  - "New relation kind: add a LinkKind variant + as_wire_str + reverse_label arms (exhaustive match forces both), extend the TryFrom message, the README kind table and the MCP tool description"

requirements-completed: [MCP-07]

coverage:
  - id: D1
    description: "Link creates one directed edge; an identical relink is an idempotent Linked no-op (edge count stays 1); a different kind is a separate edge"
    requirement: MCP-07
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/relations.rs#link_succeeds_and_a_duplicate_link_is_an_idempotent_no_op"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/mcp.rs#memory_link_and_unlink_round_trip_with_idempotent_duplicates"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/rest/handlers.rs#post_links_links_idempotently_and_delete_unlinks_then_404s"
        status: pass
    human_judgment: false
  - id: D2
    description: "Self-link and unknown kind are InvalidArgument with the exact D-01/D-02 strings on core, MCP (invalid_params) and REST (400). A self-link on a nonexistent id is still the client error"
    requirement: MCP-07
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/relations.rs#a_self_link_is_invalid_argument_naming_the_id / an_unknown_kind_is_invalid_argument_listing_the_allowed_values / every_kind_round_trips_its_wire_form"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/mcp.rs#memory_link_self_links_and_unknown_kinds_are_invalid_params"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/rest/handlers.rs#bad_link_requests_are_400_never_500"
        status: pass
    human_judgment: false
  - id: D3
    description: "A missing endpoint is a clean not-found naming the missing id on all three layers (core LinkOutcome::NotFound, MCP success with reason not_found, REST 404); nothing is written; never an FK 500"
    requirement: MCP-07
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/relations.rs#linking_to_a_missing_memory_is_a_clean_not_found_naming_the_id"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/mcp.rs#memory_link_to_a_missing_memory_is_a_clean_not_found_naming_it"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/rest/handlers.rs#post_links_to_a_missing_memory_is_404_naming_it_never_500"
        status: pass
    human_judgment: false
  - id: D4
    description: "Unlink removes exactly the directed (from, to, kind) edge. A missing edge (or the wrong direction or kind) is a clean not-found on core, MCP and REST, including over real HTTP"
    requirement: MCP-07
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/relations.rs#unlink_removes_an_edge_and_a_missing_edge_is_a_clean_not_found"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/rest/handlers.rs#links_routes_work_over_real_http"
        status: pass
    human_judgment: false
  - id: D5
    description: "Zero orphan edges after forget AND after a TTL sweep (FK cascade). The explicit backstop deletes heal orphans left by a connection without the foreign_keys pragma"
    requirement: MCP-07
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/relations.rs#forget_cascades_every_edge_touching_the_memory / a_ttl_sweep_cascades_the_expired_memorys_edges"
        status: pass
      - kind: integration
        ref: "crates/agent-memory-core/tests/relations.rs#forget_also_clears_orphans_left_by_a_connection_without_the_fk_pragma / sweep_also_clears_orphans_left_by_a_connection_without_the_fk_pragma"
        status: pass
    human_judgment: false
  - id: D6
    description: "expand_links attaches 1-hop summaries in both directions from one edge row, with natural reverse labels (superseded_by/in, caused/in, relates_to symmetric), on the keyword and semantic search legs and on list"
    requirement: MCP-07
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/relations.rs#expansion_follows_one_edge_both_ways_with_natural_labels / relates_to_reads_the_same_from_both_ends / expansion_also_applies_on_the_semantic_path"
        status: pass
    human_judgment: false
  - id: D7
    description: "Flag-less search and list JSON is byte-identical to the golden v1.0 envelope (no related substring), even with edges present. Flag on with no edges gives related: []"
    requirement: MCP-07
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/relations.rs#flag_less_envelopes_are_byte_identical_to_v1_and_never_mention_related / flag_on_without_edges_is_an_explicit_empty_related_list"
        status: pass
    human_judgment: false
  - id: D8
    description: "Cap: MAX_EXPANDED_LINKS + 2 staggered edges (mixed directions) expand to exactly 10, newest first, with the two oldest dropped. Snippets are cut at 160 chars on a char boundary with an ellipsis"
    requirement: MCP-07
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/relations.rs#expansion_is_capped_and_the_most_recent_edges_win"
        status: pass
      - kind: unit
        ref: "crates/agent-memory-core/tests/relations.rs#snippets_cut_at_160_chars_on_a_char_boundary_with_an_ellipsis / expanded_snippets_are_truncated_for_long_neighbor_content"
        status: pass
    human_judgment: false
  - id: D9
    description: "Expanded neighbors are never access-bumped (access_count and last_accessed unchanged), while the returned result is still bumped (the test waits for that bump to land)"
    requirement: MCP-07
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/relations.rs#expanded_neighbors_are_never_access_bumped"
        status: pass
    human_judgment: false
  - id: D10
    description: "expand_links is accepted from raw wire input on all four surfaces (MCP search/list args, REST search body, REST list query) as an optional boolean with default false; a non-boolean is rejected by the typed DTO"
    requirement: MCP-07
    verification:
      - kind: integration
        ref: "crates/agent-memory/src/mcp.rs#expand_links_from_raw_args_attaches_related_on_search_and_list / expand_links_is_an_optional_boolean_in_both_tool_schemas"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/rest/handlers.rs#expand_links_from_raw_query_and_body_on_list_and_search / links_routes_work_over_real_http"
        status: pass
      - kind: manual_procedural
        ref: "live stdio binary: tools/list shows 7 tools, link/unlink require [from_id,to_id,kind] with no defaults, expand_links boolean default false; link/dup/self/blocks/missing/list+expand/plain search/unlink behave as tested"
        status: pass
    human_judgment: false
  - id: D11
    description: "README documents the relations tools, kind table with reverse labels, expansion shape, REST link routes and a bounds table including MAX_EXPANDED_LINKS = 10"
    requirement: MCP-07
    verification: []
    human_judgment: true
    rationale: "Documentation clarity and accuracy is a reading judgment; no test asserts README prose"

# Metrics
duration: 10min
completed: 2026-10-23
status: complete
---

# Phase 4 Plan 03: Relations (link / unlink / 1-hop expansion) Summary

**Agents can now link memories with typed, directed relations: `memory_link` / `memory_unlink` over MCP, and `POST /api/links` / `DELETE /api/links/{from_id}/{to_id}/{kind}` over REST.** The vocabulary is closed to three kinds. A duplicate link is an idempotent no-op, a self-link is rejected, and a missing endpoint comes back as clean not-found data. Edges cascade on both forget and TTL sweep, with zero orphans either way. The opt-in `expand_links` flag attaches capped, newest-first 1-hop summaries to search and list results, using natural reverse labels and never access-bumping the neighbors. Without the flag, the envelope is byte-identical to v1.0.

## Performance

- **Duration:** ~10 min
- **Started:** 2026-10-23T18:13:31Z
- **Completed:** 2026-10-23T18:23:28Z
- **Tasks:** 2 / 2
- **Files modified:** 9 (1 created, 8 modified)

## Accomplishments

- **Phase success criterion 2 holds.** Agents can link, unlink and expand, the cascade leaves no orphan edges, and there is no graph or entity model. Storage is one directed row per link. Expansion reads it from both ends, so an incoming `supersedes` reads `superseded_by` and an incoming `caused_by` reads `caused`.
- **D-01 / D-02 contracts are exact on all three layers:**
  - Self-link: `invalid argument: from_id and to_id must differ (got N)`.
  - Unknown kind: `invalid argument: kind must be one of relates_to, supersedes, caused_by (got 'blocks')`.
  - Missing endpoint: `{..., "linked": false, "reason": "not_found", "missing_id": N}`, as a successful MCP call or a REST 404. Never a 500.
- **No orphan edges.** The FK `ON DELETE CASCADE` is the primary mechanism. `forget()` and `sweep_expired()` each also run `DELETE FROM memory_links WHERE from_id NOT IN (…) OR to_id NOT IN (…)` in the same transaction. A hand mutation that turned those deletes into no-ops failed exactly the two pragma-less-connection tests, which shows the backstop is real coverage and not decoration.
- **Expansion (D-03..D-07).** `Store::related` runs one read-pool query per anchor. It follows edges in both directions, newest first, with `LIMIT MAX_EXPANDED_LINKS` (10). The service attaches the result after truncation and after `spawn_bump(returned ids)`, on both search legs and on list. Hand mutations confirmed two tests bite: flipping the ORDER BY failed the cap test, and dropping `skip_serializing_if` failed the golden byte-compat test.
- **Live binary check.** The stdio server lists seven tools. The link/unlink schemas require `[from_id, to_id, kind]` with no `default`, and `expand_links` is a boolean with `default: false`. Every behaviour matches the tests, including a plain semantic search with no `related` member.

## Task Commits

1. **Task 1: memory_link / memory_unlink end-to-end with cascade**: `93ae867` (feat)
2. **Task 2: 1-hop expansion via expand_links on search and list**: `d636328` (feat)

**Plan metadata:** this SUMMARY commit, then a `docs(04-03)` state/roadmap commit.

_TDD note: RED was observed before implementation for both tasks. For Task 1, `tests/relations.rs` failed to compile with 42 errors for the missing `LinkKind`, `LinkOutcome`, `link` and `unlink`. For Task 2, it failed with 8 errors for the missing `RelatedLink`, `expand_links`, `related` and `MAX_EXPANDED_LINKS`. RED was not committed separately, because CLAUDE.md and the operator rules forbid committing failing tests (same as 04-01 and 04-02)._

## Files Created/Modified

- `crates/agent-memory-core/tests/relations.rs` (new, 737 lines, 19 tests): link/idempotency, self-link, unknown kind, wire round-trip, missing endpoint, unlink, cascade on forget and sweep, pragma-less orphan healing (forget and sweep), both-direction labels, relates_to symmetry, golden byte-compat, empty-on, cap with newest-first ordering, snippet rules, no neighbor bumps, semantic-path expansion
- `crates/agent-memory-core/src/domain.rs`: `LinkKind`, `LinkOutcome`, `LinkDirection`, `RelatedLink`, `snippet()`, `RELATED_SNIPPET_CHARS`, `MAX_EXPANDED_LINKS`, `validate_link_endpoints`, `MemoryView.related`, and a wire-order invariant note
- `crates/agent-memory-core/src/store/mod.rs`: `link`, `unlink`, `related` trait methods; forget/sweep docs name the edge cascade
- `crates/agent-memory-core/src/store/sqlite.rs`: the `link` (pre-check plus `INSERT OR IGNORE`), `unlink` and `related` impls, the two orphan-edge deletes, `corrupt_column`, and `related: None` in `row_to_view`
- `crates/agent-memory-core/src/service.rs`: `link`, `unlink`, `expand_related`, and `expand_links` on `SearchArgs` / `ListArgs`
- `crates/agent-memory/src/mcp.rs`: `LinkArgs`/`UnlinkArgs`, the `memory_link`/`memory_unlink` tools, `expand_links` on `SearchArgs`/`ListArgs`, updated server instructions, and 6 new tests
- `crates/agent-memory/src/rest/handlers.rs`: `LinkBody`, `link_handler`, `unlink_handler`, `expand_links` on `SearchRequest`/`ListQuery`, and 5 new tests (including a real-HTTP router test)
- `crates/agent-memory/src/rest/mod.rs`: the two `/api/links` routes
- `README.md`: the "seven tools" table, the Relations section (kind table, expansion shape, cascade note), a bounds table with `MAX_EXPANDED_LINKS = 10`, the REST rows, and curl examples

## Decisions Made

See `key-decisions` in the frontmatter. In short:
- The cap is 10 and the snippet is 160 chars.
- `related` sits on `MemoryView`: `Some` when the flag is on, skipped when it is off.
- `direction` is a typed enum on the same wire.
- The link not-found body echoes the request fields.
- Unlink of a self-pair is a not-found.
- Link body rejections are 400s.
- On REST search, `expand_links` lives in the JSON body.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical] Pragma-less orphan tests for the belt-and-braces deletes**
- **Found during:** Task 1
- **Issue:** With `foreign_keys = ON`, the FK cascade removes every edge on its own. The planned cascade tests would therefore stay green even if the explicit deletes required by T-04-12 were missing.
- **Fix:** Two tests orphan an edge through a raw connection with `foreign_keys = OFF`, then assert that an unrelated `forget()`, or a sweep with nothing to expire, heals it. Mutation-verified: they fail when the deletes are no-ops.
- **Files modified:** crates/agent-memory-core/tests/relations.rs
- **Commit:** 93ae867

**2. [Rule 2 - Missing critical] Link body rejections are 400, not axum's default status**
- **Found during:** Task 1
- **Issue:** The plan said to copy `forget_handler`, which has no body. With a plain `Json<LinkBody>` extractor, a missing field or wrong type would return axum's 422 and a missing content-type a 415, not the project's `{"error"}` 400 contract.
- **Fix:** `link_handler` takes `Result<Json<LinkBody>, JsonRejection>` and maps it to `ApiError::BadRequest`, the same as PATCH.
- **Files modified:** crates/agent-memory/src/rest/handlers.rs
- **Commit:** 93ae867

**3. [Rule 2 - Missing critical] Tests beyond the listed behaviours**
- **What was added:** a real-HTTP in-process router test (path extractor, route wiring, the `?expand_links=true` query); a `relates_to` symmetry test; a semantic-path expansion test; a flag-on-empty test; schema tests for `LinkArgs`/`UnlinkArgs` (all three fields required, no `default`) and for `expand_links` (boolean, default false); a test that a self-link on a nonexistent id is still `InvalidArgument`.
- **Commits:** 93ae867, d636328

**4. [Plan wording] `expand_links` on REST search is a body member**
- **Issue:** The plan says "REST search/list query-param DTOs". REST search is `POST /api/search` with a JSON body; it has no query-param DTO.
- **Fix:** `expand_links` was added to `SearchRequest` (the body) and to `ListQuery` (the `GET /api/memories` query string). Both have raw-input tests.
- **Commit:** d636328

**5. [Design detail] `RelatedLink.direction` is a `LinkDirection` enum rather than a `String`**
- The JSON is identical (`"out"` / `"in"`), but the type is closed, in line with the project's "pattern match exhaustively" rule. The not-found link body also echoes `from_id`/`to_id`/`kind`, which is a superset of the planned shape.
- **Commit:** d636328 / 93ae867

---

**Total deviations:** 3 auto-fixed (missing-critical), plus 2 wording/design clarifications.
**Impact on plan:** No scope creep, no new crates, no manifest or workflow changes.

## Issues Encountered

- The plan's literal gate expects `DELETE FROM memory_links WHERE from_id NOT IN` to appear 2 times in `sqlite.rs`. A shared helper would have made it 1, so the delete is written inline in both `forget()` and `sweep_expired()`.
- One clippy run reported `Finished` in 0.15s from cache. It was re-run with both crates touched and re-linted clean.

## Verification Results

- `cargo test -p agent-memory-core --test relations`: **19 passed**
- `cargo test -p agent-memory --bins`: 48 passed (37 before this plan)
- `cargo test --workspace`: **146 passed / 0 failed / 2 ignored** (04-02 baseline: 116 / 0 / 2)
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (forced re-lint)
- `cargo fmt --all --check`: clean
- Acceptance greps: `INSERT OR IGNORE INTO memory_links` appears 1 time; `DELETE FROM memory_links WHERE from_id NOT IN` appears 2 times; `map_err(|_| MemoryError::NotFound` appears 0 times; `pub const MAX_EXPANDED_LINKS: usize = 10` is present; `MAX_EXPANDED_LINKS` is in the README; `skip_serializing_if = "Option::is_none"` is on `related`.
- Hand mutations, all reverted: no-op orphan deletes failed the 2 pragma-less tests; ORDER BY ASC failed the cap test; removing the skip failed the golden test.
- Live stdio binary: checked as described under D10.

## User Setup Required

None.

## Next Phase Readiness

- **Plan 04-04 (hybrid RRF):** expansion is orthogonal to `search_mode`. The hybrid leg must keep the same order — truncate, `spawn_bump(returned ids)`, then `expand_related` when `expand_links` is set — and must thread `args.expand_links` through before `args` is moved into the store legs.
- **Phase 5 (export/import):** edges live in `memory_links (from_id, to_id, kind, created_at)`. Format v1 must carry them, and import must remap ids, because ids are local rowids.
- **Still open, outside this plan:** WR-01 (full filter-aware KNN) and WR-03 (the spawned-binary REST harness hang). No link test was added to `tests/rest.rs`. The in-process real-HTTP router test covers the HTTP surface instead.

---
*Phase: 04-memory-update-relations-hybrid-search*
*Completed: 2026-10-23*

## Self-Check: PASSED

- FOUND (tracked): crates/agent-memory-core/tests/relations.rs (737 lines, ≥ 120)
- FOUND: commit 93ae867 (Task 1)
- FOUND: commit d636328 (Task 2)
