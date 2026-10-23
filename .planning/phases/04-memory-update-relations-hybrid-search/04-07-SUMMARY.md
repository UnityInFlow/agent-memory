---
phase: 04-memory-update-relations-hybrid-search
plan: 07
subsystem: store
tags: [sqlite, migration, backup, concurrency, serde, axum, rmcp, rest, mcp, gap-closure]

# Dependency graph
requires:
  - phase: 04-memory-update-relations-hybrid-search (04-01)
    provides: STORE-05 probe -> refuse -> backup -> migrate order, pre_migration_backup, v0.0.1 fixture
  - phase: 04-memory-update-relations-hybrid-search (04-02)
    provides: UpdateArgs / UpdateBody merge-patch DTOs and the "no default: null in schema" test
  - phase: 04-memory-update-relations-hybrid-search (04-05, 04-06)
    provides: the tree this last gap wave builds on
provides:
  - deny_unknown_fields on mcp::UpdateArgs and rest::UpdateBody (only)
  - extractor_rejection helper; Path/Query rejections on REST return the {"error"} body with the status tier preserved
  - pre_migration_backup(src, path, expected_version) -> Result<Option<PathBuf>> (temp, verify, rename, prune)
  - migrate_tolerating_concurrent_upgrade (re-probe absorbs a lost race against a completed upgrade)
  - busy_timeout set first on the writer and in apply_pragmas
  - README Upgrading section, memory_update unknown-field note, REST error-body sentence
affects: [phase-04 re-verification, phase-05, phase-06 release notes]

actuals:
  tokens: 10859
  tasks: 3
  commits: 4

tech-stack:
  added: []
  patterns:
    - "Verify-then-rename backups: copy to a temp outside the retention prefix, check the copy still has the probed version, rename into place, prune afterwards"
    - "Tolerate-by-re-probe: a failed schema migration counts as success only when user_version already reads LATEST"
    - "Result<Extractor, Rejection> handler arguments mapped by status tier into the uniform ApiError body"

key-files:
  created: []
  modified:
    - crates/agent-memory/src/mcp.rs
    - crates/agent-memory/src/rest/handlers.rs
    - crates/agent-memory/tests/tools.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory-core/tests/migration_hygiene.rs
    - README.md
    - .planning/STATE.md

key-decisions:
  - "04-07: deny_unknown_fields added to UpdateArgs/UpdateBody only (new v0.1.0 surfaces, REVIEW WR-02); the [03-01] decision stands for the v0.0.1 DTOs. Over MCP, rmcp 1.8 reports the rejection as a tool error result (isError: true, 'failed to deserialize parameters: unknown field ...'), not a JSON-RPC -32602; REST is a 400 {\"error\"}."
  - "04-07: the backup temp file is pid-qualified ({db}.backup-tmp-NNNN-{pid}) and a process-wide BACKUP_LOCK serializes backups within one process, so two openers in the same process (threads) never share a temp path. Cross-process openers are kept apart by the pid."
  - "04-07: WR-01 is closed with the review's cheaper alternative (verify-then-rename plus tolerate-by-re-probe), with no file lock and no new dependency. The exclusive cross-process lock stays deferred, as the plan states."

patterns-established:
  - "A backup step must never delete the previous valid backup before the new one is in place, and must never keep a copy whose schema version differs from the one it was taken for."

requirements-completed: [MCP-06, MCP-07, STORE-05]

