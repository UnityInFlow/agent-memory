---
phase: 05-portable-export-import
verified: 2026-10-25T13:45:00Z
status: human_needed
score: 32/33 must-haves verified
behavior_unverified: 0
overrides_applied: 0
mode: mvp
verified_at_commit: efcc1dd
requirements:
  - id: DIST-04
    status: satisfied
  - id: INTEROP-02
    status: satisfied
human_verification:
  - test: "Backstop truth (05-02): run `agent-memory export --output big.jsonl` against a large database while a second process (serve-rest + a loop of POST /api/links) is concurrently creating memories and links; then `agent-memory import --from jsonl big.jsonl` into a fresh database."
    expected: "The import succeeds with no `link key N does not appear in the file` rejection and the header's exported_rows/exported_links equal the record counts — i.e. the export really was one WAL snapshot. On an interrupted export (kill -9 mid-stream) the target PATH is either absent or the previous complete file, never a partial one, and no `.export-tmp-*` sibling is left."
    why_human: "reason: insufficient_spec. The truth is tagged `verification: backstop`. Explicit evidence exists only for its second half (export_file unit tests `commit_replaces_an_existing_target_and_leaves_no_temp`, `abort_removes_the_temp_and_keeps_the_target`, CLI `an_unwritable_output_path_fails_before_the_database_is_created`). No test exercises a concurrent writer during `export_rows`, so the one-snapshot isolation claim rests on `unchecked_transaction` presence + wiring, which the honest-verifier rule does not accept as evidence for a backstop truth. A held-out test (spawn a writer thread that links during the visitor callback, then parse the output with `parse_jsonl`) would close this without a human."
  - test: "Real cross-machine move (MVP user flow, step 'on the other machine'): export on machine A, copy the file to machine B that has Ollama + nomic-embed-text running, import, start `agent-memory serve-rest`, wait for the first sweep tick, then POST /api/search with a paraphrase of an imported memory."
    expected: "Import prints the six-count line and `N rows pending embedding …`; after the first sweep the search response reports `search_mode: hybrid` and returns the imported memory for a paraphrase (not just an exact keyword)."
    why_human: "The in-process proof (`imported_rows_land_pending_and_the_sweep_backfills_them`) and the spawned-binary round trip both run on one host with a fake or dead embedder. The user-story outcome 'moves with me across machines' plus the real Ollama re-embed are external-service integration that cannot be exercised here."
---

# Phase 5: Portable Export & Import — Verification Report

**Phase Goal:** As a user, I want to export all my memories to a portable file and import them on another machine, so that my agent's memory moves with me — across machines, with no cloud sync.
**Verified:** 2026-10-25T13:45:00Z against HEAD `efcc1dd` (phase commits `00d8080..efcc1dd`, 19 commits incl. the six post-review fixes)
**Status:** human_needed
**Re-verification:** No — initial verification
**Mode:** mvp (goal validated as a User Story by `user-story.validate`: role = user; capability = export/import across machines; outcome = memory moves with me, no cloud sync)

## User Flow Coverage

User story: «As a user, I want to export all my memories to a portable file and import them on another machine, so that my agent's memory moves with me — across machines, with no cloud sync.»

