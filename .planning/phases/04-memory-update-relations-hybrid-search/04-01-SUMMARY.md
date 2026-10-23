---
phase: 04-memory-update-relations-hybrid-search
plan: 01
subsystem: database
tags: [rust, sqlite, rusqlite, rusqlite_migration, migrations, backup-api, thiserror, mcp, axum]

# Dependency graph
requires:
  - phase: 03-api-hardening-toolchain-spikes
    provides: "MemoryError::InvalidArgument seam, two-tier exhaustive mappers, bounds consts (MAX_LIMIT), 03-REVIEW findings CR-01/WR-01/WR-02"
provides:
  - "MemoryError::Internal(String): 500 / internal_error tier, used at all six writer-lock sites (WR-02)"
  - "MemoryError::SchemaTooNew { db_version, binary_version }: startup-only D-13 refusal"
  - "MemoryError::NotFound and ApiError::NotFound deleted; not-found is data only"
  - "Store::exists(source: Option<&str>, ..) with NULL-safe `source IS ?1` (CR-01)"
  - "MAX_KNN_K = MAX_LIMIT * 4 (800): 4x oversample at every limit (WR-01 partial)"
  - "Migration 0003: memory_links (from_id, to_id, kind, created_at), PK(from_id,to_id,kind), FK ON DELETE CASCADE, CHECK from!=to, WITHOUT ROWID, idx_links_to"
  - "pub const LATEST_SCHEMA_VERSION: i64 = 3 (store/migrations.rs)"
  - "SqliteStore::open: probe -> refuse-too-new -> pragmas -> sibling online backup -> migrate"
  - "Committed binary fixture tests/fixtures/v0.0.1.db + tests/migration_hygiene.rs"
affects: [04-02 memory_update, 04-03 relations/link/unlink/expansion, 04-04 hybrid RRF, 05 export/import format, 06 release notes]

# Actuals (#2632)
actuals:
  tokens: 8465
  tasks: 2
  commits: 3

# Tech tracking
tech-stack:
  added: ["rusqlite `backup` cargo feature (no new crate; 0.39 pin unchanged)"]
  patterns:
    - "Probe PRAGMA user_version BEFORE any per-connection PRAGMA, so a refused DB stays byte-for-byte untouched"
    - "Generic pre-migration backup keyed by target schema version: {db}.backup-pre-{NNNN}-{YYYYMMDD}, one per version, abort-on-failure"
    - "Schema freeze via a committed binary fixture: migrate a copy, diff normalized sqlite_master against fresh-from-zero"
    - "Server faults are MemoryError::Internal; not-found is Ok(false)/Ok(None) data, never an error variant"

key-files:
  created:
    - crates/agent-memory-core/sql/0003_relations.sql
    - crates/agent-memory-core/tests/migration_hygiene.rs
    - crates/agent-memory-core/tests/fixtures/v0.0.1.db
  modified:
    - Cargo.toml
    - crates/agent-memory-core/src/domain.rs
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory-core/src/store/mod.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory-core/src/store/migrations.rs
    - crates/agent-memory-core/tests/import.rs
    - crates/agent-memory/src/mcp.rs
    - crates/agent-memory/src/rest/handlers.rs
    - crates/agent-memory/src/rest/mod.rs

key-decisions:
  - "D-13 probe runs before prepare_connection (not after it): the journal_mode=WAL pragma rewrites header bytes on a non-WAL file, which would break the 'DB untouched' guarantee"
  - "Backup failure aborts SqliteStore::open with MemoryError::Internal naming the backup path; no migration runs (T-04-02)"
  - "Retention step removes every {db}.backup-pre-{NNNN}-* entry for the target version and errors if one cannot be removed. Other versions' backups are never touched"
  - "Backup date stamp comes from chrono::Utc::now().date_naive() via Datelike, which needs no extra chrono feature (UTC only, no Local)"
  - "ApiError::NotFound deleted along with MemoryError::NotFound: REST 404s are built from not-found data (forget's Ok(false)), so a server fault can no longer be mis-tiered as a 404"
  - "Fixture is a WAL-header DB with 2 planted rows (one source-less), generated once by an #[ignore]d, exists-guarded test from the tag-identical 0001+0002"

patterns-established:
  - "Migration hygiene: every future migration inherits probe -> backup -> migrate by bumping LATEST_SCHEMA_VERSION with the new M::up"
  - "New writer methods must map lock poisoning to MemoryError::Internal(\"writer lock poisoned\")"