coverage:
  - id: D1
    description: "memory_update over MCP refuses unknown or immutable fields (type, source, ttl_sec) with an error naming the field, and writes nothing (REVIEW WR-02, T-04-27)"
    requirement: "MCP-06"
    verification:
      - kind: e2e
        ref: "crates/agent-memory/tests/tools.rs#memory_update_with_an_unknown_field_is_invalid_params_over_stdio"
        status: pass
      - kind: unit
        ref: "crates/agent-memory/src/mcp.rs#update_args_reject_unknown_and_immutable_fields"
        status: pass
      - kind: unit
        ref: "crates/agent-memory/src/mcp.rs#update_args_schema_makes_only_id_required (now also additionalProperties == false)"
        status: pass
    human_judgment: false
  - id: D2
    description: "PATCH /api/memories/{id} with type, source or ttl_sec is a 400 naming the field, and the row is untouched"
    requirement: "MCP-06"
    verification:
      - kind: integration
        ref: "crates/agent-memory/src/rest/handlers.rs#patch_with_an_unknown_or_immutable_field_is_400_and_writes_nothing"
        status: pass
      - kind: integration
        ref: "crates/agent-memory/src/rest/handlers.rs#malformed_or_empty_patches_are_400_never_500 (unchanged, green)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Malformed path or query parameters (PATCH/DELETE /api/memories/abc, DELETE /api/links/x/{id}/relates_to, ?expand_links=1, ?limit=abc) return 400 with an application/json {\"error\"} body (REVIEW IN-02, T-04-31)"
    requirement: "MCP-07"
    verification:
      - kind: integration
        ref: "crates/agent-memory/src/rest/handlers.rs#malformed_path_and_query_params_get_the_json_error_body"
        status: pass
    human_judgment: false
  - id: D4
    description: "A backup taken after another process migrated is discarded and never replaces the genuine pre-migration backup; a valid backup replaces, then prunes (REVIEW WR-01, T-04-29)"
    requirement: "STORE-05"
    verification:
      - kind: unit
        ref: "crates/agent-memory-core/src/store/sqlite.rs#a_backup_of_an_already_migrated_database_is_discarded_and_keeps_the_valid_one"
        status: pass
      - kind: unit
        ref: "crates/agent-memory-core/src/store/sqlite.rs#a_valid_backup_replaces_then_prunes_the_stale_one"
        status: pass
    human_judgment: false
  - id: D5
    description: "A backup that fails to write, or fails to move into place, never removes the previous valid backup (REVIEW IN-03)"
    requirement: "STORE-05"
    verification:
      - kind: unit
        ref: "crates/agent-memory-core/src/store/sqlite.rs#a_failed_backup_write_never_removes_the_previous_backup"
        status: pass
      - kind: unit
        ref: "crates/agent-memory-core/src/store/sqlite.rs#a_backup_that_cannot_be_moved_into_place_keeps_the_previous_one"
        status: pass
      - kind: integration
        ref: "crates/agent-memory-core/tests/migration_hygiene.rs#a_failed_backup_aborts_the_open_before_migrating (unchanged, green)"
        status: pass
    human_judgment: false
  - id: D6
    description: "A to_latest failure is absorbed only when a re-probe shows LATEST_SCHEMA_VERSION, including the two real lost-race errors (table already exists, database is locked) (T-04-30)"
    requirement: "STORE-05"
    verification:
      - kind: unit
        ref: "crates/agent-memory-core/src/store/sqlite.rs#a_migration_failure_is_absorbed_only_when_another_process_reached_latest"
        status: pass
      - kind: unit
        ref: "crates/agent-memory-core/src/store/sqlite.rs#a_real_lost_migration_race_is_absorbed"
        status: pass
    human_judgment: false
  - id: D7
    description: "Two concurrent first opens of a temp copy of the v0.0.1 fixture both succeed; the DB ends at schema 3 with exactly one backup at schema 2 and no temp leftovers (10 rounds per run, 3 consecutive green runs)"
    requirement: "STORE-05"
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/migration_hygiene.rs#two_concurrent_first_opens_of_a_v0_0_1_database_both_succeed_with_one_genuine_backup"
        status: pass
      - kind: integration
        ref: "crates/agent-memory-core/tests/migration_hygiene.rs#upgrading_writes_one_pre_migration_backup_with_the_old_schema / re_running_the_upgrade_keeps_one_backup_per_schema_version (+ no-temp-leftover asserts)"
        status: pass
    human_judgment: false
  - id: D8
    description: "README documents the pre-migration backup, retention, restore steps (memory.db-wal / memory.db-shm) and the newer-schema refusal (REVIEW IN-01)"
    verification:
      - kind: other
        ref: "grep -n '## Upgrading' README.md; grep -n 'memory.db-wal' README.md; grep -n 'brew upgrade unityinflow/tap/agent-memory' README.md"
        status: pass
    human_judgment: false

duration: 12min
completed: 2026-10-23
status: complete
---

# Phase 04 Plan 07: Strict update payloads, uniform REST error bodies, race-safe pre-migration backup Summary

**`memory_update` and `PATCH` now reject unknown or immutable fields such as `type` or `ttl_sec`. Before, a patch carrying them reported success with the field silently dropped. Malformed REST path and query parameters get the same JSON `{"error"}` body as every other client error. When an MCP server and the REST daemon open an older database at the same time after an upgrade, both start, and the backup that survives is always a real pre-migration copy. The backup is written to a temp file, checked against the probed schema version, and renamed into place before any older copy is removed. A migration that loses the race to another process counts as success once the schema already reads 3.**

## Performance

- **Duration:** ~12 min
- **Started:** 2026-10-23T20:01:20Z
- **Completed:** 2026-10-23T20:13Z
- **Tasks:** 3/3
- **Files modified:** 7 (0 created, 7 modified)

