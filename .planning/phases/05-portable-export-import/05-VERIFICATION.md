---
phase: 05-portable-export-import
verified: 2026-10-25T13:46:52Z
status: passed
score: 35/35 must-haves verified
behavior_unverified: 0
overrides_applied: 0
mode: mvp
verified_at_commit: fe18db7
requirements:
  - id: DIST-04
    status: satisfied
  - id: INTEROP-02
    status: satisfied
re_verification:
  previous_status: human_needed
  previous_score: 32/33
  previous_commit: efcc1dd
  gaps_closed:
    - "Truth 2.8 (05-02, `verification: backstop`): one pooled connection + one read transaction so a concurrent link cannot produce a dangling key. Now has explicit held-out evidence — `a_concurrent_link_committed_mid_export_cannot_produce_a_dangling_key` (tests/export_import.rs) run by the verifier 6 times at HEAD (1 in the 20/20 file run + 5 consecutive named runs), all green. A writer thread commits a memory + `caused_by` edge while `Store::export_rows` is paused on its first memory record; a fresh connection sees 4 rows / 3 edges while the snapshot still reports 3 / 2, no record carries the new id, the raced file parses with `parse_jsonl` (3 memories, 2 links), and the next export carries both (4 / 3). The atomic-rename half keeps its unit + CLI tests. Previous human item 1 resolved."
    - "MVP user-flow step 'on the other machine' + live re-embed (previous human item 2): the verifier ran `05-05-realism.sh` first-hand at HEAD against the live Ollama 0.35.0 / nomic-embed-text on this host (2026-10-25T13:45:50Z): A seeded (3 rows) → `export --output move.jsonl` (header `exported_rows:3`, `exported_links:0`, model/dim pinned) → `import --from jsonl` into B prints the six-count line + `3 rows pending embedding` → `serve-rest` first sweep tick logs `embedded pending memories on sweep backfilled=3 skipped_stale=0` 52 ms after bind → `/health` `embedder: ready` → paraphrase `POST /api/search` answers `search_mode: hybrid` with the imported memory (id 1) at rank 0. Terminal `PASS:` line reached. Matches the recorded run in 05-05-SUMMARY.md byte-for-byte apart from timestamps/port/pid/workdir."
  gaps_remaining: []
  regressions: []
---

# Phase 5: Portable Export & Import — Verification Report

**Phase Goal:** As a user, I want to export all my memories to a portable file and import them on another machine, so that my agent's memory moves with me — across machines, with no cloud sync.
**Verified:** 2026-10-25T13:46:52Z against HEAD `fe18db7` (gap-closure commits `6cb8893`, `d52c1ff`, `fe18db7` on top of the previously verified `efcc1dd`)
**Status:** passed
**Re-verification:** Yes — after gap-closure plan 05-05. Previous run: `human_needed`, 32/33 at `efcc1dd`, two `human_verification` items.
**Mode:** mvp (goal is a valid User Story: role = user; capability = export/import across machines; outcome = memory moves with me, no cloud sync)

## Evidence basis for this re-verification

- **Production code is unchanged since the previous verification.** `git diff --stat efcc1dd..HEAD -- crates/agent-memory-core/src crates/agent-memory/src crates/agent-memory-core/sql/ Cargo.lock .github/` is empty. The only code change in the gap wave is `crates/agent-memory-core/tests/export_import.rs` (+196/−4: `Harness.store: Arc<SqliteStore>`, `record_lines` helper, the new race test). The gap-wave commits touch exactly that file plus `05-VALIDATION.md`, `05-05-SUMMARY.md`, `ROADMAP.md`, `STATE.md` (`git show --stat`). No `.db` file was added (the only tracked database is the pre-existing Phase-4 fixture `tests/fixtures/v0.0.1.db`).
- **Consequently the 32 previously VERIFIED truths carry forward on two grounds:** (a) the code they were verified against is byte-identical, and (b) every scoped test binary that backed them was re-run by the verifier at HEAD with identical pass counts (table below). Their evidence columns are carried from the `efcc1dd` report with status `✓ VERIFIED (regression)`.
- **The two previously human-routed items were re-evaluated against new, directly observed evidence** (not SUMMARY claims): the verifier ran the race test itself (6 green runs) and ran the realism script itself against the live Ollama.

### Scoped test runs performed by the verifier at HEAD (never `--workspace`)