| Step | Expected | Evidence | Status |
|------|----------|----------|--------|
| Run `agent-memory export --output memories.jsonl` | A private JSONL file: header line, then memories, then links; report `exported N memories and M links` on stderr | `crates/agent-memory/src/main.rs:351-399` (`run_export` → `export_file::begin` → `service.export` → `pending.commit`); `crates/agent-memory/src/export_file.rs:69-75,134-142` (create_new + mode 0600, same-directory temp, sync+rename); CLI test `export_import_round_trip_over_the_real_binary` asserts 4 lines, `{"record":"header"` first, mode 0600, no `.export-tmp-` sibling — PASS | ✓ |
| Copy the file to another machine | Nothing in the file is machine-bound: no vectors, no row ids used as ids | `crates/agent-memory-core/src/store/sqlite.rs:876-878` SELECT excludes `base_weight`/`decay_score`/`embedding_status`, never reads `vec_memories`; `export.rs` `Record::Memory` has no embedding field; test `memory_records_carry_exactly_the_documented_keys` — PASS | ✓ |
| Run `agent-memory import --from jsonl memories.jsonl` there (empty or populated DB) | Memories and links recreated with the target's own ids; six-count report + pending line; re-run reports only duplicates | `main.rs:273-300` (parse_jsonl BEFORE `open_service`, six-count println, pending line); `service.rs:632-712` (pass 2 key→id map via `find_by_key`/`insert_imported`, pass 3 `store.link` through the map); tests `round_trip_into_an_empty_database_recreates_memories_and_links`, `importing_into_a_non_empty_db_with_overlapping_rowids_never_attaches_phantom_edges`, `re_importing_the_same_file_is_idempotent`; CLI round trip across DB A → DB B → re-import `imported: 0, skipped duplicates: 2, … links skipped duplicates: 1` — PASS | ✓ |
| Start the server; memories become searchable again locally | Rows land pending, sweep re-embeds against the local model pin, hybrid search works | `sqlite.rs:693-727` `insert_imported` hard-codes `embedding_status: 0`, no vec write; test `imported_rows_land_pending_and_the_sweep_backfills_them` (embedder UP, status 0 → sweep → status 1, `SearchMode::Hybrid`) — PASS | ✓ |
| Outcome: "memory moves with me, no cloud sync" | Whole corpus (memories + edges) round-trips via a local file; no network call on either side | All of the above; `run_import`/`run_export` construct but never call the Ollama client; CLI tests run with `AGENT_MEMORY_OLLAMA_URL=http://127.0.0.1:9` (dead) and pass | ✓ (real cross-machine + live-Ollama run routed to human) |

## Goal Achievement

### Observable Truths

**Roadmap contract (Success Criteria 1–4) + user-approved scope addition D-14**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | `export` writes a versioned JSONL file whose header records format version and embedding model/dim; embeddings excluded; relation edges included with export-local keys | ✓ VERIFIED | `export.rs:117-180` `Record` enum (tag `record`, `deny_unknown_fields`), `FORMAT_NAME`/`FORMAT_VERSION=1`; `sqlite.rs:845-851` header pin read `FROM meta WHERE key = 'embedding_model'/'embedding_dim'`; link records carry `from`/`to` keys; golden tests `header_record_serializes_in_documented_order_with_record_first`, `export_writes_every_memory_ascending_then_every_link_as_stored`, `memory_records_carry_exactly_the_documented_keys` — all PASS |
| SC2 | Import on another machine — including a non-empty DB — recreates memories and link edges with correct key→id remapping; re-run idempotent | ✓ VERIFIED | `service.rs:646-705` two-pass with `HashMap<key,id>`; `find_by_key` dedup on `(source, mem_type, content)` NULL-safe; tests `importing_into_a_non_empty_db_with_overlapping_rowids_never_attaches_phantom_edges`, `link_endpoints_that_dedup_to_existing_rows_attach_to_those_rows`, `re_importing_the_same_file_is_idempotent` PASS; CLI re-import reports `imported: 0, skipped duplicates: 2 … links skipped duplicates: 1` |
| SC3 | Imported rows land `embedding_status = 0`, become semantically searchable after sweep backfill; no vectors travel | ✓ VERIFIED | `insert_imported` (`sqlite.rs:720` `embedding_status: 0`, no `vec_memories` INSERT); `import_jsonl_unredacted` never references `self.embedder`; test `imported_rows_land_pending_and_the_sweep_backfills_them` PASS (status 0, vec_rows 0 → after sweep status 1, vec_rows 1, `SearchMode::Hybrid`) |
| SC4 | Expired items skipped and counted; report states imported/skipped/duplicate counts; malformed files rejected as `MemoryError::InvalidArgument`, never 500/crash | ✓ VERIFIED | `service.rs:648-652` `expires_at <= now` → `skipped_expired`; `ImportReport` six fields (`service.rs:184-200`); `jsonl.rs` every rejection via `invalid_line` → `InvalidArgument("line N: …")`; 27-row table `malformed_files_are_rejected_naming_the_line` + `hostile_input_never_panics` PASS; CLI `a_malformed_file_exits_non_zero_naming_the_line_and_creates_no_database`, `invalid_utf8_is_rejected_cleanly` PASS |
| D-14 | Raw SQLite text redacted on every `MemoryService` path, both transports and CLI; `SEARCH_STORAGE_FAILURE` text unchanged | ✓ VERIFIED | `service.rs:64-80` `redact_storage_error` (exhaustive match, no catch-all); wrappers at store:291, list:336, search:398, import:496, import_jsonl:627, export:742, forget:901, link:934, unlink:966, update:1003, sweep:1068; `SEARCH_STORAGE_FAILURE` = `"search failed due to a storage error (details are in the server log)"` (template at op=search); `tests/redaction.rs` 10/10 PASS; REST `health_storage_failure_is_500_without_sqlite_text`, `store_storage_failure_is_500_without_sqlite_text`, MCP `memory_store_storage_failure_is_internal_error_without_sqlite_text` PASS |

