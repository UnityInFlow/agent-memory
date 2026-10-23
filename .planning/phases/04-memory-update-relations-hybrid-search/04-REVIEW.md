---
phase: 04-memory-update-relations-hybrid-search
reviewed: 2026-10-23T21:00:00Z
depth: standard
review_kind: re-review (gap closure 04-05..04-07, diff d3a4580..HEAD)
files_reviewed: 14
files_reviewed_list:
  - README.md
  - crates/agent-memory-core/src/domain.rs
  - crates/agent-memory-core/src/service.rs
  - crates/agent-memory-core/src/store/mod.rs
  - crates/agent-memory-core/src/store/sqlite.rs
  - crates/agent-memory-core/tests/backfill_race.rs
  - crates/agent-memory-core/tests/fallback.rs
  - crates/agent-memory-core/tests/hybrid.rs
  - crates/agent-memory-core/tests/migration_hygiene.rs
  - crates/agent-memory-core/tests/relations.rs
  - crates/agent-memory/src/mcp.rs
  - crates/agent-memory/src/rest/handlers.rs
  - crates/agent-memory/tests/rest.rs
  - crates/agent-memory/tests/tools.rs
findings:
  critical: 0
  warning: 2
  info: 6
  total: 8
status: issues_found
---

# Phase 4: Code Review Report (re-review after gap closure)

**Reviewed:** 2026-10-23
**Depth:** standard. Scope is the gap-closure diff `d3a4580..HEAD`, excluding `.planning/`.
**Files Reviewed:** 14
**Status:** issues_found (no blockers)

## Summary

Plans 04-05, 04-06 and 04-07 close every prior blocker:

- **CR-01 / CR-02.** The keyword leg now runs the raw query first and retries once with a sanitized query. Natural-language and hyphenated queries are answered in both modes.
- **CR-03.** The backfill write is a guarded compare-and-set.
- **WR-01.** The backup is written to a temp file, its schema version is checked, and only then is it renamed into place. The migration tolerates a lost race by re-reading the schema version.
- **WR-02.** `deny_unknown_fields` is now on both patch DTOs.
- **WR-03.** `relates_to` is stored canonically.

Production code adds no `unwrap()`/`expect()`. The new matches are exhaustive: `redact_search_error`, `is_symmetric`, `canonical_endpoints`, and the migrate re-probe. All SQL is still bound.

I checked the classifier against the **bundled** SQLite with a scratch crate in `/tmp`. It calls `SqliteStore::search` and `Store::keyword_candidates` directly. 13 of 15 adversarial probes return `Ok`: `{a b}: c`, `^a`, `-a`, `NEAR(a b, 99999999999999999999)`, `NEAR(a b, -1)`, `a + b`, `a.b`, `rank: auth`, an embedded NUL, `(`×300 nesting (parser stack overflow is classified), and others.

The other 2 probes fail. They are both cases of one FTS5 error class that the marker list misses (WR-01 below). That error is redacted, so no SQLite text leaks, but a client input still becomes a 500.

The second warning is a README sentence added by 04-07. It promises a JSON error body for every REST client error, but two v0.0.1 routes still return axum's plain-text rejections.

The workspace test suite was not re-run here, because a verifier runs in parallel. The 04-07 SUMMARY reports 211 passed / 0 failed / 2 ignored.

## Prior findings — resolution