## Accomplishments

- **WR-02 closed.** `#[serde(deny_unknown_fields)]` is on `mcp::UpdateArgs` and `rest::handlers::UpdateBody` only. Every field attribute is unchanged, and the explicit-null deserializer still works alongside it, so the tags/scope/ttl absent/null/value matrices are unchanged and green.
  - The 04-02 schema test still holds (`required == ["id"]`, no `default` on any patch field). It now also pins `additionalProperties == false`.
  - Rejections are proven over the real stdio binary, through rmcp's actual parameter path, and over REST. In each case the row is unchanged afterwards.
- **IN-02 closed.**
  - `forget_handler`, `update_handler` and `unlink_handler` take `Result<Path<_>, PathRejection>`, and `list_handler` takes `Result<Query<_>, QueryRejection>`.
  - A private `extractor_rejection(status, message)` turns a 4xx rejection into `ApiError::BadRequest` and a 5xx into `ApiError::Internal`, so a routing misconfiguration still shows as a 500.
  - A real-HTTP test sends 5 malformed requests. All return 400 with `application/json` and a non-empty `error`.
- **WR-01 and IN-03 closed.**
  - `busy_timeout` is now set on the writer right after `Connection::open`, before the D-13 probe. It is connection-level, so a refused file stays byte-identical, and the existing test still passes.
  - `busy_timeout` is also the first pragma in `apply_pragmas`.
  - `pre_migration_backup(src, path, expected_version) -> Result<Option<PathBuf>>` follows the plan's contract: temp file, then verify, then rename, then prune. It cleans up the temp file on a best-effort basis on every failure path.
  - `migrate_tolerating_concurrent_upgrade` wraps `to_latest`. The FTS5 and vec0 smoke checks still run after it.
- **IN-01 closed.** The README "Upgrading" section sits after "Run" and covers:
  - the backup name;
  - one backup per target version, replaced atomically;
  - no backup means no migration;
  - concurrent starts;
  - the restore steps (stop all processes, copy over, delete `memory.db-wal` / `memory.db-shm`, start the older binary);
  - the verbatim newer-schema error, which names `brew upgrade unityinflow/tap/agent-memory`.

## Task Commits

1. **Task 1 (tracer): memory_update refuses unknown and immutable fields on MCP and REST (WR-02).** `f09a69f` (fix)
2. **Task 2: malformed REST path and query params get the JSON error body (IN-02).** `cb24e84` (fix)
3. **Task 3: race-safe pre-migration backup, concurrent-upgrade-tolerant open, and README Upgrading (WR-01, IN-03, IN-01).** `a914311` (fix)

**Plan metadata:** see the `docs(04-07)` commits that follow.

## Files Created/Modified

- `crates/agent-memory/src/mcp.rs`: `deny_unknown_fields` on `UpdateArgs`, plus doc and tool-description notes; 1 new test and 1 extended schema test.
- `crates/agent-memory/src/rest/handlers.rs`:
  - `deny_unknown_fields` on `UpdateBody`;
  - the `PathRejection` / `QueryRejection` imports;
  - `extractor_rejection`;
  - 4 handler signatures, with doc comments updated;
  - the existing direct test calls wrapped in `Ok(..)`;
  - 2 new tests.
- `crates/agent-memory/tests/tools.rs`: 1 new stdio end-to-end test.
- `crates/agent-memory-core/src/store/sqlite.rs`:
  - the reordered `apply_pragmas`;
  - `BACKUP_LOCK`, `backup_temp_path`, `discard_backup_temp`, `copy_into_temp`, `prune_other_backups`;
  - the rewritten `pre_migration_backup` and the new `migrate_tolerating_concurrent_upgrade`;
  - the `open()` changes (writer `busy_timeout`, `db_version` passed through, the wrapped migrate, the numbered doc);
  - 6 new unit tests.
- `crates/agent-memory-core/tests/migration_hygiene.rs`: `assert_no_backup_temp`, leftover assertions in 2 existing tests, and the new concurrent first-open smoke test.
- `README.md`: the memory_update unknown-field note, the REST error-body sentence, and the new "## Upgrading" section.
- `.planning/STATE.md`: the `04-07: deny_unknown_fields` decision entry, added in Task 1.

## Decisions Made