**Plan 05-01 (truths not restating the above)**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1.3 | `Store::find_by_key` returns `Some(id)` NULL-safely (`source IS ?1`), `None` otherwise; `import` dedups through it | ✓ VERIFIED | `sqlite.rs:926-957` SQL `WHERE source IS ?1 AND mem_type = ?2 AND content = ?3 AND (expires_at IS NULL OR expires_at > ?4) ORDER BY id LIMIT 1`; `service.rs:520` caller; `tests/store.rs::find_by_key_returns_the_matching_id_null_safely` PASS |
| 1.4 | `Store::link` reports `Linked { created: true/false }`; wire JSON unchanged | ✓ VERIFIED | `sqlite.rs:1179-1181` `tx.changes() == 1` read before commit; MCP `memory_link_and_unlink_round_trip_with_idempotent_duplicates` PASS; relations tests pass in lib run |
| 1.5 | `Store::insert_imported` preserves created_at/access_count/expires_at, `last_accessed = now`, status 0, no vector row | ✓ VERIFIED | `sqlite.rs:693-727` binds exactly that via shared `insert_row`; round-trip test asserts `source`, `created_at`, `access_count` preserved and `last_accessed` refreshed |
| 1.6 | README scopes redaction to every surface and lists it as behavioral change 4 | ✓ VERIFIED | `README.md:466` and `:518-525` (item 4 names store, list incl. /health, update, link/unlink, forget, import, sweep) |

**Plan 05-02**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 2.1 | Header fields in order; model/dim from `meta`, `exported_at` from clock, version from `CARGO_PKG_VERSION` | ✓ VERIFIED | `service.rs:759-773` builds header from `ExportSnapshot` + `now` + param; `main.rs:357` `env!("CARGO_PKG_VERSION")`; test `export_of_an_empty_database_is_exactly_one_header_line` PASS |
| 2.2 | Memory record carries exactly the 11 documented fields in order; derived data excluded | ✓ VERIFIED | `export.rs` declaration order; golden `memory_record_serializes_in_documented_order_with_record_first`; `memory_records_carry_exactly_the_documented_keys` PASS |
| 2.3 | Link record `record, from, to, kind, created_at`, endpoints as stored, after all memories | ✓ VERIFIED | `sqlite.rs:903-906` second scan after memories; golden `link_record_serializes_in_documented_order_with_record_first`; `export_writes_every_memory_ascending_then_every_link_as_stored` PASS |
| 2.4 | Export writes every row incl. expired, no filter, ascending id, one streaming read | ✓ VERIFIED | `sqlite.rs:876-878` no WHERE clause, `ORDER BY id`, `query_map` stepped lazily through the visitor; `expired_rows_are_still_exported` PASS |
| 2.5 | Export is CLI-only | ✓ VERIFIED | `rest/mod.rs:71-86` six routes, none export/import; `grep -rn "memory_export\|memory_import\|/api/export\|/api/import" crates/agent-memory/src/mcp.rs crates/agent-memory/src/rest/` → empty |
| 2.6 | Empty DB → exactly one header line, 0/0 | ✓ VERIFIED | `export_of_an_empty_database_is_exactly_one_header_line` PASS |
| 2.7 | Ascending ids, links by `(from_id,to_id,kind)`, byte-identical re-export | ✓ VERIFIED | `sqlite.rs:905-906` ORDER BY; `two_exports_of_an_unchanged_database_are_byte_identical` PASS |
| 2.8 | **(backstop)** One pooled connection, one read transaction, so a concurrent link cannot produce a dangling key; `--output` temp-beside-target renamed so an interrupted export never leaves a partial target | ⚠️ insufficient_spec | `sqlite.rs:843` `conn.unchecked_transaction()` spans pin, counts, both scans — present and wired. Atomic half has explicit evidence (`export_file` tests `commit_replaces_an_existing_target_and_leaves_no_temp`, `abort_removes_the_temp_and_keeps_the_target`; CLI `an_unwritable_output_path_fails_before_the_database_is_created`). **No test races a writer against `export_rows`**; per honest-verifier, presence+wiring is not evidence for a backstop truth → abstain, routed to human (see Human Verification 1) |
| 2.9 | `--output` creates `PATH.export-tmp-<pid>` beside PATH, 0600, `create_new`, rename after flush+sync, temp removed on failure, stdout default | ✓ VERIFIED | `export_file.rs:50-75,96-118,134-142`; `main.rs:361-393` (stdout BufWriter default; begin → export → commit/abort); unit tests `begin_creates_a_private_temp_beside_the_target`, `begin_refuses_to_clobber_an_existing_temp` + CLI mode/temp assertions PASS |
| 2.10 | Export storage fault → `export failed due to a storage error …`; writer I/O → `export write failed: …` | ✓ VERIFIED | `service.rs:742,790-791`; `redaction.rs::export_storage_failure_is_redacted` PASS (added by IN-02 fix `22cd8d7`) |