| ID | Prior severity | Resolution | Evidence |
|----|----------------|------------|----------|
| CR-01 | BLOCKER | **Resolved** (one residual class, see new WR-01) | `sqlite.rs:498-504` adds `no such column`, `unknown special query` and `expected integer` to `FTS5_QUERY_PARSE_MARKERS`. `sqlite.rs:519-530` gates on primary code `SQLITE_ERROR`. `sqlite.rs:569-588` adds the raw-first retry, shared by `search` (`:866`) and `keyword_candidates` (`:908`). Tests: `tests/hybrid.rs` two-mode table, `tests/rest.rs` (`sqlite-vec` → 200), `tests/tools.rs` stdio e2e. |
| CR-02 | BLOCKER | **Resolved** (user decision option-a) | `sqlite.rs:541-552` `sanitize_fts_query`; a tokenless query returns an empty leg (`:582`). The decision is recorded in `.planning/STATE.md:118`. The MCP description (`mcp.rs:330`) and README (`README.md:203-212`, `:482-488`) describe the forgiving contract. |
| CR-03 | BLOCKER | **Resolved** | `sqlite.rs:706-713` adds a guarded claim (`embedding_status = 0 AND content = ?2`) as the first statement of the writer tx, with an `Ok(false)` rollback. `service.rs:697-703` passes the embedded content back. `tests/backfill_race.rs` has 7 interleavings, including a recycled rowid and a gated real sweep. |
| WR-01 | WARNING | **Resolved** for both observable failure modes. The cross-process exclusive lock is **deferred-as-documented** | `sqlite.rs:235-301`: temp file, then version check, then rename, then prune. A post-migration copy is discarded (`:273-282`). `sqlite.rs:315-334` absorbs a failed migrate only when the re-read version is `LATEST`. `busy_timeout` is set before the probe (`:387`) and first in `apply_pragmas` (`:89`). Tests: `sqlite.rs` seam tests and `a_real_lost_migration_race_is_absorbed`, plus `migration_hygiene.rs` two-opener smoke. |
| WR-02 | WARNING | **Resolved** | `mcp.rs:169` and `handlers.rs:107` add `#[serde(deny_unknown_fields)]`. The schema advertises `additionalProperties: false`. The MCP wire shape is rmcp's `isError: true` tool result, as documented at `README.md:265-269`. The rmcp interaction looks sound: `_meta` travels outside `arguments`, and the explicit-null deserializers still compose. |
| WR-03 | WARNING | **Resolved** (stale wording remains, see IN-03) | `domain.rs:127-149` adds `is_symmetric` / `canonical_endpoints`. `sqlite.rs:977-982` inserts the canonical pair. `sqlite.rs:998-1003` makes symmetric unlink a single bound DELETE. Tests: 8 new in `tests/relations.rs`. The MCP and REST echo tests pass. |
| WR-04 | WARNING | **Deferred-as-documented** (doc-only, by user instruction) | The `domain.rs:360-371` doc is corrected and `README.md:363` adds "pass a `limit` on large stores". Behaviour is unchanged (`service.rs:232-246`). Note: the `memory_list` **tool description** (`mcp.rs:297`), which is what agents read, still does not mention the unbounded case. |
| IN-01 | INFO | **Resolved** | `README.md:84-121` adds the Upgrading section: backup name, retention, restore steps including `-wal`/`-shm`, and the newer-schema error text. |
| IN-02 | INFO | **Resolved** for the `Path`/`Query` routes the finding named. README now overclaims, see new WR-02 | `handlers.rs:220-226` `extractor_rejection`. It is used at `:236`, `:287`, `:319` and `:397`, and keeps a 5xx for routing faults. |
| IN-03 | INFO | **Resolved** | `sqlite.rs:265-293`: no backup is deleted before the new copy has been renamed into place. Tests: `a_failed_backup_write_never_removes_the_previous_backup` and `a_backup_that_cannot_be_moved_into_place_keeps_the_previous_one`. |
| IN-04 | INFO | **Resolved** | `service.rs:345` now reads "Same order as the hybrid path". |
| IN-05 | INFO | **Deferred-as-documented** (by user instruction) | `store/mod.rs:161-165` and `README.md:291-295`. |
| IN-06 | INFO | **Deferred-as-documented** (by user instruction) | `domain.rs:220-232` doc on `RELATED_SNIPPET_CHARS` / `snippet`. |

## New findings

## Warnings

### WR-01: A long FTS5 `NOT` chain still turns client input into a 500 / `internal_error`

**File:** `crates/agent-memory-core/src/store/sqlite.rs:498-504` (markers), `:519-530` (classifier), `:586` (unclassified error becomes `Sqlite`), `crates/agent-memory-core/src/service.rs:302-310` (redacted to the internal tier)

**Issue:** FTS5 enforces `SQLITE_FTS5_MAX_EXPR_DEPTH` (256). `NOT` nodes are never flattened (`fts5ExprAddChildren` skips `FTS5_NOT`), so `a NOT a NOT a …` with more than 256 `NOT`s builds a left-deep tree. That tree fails at step time with:

```
fts5 expression tree is too large (maximum depth 256)
```

The message starts with `fts5 expression`, not `fts5: `. None of the five markers matches it, so `match_with_sanitized_retry` takes the `Err(e) => Err(MemoryError::Sqlite(e))` arm. `redact_search_error` then maps it to `SEARCH_STORAGE_FAILURE`, which means HTTP 500 / MCP `internal_error` plus an `ERROR` log line.

I verified this against the bundled SQLite with the scratch probe. Both `NOT`×257 and `NOT`×300 return `Err("sqlite error: fts5 expression tree is too large (maximum depth 256)")` from `search` **and** from `keyword_candidates`. The parser-stack-overflow class (deep parentheses) is classified correctly and returns `Ok`.

