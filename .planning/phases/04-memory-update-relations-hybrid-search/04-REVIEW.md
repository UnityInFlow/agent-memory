---
phase: 04-memory-update-relations-hybrid-search
reviewed: 2026-10-23T00:00:00Z
depth: standard
files_reviewed: 20
files_reviewed_list:
  - Cargo.toml
  - README.md
  - crates/agent-memory-core/sql/0003_relations.sql
  - crates/agent-memory-core/src/domain.rs
  - crates/agent-memory-core/src/service.rs
  - crates/agent-memory-core/src/store/migrations.rs
  - crates/agent-memory-core/src/store/mod.rs
  - crates/agent-memory-core/src/store/sqlite.rs
  - crates/agent-memory-core/tests/hybrid.rs
  - crates/agent-memory-core/tests/import.rs
  - crates/agent-memory-core/tests/migration_hygiene.rs
  - crates/agent-memory-core/tests/relations.rs
  - crates/agent-memory-core/tests/semantic.rs
  - crates/agent-memory-core/tests/update.rs
  - crates/agent-memory/Cargo.toml
  - crates/agent-memory/src/main.rs
  - crates/agent-memory/src/mcp.rs
  - crates/agent-memory/src/rest/handlers.rs
  - crates/agent-memory/src/rest/mod.rs
  - crates/agent-memory/src/serde_util.rs
findings:
  critical: 3
  warning: 4
  info: 6
  total: 13
status: issues_found
---

# Phase 4: Code Review Report

**Reviewed:** 2026-10-23
**Depth:** standard (diff `907161c..HEAD`)
**Files Reviewed:** 20 (binary fixture `tests/fixtures/v0.0.1.db` skipped)
**Status:** issues_found

## Summary

Phase 4 adds `memory_update` (merge-patch with an in-transaction vector swap), flat typed
relations with 1-hop expansion, RRF hybrid search, and migration 0003 with a pre-migration
backup and a newer-schema refusal. The code is careful in many places. SQL is parameterized
everywhere. The `MemoryError` mappers are exhaustive. No `unwrap()` was added to production
code. Clippy (`-D warnings`, all targets) is clean and all `agent-memory-core` tests pass
(verified locally).

The main problems come from making hybrid the default search path, and from content becoming
mutable:

1. Hybrid sends the raw query to FTS5 on every search while Ollama is up. FTS5 reports
   column-filter parse errors (`sqlite-vec`, `re-embed`, `note: x`) as `no such column`. The
   classifier does not recognise that message, so these queries now return **HTTP 500 /
   `internal_error`** on the primary path. v0.0.1 answered them.
2. Natural-language punctuation (`?`, `,`, `'`) now returns 400 whenever Ollama is up. The
   brief asked for an explicit assessment of this. The verdict is a **regression, not an
   acceptable trade-off** (CR-02).
3. Content can now change, so the sweep's embedding backfill can write a vector for *old*
   content over a row that was updated meanwhile. That leaves a permanently wrong vector with
   `embedding_status = 1`.

The FTS5 behaviour behind CR-01/CR-02 was checked against the bundled SQLite (3.51.3, via
rusqlite 0.39):

| query | FTS5 result | mapped to | tier |
|---|---|---|---|
| `what is it?` | `fts5: syntax error near "?"` | `InvalidQuery` | 400 / invalid_params |
| `sqlite-vec` | `no such column: vec` | `Sqlite` | **500 / internal_error** |
| `re-embed` | `no such column: embed` | `Sqlite` | **500 / internal_error** |
| `note: auth` | `no such column: note` | `Sqlite` | **500 / internal_error** |

Known, already-tracked items (WR-01 filter-after-KNN-cut, WR-03 REST harness hang) are not
re-reported.

## Narrative Findings (AI reviewer)

## Critical Issues

### CR-01: Hyphenated or colon-bearing queries are 500 / `internal_error` on the default (hybrid) search path