**Plan 05-03 (truths not restating SC2/SC3)**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 3.2 | Re-import reports 0 imported / N dup / 0 links / M link-dup; source verbatim; identical rows collapse | ✓ VERIFIED | `re_importing_the_same_file_is_idempotent`, `identical_rows_collapse_to_one_on_import_and_count_a_duplicate` PASS; round-trip asserts `d.source == s.source` |
| 3.4 | created_at/access_count/expires_at preserved; last_accessed refreshed; expired skipped+counted; endpoint-skipped link → `links_dropped`; link created_at preserved; skew > 3600 s warns and continues | ✓ VERIFIED | `service.rs:633-636` `clock_skew_message` → `tracing::warn!` then proceeds; `clock_skew_message_fires_only_beyond_the_tolerance` PASS; `expired_rows_are_skipped_and_counted_and_their_links_dropped` PASS; `store.link(*a,*b,kind,created_at)` passes record's created_at as `now` |
| 3.5 | type/kind via `try_from`, empty content & self-links `InvalidArgument`, tags array, expires_at int-or-null | ✓ VERIFIED | `jsonl.rs:300-303,347-349` `MemoryType::try_from`/`LinkKind::try_from`/`validate_link_endpoints` with `rewrap`; table rows "bad type", "blank content", "self link", "tags not an array", "fractional expires_at" PASS |
| 3.6 | Every malformed class rejected naming the 1-based line; nothing written; parsing before DB open; gsd-state unchanged | ✓ VERIFIED | 27-row table PASS; `nothing_is_written_when_validation_fails` PASS; `main.rs:282-285` parse before `open_service`; `git diff 00d8080^..efcc1dd -- import/gsd_state.rs` empty; gsd-state unit tests in lib run PASS |
| 3.7 | `ImportReport` six counts; gsd-state reports zeros; CLI one line + pending line | ✓ VERIFIED | `service.rs:184-200`; `tests/import.rs:51-54` zeros; `main.rs:287-299`; CLI test pins exact strings |
| 3.8 | Import CLI-only; core exposes `parse_jsonl` + `import_jsonl` with `InvalidArgument` contract | ✓ VERIFIED | `pub fn parse_jsonl` (`jsonl.rs:219`), `pub async fn import_jsonl` (`service.rs:624`); route/tool grep empty |
| 3.9 | Two identical source rows both exported, collapse to one, one duplicate counted | ✓ VERIFIED | `identical_rows_collapse_to_one_on_import_and_count_a_duplicate` PASS; WR-01 follow-up `a_link_between_two_rows_that_collapse_on_import_is_dropped_not_miscounted` PASS |
| 3.10 | Header-only file from empty DB imports as all-zero no-op | ✓ VERIFIED | `a_header_only_file_imports_nothing_and_reports_zeros` PASS |
| 3.11 | Link whose endpoints dedup to existing rows attaches to those rows, never an unrelated id | ✓ VERIFIED | `link_endpoints_that_dedup_to_existing_rows_attach_to_those_rows` PASS |
| 3.12 | Header-only → zeros; zero-byte → `InvalidArgument` line 1 | ✓ VERIFIED | `a_zero_byte_file_is_rejected_at_line_one`, `an_empty_file_is_rejected_at_line_one` PASS |
| 3.13 | Links applied after all memories regardless of file order; same counts every run | ✓ VERIFIED | `jsonl.rs:356-366` post-loop key resolution; `links_are_applied_after_all_memories_regardless_of_file_order`, `links_may_precede_the_memories_they_name` PASS |