- **Per-process backup lock alongside the pid-qualified temp name.** The plan's temp path `{db}.backup-tmp-{NNNN}-{pid}` keeps separate processes apart. The plan's own smoke test, however, races two threads in one process, which share a pid. A static `BACKUP_LOCK` serializes backups within a process, so the exact temp path from the plan, and its "failed write" seam test, are kept unchanged. A poisoned lock is recovered with `PoisonError::into_inner`, because the guard protects no data and no `unwrap` is needed.
- **Prune tolerates `NotFound`.** Two concurrent openers can both try to remove the same stale `{prefix}20000101`. The one that loses gets `NotFound`, which must not abort startup. Any other prune error still aborts, so the existing `a_failed_backup_aborts_the_open_before_migrating` blocker-directory test stays green.
- **The destination is closed explicitly** with `Connection::close()` before the rename, and the error is surfaced, rather than relying on drop.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Plan assumption] rmcp 1.8 returns an argument-deserialization failure as a tool error result, not a JSON-RPC -32602**
- **Found during:** Task 1 (the stdio end-to-end test failed on its first run)
- **Issue:** The plan's interface contract and `<behavior>` expected a JSON-RPC `error` member with code -32602. In practice, rmcp-1.8.0 `src/handler/server/router/tool.rs` `into_tool_argument_error` deliberately turns an `INVALID_PARAMS` error whose message starts with `failed to deserialize parameters:` into `CallToolResult::error(..)`. That is, it returns `{"result": {"isError": true, "content": [{"text": "failed to deserialize parameters: unknown field `type`, expected one of ..."}]}}`, following the MCP rule that input-validation errors are tool execution errors the model can correct.
- **Fix:** The stdio test now asserts the real wire shape: no `error` member, `isError: true`, and text that starts with the rmcp prefix and names the field. The rejection still happens before the handler runs, names the field, and writes nothing, so WR-02's intent (no silent partial write) is fully met. The `UpdateArgs` doc, the tool description, the README and the STATE decision describe the actual shape. The test keeps the name the plan gave it.
- **Files modified:** `crates/agent-memory/tests/tools.rs`, `crates/agent-memory/src/mcp.rs`, `README.md`, `.planning/STATE.md`
- **Commit:** `f09a69f`

