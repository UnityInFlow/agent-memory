---
phase: 05-portable-export-import
plan: 01
subsystem: core-service, store
tags: [rust, sqlite, rusqlite, redaction, error-tiering, store-contract, import, relations]

# Dependency graph
requires:
  - phase: 04-memory-update-relations-hybrid-search
    provides: "redact_search_error / SEARCH_STORAGE_FAILURE (T-04-19), Store::exists with NULL-safe source IS ?1 (CR-01), LinkOutcome + INSERT OR IGNORE link (D-02), the two exhaustive transport error mappers"
  - phase: 03-api-hardening-toolchain-spikes
    provides: "MemoryError::InvalidArgument client tier at the MemoryService seam (D-04/D-06)"
provides:
  - "redact_storage_error(op, e) applied to every public MemoryService method (store, list, search, import, forget, link, unlink, update, sweep) via the *_unredacted inner-fn shape — closes 04-REVIEW IN-01 (D-14)"
  - "tests/redaction.rs: eight fault-injection proofs (ALTER TABLE ... RENAME) plus REST /health, POST /api/memories and MCP memory_store in-process proofs with no `sqlite` substring"
  - "Store::find_by_key(source, mem_type, content) -> Option<i64> replacing Store::exists (D-07)"
  - "LinkOutcome::Linked { created: bool } read from tx.changes() before commit (D-12 countability)"
  - "ImportedMemory + Store::insert_imported(m, now): preserved created_at/access_count/expires_at, last_accessed = now, embedding_status = 0, no vector row (D-10, D-13)"
  - "one private insert_row(tx, &MemoryRow) helper holding THE single `memories` INSERT behind insert and insert_imported (T-05-15)"
  - "README: redaction sentence widened to every surface; 'Behavioral changes in v0.1.0' item 4"
affects: [05-02 export tracer, 05-03 import tracer, 05-04 docs + CLI realism, phase-6 release notes]

# Actuals (#2632) — same estimateTokens scale as the plan's `estimate` (chars/4 over the realized diff)
actuals:
  tokens: 10400
  tasks: 3
  commits: 3

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Service-seam error tiering: every public MemoryService method is `self.<name>_unredacted(..).await.map_err(|e| redact_storage_error(\"<name>\", e))`; a tail map_err on a ?-laden body is never acceptable (early returns would escape unredacted)"
    - "Fault injection for storage-tier tests: ALTER TABLE <t> RENAME TO <t>_offline on a raw side connection (never DROP); rename memory_links instead of memories when the endpoint pre-check must still pass"
    - "One INSERT per table: a private row struct + helper (MemoryRow / insert_row) shared by every insert path so column order and defaults cannot drift"

key-files:
  created:
    - crates/agent-memory-core/tests/redaction.rs
  modified:
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory-core/src/domain.rs
    - crates/agent-memory-core/src/store/mod.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory-core/tests/relations.rs
    - crates/agent-memory-core/tests/store.rs
    - crates/agent-memory/src/rest/handlers.rs
    - crates/agent-memory/src/mcp.rs
    - README.md

key-decisions:
  - "Redaction lives at the MemoryService seam (not in the two transport mappers, which 04-REVIEW IN-01 suggested): one change covers MCP, REST including /health, and the CLI; the mappers are byte-identical and still match every MemoryError variant exhaustively"
  - "The nine op strings are store, list, search, import, forget, link, unlink, update, sweep; SEARCH_STORAGE_FAILURE keeps its exact text and a unit test pins it to the template at op = \"search\" so Phase 4 goldens cannot drift"
  - "LinkOutcome::Linked gained `created: bool` across all 7 sites (store impl, 2 transport arms, 4 relations.rs asserts) — storage-only, not a wire change: transports match `Linked { .. }` and emit byte-identical JSON"
  - "ImportedMemory is a dedicated type + Store::insert_imported (zero of the 15 NewMemory construction sites touched), with no embedding parameter so a restored row can never land with a vector"
  - "The two expiry predicates are named in the trait doc: the importer skips `expires_at <= now`, the sweep deletes `expires_at < now`"
  - "Tracer feedback gate resolved by re-running the fully automated <verify> (all green) rather than returning a mid-plan human-verify checkpoint, because project config sets human_verify_mode: end-of-phase and the tracer's verify contains no human-judgment step"

