---
phase: 05-portable-export-import
reviewed: 2026-10-25T12:43:01Z
depth: standard
review_kind: phase review (diff f96431b..HEAD, code commits 00d8080..033bd86)
files_reviewed: 19
files_reviewed_list:
  - crates/agent-memory-core/src/domain.rs
  - crates/agent-memory-core/src/export.rs
  - crates/agent-memory-core/src/import/jsonl.rs
  - crates/agent-memory-core/src/import/mod.rs
  - crates/agent-memory-core/src/lib.rs
  - crates/agent-memory-core/src/service.rs
  - crates/agent-memory-core/src/store/mod.rs
  - crates/agent-memory-core/src/store/sqlite.rs
  - crates/agent-memory-core/tests/export_import.rs
  - crates/agent-memory-core/tests/import.rs
  - crates/agent-memory-core/tests/redaction.rs
  - crates/agent-memory-core/tests/relations.rs
  - crates/agent-memory-core/tests/store.rs
  - crates/agent-memory/src/export_file.rs
  - crates/agent-memory/src/main.rs
  - crates/agent-memory/src/mcp.rs
  - crates/agent-memory/src/rest/handlers.rs
  - crates/agent-memory/tests/cli.rs
  - README.md
findings:
  critical: 0
  warning: 3
  info: 3
  total: 6
status: issues_found
---

# Phase 5: Code Review Report

**Reviewed:** 2026-10-25T12:43:01Z
**Depth:** standard
**Files Reviewed:** 19
**Status:** issues_found

## Summary

Reviewed the Phase 5 diff (`f96431b..HEAD`): the format-v1 wire types (`export.rs`), the strict JSONL reader (`import/jsonl.rs`), the `export` / `import_jsonl` service paths and the D-14 `redact_storage_error` seam (`service.rs`), the `insert_row` / `insert_imported` / `find_by_key` / `export_rows` / `Linked { created }` store changes (`store/mod.rs`, `store/sqlite.rs`), the atomic private `--output` writer (`export_file.rs`), the CLI arms (`main.rs`), the two transport pattern updates, the five test files and the README section.

The locked decisions are implemented as written. Checked and found sound: the one-snapshot export (`unchecked_transaction` on one pooled connection, counts and both scans inside it, WAL); `PendingExport` (0600 at `create_new`, temp beside the target, `sync_all` then `rename`, temp removed on every failure path, `BufWriter` flushed inside the blocking task before `commit`); the lenient header probe before the strict parse; `rewrap` lifting `InvalidArgument` payloads under the line prefix; `serde_message` stripping serde's own position; the `--from jsonl` arm rejecting `--scope` and never opening the database on a parse failure; `clock_skew_message` with `saturating_sub`; `insert_row` as the single `memories` INSERT; `tx.changes()` read before commit for `Linked { created }`; every public `MemoryService` method wrapped and `SEARCH_STORAGE_FAILURE` text unchanged; no `unwrap()` / `expect()` outside tests; bound SQL only; no new crates; migrations untouched. Hostile-input probes (deep nesting, 1 MiB of `{`, NUL, i64 overflow in timestamps, negative / fractional / string `version`, duplicate keys, dangling link keys, self-links by file key) are all rejected or stored as data without panicking, and the CLI test proves invalid UTF-8 is refused at `read_to_string`.

Three warnings remain, all in the import pass. The most important one is a legitimate-export case the kill-tests do not cover: two rows identical on the dedup key collapse to one on import (D-07, accepted), but a link *between* those two rows then becomes a self-link that bypasses the service's self-link seam and is silently swallowed by `INSERT OR IGNORE` (SQLite's `OR IGNORE` applies to `CHECK` violations), so it is reported as a skipped duplicate. The other two are an unbounded `access_count` that can poison the row so every later `export` of the database fails, and the dedup probe matching DB rows that are already expired-but-unswept, so a restore can attach imported edges to a row the next sweep deletes.

No `<structural_findings>` block was supplied, so there is no fallow section. The 04-REVIEW carry-forwards (IN-02, IN-04, IN-05, IN-06, WR-01) are unchanged by this phase and are not re-raised; IN-01 is closed by D-14.

## Narrative Findings (AI reviewer)

## Warnings

### WR-01: A link between two rows that collapse on import becomes a self-link that `INSERT OR IGNORE` silently swallows and the report mis-counts

**File:** `crates/agent-memory-core/src/service.rs:677-686` (pass 3 calls `store.link` directly), `crates/agent-memory-core/src/store/sqlite.rs:1161-1171` (`INSERT OR IGNORE` + `changes() == 1`), `crates/agent-memory-core/sql/0003_relations.sql:19` (`CHECK (from_id != to_id)`)