**File:** `crates/agent-memory-core/src/store/sqlite.rs:333-340` (classifier), `crates/agent-memory-core/src/service.rs:427` (hybrid propagates the keyword leg's error), `crates/agent-memory/src/mcp.rs:310`, `README.md:165-170`
**Issue:** `map_fts_query_error` only treats `fts5: syntax error` and `unterminated string` as
client input. FTS5 parses `a-b` and `a: b` as column filters and fails at *step* time with
`no such column: b`. That error falls through to `MemoryError::Sqlite`, which both mappers send
to the internal tier.

Before Phase 4, this only affected keyword-fallback mode (Ollama down). Now every search with
Ollama up runs `keyword_candidates`, and its error takes priority over the KNN leg
(service.rs:427). So an ordinary query such as `sqlite-vec`, `re-embed`, `pre-migration
backup`, `TODO: auth` or `x86-64`:

- returned 200 (semantic) in v0.0.1;
- now returns HTTP 500 / MCP `internal_error`.

This breaks the project's core contract that bad client input never surfaces as a 500 (02-05,
Phase 3 D-06). It also defeats agent error handling: an agent that sees `internal_error` assumes
a server fault and will not rephrase. The README (L165-170) and the `memory_search` tool
description promise `invalid_params` for queries FTS5 cannot parse, so both documents are wrong
for this class of input. No test covers it: `hybrid.rs:538-570` only exercises `"unbalanced`,
`AND` and `?`.

**Fix:** Classify step-time column-filter failures as client input. The statement prepares
successfully, so a `no such column` at step time can only come from the MATCH expression. Then
add a regression row for `sqlite-vec` and `note: x` to the hybrid/keyword decision-table test.
```rust
fn map_fts_query_error(query: &str, e: rusqlite::Error) -> MemoryError {
    let msg = e.to_string();
    if msg.contains("fts5: syntax error")
        || msg.contains("unterminated string")
        // FTS5 column filters (`a-b`, `col: x`) on the single-column
        // memories_fts table: only reachable from the bound MATCH string,
        // because the statement itself prepared successfully.
        || msg.contains("no such column")
    {
        MemoryError::InvalidQuery(query.to_string())
    } else {
        MemoryError::Sqlite(e)
    }
}
```
(CR-02's sanitising retry removes most of this input class altogether. This mapping is still
the required backstop.)

### CR-02: Assessment of the requested behaviour change — natural-language queries now fail with the embedder up (regression)

**File:** `crates/agent-memory-core/src/service.rs:244-265, 412-428`, `crates/agent-memory-core/src/store/sqlite.rs:606-647`, `crates/agent-memory-core/tests/hybrid.rs:538-570`, `README.md:165-170, 407-414`
**Issue:** With Ollama up, which is the recommended setup, every search now depends on the raw
query being a valid FTS5 expression. Queries like `what did we decide about auth?`, `don't
retry`, `auth, tokens`, `C++ build` and an empty string are rejected with 400 /
`invalid_params`. v0.0.1 answered all of them semantically. Including CR-01's inputs, this
covers most of what an LLM agent naturally sends: questions, sentences with commas or
apostrophes, and hyphenated technical terms.

Severity: **BLOCKER**. The degradation is documented (README L407-414, the tool description),
but documenting it does not make it acceptable for an agent-facing tool.

- **The semantic search value is lost.** The product's main promise is "semantic search via
  local Ollama embeddings". In the configuration where that should work best, the query
  embedding is now discarded whenever the query contains common punctuation.
- **The T-04-15 rationale does not hold.** The plan's justification (04-04-PLAN T-04-15) is
  that answering semantically in one mode and with a 400 in the other would let a caller probe
  which mode is active. But every response already reports `search_mode`, so there is no hidden
  mode to probe. The second reason, the error taxonomy, only requires the *same* classification
  in both modes. It does not require that classification to be an error.
- **Agents pay a retry cost.** An agent receiving `invalid_params` will rewrite and retry, which
  costs tokens on every punctuated question. Agents rarely read the "use plain words" caveat in
  the README.