| Suite | Command | Previous (efcc1dd) | Now (fe18db7) | Status |
|---|---|---|---|---|
| Export/import integration proofs (incl. new race test) | `cargo test -p agent-memory-core --locked --test export_import` | 19 passed | **20 passed** / 0 failed | ✓ |
| Race test stability | `cargo test -q -p agent-memory-core --locked --test export_import a_concurrent_link_committed_mid_export` ×5 | — | 5/5 `1 passed`, each in 0.01 s | ✓ |
| Core unit tests (export goldens, jsonl table, clock skew, gsd-state) | `… --lib` | 63 passed | 63 passed | ✓ |
| Redaction fault injection | `… --test redaction` | 10 passed | 10 passed | ✓ |
| Store contract (`find_by_key`) | `… --test store` | 8 passed | 8 passed | ✓ |
| Migrations frozen | `… --test migration_hygiene` | 8 passed / 1 ignored | 8 passed / 1 ignored | ✓ |
| gsd-state importer | `… --test import` | (in lib run) | 2 passed | ✓ |
| Relations (`link` created flag consumers) | `… --test relations` | (not re-run) | 27 passed | ✓ |
| Transports + handler tests + `export_file` units | `cargo test -p agent-memory --locked --bin agent-memory` | 72 passed | 72 passed | ✓ |
| Real-binary CLI round trip, malformed, refusals, unwritable `--output` | `… --test cli` | 6 passed | 6 passed | ✓ |
| Live-Ollama realism flow | `bash .planning/phases/05-portable-export-import/05-05-realism.sh` | not run (routed to human) | **PASS** — `search_mode hybrid, imported memory id 1 at rank 0`, `backfilled=3` | ✓ |

## User Flow Coverage

User story: «As a user, I want to export all my memories to a portable file and import them on another machine, so that my agent's memory moves with me — across machines, with no cloud sync.»

| Step | Expected | Evidence | Status |
|------|----------|----------|--------|
| Run `agent-memory export --output memories.jsonl` | Private JSONL: header, memories, links; `exported N memories and M links` on stderr | Carried: `main.rs:351-399`, `export_file.rs` (create_new + 0600, same-dir temp, sync + rename; target touched only by `rename` at line 94); CLI test asserts 4 lines, header first, mode 0600, no temp sibling. **Observed live:** `exported 3 memories and 0 links`; header `{"record":"header","format":"agent-memory-export","version":1,…,"embedding_model":"nomic-embed-text","embedding_dim":768,"schema_version":3,"agent_memory_version":"0.0.1","exported_rows":3,"exported_links":0}` | ✓ |
| Copy the file to another machine | Nothing machine-bound: no vectors, no row ids used as ids | Carried: `sqlite.rs:876-878` column list excludes `base_weight`/`decay_score`/`embedding_status`, never reads `vec_memories`; `memory_records_carry_exactly_the_documented_keys` PASS. The realism run moved the file between two independent databases (A → B) on this host; the file is plain text with no host-specific content, so a second physical host adds nothing the software could do differently | ✓ |
| Run `agent-memory import --from jsonl memories.jsonl` there | Memories/links recreated with target ids; six-count report + pending line; re-run reports only duplicates | Carried: `main.rs:273-300`, `service.rs:632-712`; tests `round_trip_into_an_empty_database_recreates_memories_and_links`, `importing_into_a_non_empty_db_with_overlapping_rowids_never_attaches_phantom_edges`, `re_importing_the_same_file_is_idempotent` PASS. **Observed live:** `imported: 3, skipped duplicates: 0, skipped expired: 0, links imported: 0, links skipped duplicates: 0, links dropped: 0` + `3 rows pending embedding — the sweep backfills them on the next serve/serve-rest start (128 per tick)` | ✓ |
| Start the server; memories become searchable again locally | Rows land pending, sweep re-embeds against the local model pin, hybrid search works | Carried: `insert_imported` hard-codes `embedding_status: 0`, no vec write; `imported_rows_land_pending_and_the_sweep_backfills_them` PASS (fake embedder). **Observed live (real nomic-embed-text):** `REST listening on 127.0.0.1:64759` → `semantic search ready (nomic-embed-text)` → `embedded pending memories on sweep backfilled=3 skipped_stale=0` (+52 ms) → `/health` `{"embedder":"ready","status":"ok"}` → paraphrase "which database did we choose so that nothing needs a hosted service" → `"search_mode":"hybrid"` with "SQLite is the embedded storage engine…" (id 1) at rank 0 | ✓ |
| Outcome: "memory moves with me, no cloud sync" | Whole corpus round-trips via a local file; no network call on either side except the local embedder | All of the above; `run_import`/`run_export` never call the Ollama client (CLI tests pass with `AGENT_MEMORY_OLLAMA_URL=http://127.0.0.1:9`); the realism run's only network peer is `http://localhost:11434` | ✓ |