**Issue:** The parser validates `from != to` on **file keys** (`jsonl.rs:338`), but D-07 then maps keys to ids through `find_by_key`, and two distinct keys can map to the **same** id. That is not exotic: `store` has no uniqueness constraint, so a user who stored the same `(source, type, content)` twice and linked the two rows produces a perfectly valid export (`identical_rows_collapse_to_one_on_import_and_count_a_duplicate` builds exactly this corpus, minus the link). On import both keys resolve to one id, and pass 3 calls `store.link(N, N, kind, ts)`:

- the service-seam `validate_link_endpoints` is bypassed (the importer calls the store directly), so the only guard is the 0003 `CHECK`;
- `INSERT OR IGNORE` treats a `CHECK` violation as an ignorable conflict (SQLite: `OR IGNORE` applies to `UNIQUE`, `NOT NULL`, `CHECK` and `PRIMARY KEY`), so no error is raised and `changes()` is 0 — verified on this machine's SQLite: `INSERT OR IGNORE` into a `CHECK(f <> t)` table with `(1,1)` yields `changes() = 0` and no row;
- the importer therefore returns `Linked { created: false }` and counts the edge as `links_skipped_duplicates`, which the README defines as "edges that already existed between the same two rows". Nothing existed; the edge was lost, and the report claims idempotent success.

Also note `relates_to`'s `canonical_endpoints` makes `(N, N)` in both orders, so this is independent of kind.

**Fix:** Treat a post-remap self-pair as a drop (it is the link-level consequence of the D-07 collapse) and never reach the store with it:

```rust
// service.rs, pass 3
match (ids.get(&from), ids.get(&to)) {
    (Some(a), Some(b)) if a == b => {
        // Both endpoints collapsed onto one row (D-07): the edge has no
        // second endpoint any more. Count it as dropped, like an expired
        // endpoint, instead of handing the store a self-link that
        // INSERT OR IGNORE would swallow as a "duplicate".
        report.links_dropped += 1;
    }
    (Some(a), Some(b)) => match store.link(*a, *b, kind, created_at)? { /* unchanged */ },
    (None, _) | (_, None) => report.links_dropped += 1,
}
```

Add a kill-test next to `identical_rows_collapse_to_one_on_import_and_count_a_duplicate`: store the twin twice, `link(first, second, RelatesTo)`, export, import into an empty DB, assert `six(1, 1, 0, 0, 0, 1)` and `edge_count == 0`. Update the README `links dropped` row ("edges whose endpoint was skipped as expired **or collapsed into its other endpoint**").

### WR-02: An unbounded `access_count` in the file can poison a row so that every later `export` of the database fails

**File:** `crates/agent-memory-core/src/import/jsonl.rs:293-330` (no range check on `access_count`), `crates/agent-memory-core/src/store/sqlite.rs:1386` (`access_count = access_count + 1`), `crates/agent-memory-core/src/store/sqlite.rs:891` (`row.get("access_count")` as `i64`)

**Issue:** `access_count` is carried verbatim (D-10) with no bounds: `9223372036854775807` and negative values both parse and land in the row. The first search or update that bumps the row executes `access_count + 1`; SQLite does not raise on integer overflow in `+`, it converts the result to `REAL` (verified: `SELECT typeof(a) ... UPDATE t SET a = a + 1` on `i64::MAX` gives `real|9.22337203685478e+18`). rusqlite's `FromSql for i64` only accepts `Integer`, so the next `export_rows` fails on that row with `InvalidColumnType`, which `redact_storage_error` turns into `export failed due to a storage error (details are in the server log)` — for the **whole** database, persistently, until someone repairs the column by hand. `list`/`search` keep working because `row_to_view` does not read `access_count`, so the damage is invisible until the user tries to back up. One hand-edited line in a file the README calls "agent-memory's own contract" therefore breaks the tool's only portable backup path. `created_at` and `expires_at` are only compared or echoed, so they are safe; `access_count` is the one preserved column that is later used in arithmetic.

**Fix:** Validate it in pass 1 like every other field, under the D-11 line prefix:

```rust
// jsonl.rs, Memory arm, after the content check
if access_count < 0 {
    return Err(invalid_line(line_no, format!("access_count must be >= 0 (got {access_count})")));
}
```

and make the bump saturate so no path can ever push the column out of integer range:

```sql
UPDATE memories SET last_accessed = ?1,
    access_count = CASE WHEN access_count < 9223372036854775807 THEN access_count + 1 ELSE access_count END
WHERE id IN (...)
```