patterns-established:
  - "redact_storage_error(op, e): Sqlite | Pool | Join | Migration -> tracing::error!(op, error = %e, \"storage error\") then Internal(\"<op> failed due to a storage error (details are in the server log)\"); client tier and crate-authored text pass through; no catch-all arm"
  - "Plans 02/03 add op = \"export\" and reuse op = \"import\" through the same helper and inner-fn shape"

requirements-completed: [INTEROP-02]

# Coverage metadata (#1602)
coverage:
  - id: D1
    description: "A storage fault on any of the nine MemoryService paths reaches REST (incl. /health), MCP and the CLI only as `<op> failed due to a storage error (details are in the server log)`; no `sqlite` substring; SEARCH_STORAGE_FAILURE byte-identical"
    requirement: INTEROP-02
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/redaction.rs (8 tests: store, list, import, forget, link, unlink, update, sweep)"
        status: pass
      - kind: unit
        ref: "crates/agent-memory-core/src/service.rs#storage_error_redaction_search_text_equals_the_constant, #storage_error_redaction_passes_client_and_crate_text_through"
        status: pass
      - kind: integration
        ref: "cargo test -p agent-memory --bin agent-memory storage_failure (health_storage_failure_is_500_without_sqlite_text, store_storage_failure_is_500_without_sqlite_text, memory_store_storage_failure_is_internal_error_without_sqlite_text + 2 existing search tests)"
        status: pass
      - kind: integration
        ref: "cargo test -p agent-memory-core --test hybrid (SEARCH_STORAGE_FAILURE equality goldens unchanged)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Store::find_by_key returns Some(id) for the exact (source, mem_type, content) key with NULL-safe source equality, None otherwise; import dedups through it"
    requirement: INTEROP-02
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/store.rs#find_by_key_returns_the_matching_id_null_safely"
        status: pass
      - kind: integration
        ref: "cargo test -p agent-memory-core --test import (idempotent re-import unchanged)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Store::link reports Linked { created: true } on a fresh edge and Linked { created: false } on a duplicate or symmetric relates_to re-link; MCP/REST link JSON unchanged"
    requirement: INTEROP-02
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/relations.rs#link_succeeds_and_a_duplicate_link_is_an_idempotent_no_op, #relates_to_linked_both_ways_is_one_relation_listed_once_from_each_side"
        status: pass
      - kind: integration
        ref: "cargo test -p agent-memory --bin agent-memory link (15 tests, JSON assertions untouched)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Store::insert_imported lands a row with preserved created_at/access_count/expires_at, last_accessed = now, decay_score 1.0, base_weight from the type pin, embedding_status 0 and no vector row, visible via list and keyword search"
    requirement: INTEROP-02
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/store.rs#insert_imported_preserves_provenance_and_lands_pending"
        status: pass
      - kind: integration
        ref: "cargo test -p agent-memory-core --test store --test ttl --test decay --test update --test backfill_race (insert refactor behaviour-preserving)"
        status: pass
    human_judgment: false
  - id: D5
    description: "README scopes storage-error redaction to every request the daemon serves and every CLI import/export, and lists it as behavioral change 4 for v0.1.0"
    requirement: INTEROP-02
    verification:
      - kind: other
        ref: "grep -c 'failed due to a storage error (details are in the server log)' README.md -> 2; sed -n '/^## Behavioral changes/,/^## /p' README.md | grep -c '^4\\. ' -> 1"
        status: pass
    human_judgment: true
    rationale: "Presence is grep-proven, but whether the release-note wording is clear enough for clients that parsed `sqlite error: …` strings is an editorial judgment"

# Metrics
duration: 14min
completed: 2026-10-25
status: complete
---

# Phase 5 Plan 01: Redact storage-engine text on every service path; teach the store the format-v1 contract Summary

**`redact_storage_error(op, e)` on all nine `MemoryService` methods (closing 04-REVIEW IN-01 on REST, MCP and CLI with 13 in-process proofs), plus the store contract the export/import tracers build on: `Store::find_by_key -> Option<i64>`, `LinkOutcome::Linked { created }`, and `ImportedMemory` + `Store::insert_imported` behind one shared `insert_row`.**