The input is contrived, but it breaks the contract this phase re-established in `README.md:203` ("a query is never rejected") and in D-06 (client input never surfaces as a 500). It also lets any client write `ERROR`-level log entries on demand. The sanitized form would parse: OR chains are flattened, and a 3000-term OR chain was verified to run.

`every_bundled_fts5_parse_error_is_classified_as_match_input` does not cover this class, so the test suite cannot catch it.

**Fix:** Add the marker, then add the probe to the bundled-SQLite classifier test and to the two-mode decision table.
```rust
const FTS5_QUERY_PARSE_MARKERS: [&str; 6] = [
    "fts5: ",
    "fts5 expression tree is too large", // SQLITE_FTS5_MAX_EXPR_DEPTH, e.g. 257+ chained NOTs
    "unterminated string",
    "no such column",
    "unknown special query",
    "expected integer",
];
// test probe: format!("a {}", "NOT a ".repeat(300)) -> Ok in both modes
```

### WR-02: README promises a JSON `{"error"}` body for every REST client error, but `POST /api/memories` and `POST /api/search` still return axum's plain-text rejections (and 422/415, not 400)

**File:** `README.md:436-438`, `crates/agent-memory/src/rest/handlers.rs:194-197` (`store_handler`), `:257-260` (`search_handler`)

**Issue:** 04-07 added this sentence: "Every client error, including a malformed path segment … or query parameter …, returns a JSON `{"error": "..."}` body."

That is not true for the two POST routes that kept the bare `Json<T>` extractor:

- `POST /api/memories` with `{}` (missing `content`) returns axum's `JsonRejection`: **422** `text/plain` "Failed to deserialize the JSON body into the target type: missing field `content`".
- `POST /api/search` with a non-JSON body or no `Content-Type` returns **400/415** `text/plain`.

A client that parses `error` from every non-2xx response, as the README now invites, fails on these routes. The status codes also contradict the README's 400-for-client-input story.

The handlers are v0.0.1 code, but the README claim is new in this gap closure. It turns a known inconsistency into a documented contract that does not hold.

**Fix:** Take the rejection explicitly, as `update_handler` / `link_handler` already do. Add one real-HTTP row per route to `malformed_path_and_query_params_get_the_json_error_body`.
```rust
pub async fn store_handler(
    State(state): State<Arc<AppState>>,
    body: Result<Json<StoreRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(req) = body.map_err(|r| ApiError::BadRequest(r.body_text()))?;
    // ...
}
// same for search_handler
```
Alternatively, narrow the README sentence to the routes that actually comply.

## Info

### IN-01: Non-search surfaces still put raw SQLite text in client responses

**File:** `crates/agent-memory/src/rest/handlers.rs:165-180` (`ApiError::Internal(e.to_string())`), `crates/agent-memory/src/mcp.rs:35-50`

**Issue:** `redact_search_error` (`service.rs:46-60`) covers `search` only. A storage fault in `store`, `list` (including `expand_links` → `related()`), `update`, `link`, `unlink`, `forget` or `/health` still sends `sqlite error: …` to the client. That text includes table and column names and engine detail.

The README scopes its promise to search (`README.md:438-440`), and 04-05 lists this as deferred, so the code and docs agree. The risk is low: the server is loopback, single-user and unauthenticated by design. It is recorded here because this re-review was asked to look for remaining raw SQLite text.

**Fix:** When the deferral is picked up, move the redaction into the two transport mappers. On the `Sqlite | Pool | Join | Migration` arm: log the error, then return a generic message. Keep `Internal` / `SchemaTooNew` as they are.

### IN-02: Keyword mode: a punctuation-free question often finds nothing, while the same question with `?` finds results

**File:** `crates/agent-memory-core/src/store/sqlite.rs:569-588`

**Issue:** The retry only fires when FTS5 fails to *parse*. `what vector store do we use` is valid FTS5, an implicit AND of six words, so it usually matches nothing and returns `[]` in keyword mode. `what vector store do we use?` fails to parse, takes the OR fallback, and returns results. Adding a question mark makes the query broader.

Hybrid mode hides this, because the semantic leg answers. With Ollama down, an agent that drops the `?` gets nothing. This follows from "valid FTS5 runs exactly as written", so it is a design consequence, not a regression from v0.0.1. It is still surprising, and the README example (`README.md:203-209`) implies the punctuation is incidental.

**Fix:** Either document it ("an unpunctuated multi-word query is an AND of all its words"), or also run the sanitized OR form when the raw query returned zero rows and contains no explicit FTS5 operators or quotes.