**Fix:** Keep exact FTS5 syntax available to power users, but stop rejecting natural language.
Apply the same logic in both modes so the T-04-15 "same classification everywhere" property
still holds:
```rust
// sqlite.rs: a fallback MATCH built from the query's word tokens, each
// phrase-quoted (inner quotes doubled) and OR-joined. It always parses.
fn sanitize_fts_query(raw: &str) -> Option<String> {
    let terms: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}
```
In `keyword_candidates`, and in `search` for the fallback path: run the raw MATCH first. If it
fails with `InvalidQuery`, rerun once with `sanitize_fts_query(&args.query)`. If no tokens are
left, return an empty keyword leg so hybrid fuses KNN-only. Only report `InvalidQuery` when the
caller clearly asked for FTS syntax (for example an unbalanced `"`), or never. Then update
`a_malformed_match_is_invalid_query_in_hybrid_mode_exactly_as_in_keyword_mode`: `what is the
plan?` should succeed in both modes. If the owners still choose to keep the 400, record that as
an explicit decision in the CONTEXT decisions log, and fix CR-01 either way.

### CR-03: The sweep backfill can overwrite an updated memory's vector with an embedding of its *old* content, permanently

**File:** `crates/agent-memory-core/src/service.rs:612-637`, `crates/agent-memory-core/src/store/sqlite.rs:439-479`, `crates/agent-memory-core/src/store/sqlite.rs:812-836`
**Issue:** The backfill has three steps:

1. It reads `(id, content)` for pending rows (`pending_embeddings`).
2. It `await`s a batch embed of up to 128 texts outside any lock. On a CPU Ollama that takes
   seconds.
3. `insert_embedding` then runs an unconditional `DELETE` + `INSERT` of the vector and
   `embedding_status = 1`.

Before Phase 4, content was immutable, so this was safe. `memory_update` breaks that
assumption. Interleaving:

1. Row 7 is pending (stored while Ollama was down).
2. Ollama recovers. The sweep reads `(7, "old text")` and starts embedding it.
3. A client PATCHes row 7 with `"new text"`. `update` embeds the new text successfully, swaps
   the vector and sets status 1.
4. The sweep finishes and `insert_embedding(7, vec("old text"))` deletes the fresh vector,
   writes the stale one, and sets status 1.

Row 7 now matches its old meaning semantically, forever: status is 1, so no later sweep repairs
it. The same happens if the update's own embed fails (status 0, no vector), since the sweep
"repairs" it with the stale vector. The race is wider than it looks. `main.rs` documents that
several MCP stdio processes and the REST daemon share one DB file, each with its own sweep
task, while the writer `Mutex` is only per-process.

Separately, a sweep racing `forget` re-inserts a vector for a deleted id. That vector can then
attach to a new memory that reuses the rowid (`INTEGER PRIMARY KEY` without `AUTOINCREMENT`).
That race already existed, but the fix below closes it too.

**Fix:** Make the backfill write conditional on the row still being the pending row whose
content was embedded. Do it in the same writer transaction:
```rust
fn insert_embedding(&self, memory_id: i64, expected_content: &str, embedding: Vec<f32>)
    -> Result<bool, MemoryError> {
    let mut conn = self.writer.lock().map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
    let tx = conn.transaction()?;
    let claimed = tx.execute(
        "UPDATE memories SET embedding_status = 1 \
         WHERE id = ?1 AND embedding_status = 0 AND content = ?2",
        params![memory_id, expected_content],
    )?;
    if claimed == 0 {
        return Ok(false); // updated, already embedded, or forgotten: skip; tx rolls back
    }
    let blob: &[u8] = bytemuck::cast_slice(&embedding);
    tx.execute("DELETE FROM vec_memories WHERE memory_id = ?1", params![memory_id])?;
    tx.execute("INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)", params![memory_id, blob])?;
    tx.commit()?;
    Ok(true)
}
```
Pass the content from `pending_embeddings` through `sweep()`. Add a test: pending row, then
the sweep reads it, then `update` with new content, then the backfill completes. Assert that
KNN on the new content's vector finds the row and KNN on the old content's vector does not.

## Warnings

### WR-01: Concurrent first open after an upgrade can fail startup, or leave a "pre-0003" backup that is really schema v3