## Performance

- **Duration:** 14 min
- **Started:** 2026-10-25T11:24:54Z
- **Completed:** 2026-10-25T11:39:13Z
- **Tasks:** 3
- **Files modified:** 10 (1 created, 9 modified; +928 / −69 lines)

## Accomplishments

- **D-14 landed before any new write path exists.** `redact_search_error` became `redact_storage_error(op, e)`; every public `MemoryService` method — `store`, `list`, `search`, `import`, `forget`, `link`, `unlink`, `update`, `sweep` — now delegates to an `async fn <name>_unredacted` body and maps its result with the op string, so no early `?` return can escape unredacted. The storage tier (`Sqlite | Pool | Join | Migration`) is logged at error level with full detail and replaced by `Internal("<op> failed due to a storage error (details are in the server log)")`; client tier and crate-authored text pass through. `SEARCH_STORAGE_FAILURE` keeps its exact text and a unit test pins it to the template at `op = "search"`. Both transport mappers are byte-identical.
- **Proven on every surface class, in process.** `tests/redaction.rs` takes `memories` (or `memory_links` for `link`/`unlink`, so the endpoint pre-check still passes) offline with the T-04-19 `ALTER TABLE … RENAME` recipe and asserts the exact redacted text and no `sqlite` substring for all eight non-search ops. REST `GET /health` and `POST /api/memories` return 500 with `list failed…` / `store failed…`; MCP `memory_store` returns `internal_error` with `store failed…`. All Phase 4 goldens (`hybrid.rs` equality on the constant, the two existing search redaction tests) are unchanged and green.
- **`Store::find_by_key` replaces `Store::exists`** (one caller, one impl): same NULL-safe `source IS ?1` three-column key (CR-01), now `SELECT id` so plan 03's importer can map an export-local key onto the existing row (D-07). `import()` dedups through `.is_some()`; `tests/import.rs` is untouched and green.
- **`LinkOutcome::Linked { created: bool }`** read from `tx.changes() == 1` before `commit`. Seven sites updated: the store impl, the MCP and REST arms (`Linked { .. }`, JSON byte-identical), and four `relations.rs` asserts that are now stronger idempotency proofs (`created: true` on the first insert, `created: false` on the D-02 duplicate and the symmetric `b→a` re-link). Not a wire change.
- **`ImportedMemory` + `Store::insert_imported(m, now)`** — a dedicated type (zero of the 15 `NewMemory` construction sites touched) carrying absolute `created_at`, `access_count`, `expires_at`; the store writes `last_accessed = now`, `decay_score = 1.0`, `base_weight` from the pin, `embedding_status = 0` and NO vector row (D-10, D-13). The single `memories` INSERT moved into a private `insert_row(tx, &MemoryRow)` helper used by both `insert` and `insert_imported`, with `access_count`/`created_at`/`last_accessed` bound separately (today's `?7` double-bind and literal `0` are gone). `grep -v '^\s*//' sqlite.rs | grep -c "INTO memories "` is exactly 1. The trait doc names both expiry predicates (importer `<= now` skip vs sweep `< now` delete).
- **README:** the search-only redaction sentence is widened to "any request the daemon serves — and any CLI `import`/`export`"; "Behavioral changes in v0.1.0" now lists four changes, item 4 telling clients that parsed `sqlite error: …` strings to stop.
- **No migration, no dependency change:** `sql/` diff empty, `LATEST_SCHEMA_VERSION = 3`, `Cargo.lock` byte-identical under `--locked`.

## Task Commits

Each task was committed atomically (fmt + clippy `-D warnings` + full workspace suite green before each):

1. **Task 1 (tracer): A storage fault on any service path reaches REST, MCP and the CLI only as the redacted internal error** — `00d8080` (feat)
2. **Task 2: The dedup probe returns the matching row id, and a link insert reports whether it created the edge** — `9ee42ed` (feat)
3. **Task 3: The store accepts a restored memory with its provenance preserved and always pending embedding** — `5994d38` (feat)

**Plan metadata:** see the `docs(05-01)` commit that follows this summary.

## Files Created/Modified

- `crates/agent-memory-core/src/service.rs` — `redact_storage_error(op, e)` (exhaustive, no catch-all), nine `*_unredacted` inner fns, `SEARCH_STORAGE_FAILURE` doc reworded, `#[cfg(test)] mod tests` with the two pinning tests; `import_unredacted` dedups via `find_by_key(..)?.is_some()`
- `crates/agent-memory-core/tests/redaction.rs` (new) — harness copied from relations.rs, `take_offline(db_path, table)`, `assert_redacted(err, op)`, eight `#[tokio::test]`s
- `crates/agent-memory-core/src/domain.rs` — `LinkOutcome::Linked { created: bool }`; `pub struct ImportedMemory` directly after `NewMemory`
- `crates/agent-memory-core/src/store/mod.rs` — `fn find_by_key(..) -> Result<Option<i64>, _>` (CR-01 paragraph kept, D-07 sentence added); `fn insert_imported(m, now)` with the preserved/refreshed column list and both expiry predicates; `link` doc reworded to `created: false`
- `crates/agent-memory-core/src/store/sqlite.rs` — `struct MemoryRow<'a>` + `fn insert_row`; `insert` rebuilt on it (behaviour identical); `insert_imported`; `find_by_key` (`SELECT id`, same `source IS ?1`); `link` reads `tx.changes()` before commit
- `crates/agent-memory-core/tests/relations.rs` — four asserts upgraded to `Linked { created: true | false }`
- `crates/agent-memory-core/tests/store.rs` — `find_by_key_returns_the_matching_id_null_safely`, `insert_imported_preserves_provenance_and_lands_pending` (+ `RawRow`/`raw_row`/`vec_rows` raw-connection helpers)
- `crates/agent-memory/src/rest/handlers.rs` — `Linked { .. }` arm; tests `health_storage_failure_is_500_without_sqlite_text`, `store_storage_failure_is_500_without_sqlite_text` (+ `take_memories_offline` helper); `map_memory_error` unchanged
- `crates/agent-memory/src/mcp.rs` — `Linked { .. }` arm; test `memory_store_storage_failure_is_internal_error_without_sqlite_text`; `map_mcp_error` unchanged
- `README.md` — widened redaction sentence; "Four deliberate contract changes" + item 4

## Decisions Made

- **Seam, not mappers.** 04-REVIEW IN-01 proposed redacting inside the two transport mappers; the plan (D-14) chose the service seam and that is what shipped: one helper, nine call sites, MCP + REST (`/health` included) + CLI covered together, mappers untouched. Widening/narrowing the redacted set later is a local edit.
- **The nine op strings** are exactly `store`, `list`, `search`, `import`, `forget`, `link`, `unlink`, `update`, `sweep`. Plans 02/03 add `export` and reuse `import`.
- **`insert_row` takes a struct, not positional args** — keeps the helper under clippy's `too_many_arguments` limit and makes a transposed column a compile error rather than a silent swap.
- **Tracer feedback gate:** auto mode is off, but `.planning/config.json` sets `human_verify_mode: "end-of-phase"` and the tracer's `<verify>` is wholly `<automated>`. I re-ran it end-to-end after the Task 1 commit (lib redaction tests, `--test redaction`, `--test hybrid`, `--bin agent-memory storage_failure`, workspace clippy — all green) and continued to the expansion tasks instead of returning a mid-plan `checkpoint:human-verify`. Recorded here so the orchestrator can route the end-of-phase UAT.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Renamed the two service.rs unit tests so the acceptance grep pins exactly one helper**
- **Found during:** Task 1 (acceptance-criteria gate)
- **Issue:** The plan prescribes the test names `redact_storage_error_search_text_equals_the_constant` / `redact_storage_error_passes_client_and_crate_text_through` AND the criterion `grep -c "fn redact_storage_error" service.rs` → `1`. Both cannot hold: the test names begin with `fn redact_storage_error`, so the grep returned 3.
- **Fix:** Tests renamed to `storage_error_redaction_search_text_equals_the_constant` and `storage_error_redaction_passes_client_and_crate_text_through`; bodies exactly as specified in `<behavior>`.
- **Files modified:** `crates/agent-memory-core/src/service.rs`
- **Verification:** grep → 1; `cargo test -p agent-memory-core --lib storage_error_redaction` 2 passed
- **Committed in:** `00d8080`

**2. [Rule 3 - Blocking] Reflowed two README passages so the redaction template sits on one line**
- **Found during:** Task 1 (acceptance-criteria gate)
- **Issue:** The widened sentence and item 4 wrapped `… storage error\n(details are in the server log)`, so the one-line grep `failed due to a storage error (details are in the server log)` returned 0.
- **Fix:** Line breaks moved so the full template phrase is on a single line in both places (wording unchanged).
- **Files modified:** `README.md`
- **Verification:** grep → 2; `sed -n '/^## Behavioral changes/,/^## /p' README.md | grep -c '^4\. '` → 1
- **Committed in:** `00d8080`

### Process notes (not code deviations)

- **TDD RED/GREEN without a separate RED commit.** All three tasks are `tdd="true"`. RED was executed and observed — Task 1's eight redaction tests were run before implementation and failed with the raw `Sqlite(… no such table …)` leak IN-01 describes; Tasks 2/3 introduce new API (`find_by_key`, `Linked { created }`, `insert_imported`) so their tests fail to compile against the old code by construction. The failing tests were NOT committed on their own because the project operating rules forbid committing failing tests and the plan calls for exactly one commit per task; each task's `feat(05-01)` commit carries test + implementation together.

---

**Total deviations:** 2 auto-fixed (both Rule 3, both plan-internal criterion conflicts; no behaviour change)
**Impact on plan:** None on scope or behaviour. Both fixes exist only to make the plan's own acceptance greps pass as written.

## Issues Encountered

None. Every targeted `<verify>` and the full workspace suite passed on the first run after each implementation (post-RED); no fix-attempt budget was used.

## Verification

Plan-level `<verification>` re-run after the third commit:

- `cargo fmt --all -- --check` — clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean; `cargo test --workspace --locked` — **19 suites, 232 passed, 0 failed, 2 ignored** (230 after Task 1, 231 after Task 2, 232 after Task 3).
- `cargo test -p agent-memory-core --test redaction` — 8 passed; `cargo test -p agent-memory --bin agent-memory storage_failure` — 5 passed (health, store on REST, memory_store on MCP, plus the two existing search tests).
- `cargo test -p agent-memory-core --test relations --test import --test store` — 27 + 2 + 7 passed with the stronger `created` asserts and the two new store tests.
- `git diff --stat -- crates/agent-memory-core/sql/` empty; `Cargo.lock` unchanged; `LATEST_SCHEMA_VERSION: i64 = 3`.
- Transport mappers: `git diff` of `handlers.rs` / `mcp.rs` across the plan is confined to the `#[cfg(test)]` modules plus the two `Linked { .. }` arms.

## Known Stubs

None — no placeholder values, TODO/FIXME markers, skipped tests, or unwired data paths were introduced.

## Threat Flags

None. No new network endpoint, auth path, file access or schema surface was added; T-05-01 (information disclosure via engine text) is mitigated as planned, T-05-15 (divergent INSERTs) is structurally closed by `insert_row`, T-05-16's two predicates are fixed in the trait doc for plan 03 to enforce.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- **Plan 05-02 (export tracer)** can add `MemoryService::export` as `export_unredacted` + `redact_storage_error("export", e)` and read `meta` through the store; the inner-fn shape and op-string convention are established.
- **Plan 05-03 (import tracer)** compiles against a tested contract: `find_by_key` for key→existing-id mapping (D-07), `insert_imported` for restored rows (D-10/D-13, caller owns the `expires_at <= now` skip), and `Linked { created: false }` to count `links_skipped_duplicates` (D-12).
- **Plan 05-04 (docs)** inherits a README that already states the every-surface redaction and behavioral change 4; it only needs the "Export & import" section.
- No blockers. `.planning/config.json` and the untracked harness files were left unstaged as instructed.

---
*Phase: 05-portable-export-import*
*Completed: 2026-10-25*

## Self-Check: PASSED

All 6 key files exist on disk; task commits 00d8080, 9ee42ed, 5994d38 are present in git history.