(the same expression at `sqlite.rs:1271` for `update`). Add the `i64::MAX` and `-1` cases to `malformed_files_are_rejected_naming_the_line` / `hostile_input_never_panics`.

### WR-03: The dedup probe matches target rows that are already expired-but-unswept, so a restore can attach imported edges to a row the next sweep deletes

**File:** `crates/agent-memory-core/src/service.rs:648-666` (expiry skip uses the **file** row only; `find_by_key` has no `now`), `crates/agent-memory-core/src/store/sqlite.rs:926-945` (`find_by_key` has no expiry predicate)

**Issue:** D-10 skips a *file* row whose `expires_at <= now`, but the dedup probe that runs right after it is blind to the *target* row's expiry. A CLI import runs no sweep, so a target row whose TTL lapsed while no server was running is still present. If the file carries the same `(source, type, content)` with a later or null `expires_at`, the importer maps the key onto the expired row, counts `skipped_duplicates`, and pass 3 links every imported neighbour to it (`links_imported`). The next `serve` tick's `sweep_expired` then deletes that row and cascades those freshly imported edges away. The report said "skipped duplicate, N links imported"; the corpus ends with neither. This was latent in the gsd-state path (04 code), but Phase 5 makes it reachable: JSONL rows carry absolute `expires_at`, and links now ride on the mapping.

**Fix:** Make the probe see the same world the file policy sees. Either give `find_by_key` the clock and exclude expired rows:

```rust
fn find_by_key(&self, source: Option<&str>, mem_type: MemoryType, content: &str, now: i64)
    -> Result<Option<i64>, MemoryError>;
// SQL: ... AND content = ?3 AND (expires_at IS NULL OR expires_at > ?4) ORDER BY id LIMIT 1
```

(then the file's live copy is inserted and the stale row is swept on its own), or, if the Phase 4 trait shape must stay, run `store.sweep_expired(now)?` as the first statement of the `import_jsonl` blocking hop so the target has no expired rows when the probe runs. Either way add a test: store with `ttl_secs = 1`, advance the `TestClock` past it without sweeping, import a file carrying that content with `expires_at: null` plus a link to it, sweep, and assert the memory and its edge survive.

## Info

### IN-01: `find_by_key` uses `LIMIT 1` with no `ORDER BY`, so the key-to-id mapping depends on scan order when duplicates exist

**File:** `crates/agent-memory-core/src/store/sqlite.rs:926-945`

**Issue:** The INTEROP-01 probe only ever answered "exists?", where any row would do. D-07 now reads its **id** and hangs edges on it. With duplicate rows (which `store` permits) the row chosen is whatever SQLite steps first; today that is rowid order because no index covers `(source, mem_type, content)`, but that is a planner accident, not a contract, and a future index would change which duplicate gets the imported edges between two runs.

**Fix:** `... LIMIT 1` → `... ORDER BY id LIMIT 1`, and say "the lowest matching id" in the trait doc.

### IN-02: No test proves `export` and `import_jsonl` are redacted (T-05-17 / T-05-20 rest on the wrapper alone)

**File:** `crates/agent-memory-core/tests/redaction.rs` (covers store, list, import, forget, link, unlink, update, sweep), `crates/agent-memory-core/tests/export_import.rs` (no storage-fault case)

**Issue:** The two new service paths are wrapped (`service.rs:716-724`, `622-626`), but neither has the `take_offline` proof the other eight paths have, so a future refactor that unwraps one would not turn anything red. `export` additionally has a second error family (`Internal("export write failed: …")`) whose pass-through is also untested at the service level.

**Fix:** Add two tests to `redaction.rs`: rename `memories` away, call `service.export(Box::new(Vec::new()), "t".into())`, `assert_redacted(&err, "export")`; rename `memory_links` away, call `import_jsonl` on a parsed file with one link, `assert_redacted(&err, "import")`.

### IN-03: `run_export` duplicates `open_service` and opens (or creates) the database before the `--output` path is validated

**File:** `crates/agent-memory/src/main.rs:348-357` (duplicate of `open_service`, 318-329), `crates/agent-memory/src/main.rs:368` (`export_file::begin` after the open)

**Issue:** The wiring block is copied rather than reused, and the order means `agent-memory export --output /nonexistent/dir/out.jsonl` first resolves, creates and migrates the default database and only then fails on `create_new`. The `import --from jsonl` arm deliberately validates before opening; `export` should match.

**Fix:** Call `export_file::begin(&path)` before `open_service(db_flag, ollama_url)?` (abort the pending temp if the open fails), and replace lines 348-357 with `let service = open_service(db_flag, ollama_url)?;`.

---

_Reviewed: 2026-10-25T12:43:01Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