**Plan 05-04**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 4.1 | README "Export & import" documents usage, format v1 tables, strict rules + version policy, dedup/idempotency, timestamps, pending embeddings, six-count report, atomic `--output`, CLI-only rationale, backup tip | ✓ VERIFIED | `README.md:548-741` ten `###` subsections in order; all required terms present (`exported_rows`, `upgrade agent-memory`, `VACUUM INTO`, `0600`, `export-tmp-`, `links dropped`, `128`, `skipped expired`); Lifecycle cross-ref and Run example each grep 1 |
| 4.2 | Real binary round-trips with documented six-count line and pending line; re-import reports only duplicates | ✓ VERIFIED | `tests/cli.rs::export_import_round_trip_over_the_real_binary` PASS (spawned `CARGO_BIN_EXE_agent-memory`) |
| 4.3 | Malformed file → non-zero exit naming line, DB not created; `--from bogus` and `--scope` refused; invalid UTF-8 no panic | ✓ VERIFIED | `a_malformed_file_exits_non_zero_naming_the_line_and_creates_no_database`, `an_unsupported_from_value_still_bails`, `scope_is_rejected_for_jsonl`, `invalid_utf8_is_rejected_cleanly` PASS |

**Score:** 32/33 truths verified (1 abstained as `insufficient_spec`; 0 present-behavior-unverified)

### Prohibitions (must-NOT, all `verification: test`)

| Prohibition | Plans | Status | Enforcement evidence |
|---|---|---|---|
| No migration / `LATEST_SCHEMA_VERSION` change | 01 | ✓ VERIFIED | `git diff --stat 00d8080^..efcc1dd -- crates/agent-memory-core/sql/` empty; `tests/migration_hygiene.rs` 8 passed (fixture divergence gate) |
| `(source, mem_type, content)` identity contract unchanged, no new identity column | 01 | ✓ VERIFIED | `find_by_key` SQL binds exactly the three columns (+ liveness predicate); no schema change; `find_by_key_returns_the_matching_id_null_safely` PASS |
| gsd-state importer tolerance unchanged | 01, 03 | ✓ VERIFIED | `gsd_state.rs` byte-identical across the phase (git diff empty); its unit tests PASS in the lib run |
| No embedding vectors in the file | 02 | ✓ VERIFIED | export SQL never touches `vec_memories`; `Record::Memory` has no vector field; `memory_records_carry_exactly_the_documented_keys` PASS |
| Export/import never exposed over MCP or REST | 02, 03, 04 | ✓ VERIFIED (direct observation) | `rest/mod.rs:71-86` route table (memories, search, links, health only); no `memory_export`/`memory_import` tool in `mcp.rs`; grep empty. *Note:* no negative test pins the tool/route list — recommend a `tool_router().list_all()` name-set assertion so a future addition turns red (info, not a gap) |
| Export never filters or skips rows | 02 | ✓ VERIFIED | no WHERE clause; `expired_rows_are_still_exported` PASS |
| Import writes nothing when validation fails | 03 | ✓ VERIFIED | `import_jsonl` accepts only a parsed `JsonlFile` (structural); `nothing_is_written_when_validation_fails` PASS; CLI malformed test asserts `--db` never created |
| JSONL importer never calls the embedder / writes a vector | 03 | ✓ VERIFIED | `import_jsonl_unredacted` has no embedder reference; `imported_rows_land_pending_and_the_sweep_backfills_them` runs with embedder UP and asserts status 0 / vec_rows 0 before the sweep |
| Import never rewrites provenance (`source`, `scope` verbatim) | 03 | ✓ VERIFIED | `insert_imported` binds `m.source`/`m.scope` as-is; round-trip asserts `source is preserved verbatim (D-07)` and `scope` equality |