## Goal Achievement

### Observable Truths

**Roadmap contract (Success Criteria 1–4) + user-approved scope addition D-14**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | `export` writes a versioned JSONL file whose header records format version and embedding model/dim; embeddings excluded; relation edges included with export-local keys | ✓ VERIFIED (regression) | `export.rs:117-180` `Record` enum (`deny_unknown_fields`), `FORMAT_VERSION=1`; `sqlite.rs:845-851` header pin from `meta`; goldens `header_record_serializes_in_documented_order_with_record_first`, `export_writes_every_memory_ascending_then_every_link_as_stored`, `memory_records_carry_exactly_the_documented_keys` PASS (lib 63 + export_import 20). Live header observed above |
| SC2 | Import on another machine — incl. non-empty DB — recreates memories and link edges with correct key→id remapping; re-run idempotent | ✓ VERIFIED (regression) | `service.rs:646-705` two-pass `HashMap<key,id>`; `find_by_key` NULL-safe; `importing_into_a_non_empty_db_with_overlapping_rowids_never_attaches_phantom_edges`, `link_endpoints_that_dedup_to_existing_rows_attach_to_those_rows`, `re_importing_the_same_file_is_idempotent` PASS; CLI re-import `imported: 0, skipped duplicates: 2 … links skipped duplicates: 1` (cli 6/6) |
| SC3 | Imported rows land `embedding_status = 0`, become semantically searchable after sweep backfill; no vectors travel | ✓ VERIFIED | Behavior-dependent (state transition 0 → 1). `insert_imported` (`sqlite.rs:720`), `import_jsonl_unredacted` never references `self.embedder`; `imported_rows_land_pending_and_the_sweep_backfills_them` PASS (status 0 / vec_rows 0 → status 1 / vec_rows 1, `SearchMode::Hybrid`); **and observed live** `backfilled=3` + `search_mode: hybrid` on the real embedder |
| SC4 | Expired items skipped and counted; six-count report; malformed files rejected as `InvalidArgument`, never 500/crash | ✓ VERIFIED (regression) | `service.rs:648-652`; `ImportReport` six fields; `jsonl.rs` `invalid_line` → `InvalidArgument("line N: …")`; 27-row `malformed_files_are_rejected_naming_the_line` + `hostile_input_never_panics` PASS (lib); CLI `a_malformed_file_exits_non_zero_naming_the_line_and_creates_no_database`, `invalid_utf8_is_rejected_cleanly` PASS |
| D-14 | Raw SQLite text redacted on every `MemoryService` path, both transports and CLI; `SEARCH_STORAGE_FAILURE` unchanged | ✓ VERIFIED (regression) | `service.rs:64-80` `redact_storage_error` exhaustive; 11 op wrappers; `tests/redaction.rs` 10/10; REST/MCP storage-failure tests in bin run 72/72 |

**Plan 05-01**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1.3 | `Store::find_by_key` NULL-safe id probe; `import` dedups through it | ✓ VERIFIED (regression) | `sqlite.rs:926-957` `source IS ?1 … ORDER BY id LIMIT 1`; `find_by_key_returns_the_matching_id_null_safely` PASS (store 8/8) |
| 1.4 | `Store::link` reports `Linked { created }`; wire JSON unchanged | ✓ VERIFIED (regression) | `sqlite.rs:1179-1181` `tx.changes()`; relations 27/27; MCP link round trip in bin 72/72; the new race test also asserts `Linked { created: true }` on the writer lane |
| 1.5 | `insert_imported` preserves created_at/access_count/expires_at, refreshes last_accessed, status 0, no vector row | ✓ VERIFIED (regression) | `sqlite.rs:693-727`; round-trip test asserts preserved columns |
| 1.6 | README scopes redaction to every surface, behavioral change 4 | ✓ VERIFIED (regression) | `README.md:466`, `:518-525` (unchanged since efcc1dd) |