**2. [Rule 1 - Bug in the plan's contract] Two openers in one process would share the pid-qualified temp path**
- **Found during:** Task 3 design
- **Issue:** The two-thread smoke test runs both openers in one process, so `{db}.backup-tmp-NNNN-{pid}` would collide: two Online Backups into one file, and one of them renaming it mid-write.
- **Fix:** A process-wide `BACKUP_LOCK` around the backup step (see Decisions). The temp name is exactly the plan's.
- **Commit:** `a914311`

### Additive test strengthening (no scope change)

**3. [Rule 2 - Coverage] Real lost-race test for the migration tolerance**
- **Found during:** Task 3 mutation check
- **Issue:** Removing the `migrate_tolerating_concurrent_upgrade` wrapper never turned the two-thread smoke test red in 30 rounds. With the in-process backup lock, B's backup reliably outlasts A's migration, so B's `to_latest` sees version 3 and does nothing. The closure seam test uses a simulated error only.
- **Fix:** Added `a_real_lost_migration_race_is_absorbed`. It uses two connections on one WAL database: the winner runs the real `to_latest`, and the loser then runs 0003 inside its own transaction, as rusqlite_migration does after its out-of-transaction version read. This reproduces both real failure modes, with and without a stale read snapshot:
  - `table memory_links already exists`;
  - `database is locked`, the SQLITE_BUSY_SNAPSHOT case.

  Both are absorbed. With the tolerance disabled, the test fails.
- **Commit:** `a914311`

**4. [Rule 2 - Coverage] Rename-failure seam test**
- Added `a_backup_that_cannot_be_moved_into_place_keeps_the_previous_one`: a non-empty directory sits at today's final name, so the copy succeeds but the rename fails. The previous backup survives and no temp is left. This covers the IN-03 step after the copy, in addition to the plan's failed-temp-write case.

### Process

**5. Tracer feedback gate run as an automated re-verify, not an interactive checkpoint**
- Auto mode is off (`_auto_chain_active=false`, `auto_advance=false`), but the orchestrator asked for the whole plan in one run, and the tracer's `<verify>` is fully automated. Task 1's verify was:
  - `cargo test -p agent-memory --bins`: 62 passed;
  - the stdio test: 6/6 in `--test tools`;
  - `clippy -p agent-memory -D warnings`: clean.

  It ran on exactly the tree that was committed, which `cargo fmt --check` confirmed was unchanged. That was the gate, as in 04-05 and 04-06.

**6. RED proven by temporary mutation, not separate RED commits**
- CLAUDE.md forbids committing failing tests, so each fix lands together with its tests in one commit. RED was observed in place, and the files were then restored from `cmp`-verified copies:
  - **Task 1:** removing both `deny_unknown_fields` attributes failed `update_args_reject_unknown_and_immutable_fields`, `update_args_schema_makes_only_id_required` and the stdio test.
  - **Task 2:** the new real-HTTP test, run against the pre-change `handlers.rs`, failed with `content-type "text/plain; charset=utf-8"`.
  - **Task 3:** disabling the copy-version check failed the concurrent smoke test (`round 5: the surviving backup is a genuine pre-migration copy`). Disabling the tolerance failed `a_real_lost_migration_race_is_absorbed`.

---

**Total deviations:** 2 auto-fixed (Rule 1), 2 additive (Rule 2), 2 process. **Impact:** none on scope. The MCP wire shape of the WR-02 rejection is rmcp's, and it is documented.

## TDD Gate Compliance

All three tasks carry `tdd="true"` in a `type: execute` plan. Each lands as one `fix(...)` commit that bundles its tests, because CLAUDE.md forbids committing failing tests. RED was observed by temporary mutation (deviation 6), and GREEN is each task's commit.

## Concurrent smoke test run count

`cargo test -p agent-memory-core --test migration_hygiene two_concurrent` passed on **3 consecutive runs** (10 race rounds each, 0.17-0.18 s per run). It also passed in the full `--test migration_hygiene` run, in the plan verify, and in the final `cargo test --workspace`, so it has 5 green runs in total and 0 flakes.

## Verification

- `cargo fmt --all --check`: clean
- `cargo clippy --workspace --all-targets -- -D warnings`: clean
- `cargo test --workspace`: **211 passed / 0 failed / 2 ignored**. That is up from 200 after 04-06, an increase of 11:
  - 1 MCP unit test;
  - 2 REST tests;
  - 1 stdio test;
  - 6 sqlite seam tests;
  - 1 migration_hygiene test.

  Both ignored tests pre-date this plan; one is the fixture generator.
- `cargo test -p agent-memory-core --test migration_hygiene`: 8 passed, 1 ignored. This includes the byte-for-byte too-new refusal and the failed-backup abort, both unchanged.
- `git diff --stat -- crates/agent-memory-core/sql/ crates/agent-memory-core/tests/fixtures/`: empty. The migrations and the committed v0.0.1 fixture are untouched, and every test works on temp copies.
- All plan acceptance greps are present:
  - `deny_unknown_fields`: 3 in mcp.rs, 2 in handlers.rs;
  - `PathRejection`, `QueryRejection`;
  - `fn migrate_tolerating_concurrent_upgrade`;
  - `pre_migration_backup(&writer, path, db_version)`;
  - `backup-tmp-`, `writer.busy_timeout`;
  - `## Upgrading`, `memory.db-wal`, `memory.db-shm`, `brew upgrade unityinflow/tap/agent-memory`;
  - `04-07: deny_unknown_fields` in STATE.md.
- No `unwrap()` was added to non-test code. No new dependency, no migration, no CI workflow change, and no tag, release or push.

## Known Stubs

None.

## Threat Flags

None. No new network endpoint, auth path or schema change. The new file surface is the pid-qualified backup temp file beside the DB, which the plan already covers as T-04-32.

## Deferred (as the plan states)

- **WR-01 full fix with a cross-process exclusive lock** (a lock file, or `BEGIN IMMEDIATE` spanning both backup and migration). It needs a new dependency or a restructure of rusqlite_migration's transaction. Verify-then-rename plus re-probe closes both observable failure modes.
- **`deny_unknown_fields` on the v0.0.1 DTOs and on the link DTOs**, per the [03-01] decision.
- **Automatic cleanup of stale `.backup-tmp-*` files** left by crashed processes. That needs liveness detection of the owning pid (T-04-32).
- **WR-04 behaviour change** (unbounded expanded `memory_list`) remains an open API-contract question for Phase 5/6, carried over from 04-06.
- **Observation, not changed:** over MCP, a deserialization rejection (an unknown field) is a tool error result, while a seam validation error (for example empty content) is a JSON-RPC `invalid_params` error. That split is rmcp's design and pre-dates this plan.

## User Setup Required

None.

## Next Phase Readiness

- All 7 phase-04 plans are executed. MCP-06, MCP-07 and STORE-05 were already ticked in REQUIREMENTS.md, so mark-complete is idempotent here. The phase is ready for re-verification. It is not yet marked complete; the orchestrator does that after verification passes.

## Self-Check: PASSED

- All 7 modified files are present on disk.
- Commits `f09a69f`, `cb24e84` and `a914311` were found in `git log`.