**File:** `crates/agent-memory-core/src/store/sqlite.rs:224-252`, `crates/agent-memory-core/src/store/sqlite.rs:140-189`
**Issue:** `open()` has three unlocked steps: probe `user_version`, write the backup, then
`to_latest()`. rusqlite_migration 2.5 reads `user_version` outside its deferred transaction
(`goto` → `goto_up`, lib.rs:713-651). 0003 is the first migration ever applied to deployed
v0.0.1 databases, and several processes share one DB by design (an MCP stdio server per editor
session plus the REST daemon). That makes simultaneous first opens after `brew upgrade` a
realistic scenario:

- **Startup failure:** A and B both probe version 2. A migrates and commits. B's `CREATE TABLE
  memory_links` then fails with `table memory_links already exists`, or `SQLITE_BUSY_SNAPSHOT`.
  The transaction rolls back, but B's server fails to start.
- **Silently wrong backup:** B probes 2, A migrates to 3, and only then does B run
  `pre_migration_backup`. B deletes A's valid `*.backup-pre-0003-*` file and writes a copy of
  the *already-migrated* v3 database under the "pre-0003" name. B's `to_latest` is then a
  no-op. There are no down-migrations (migrations.rs:6-8), so the backup is the only rollback
  path, and it is now wrong.

**Fix:** Run probe, backup and migrate under one exclusive lock, and re-read the version under
it. For example: `BEGIN IMMEDIATE` on the writer, re-read `user_version`, skip the backup if it
is no longer pending, then `COMMIT` and migrate. A cheaper alternative: write the backup to a
temp name, open it, check that its `user_version == db_version`, then rename it into place and
remove the stale backups. Treat a `to_latest` failure caused by the version having moved to
`LATEST` as success.

### WR-02: Patch DTOs silently ignore non-patchable or misspelled fields

**File:** `crates/agent-memory/src/mcp.rs:154-186`, `crates/agent-memory/src/rest/handlers.rs:101-127`
**Issue:** Neither `UpdateArgs` nor `UpdateBody` uses `deny_unknown_fields`. Two cases:

- `{"content": "x", "type": "DECISION"}` returns 200 with the type unchanged. `type` and
  `source` are documented as immutable, but the caller is told the update succeeded.
- A typo such as `{"content": "x", "ttl_sec": 60}` updates the content and silently drops the
  TTL.

With merge-patch semantics, a silently dropped member is a silent partial write. This violates
the seam's "bad input is always the client tier" rule.

**Fix:** Add `#[serde(deny_unknown_fields)]` to both DTOs. On MCP, `id` is a declared field,
so it is fine. On REST, the explicit `JsonRejection` handling already turns this into a 400.
Add one test per transport for `{"content": "x", "type": "DECISION"}` returning
`invalid_params` / 400.

### WR-03: `relates_to` is documented as symmetric but stored and deduplicated as directed

**File:** `crates/agent-memory-core/src/store/sqlite.rs:710-714, 719-728, 856-905`, `crates/agent-memory-core/src/domain.rs:77`
**Issue:** The primary key is `(from_id, to_id, kind)`, so `link(a, b, relates_to)` and
`link(b, a, relates_to)` create two edges for one symmetric relation. Consequences:

- Expansion lists the neighbor twice, once `out` and once `in`, and both copies count against
  `MAX_EXPANDED_LINKS`.
- `unlink(a, b, relates_to)` reports `unlinked: true` while `b -> a` keeps the "symmetric"
  relation alive.
- The D-02 idempotency promise ("duplicate link is a no-op") fails for the only kind whose
  direction is meaningless.

**Fix:** Canonicalise `relates_to` endpoints at the service seam: store `(min(a,b), max(a,b))`
for `link` and look it up the same way for `unlink`. Optionally, dedupe by neighbor id in
`related()`. Document that the direction shown for `relates_to` is not meaningful.

### WR-04: `memory_list` with `expand_links` and no `limit` is unbounded, contradicting the documented T-04-11 bound