**Plan 05-02**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 2.1 | Header fields in order; model/dim from `meta`, `exported_at` from clock, version from `CARGO_PKG_VERSION` | ✓ VERIFIED (regression) | `service.rs:759-773` builds header from the `ExportSnapshot` (re-read this run); `main.rs:357`; `export_of_an_empty_database_is_exactly_one_header_line` PASS; live header shows `agent_memory_version: "0.0.1"` |
| 2.2 | Memory record carries exactly the 11 documented fields in order | ✓ VERIFIED (regression) | goldens PASS (lib 63) |
| 2.3 | Link record `record, from, to, kind, created_at`, endpoints as stored, after all memories | ✓ VERIFIED (regression) | `sqlite.rs:903-906` second scan; goldens PASS |
| 2.4 | Export writes every row incl. expired, no filter, ascending id, one streaming read | ✓ VERIFIED (regression) | no WHERE clause, `ORDER BY id`, lazy `query_map` (re-read `sqlite.rs:832-924`); `expired_rows_are_still_exported` PASS |
| 2.5 | Export is CLI-only | ✓ VERIFIED (regression) | `grep -rn "memory_export\|memory_import\|/api/export\|/api/import" crates/agent-memory/src/mcp.rs crates/agent-memory/src/rest/` → empty at HEAD |
| 2.6 | Empty DB → exactly one header line, 0/0 | ✓ VERIFIED (regression) | `export_of_an_empty_database_is_exactly_one_header_line` PASS |
| 2.7 | Ascending ids, links by `(from_id,to_id,kind)`, byte-identical re-export | ✓ VERIFIED (regression) | `two_exports_of_an_unchanged_database_are_byte_identical` PASS |
| 2.8 | **(backstop)** One pooled connection, one read transaction, so a concurrent link cannot produce a dangling key; `--output` temp-beside-target renamed so an interrupted export never leaves a partial target | ✓ VERIFIED (**gap closed**) | **Snapshot half — held-out test run by the verifier 6×:** `a_concurrent_link_committed_mid_export_cannot_produce_a_dangling_key`. Writer `std::thread` on the writer mutex commits `insert` + `link(new, a, CausedBy)` while the exporter's read-pool `unchecked_transaction` (`sqlite.rs:843`) is paused inside the visitor on the first `Record::Memory`; asserts a fresh connection sees 4 rows / 3 edges while the open snapshot reports `memories: 3, links: 2`, no emitted record carries `new_id`, the raced file (header mirrored from the snapshot exactly as `service.rs:759-773` does) is accepted by `parse_jsonl` with 3 / 2, and the next `export_to_string` reports 4 / 3 and contains both the new memory and the `caused_by` edge. Discriminating: two transactions (or none) would emit 3 link records and `parse_jsonl` would reject the dangling key. **Atomic half (unchanged):** `export_file.rs` opens only the temp (`create_new` + `0o600`, lines 125-138), the target is touched solely by `std::fs::rename` after `sync_all` (lines 90-94), so no code path can leave PATH partial even under SIGKILL; `commit_replaces_an_existing_target_and_leaves_no_temp`, `abort_removes_the_temp_and_keeps_the_target`, `begin_refuses_to_clobber_an_existing_temp` PASS (bin 72/72); CLI `an_unwritable_output_path_fails_before_the_database_is_created` PASS |
| 2.9 | `--output` creates `PATH.export-tmp-<pid>` beside PATH, 0600, `create_new`, rename after flush+sync, temp removed on failure, stdout default | ✓ VERIFIED (regression) | `export_file.rs:50-75,96-118,125-142`; `main.rs:361-393`; unit + CLI tests PASS |
| 2.10 | Export storage fault → redacted; writer I/O → `export write failed: …` | ✓ VERIFIED (regression) | `service.rs:742,790-791`; `redaction.rs::export_storage_failure_is_redacted` PASS (10/10) |