### Required Artifacts

| Artifact | Expected | Status | Details |
|---|---|---|---|
| `crates/agent-memory-core/src/export.rs` | format v1 single definition, `pub enum Record`, consts, module-doc spec, golden tests | ✓ VERIFIED | 347 lines; 9 unit tests PASS; imported by `service.rs`, `import/jsonl.rs`, `store/sqlite.rs` |
| `crates/agent-memory-core/src/import/jsonl.rs` | `pub fn parse_jsonl`, strict two-step header probe, 1-based lines | ✓ VERIFIED | 658 lines; called from `main.rs:282`; 11 unit tests PASS |
| `crates/agent-memory-core/src/import/mod.rs` | `pub mod jsonl;` | ✓ VERIFIED | present |
| `crates/agent-memory-core/src/service.rs` | `redact_storage_error`, `pub async fn export`, `pub async fn import_jsonl`, `ImportReport` +4, `clock_skew_message` | ✓ VERIFIED | all present and wired (lines cited above) |
| `crates/agent-memory-core/src/domain.rs` | `pub struct ImportedMemory`, `LinkOutcome::Linked { created }` | ✓ VERIFIED | consumed by `jsonl.rs`, `sqlite.rs`, both transports |
| `crates/agent-memory-core/src/store/mod.rs` | `find_by_key`, `insert_imported`, `export_rows` trait methods | ✓ VERIFIED | lines 57, 140, 166; doc states "lowest matching live id" (IN-01) |
| `crates/agent-memory-core/src/store/sqlite.rs` | `insert_row` shared helper, `export_rows` one snapshot, `link` reads `tx.changes()` | ✓ VERIFIED | lines 622, 832-924, 1179 |
| `crates/agent-memory/src/main.rs` | `Command::Export { output }`, `run_export`, `"jsonl" =>` arm, `open_service` | ✓ VERIFIED | `run_export` reuses `open_service` and calls `begin` before the open (IN-03 fix) |
| `crates/agent-memory/src/export_file.rs` | `pub struct PendingExport` begin/commit/abort | ✓ VERIFIED | 5 unit tests PASS |
| `crates/agent-memory-core/tests/export_import.rs` | export + import proofs incl. `phantom` kill-test | ✓ VERIFIED | 19 tests PASS |
| `crates/agent-memory-core/tests/redaction.rs` | fault-injection for all paths incl. export/import_jsonl | ✓ VERIFIED | 10 tests PASS |
| `crates/agent-memory/tests/cli.rs` | spawned-binary realism | ✓ VERIFIED | 6 tests PASS (plan said 5; IN-03 added one) |
| `README.md` | `## Export & import`, redaction sentence, behavioral change 4 | ✓ VERIFIED | section at 548-741 |

### Key Link Verification

`gsd-tools query verify.key-links` reported 0/4, 0/4, 1/3, 0/2 because the plans' `from:` fields are descriptive ("file.rs fn()") rather than bare paths — tooling limitation, not a wiring failure. Each pattern was checked by hand:

| From | To | Via | Status | Details |
|---|---|---|---|---|
| service.rs public methods | `redact_storage_error` | `*_unredacted` + op string | WIRED | 11 ops: store, list, search, import (×2 incl. import_jsonl), export, forget, link, unlink, update, sweep |
| service.rs `import()` | `Store::find_by_key` | dedup probe returns id | WIRED | `service.rs:520` (passes `now`) |
| sqlite.rs `link()` | `LinkOutcome::Linked { created }` | `tx.changes()` before commit | WIRED | `sqlite.rs:1179-1181` |
| sqlite.rs `insert_imported()` | `insert_row` | shared INSERT | WIRED | `sqlite.rs:710` |
| main.rs `run_export` | `MemoryService::export` | boxed `Write + Send`, `CARGO_PKG_VERSION` | WIRED | `main.rs:364,378` |
| service.rs `export_unredacted` | `Store::export_rows` | one spawn_blocking hop | WIRED | `service.rs:757` |
| sqlite.rs `export_rows` | meta + memories + memory_links | one read tx | WIRED | `sqlite.rs:843-906` |
| main.rs `run_export` | `PendingExport::commit/abort` | commit on Ok, abort on Err | WIRED | `main.rs:383,389` |
| main.rs jsonl arm | `parse_jsonl` then `import_jsonl` | parse BEFORE `open_service` | WIRED | `main.rs:282-286` |
| service.rs `import_jsonl_unredacted` | `find_by_key` / `insert_imported` / `link` | key→id HashMap | WIRED | `service.rs:658-700` |
| jsonl.rs | `Record` + `MemoryType::try_from` + `LinkKind::try_from` + `validate_link_endpoints` | seam conversion + `rewrap` | WIRED | `jsonl.rs:300-349` (tool-verified) |
| README Export & import | export.rs module doc | field tables in `Record` order | WIRED | tables match declaration order (checked by eye) |
| tests/cli.rs | main.rs `run_export`/`run_import` | spawned `CARGO_BIN_EXE_agent-memory` | WIRED | `cli.rs:33,147,211` |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|---|---|---|---|---|
| export header | `embedding_model`, `embedding_dim`, counts | `SELECT value FROM meta …`, `count(*)` inside the snapshot tx | Yes | ✓ FLOWING |
| export memory/link lines | `Record::Memory`/`Link` | `SELECT … FROM memories ORDER BY id`, `… FROM memory_links ORDER BY from_id,to_id,kind` via lazy `query_map` | Yes | ✓ FLOWING |
| import report | six counters | incremented per `insert_imported`/`find_by_key`/`link` outcome | Yes | ✓ FLOWING |
| CLI pending line | `report.imported` | same report | Yes | ✓ FLOWING |

### Behavioral Spot-Checks

All run in this verification (scoped, never `--workspace`):

| Behavior | Command | Result | Status |
|---|---|---|---|
| Core unit tests (export goldens, jsonl table, clock skew, gsd-state untouched) | `cargo test -p agent-memory-core --locked --lib` | 63 passed / 0 failed | ✓ PASS |
| Export/import integration proofs | `… --test export_import` | 19 passed | ✓ PASS |
| Redaction fault injection (10 paths) | `… --test redaction` | 10 passed | ✓ PASS |
| Store contract (`find_by_key` lowest-live-id) | `… --test store` | 8 passed | ✓ PASS |
| Migrations frozen | `… --test migration_hygiene` | 8 passed / 1 ignored (fixture generator) | ✓ PASS |
| Transport redaction (REST /health + store 500, MCP store internal_error) + all handler tests | `cargo test -p agent-memory --locked --bin agent-memory` | 72 passed | ✓ PASS |
| Real binary round trip, malformed, refusals, unwritable `--output` | `… --test cli` | 6 passed | ✓ PASS |
| D-01 transport surface | `grep -rn "memory_export\|memory_import\|/api/export\|/api/import" crates/agent-memory/src/mcp.rs crates/agent-memory/src/rest/` | empty | ✓ PASS |
| No schema/lock/workflow drift | `git diff --stat 00d8080^..efcc1dd -- crates/agent-memory-core/sql/ Cargo.lock .github/` | empty | ✓ PASS |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and no plan declares a probe. Step 7c: SKIPPED (no probes declared).

### Review Fixes (05-REVIEW.md "Fixes applied") — confirmed in code

| ID | Commit | Confirmed |
|---|---|---|
| WR-01 | `80e905b` | `service.rs:676-687` `(Some(a), Some(b)) if a == b => links_dropped += 1`; test `a_link_between_two_rows_that_collapse_on_import_is_dropped_not_miscounted`; README `links dropped` row widened |
| WR-02 | `c28fc04` | `jsonl.rs:309-314` `access_count < 0` rejected; `sqlite.rs:1286,1409` `CASE WHEN access_count < 9223372036854775807 …`; tests incl. `an_access_count_at_i64_max_imports_survives_a_bump_and_still_exports`; README strict list has "a negative `access_count`" |
| WR-03 | `3c28202` | `find_by_key(…, now)` with `(expires_at IS NULL OR expires_at > ?4)`; both callers pass the clock; test `an_expired_but_unswept_target_row_is_not_a_dedup_hit_so_imported_edges_survive_the_sweep` |
| IN-01 | `3c28202` | `ORDER BY id LIMIT 1`; trait doc "lowest matching live id" |
| IN-02 | `22cd8d7` | `export_storage_failure_is_redacted`, `import_jsonl_storage_failure_is_redacted` |
| IN-03 | `621b29a` | `run_export` → `open_service`; `export_file::begin` precedes the open inside one aborted result; test `an_unwritable_output_path_fails_before_the_database_is_created` |