requirements-completed: [STORE-05]

coverage:
  - id: D1
    description: "Poisoned writer lock maps to the internal tier on both transports (REST 500, MCP internal_error); MemoryError::NotFound deleted"
    verification:
      - kind: unit
        ref: "crates/agent-memory/src/mcp.rs#map_mcp_error_splits_client_and_internal_tiers"
        status: pass
      - kind: unit
        ref: "crates/agent-memory/src/rest/handlers.rs#map_memory_error_puts_internal_faults_in_the_500_tier"
        status: pass
    human_judgment: false
  - id: D2
    description: "Re-importing a source:None draft is idempotent (CR-01)"
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/import.rs#reimporting_a_sourceless_draft_is_idempotent"
        status: pass
    human_judgment: false
  - id: D3
    description: "MAX_KNN_K = MAX_LIMIT * 4 = 800 keeps the 4x KNN oversample at every valid limit (WR-01 partial)"
    verification:
      - kind: unit
        ref: "crates/agent-memory-core/src/store/sqlite.rs#max_knn_k_keeps_four_x_oversample_at_max_limit"
        status: pass
    human_judgment: false
  - id: D4
    description: "Shipped 0001/0002 are frozen: the committed v0.0.1 fixture migrated to latest equals a fresh DB's normalized schema"
    requirement: STORE-05
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/migration_hygiene.rs#fixture_migrated_to_latest_matches_a_fresh_database"
        status: pass
      - kind: integration
        ref: "crates/agent-memory-core/tests/migration_hygiene.rs#committed_fixture_is_the_v0_0_1_schema"
        status: pass
      - kind: unit
        ref: "crates/agent-memory-core/src/store/migrations.rs#latest_schema_version_matches_the_migration_chain"
        status: pass
    human_judgment: false
  - id: D5
    description: "Upgrading a v0.0.1 DB writes one sibling online backup (user_version 2, pre-upgrade rows), one per schema version; a fresh DB gets none; a failed backup aborts before migrating"
    requirement: STORE-05
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/migration_hygiene.rs#upgrading_writes_one_pre_migration_backup_with_the_old_schema"
        status: pass
      - kind: integration
        ref: "crates/agent-memory-core/tests/migration_hygiene.rs#re_running_the_upgrade_keeps_one_backup_per_schema_version"
        status: pass
      - kind: integration
        ref: "crates/agent-memory-core/tests/migration_hygiene.rs#opening_a_fresh_database_creates_no_backup"
        status: pass
      - kind: integration
        ref: "crates/agent-memory-core/tests/migration_hygiene.rs#a_failed_backup_aborts_the_open_before_migrating"
        status: pass
    human_judgment: false
  - id: D6
    description: "A newer-schema DB is refused with a message naming 99, 3 and the brew upgrade command; the file is byte-untouched; the binary prints it on stderr with empty stdout"
    requirement: STORE-05
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/migration_hygiene.rs#a_newer_schema_database_is_refused_and_left_untouched"
        status: pass
      - kind: manual_procedural
        ref: "agent-memory --db <user_version=99 db> serve </dev/null -> exit 1, stdout 0 bytes, stderr names both versions + brew command, shasum unchanged"
        status: pass
    human_judgment: false

# Metrics
duration: 9min
completed: 2026-10-23
status: complete
---

# Phase 4 Plan 01: Storage and Error Foundation Summary

**Migration 0003 (`memory_links`) ships behind a user_version probe, a sibling SQLite Online-Backup snapshot and a friendly newer-schema refusal, all locked by a committed v0.0.1 binary fixture. The plan also closes the CR-01 / WR-01 / WR-02 review carry-forwards: NULL-safe import dedup, a 4x KNN oversample cap, and `MemoryError::Internal`.**

## Performance

- **Duration:** ~9 min
- **Started:** 2026-10-23T17:48:14Z
- **Completed:** 2026-10-23T17:57:30Z
- **Tasks:** 2 / 2
- **Files modified:** 13 (3 created, 10 modified)

## Accomplishments