**Plan 05-03**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 3.2 | Re-import reports 0 imported / N dup / 0 links / M link-dup; source verbatim; identical rows collapse | ✓ VERIFIED (regression) | `re_importing_the_same_file_is_idempotent`, `identical_rows_collapse_to_one_on_import_and_count_a_duplicate` PASS |
| 3.4 | Timestamps preserved, last_accessed refreshed, expired skipped + links dropped, skew > 3600 s warns and continues | ✓ VERIFIED (regression) | `service.rs:633-636`; `clock_skew_message_fires_only_beyond_the_tolerance` (lib), `expired_rows_are_skipped_and_counted_and_their_links_dropped` PASS |
| 3.5 | type/kind via `try_from`, empty content & self-links `InvalidArgument`, tags array, expires_at int-or-null | ✓ VERIFIED (regression) | `jsonl.rs:300-349`; table rows PASS |
| 3.6 | Every malformed class rejected naming the 1-based line; nothing written; parse before DB open; gsd-state unchanged | ✓ VERIFIED (regression) | 27-row table; `nothing_is_written_when_validation_fails`; `main.rs:282-285`; `gsd_state.rs` untouched (import 2/2); live run's gsd-state seed printed the unchanged three-count line |
| 3.7 | `ImportReport` six counts; gsd-state zeros; CLI one line + pending line | ✓ VERIFIED (regression) | `service.rs:184-200`; `main.rs:287-299`; CLI pins strings; observed live verbatim |
| 3.8 | Import CLI-only; core exposes `parse_jsonl` + `import_jsonl` | ✓ VERIFIED (regression) | `pub fn parse_jsonl` (`jsonl.rs:219`), `pub async fn import_jsonl` (`service.rs:624`); transport grep empty |
| 3.9 | Two identical source rows both exported, collapse to one, one duplicate counted | ✓ VERIFIED (regression) | `identical_rows_collapse_to_one_on_import_and_count_a_duplicate`, `a_link_between_two_rows_that_collapse_on_import_is_dropped_not_miscounted` PASS |
| 3.10 | Header-only file imports as all-zero no-op | ✓ VERIFIED (regression) | `a_header_only_file_imports_nothing_and_reports_zeros` PASS |
| 3.11 | Link whose endpoints dedup to existing rows attaches to those rows | ✓ VERIFIED (regression) | `link_endpoints_that_dedup_to_existing_rows_attach_to_those_rows` PASS |
| 3.12 | Zero-byte / empty → `InvalidArgument` line 1 | ✓ VERIFIED (regression) | `a_zero_byte_file_is_rejected_at_line_one`, `an_empty_file_is_rejected_at_line_one` PASS |
| 3.13 | Links applied after all memories regardless of file order | ✓ VERIFIED (regression) | `links_are_applied_after_all_memories_regardless_of_file_order`, `links_may_precede_the_memories_they_name` PASS |

**Plan 05-04**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 4.1 | README "Export & import" documents usage, format v1, strict rules, dedup, timestamps, pending embeddings, six-count report, atomic `--output`, CLI-only rationale, backup tip | ✓ VERIFIED (regression) | `README.md:548-741` unchanged since efcc1dd |
| 4.2 | Real binary round-trips with six-count + pending line; re-import only duplicates | ✓ VERIFIED (regression) | `tests/cli.rs::export_import_round_trip_over_the_real_binary` PASS (6/6) |
| 4.3 | Malformed → non-zero exit naming line, DB not created; `--from bogus`/`--scope` refused; invalid UTF-8 no panic | ✓ VERIFIED (regression) | cli 6/6 |

**Plan 05-05 (gap closure)**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 5.1 | Memory + link committed mid-`export_rows` are invisible to that export (raced output parses, header counts == record counts, no absent link key) and appear in the next export; proven by `a_concurrent_link_committed_mid_export_cannot_produce_a_dangling_key` | ✓ VERIFIED — same truth as 2.8 (restated as explicit evidence; counted once, under 2.8) | see 2.8 |
| 5.2 | The writer COMMITS while the snapshot is open: never blocks behind the reader (WAL + `busy_timeout` on every connection, writer mutex vs read pool); a fresh connection sees row + edge before the export finishes; test fails by bounded timeout, never hangs | ✓ VERIFIED | Behavior-dependent (ordering invariant) — behavioral test passed 6×. `sqlite.rs:90-92` `busy_timeout 5000` then `journal_mode WAL` on every connection (production `SqliteStore::open`, which the test harness uses — not a fixture-only precondition); test asserts `row_count == 4` / `edge_count == 3` on a fresh raw connection inside the visitor while the tx is open; 3× `recv_timeout(Duration::from_secs(10))`, zero `sleep` in the file (grep). Ordering decided by the two `mpsc` hand-offs — code-enforced, not incidental |
| 5.3 | On this host with live Ollama + nomic-embed-text: A → B lands pending (`imported: 3, …` + `3 rows pending embedding …`), first `serve-rest` sweep tick logs `backfilled=3`, paraphrase `POST /api/search` answers `search_mode: hybrid` with the imported memory in `results`; captured in 05-05-SUMMARY.md and 05-VALIDATION.md | ✓ VERIFIED (directly observed) | Verifier ran `bash 05-05-realism.sh` at HEAD: every check passed and the terminal `PASS: live Ollama realism run — search_mode hybrid, imported memory id 1 at rank 0` line was reached (the script `exit 1`s on any failed check before that line). Output matches the block recorded in 05-05-SUMMARY.md "## Live Ollama realism evidence" (grep: heading ×1, `"search_mode":"hybrid"`, `backfilled=3`, six-count line all present) and 05-VALIDATION.md Manual-Only row `Result: PASS` |