### Requirements Coverage

| Requirement | Source Plan(s) | Description | Status | Evidence |
|---|---|---|---|---|
| DIST-04 | 05-02, 05-03, 05-04 | Export all memories to a versioned JSONL file (header with format version + model/dim; embeddings excluded; edges with export-local key remapping) | ✓ SATISFIED | SC1 + truths 2.1–2.7, 2.9 |
| INTEROP-02 | 05-01, 05-03, 05-04 | Import that JSONL on another machine — idempotent, rows land `embedding_status = 0` and re-embed via sweep, expired skipped and counted | ✓ SATISFIED | SC2–SC4 + truths 3.2–3.13 |

REQUIREMENTS.md maps exactly DIST-04 and INTEROP-02 to Phase 5 (lines 72-73); both are claimed by plans. No ORPHANED requirements.

### Anti-Patterns Found

Scanned all 11 production files touched by the phase.

| File | Line | Pattern | Severity | Impact |
|---|---|---|---|---|
| — | — | No `TBD`/`FIXME`/`XXX`/`TODO`/`HACK`/placeholder markers | — | — |
| — | — | No `.unwrap()`/`.expect(` outside `#[cfg(test)]` (only `unwrap_or_else`/`unwrap_or_default` fallbacks) | — | — |
| — | — | No catch-all `_ =>` arms in `export.rs`, `jsonl.rs`, `export_file.rs`; `redact_storage_error` exhaustive | — | — |
| `crates/agent-memory/src/mcp.rs`, `rest/mod.rs` | — | D-01 prohibition has no negative regression test (tool/route list not pinned) | ℹ️ Info | A future `memory_export` tool would not turn anything red; grep + review are the only guards |

### Human Verification Required

#### 1. One-snapshot export under concurrent writes (backstop truth 2.8 — `insufficient_spec`)

**Test:** Run `agent-memory export --output big.jsonl` on a large DB while a second process creates memories and links concurrently (e.g. `serve-rest` plus a loop of `POST /api/memories` + `POST /api/links`); then `agent-memory import --from jsonl big.jsonl` into a fresh DB. Separately, `kill -9` an in-flight `export --output` and inspect the directory.
**Expected:** Import succeeds with no `link key N does not appear in the file`; header counts equal record counts. After the kill, PATH is absent or the prior complete file, and no `.export-tmp-*` sibling remains.
**Why human:** Tagged `verification: backstop`. The atomic-rename half has unit + CLI tests; the snapshot-isolation half has only `unchecked_transaction` presence. A held-out test (spawn a writer during the `export_rows` visitor, then `parse_jsonl` the output) would make this automatic.

#### 2. Real cross-machine move with live Ollama (MVP user flow)

**Test:** Export on machine A; copy to machine B running Ollama + `nomic-embed-text`; import; `agent-memory serve-rest`; after the first sweep tick, `POST /api/search` with a paraphrase of an imported memory.
**Expected:** Six-count line + `N rows pending embedding …` on import; search response shows `search_mode: hybrid` and returns the imported memory.
**Why human:** All automated proofs run on one host with a fake or dead embedder; the "across machines" outcome and the live re-embed are external integration.

### Gaps Summary

No gaps. Every roadmap success criterion, the D-14 scope addition, all four plans' truths (except the one abstained backstop item), all artifacts, all key links, all nine prohibitions and all six review fixes are confirmed in the code at `efcc1dd`, with the behavioral tests run in this verification (196 scoped tests, 0 failures). Status is `human_needed` solely because (a) the 05-02 backstop truth lacks the concurrent-write evidence the honest-verifier rule requires, and (b) MVP mode routes the real cross-machine/live-Ollama user flow to a human. Neither blocks Phase 6 planning; the first is best closed by adding one held-out concurrency test.

---

_Verified: 2026-10-25T13:45:00Z_
_Verifier: Claude (gsd-verifier)_