- **WR-02 closed:** all six `writer.lock()` sites map poisoning to `MemoryError::Internal("writer lock poisoned")`, and both exhaustive mappers classify it as internal (REST 500 / MCP `internal_error`). `MemoryError::NotFound` and the unconstructable `ApiError::NotFound` are deleted; the compiler confirms there are zero producers.
- **CR-01 closed:** `Store::exists` takes `Option<&str>` and probes with `source IS ?1`, and `import()` no longer coerces `None` to `""`. Re-importing a source-less draft now reports `skipped_duplicates == 1`.
- **WR-01 (partial):** `MAX_KNN_K = MAX_LIMIT * 4 = 800`, so the 4x oversample holds at `limit = 200`. The doc comment separates it from the RRF smoothing constant `k = 60`.
- **STORE-05:** migration 0003 adds `memory_links`. `SqliteStore::open` now runs probe, then refuse-too-new, then pragmas, then backup, then migrate. A committed v0.0.1 fixture keeps 0001/0002 provably frozen. A mutation test was run by hand: editing a 0001 column default turned the freeze test red, and 0001 was then restored byte-identical to the tag.
- **End to end:** running the binary against a `user_version = 99` DB exits 1 with empty stdout (MCP-05 purity). Stderr reads `database schema is version 99 but this binary supports up to version 3 — upgrade agent-memory (brew upgrade unityinflow/tap/agent-memory) or point --db at a different file`, and the DB sha is unchanged.

## Task Commits

1. **Task 1: Internal error tier (WR-02), NotFound removal, MAX_KNN_K decouple (WR-01), NULL-safe import dedup (CR-01)** - `2541dbc` (fix)
2. **Task 2: Migration 0003 + pre-migration backup + newer-schema refusal + fixture freeze** - `46a63fa` (feat)

**Plan metadata:** recorded in the final `docs(04-01)` commit.

_TDD note: RED was observed locally for both tasks before implementation. For Task 1, the source:None test failed with `left: 1 right: 0`. For Task 2, 6 of 7 hygiene tests failed against a 2-migration chain. RED was not committed as a separate `test(...)` commit because CLAUDE.md and the operator rules forbid committing failing tests. Each task commit carries its tests plus the implementation._

## Files Created/Modified

- `crates/agent-memory-core/sql/0003_relations.sql`: the `memory_links` relations schema (directed edges, cascade, self-link CHECK, reverse index)
- `crates/agent-memory-core/tests/migration_hygiene.rs`: STORE-05 suite (7 tests plus the `#[ignore]`d one-shot fixture generator)
- `crates/agent-memory-core/tests/fixtures/v0.0.1.db`: committed v0.0.1-schema binary fixture (user_version 2, WAL header, 2 rows, sha1 `7c6f9f0c…`)
- `Cargo.toml`: rusqlite gains the `backup` feature (version 0.39 unchanged; Cargo.lock unchanged)
- `crates/agent-memory-core/src/domain.rs`: `Internal` and `SchemaTooNew` added, `NotFound` removed
- `crates/agent-memory-core/src/store/migrations.rs`: 0003 appended, `LATEST_SCHEMA_VERSION`, chain-length test
- `crates/agent-memory-core/src/store/sqlite.rs`: open() hygiene flow, `pre_migration_backup`, `utc_yyyymmdd`, Internal lock mapping, NULL-safe `exists`, `MAX_KNN_K`, const unit test
- `crates/agent-memory-core/src/store/mod.rs`: `exists(source: Option<&str>, ..)`
- `crates/agent-memory-core/src/service.rs`: import dedup passes `draft.source.as_deref()`
- `crates/agent-memory-core/tests/import.rs`: CR-01 regression test
- `crates/agent-memory/src/mcp.rs`: mapper arms for `Internal` and `SchemaTooNew`, tier test
- `crates/agent-memory/src/rest/handlers.rs`: mapper arms for `Internal` and `SchemaTooNew`, 500-tier test
- `crates/agent-memory/src/rest/mod.rs`: `ApiError::NotFound` removed

## Decisions Made