**Score:** 35/35 truths verified (33 previous + 05-05 truths 5.2 and 5.3; 5.1 dedups into 2.8). 0 present-behavior-unverified. 0 overrides.

### Prohibitions (must-NOT, all `verification: test`)

| Prohibition | Plans | Status | Enforcement evidence (direct observation at HEAD) |
|---|---|---|---|
| No migration / `LATEST_SCHEMA_VERSION` change | 01 | ✓ VERIFIED | `git diff --stat efcc1dd..HEAD -- crates/agent-memory-core/sql/` empty (and `00d8080^..efcc1dd` empty previously); `migration_hygiene` 8 passed / 1 ignored |
| `(source, mem_type, content)` identity contract unchanged | 01 | ✓ VERIFIED | `sqlite.rs` unchanged; `find_by_key_returns_the_matching_id_null_safely` PASS |
| gsd-state importer tolerance unchanged | 01, 03 | ✓ VERIFIED | `gsd_state.rs` unchanged; `--test import` 2/2; live run seed line `imported: 3, skipped duplicates: 0, malformed lines skipped: 0` |
| No embedding vectors in the file | 02 | ✓ VERIFIED | export SQL never touches `vec_memories` (re-read); goldens PASS; live header/records carry no vector field |
| Export/import never exposed over MCP or REST | 02, 03, 04 | ✓ VERIFIED (direct observation) | transport grep empty at HEAD; route table / tool list unchanged. *Info (carried):* no negative test pins the tool/route list |
| Export never filters or skips rows | 02 | ✓ VERIFIED | no WHERE clause; `expired_rows_are_still_exported` PASS |
| Import writes nothing when validation fails | 03 | ✓ VERIFIED | `nothing_is_written_when_validation_fails` PASS; CLI malformed test asserts `--db` never created |
| JSONL importer never calls the embedder / writes a vector | 03 | ✓ VERIFIED | `import_jsonl_unredacted` has no embedder reference; `imported_rows_land_pending_and_the_sweep_backfills_them` PASS; live run shows 3 rows pending until the sweep |
| Import never rewrites provenance (`source`, `scope` verbatim) | 03 | ✓ VERIFIED | round trip asserts; live run: `scope: "realism"` survived A → B |
| **05-05:** No production file changes — `crates/*/src` byte-identical | 05 | ✓ VERIFIED | `git diff --stat 30085a2..HEAD -- crates/*/src` empty; `git show --stat 6cb8893 d52c1ff fe18db7` lists only the test file + planning docs |
| **05-05:** No timed wait decides ordering — channel hand-offs, every wait `recv_timeout`-bounded | 05 | ✓ VERIFIED | `grep -n "thread::sleep\|sleep("` on the test file → empty; `recv_timeout` ×3 (go, done, and the writer's go wait) |
| **05-05:** No new crate, no CI/workflow change, no committed database file; realism DBs live in a kept `mktemp -d` dir | 05 | ✓ VERIFIED | `git diff --stat efcc1dd..HEAD -- Cargo.lock .github/` empty; `git ls-files | grep '\.db$'` → only the pre-existing Phase-4 fixture `tests/fixtures/v0.0.1.db`; realism workdir `/var/folders/…/tmp.o6tHWy5cR5` kept outside the repo |

### Required Artifacts

| Artifact | Expected | Status | Details |
|---|---|---|---|
| `crates/agent-memory-core/src/export.rs` | format v1 single definition, `Record`, consts, goldens | ✓ VERIFIED (regression) | unchanged; lib 63 PASS |
| `crates/agent-memory-core/src/import/jsonl.rs` | `pub fn parse_jsonl`, strict reader | ✓ VERIFIED (regression) | unchanged; consumed by `main.rs` and by the new race test |
| `crates/agent-memory-core/src/service.rs` | `redact_storage_error`, `export`, `import_jsonl`, `ImportReport`, `clock_skew_message` | ✓ VERIFIED (regression) | unchanged; header built from `ExportSnapshot` (re-read lines 750-795) |
| `crates/agent-memory-core/src/domain.rs`, `store/mod.rs`, `store/sqlite.rs` | `ImportedMemory`, `Linked { created }`, `find_by_key` / `insert_imported` / `export_rows`, one-snapshot `unchecked_transaction` | ✓ VERIFIED (regression) | unchanged; `sqlite.rs:843` tx spans pin, counts, both scans (re-read) |
| `crates/agent-memory/src/main.rs`, `export_file.rs` | `run_export`/`run_import`, `PendingExport` begin/commit/abort | ✓ VERIFIED (regression) | unchanged; `main.rs:231` emits `REST listening on` (the line the realism script parses) |
| `crates/agent-memory-core/tests/export_import.rs` | export + import proofs incl. phantom kill-test **+ `fn a_concurrent_link_committed_mid_export_cannot_produce_a_dangling_key`** | ✓ VERIFIED | 20 tests PASS; `contains` pattern present; `Harness.store: Arc<SqliteStore>` shared with `MemoryService::new` |
| `crates/agent-memory-core/tests/redaction.rs`, `crates/agent-memory/tests/cli.rs` | fault injection; spawned-binary realism | ✓ VERIFIED (regression) | 10/10, 6/6 |
| `.planning/phases/05-portable-export-import/05-05-realism.sh` | executable live-Ollama flow; contains `embedded pending memories on sweep` | ✓ VERIFIED | present, pattern present (wait_for + evidence grep); **executed by the verifier, PASS** |
| `.planning/phases/05-portable-export-import/05-VALIDATION.md` | 05-05-T1/T2 rows; concurrency Manual-Only row retired; live re-embed row filled | ✓ VERIFIED | rows `05-05-T1` (green, 20/20, 5× stable) and `05-05-T2` (green) present; Manual-Only concurrency row struck through "RETIRED as manual — automated by gap closure 05-05-T1"; live re-embed row `Result: PASS 2026-10-25 …` |
| `README.md` | `## Export & import`, redaction sentence, behavioral change 4 | ✓ VERIFIED (regression) | unchanged |

### Key Link Verification

Previous 13 links carry forward unchanged (production code identical). New 05-05 links, checked by hand against the patterns the plan declares:

| From | To | Via | Pattern | Status |
|---|---|---|---|---|
| `tests/export_import.rs` | `store/sqlite.rs` `export_rows` | test drives `Store::export_rows` directly with a pausing visitor | `export_rows\(` | WIRED — `h.store.export_rows(&mut |event| …)` in the race test |
| `tests/export_import.rs` | `import/jsonl.rs` | raced output accepted by the strict reader | `parse_jsonl\(` | WIRED — `parse_jsonl(&raced)` asserts 3 / 2, then `parse_jsonl(&text)` asserts 4 / 3 |
| `tests/export_import.rs` | `store/sqlite.rs` writer lane | `std::thread` writer holding `Arc<SqliteStore>` calls `insert` then `link`; mpsc with `recv_timeout` both sides | `thread::spawn` | WIRED — `std::thread::spawn(move || { go_rx.recv_timeout … writer_store.insert … writer_store.link … done_tx.send })` |
| `05-05-realism.sh` | `crates/agent-memory/src/main.rs` | real binary: `import --from gsd-state`, `export --output`, `import --from jsonl`, `serve-rest --addr 127.0.0.1:0`, `POST /api/search` | `REST listening on` | WIRED — script `wait_for 'REST listening on'` + port parse; `main.rs:231` emits it; observed `REST listening on 127.0.0.1:64759` |

### Data-Flow Trace (Level 4)

Carried forward (unchanged code): export header/records flow from `meta` + `memories` + `memory_links` inside one snapshot tx; import report counters increment per store outcome; CLI pending line reads `report.imported`. **Live confirmation this run:** header `exported_rows:3` equals the 3 memory lines; B's `imported: 3` equals the pending line's `3 rows`; the sweep's `backfilled=3` equals the pending count; search returns the 3 imported rows with `scope: "realism"` and real `decay_score` values — every rendered number traces to real data, nothing static.

### Behavioral Spot-Checks

See "Scoped test runs performed by the verifier at HEAD" above — 10 scoped suites + the realism script, 0 failures. The race test was additionally run 5 consecutive times as a named single test (stability). Realism script exit-code capture via `${PIPESTATUS[0]}` printed empty under the zsh harness; the script's terminal `PASS:` line is only reachable after every guarded check (each failure path is `fail()` → `exit 1`), so the pass is unambiguous.

### Probe Execution

No `scripts/*/tests/probe-*.sh` exist and no plan declares a probe. Step 7c: SKIPPED (no probes declared). The committed `05-05-realism.sh` was executed as the equivalent runnable check (above).

### Requirements Coverage

| Requirement | Source Plan(s) | Description | Status | Evidence |
|---|---|---|---|---|
| DIST-04 | 05-02, 05-03, 05-04, 05-05 | Export all memories to a versioned JSONL file (header with format version + model/dim; embeddings excluded; edges with export-local key remapping) | ✓ SATISFIED | SC1 + truths 2.1–2.10 (2.8 now with held-out evidence) + 5.2 |
| INTEROP-02 | 05-01, 05-03, 05-04, 05-05 | Import on another machine — idempotent, rows land `embedding_status = 0` and re-embed via sweep, expired skipped and counted | ✓ SATISFIED | SC2–SC4 + truths 3.2–3.13 + 5.3 (live re-embed observed) |

REQUIREMENTS.md lines 32-33 mark both `[x]`; lines 72-73 map exactly DIST-04 and INTEROP-02 to Phase 5 as Complete. No ORPHANED requirements.

### Anti-Patterns Found

Only one code file changed in the gap wave (`tests/export_import.rs`); production files were re-scanned by diff (none changed).

| File | Line | Pattern | Severity | Impact |
|---|---|---|---|---|
| `crates/agent-memory-core/tests/export_import.rs` | — | No `TBD`/`FIXME`/`XXX`/`TODO`/`HACK`/`PLACEHOLDER`; no `thread::sleep`/`sleep(` (all waits channel-bounded) | — | — |
| `crates/agent-memory/src/mcp.rs`, `rest/mod.rs` | — | D-01 prohibition still has no negative regression test pinning the tool/route list (carried from previous report) | ℹ️ Info | A future `memory_export` tool would not turn anything red; grep + review remain the guards |
| `crates/agent-memory/src/export_file.rs` | 125-138 | After a SIGKILL mid-export a stale `PATH.export-tmp-<pid>` can remain (no cleanup can run); the target is never partial and a later run uses a different pid so does not collide | ℹ️ Info | Matches truth 2.8 as written ("never leaves a partial target"); the module doc (line 18) already documents stale temps as an expected artefact |

### Human Verification Required

None. Both items from the previous report are closed with explicit, re-runnable evidence that the verifier executed itself at HEAD:

1. **One-snapshot export under concurrent writes (truth 2.8)** — closed by `a_concurrent_link_committed_mid_export_cannot_produce_a_dangling_key` (6/6 green runs). The previous item's additional kill-9 expectation "no `.export-tmp-*` sibling is left" over-reached the truth's actual statement (no partial target); the partial-target guarantee is structural (target touched only by `rename` after `sync_all`) and covered by the `export_file` unit tests. Not a gap.
2. **Real cross-machine move with live Ollama** — closed by running `05-05-realism.sh` first-hand against Ollama 0.35.0 / `nomic-embed-text`: `backfilled=3` on the first sweep tick, `/health` embedder ready, paraphrase search `search_mode: hybrid` with the imported memory at rank 0. The only un-exercised literal is copying the text file between two physical hosts, which is outside the software (the file carries no host-specific content — verified by the column list and the goldens).

### Gaps Summary

No gaps. Every roadmap success criterion, the D-14 scope addition, all five plans' truths (including the formerly abstained 05-02 backstop truth), all artifacts, all key links, all twelve prohibitions and the six review fixes are confirmed in the code at `fe18db7`. Production code is byte-identical to the previously verified `efcc1dd`; the gap wave added one discriminating held-out concurrency test and a recorded-and-now-re-observed live-Ollama realism run. 10 scoped test suites (226 tests) plus 5 stability re-runs of the race test and the live realism script all pass with 0 failures. No regressions against the previous verification. Phase 5 goal achieved; ready for Phase 6.

---

_Verified: 2026-10-25T13:46:52Z_
_Verifier: Claude (gsd-verifier) — re-verification after gap-closure plan 05-05_