### IN-03: Stale "directed" wording after the WR-03 symmetric `relates_to` change

**File:** `README.md:154`, `crates/agent-memory-core/src/service.rs:561`, `crates/agent-memory-core/src/service.rs:587-590`, `crates/agent-memory/src/rest/handlers.rs:135-144`

**Issue:** These still describe every relation as a directed edge whose direction must match:

- the tools table: "Link two memories with a typed, directed relation";
- the `MemoryService::link` / `unlink` docs;
- the `LinkBody` doc and its `kind` field.

The MCP descriptions, store-trait docs and README Relations section were updated, so the wording is now inconsistent.

**Fix:** Reword them to "typed relation (`relates_to` is symmetric; `supersedes` / `caused_by` are directed)", matching `mcp.rs:418`.

### IN-04: Backup edge cases: pid-only temp name, and a prune failure aborts startup after a valid backup is already in place

**File:** `crates/agent-memory-core/src/store/sqlite.rs:140-145`, `crates/agent-memory-core/src/store/sqlite.rs:293`

**Issue:**

1. `backup_temp_path` makes the temp name unique by pid only. Two processes in different PID namespaces can both be pid 1. Example: the REST daemon in a container and another container sharing the same volume. Those two openers write Online Backups into the same temp file, and one renames it while the other is still writing. The result is a torn backup, or an `ENOENT` rename failure that aborts that opener. This case is rare for a Homebrew/binary tool.
2. When the rename succeeds but `prune_other_backups` cannot remove an older same-version backup (permissions, or an entry that is not a file), `open()` refuses to start, even though the new genuine backup is already in place. Refusing to migrate without a backup is the T-04-02 rule. Refusing to start because an *old* backup could not be deleted goes further than that rule requires.

**Fix:**

1. Add a random suffix to the temp name, for example `{pid}-{nanos}`, or create it with `O_EXCL` / `tempfile::Builder::new().tempfile_in(dir)`.
2. Log prune failures as warnings and return `Ok(Some(final_path))`.

### IN-05: When a newer binary wins the upgrade race, the user gets a raw migration error instead of the `SchemaTooNew` message

**File:** `crates/agent-memory-core/src/store/sqlite.rs:323-333`

**Issue:** The scenario: a v3 binary probes 2 while a v4 binary migrates the same file to 4.

- The v3 binary's backup copy is version 4, so it is discarded with `Ok(None)`.
- Its `to_latest` then fails with rusqlite_migration's `DatabaseTooFarAhead`.
- The re-probe reads 4, which is not `LATEST_SCHEMA_VERSION`, so the original migration error is returned.

The user does not get the friendly D-13 text that names `brew upgrade`. The refusal itself is still correct.

**Fix:** In the re-probe `match`, map `Ok(v) if v > LATEST_SCHEMA_VERSION` to `MemoryError::SchemaTooNew { db_version: v, binary_version: LATEST_SCHEMA_VERSION }`.

### IN-06: Test name contradicts its assertion

**File:** `crates/agent-memory/tests/tools.rs:418` (`memory_update_with_an_unknown_field_is_invalid_params_over_stdio`)

**Issue:** The test asserts that there is **no** JSON-RPC `error` member and that `result.isError == true`. That is a tool error result, not `invalid_params`. 04-07 kept the plan's name on purpose, but anyone triaging a failure will be misled.

**Fix:** Rename it to `memory_update_with_an_unknown_field_is_a_tool_error_over_stdio`.

## Fixes applied (2026-10-23)

| ID | Status | Commit | Change |
|----|--------|--------|--------|
| WR-01 (new) | Fixed | `36df937` | Added `fts5 expression tree is too large` to `FTS5_QUERY_PARSE_MARKERS`. The NOT×257 and NOT×300 probes are in the bundled-SQLite classifier and fallback tests. A boundary test pins 256 NOTs as accepted, 257 as the classified depth error, and a 3000-NOT chain's flat-OR fallback as parsing. The two-mode test `a_not_chain_past_the_fts5_depth_limit_is_answered_in_both_modes` covers hybrid and keyword mode. |
| WR-02 (new) | Fixed (doc only) | `68b479c` | The README now lists the exact routes that return a 400 `{"error"}` for a malformed body, path, or query. It also states that `POST /api/memories` and `POST /api/search` keep axum's plain-text 422/400/415 body rejection. No behaviour change. |

Workspace after the fixes: 213 passed / 0 failed / 2 ignored. `cargo clippy --workspace --all-targets -- -D warnings` is clean.

---

_Reviewed: 2026-10-23_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