- **Probe before pragmas.** The plan placed the probe "between prepare_connection and to_latest". It runs before `prepare_connection` instead, because `journal_mode=WAL` rewrites header bytes 18-19 of a rollback-journal file. Probing after it would have broken D-13's "DB untouched" contract and the byte-equality test. The backup still runs after the pragmas and before `to_latest`, so Pitfall 8 ordering holds.
- **Backup failure aborts** (CONTEXT discretion, resolved in planning). The error is `MemoryError::Internal("pre-migration backup failed (…); refusing to migrate <db>")`.
- **The retention step is strict.** Any entry matching `{db}.backup-pre-{NNNN}-*` that cannot be removed aborts the open. That makes the abort path testable without platform-specific permission tricks.
- **The fixture has data.** Two planted rows (one with NULL source) let the backup test assert that real rows survive in the snapshot. The plan only required the schema.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Deleted `ApiError::NotFound` in `rest/mod.rs` (file not in the plan's list)**
- **Found during:** Task 1
- **Issue:** With `MemoryError::NotFound` gone, `ApiError::NotFound` had no constructor left. That is a `dead_code` warning in the binary crate, which `clippy -D warnings` fails on.
- **Fix:** Removed the variant and its `IntoResponse` arm, and rewrote the doc to say REST 404s come from not-found data. Plans 04-02 and 04-03 build their 404s directly, as forget_handler does, so nothing downstream needs it.
- **Files modified:** crates/agent-memory/src/rest/mod.rs
- **Verification:** clippy clean; all REST tests (in-process and spawned-binary) green
- **Committed in:** 2541dbc

**2. [Rule 1 - Bug] D-13 probe moved ahead of `prepare_connection`**
- **Found during:** Task 2
- **Issue:** As written, the ordering would apply the WAL pragma to a too-new DB before refusing it, which mutates the file. That violates D-13 and the plan's own Test 5.
- **Fix:** The probe reads `user_version` on the freshly opened connection before any pragma. The backup and migration still follow the pragmas.
- **Files modified:** crates/agent-memory-core/src/store/sqlite.rs
- **Verification:** `a_newer_schema_database_is_refused_and_left_untouched` (byte equality) plus the manual binary shasum check
- **Committed in:** 46a63fa

**3. [Rule 2 - Missing critical] Added tests beyond the five planned hygiene behaviours**
- **Found during:** Task 2
- **Issue:** The plan's resolved Open Question 3 says the abort-on-backup-failure behaviour is "test-covered", but no listed behaviour covered it. A drifting `LATEST_SCHEMA_VERSION` would also silently disable both the probe and the backup trigger.
- **Fix:** Added `a_failed_backup_aborts_the_open_before_migrating`, `committed_fixture_is_the_v0_0_1_schema`, and the unit test `latest_schema_version_matches_the_migration_chain`.
- **Committed in:** 46a63fa

---

**Total deviations:** 3 auto-fixed (1 blocking, 1 bug, 1 missing-critical test coverage)
**Impact on plan:** All three were needed for correctness or a clean `-D warnings` build. No scope creep, and no new crates.

## Issues Encountered

- The fixture path is caught by the user's local, uncommitted `.gitignore` (`*.db`), so it was staged with `git add -f` as the plan anticipated. That `.gitignore` itself was left untouched and unstaged.
- Project hooks block shell `cp`/`rm` of `.db` files. The manual end-to-end check used a fresh `mktemp -d` directory and Python's `sqlite3` instead of deleting anything.

## Verification Results

- `cargo test -p agent-memory-core --test import`: 2 passed
- `cargo test -p agent-memory-core --test migration_hygiene`: 7 passed, 1 ignored (the generator)
- `cargo test --workspace`: **87 passed / 0 failed / 2 ignored** (baseline was 76 / 0 / 1)
- `cargo clippy --workspace --all-targets -- -D warnings`: clean
- `cargo fmt --all --check`: clean
- `git diff v0.0.1 -- sql/0001_init.sql sql/0002_embeddings.sql`: empty
- `cargo tree -p rusqlite --depth 0`: `rusqlite v0.39.0`

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness

- Plan 04-02 (`memory_update`) can write new writer methods against `MemoryError::Internal`.
- Plan 04-03 (relations) has the `memory_links` table. It still owns the belt-and-braces orphan deletes in `forget()` and `sweep_expired()`, and the `LinkKind` and `LinkOutcome` types.
- Plan 04-04 (hybrid) inherits the 4x `MAX_KNN_K` oversample.
- Still deferred: the full WR-01 fix (filter-aware KNN retry or vec0 metadata pre-filtering) and WR-03 (the REST harness hang). Both are outside this plan's scope, per 04-RESEARCH.

---
*Phase: 04-memory-update-relations-hybrid-search*
*Completed: 2026-10-23*

## Self-Check: PASSED

- FOUND: crates/agent-memory-core/sql/0003_relations.sql
- FOUND: crates/agent-memory-core/tests/migration_hygiene.rs
- FOUND: crates/agent-memory-core/tests/fixtures/v0.0.1.db (tracked: `git ls-files --error-unmatch` exit 0)
- FOUND: commit 2541dbc (Task 1)
- FOUND: commit 46a63fa (Task 2)