**File:** `crates/agent-memory-core/src/service.rs:194-208`, `crates/agent-memory-core/src/domain.rs:316-321`
**Issue:** The `MAX_EXPANDED_LINKS` doc claims that, together with `MAX_LIMIT`, it "bounds an
expanded response at 200 × 10 summaries (T-04-11)". But `list` with an omitted `limit` stays
unlimited (Pitfall 6). An expanded list therefore returns every memory plus up to 10 neighbors
each, and runs one `related()` query per row inside a single `spawn_blocking`. So the threat
mitigation the plan relies on does not hold for `GET /api/memories?expand_links=true`.

**Fix:** When `expand_links` is set and `limit` is `None`, either apply `DEFAULT_SEARCH_LIMIT`
(or `MAX_LIMIT`) or reject with `InvalidArgument` ("limit is required with expand_links").
Correct the doc comment and the README accordingly.

## Info

### IN-01: The pre-migration backup and the newer-schema refusal are not documented for users

**File:** `README.md` (no section)
**Issue:** The README never mentions `backup`, `migration` or `upgrade`. Users will find
`memories.db.backup-pre-0003-YYYYMMDD` files next to their DB with no explanation, no
restore procedure, and no note that a newer DB refuses an older binary (D-11 to D-13).
**Fix:** Add a short "Upgrading" section covering the backup name pattern, the retention rule,
how to restore (stop the servers, copy the backup over `memories.db`, delete `-wal`/`-shm`),
and the newer-schema error message.

### IN-02: Path and Query extractor rejections on the new routes return plain-text bodies, not `{"error": ...}`

**File:** `crates/agent-memory/src/rest/handlers.rs:287-291, 364-367, 209-211`
**Issue:** `PATCH /api/memories/abc`, `DELETE /api/links/x/2/relates_to` and
`GET /api/memories?expand_links=1` hit axum's default `Path`/`Query` rejection, which returns a
plain-text body. The new handlers deliberately normalise `JsonRejection` to `{"error"}`, so the
error body shape is now inconsistent between body and path failures.
**Fix:** Take `Result<Path<_>, PathRejection>` / `Result<Query<_>, QueryRejection>` the same way
`update_handler` takes the JSON body, and map them to `ApiError::BadRequest`.

### IN-03: The backup step deletes the previous same-version backup before the new one is written

**File:** `crates/agent-memory-core/src/store/sqlite.rs:156-182`
**Issue:** If the new backup then fails (disk full, permissions), the previous pre-0003 backup
is already gone. The DB itself stays untouched because the open aborts, so this is low risk,
but the retention step is not atomic.
**Fix:** Write to `{prefix}{date}.tmp`, then rename it into place, and only then remove the
other `{prefix}*` entries.

### IN-04: Stale comment refers to a "semantic leg" that no longer exists

**File:** `crates/agent-memory-core/src/service.rs:289`
**Issue:** "Same order as the semantic leg" — the semantic-only path was removed. The fallback
warnings at L270-275 still say "semantic search unavailable", which is fine. The comment should
say "the hybrid path".
**Fix:** Reword the comment.

### IN-05: `memory_update` can revive a row whose TTL has passed but which the sweep has not yet removed

**File:** `crates/agent-memory-core/src/store/sqlite.rs:747-791`
**Issue:** `update` does not check `expires_at < now`. A patch with `ttl_secs: null` (or a new
TTL) on an expired row that has not been swept yet brings it back. This matches list/search,
which also show expired-but-unswept rows, but it should be a deliberate choice.
**Fix:** Either treat `expires_at < now` as not-found in `update`, or document the behaviour.

### IN-06: Snippet truncation counts Unicode scalars, not grapheme clusters

**File:** `crates/agent-memory-core/src/domain.rs:192-197`
**Issue:** The cut never splits a UTF-8 code point, but it can split a combining sequence or a
ZWJ emoji, leaving a dangling modifier before the `…`. This is cosmetic.
**Fix:** Accept and document it, or cut at the last whitespace before the limit (D-05 leaves
the choice open).

---

_Reviewed: 2026-10-23_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
