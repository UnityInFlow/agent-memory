---
phase: 05-portable-export-import
plan: 02
subsystem: core-service, store, cli
tags: [rust, serde, jsonl, sqlite, rusqlite, export, streaming, atomic-write, file-permissions]

# Dependency graph
requires:
  - phase: 05-portable-export-import
    provides: "plan 05-01: redact_storage_error(op, e) + the *_unredacted inner-fn shape (D-14); LinkOutcome::Linked { created }; the one-INSERT store contract"
  - phase: 04-memory-update-relations-hybrid-search
    provides: "memory_links schema (0003) with relates_to stored canonically as (min, max); LinkKind wire strings; the backup temp-file + rename discipline this plan mirrors"
  - phase: 02-semantic-search-import-release
    provides: "meta table (0002) pinning embedding_model / embedding_dim — the header's source of truth (D-03)"
provides:
  - "export.rs: format v1 defined ONCE — FORMAT_NAME, FORMAT_VERSION, `Record { Header, Memory, Link }` (tag = record, lowercase, deny_unknown_fields), ExportSnapshot / ExportEvent store→service stream protocol, module-doc spec, 9 golden + strictness tests"
  - "Store::export_rows(visit): one pooled connection, one read transaction (unchecked_transaction) spanning the meta pin, both counts, memories ORDER BY id and links ORDER BY from_id, to_id, kind; rows visited lazily, never collected"
  - "MemoryService::export(out: Box<dyn Write + Send>, agent_memory_version) -> ExportReport { memories, links }: one spawn_blocking hop, header built from the snapshot, one JSON line per record, flush inside the task, redact_storage_error(\"export\", _)"
  - "CLI `agent-memory export [--output PATH]`: stdout BufWriter by default; --output goes through export_file::PendingExport (create_new + 0600 temp beside the target, sync_all + rename on Ok, best-effort remove on Err); report line on stderr"
  - "tests/export_import.rs (export half): harness + export_to_string + lines helpers; 5 proofs (empty-DB header from meta/clock, ascending memories then links as stored, exactly 11 memory keys, expired rows exported, byte-identical re-export) — plan 03 appends the import tests"
affects: [05-03 import tracer (reads Record via import::jsonl, uses tests/export_import.rs harness), 05-04 README Export & import section, phase-6 release notes]

# Actuals (#2632) — same estimateTokens scale as the plan's `estimate` (chars/4 over the realized diff)
actuals:
  tokens: 13300
  tasks: 2
  commits: 2

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Wire contract as one serde enum: a single internally-tagged, deny_unknown_fields `Record` enum is the only definition of the file format; wire field order == declaration order with the tag first, pinned by exact-string golden tests"
    - "Visitor-driven streaming read: `Store::export_rows(&mut dyn FnMut(ExportEvent) -> Result)` on ONE pooled connection + ONE read transaction (`conn.unchecked_transaction()`), stepping `query_map` lazily — the first read path in the repo that never builds a Vec"
    - "Boxed sink through spawn_blocking: `Box<dyn Write + Send>` moved into the blocking task, written with `serde_json::to_writer` + `b\"\\n\"`, flushed INSIDE the task before Ok; stdout sink is `BufWriter::new(std::io::stdout())` because `StdoutLock` is !Send"
    - "Private atomic file publish (binary crate): `{target}.export-tmp-{pid}` beside the target, `OpenOptions::create_new(true).mode(0o600)` under cfg(unix), `sync_all` then `rename`, temp removed best-effort on every failure path; one `async {}` block collects every fallible step between begin and commit so exactly one abort covers them"

key-files:
  created:
    - crates/agent-memory-core/src/export.rs
    - crates/agent-memory-core/tests/export_import.rs
    - crates/agent-memory/src/export_file.rs
  modified:
    - crates/agent-memory-core/src/lib.rs
    - crates/agent-memory-core/src/store/mod.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory/src/main.rs

key-decisions:
  - "Format v1 frozen (D-03, rated one-way, pre-approved): wire order is exactly `record, format, version, exported_at, embedding_model, embedding_dim, schema_version, agent_memory_version, exported_rows, exported_links` for the header; `record, key, type, content, tags, source, scope, created_at, last_accessed, access_count, expires_at` for a memory; `record, from, to, kind, created_at` for a link. The header carries `exported_rows` / `exported_links` (orchestrator resolution 1, decided now so it is inside v1)"
  - "The one-snapshot read uses `rusqlite::Connection::unchecked_transaction()` on the pooled `&Connection` (RESEARCH assumption A1 confirmed: present in rusqlite 0.39); the `BEGIN`/`COMMIT` execute_batch fallback was not needed"
  - "Research correction applied: the stdout sink is `BufWriter::new(std::io::stdout())`, NOT `stdout().lock()` — `StdoutLock` is `!Send` and cannot live in the `Box<dyn Write + Send>` that crosses into the blocking task; `Stdout` is `Send + Sync` and `BufWriter` keeps per-call locking cheap"
  - "Links are ordered by `(from_id, to_id, kind)` (D-05 fixes only 'after all memories'), making two exports of one database byte-identical and giving plan 03's round-trip diff a stable baseline"
  - "`run_export` wraps `pending.writer()` and `service.export` in one `async {}` result so a `try_clone` failure aborts the temp exactly like an export failure does — one `pending.commit()`, one `pending.abort()`, no temp can outlive an error"
  - "Tracer feedback gate resolved by re-running the fully automated <verify> on the committed tree (goldens, 5 export tests, workspace clippy, real-binary header + stderr report) rather than returning a mid-plan human-verify checkpoint: auto mode is off, but config sets human_verify_mode: end-of-phase and the tracer's verify has no human-judgment step (same resolution as 05-01)"

patterns-established:
  - "redact_storage_error op strings now number ten: store, list, search, import, forget, link, unlink, update, sweep, export"
  - "Crate-authored writer failures: `MemoryError::Internal(\"export write failed: {e}\")` for serde_json / io errors from the sink — passes redaction unchanged, never names the storage engine"
  - "export_file.rs is the ONE place that touches export-file permissions and atomic replacement (mirrors config.rs for the data dir); Windows ACL inheritance is the Phase 6 seam (DIST-03)"

requirements-completed: [DIST-04]

# Coverage metadata (#1602)
coverage:
  - id: D1
    description: "`agent-memory export` writes line 1 as the format-v1 header: format/version constants, exported_at from the clock, embedding_model/embedding_dim from the source DB's meta table, schema_version 3, agent_memory_version as passed by the binary (CARGO_PKG_VERSION), exported_rows/exported_links from the snapshot"
    requirement: DIST-04
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/export_import.rs#export_of_an_empty_database_is_exactly_one_header_line"
        status: pass
      - kind: unit
        ref: "crates/agent-memory-core/src/export.rs#header_record_serializes_in_documented_order_with_record_first"
        status: pass
      - kind: e2e
        ref: "cargo run -q -p agent-memory -- export --db <tmp>/memory.db | head -1 | grep -c '^{\"record\":\"header\",\"format\":\"agent-memory-export\",\"version\":1,' -> 1; stderr has 'exported 0 memories and 0 links'"
        status: pass
    human_judgment: false
  - id: D2
    description: "Every memory record carries exactly the 11 documented keys in declaration order (record, key, type, content, tags, source, scope, created_at, last_accessed, access_count, expires_at); decay_score, base_weight, embedding_status and vectors never travel; optional fields are written explicitly as null"
    requirement: DIST-04
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/export_import.rs#memory_records_carry_exactly_the_documented_keys"
        status: pass
      - kind: unit
        ref: "crates/agent-memory-core/src/export.rs#memory_record_serializes_in_documented_order_with_record_first"
        status: pass
    human_judgment: false
  - id: D3
    description: "Memories stream in ascending id order with values equal to the stored rows (type, tags, source, scope, created_at, last_accessed, access_count, expires_at = created_at + ttl or null); links follow all memories ordered by (from, to, kind) with endpoints exactly as stored (relates_to as (min, max), supersedes as given) and their original created_at; ExportReport equals the header counts"
    requirement: DIST-04
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/export_import.rs#export_writes_every_memory_ascending_then_every_link_as_stored"
        status: pass
    human_judgment: false
  - id: D4
    description: "Export writes every row present — an expired row (ttl passed, no sweep) is exported with its expires_at (D-06) — and two exports of an unchanged database under the same clock are byte-identical"
    requirement: DIST-04
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/export_import.rs#expired_rows_are_still_exported, #two_exports_of_an_unchanged_database_are_byte_identical"
        status: pass
    human_judgment: false
  - id: D5
    description: "The reader side of the contract is strict: an unknown field inside a record, an unknown record tag, a string key and a fractional header version are all rejected; omitted optional fields read as None"
    requirement: DIST-04
    verification:
      - kind: unit
        ref: "crates/agent-memory-core/src/export.rs#an_unknown_field_inside_a_memory_record_is_rejected, #an_unknown_record_tag_is_rejected, #a_string_key_is_rejected, #a_fractional_header_version_is_rejected, #missing_optional_fields_read_as_none, #every_record_round_trips_through_its_own_json"
        status: pass
    human_judgment: false
  - id: D6
    description: "`export --output PATH` creates `PATH.export-tmp-<pid>` in PATH's directory with mode 0600 (create_new — a stale temp is an AlreadyExists error, never truncated), renames it over PATH after sync, replaces an existing PATH, removes the temp on abort, and the real binary leaves exactly one line in out.jsonl, no temp sibling, and the stderr report"
    requirement: DIST-04
    verification:
      - kind: unit
        ref: "cargo test -p agent-memory --bin agent-memory export_file:: (5 tests: begin_creates_a_private_temp_beside_the_target incl. the cfg(unix) 0600 assertion, commit_replaces_an_existing_target_and_leaves_no_temp, abort_removes_the_temp_and_keeps_the_target, begin_refuses_to_clobber_an_existing_temp, export_temp_path_stays_in_the_target_directory)"
        status: pass
      - kind: e2e
        ref: "cargo run -q -p agent-memory -- export --db <tmp>/memory.db --output <tmp>/out.jsonl -> exit 0, 1 line, 0 *.export-tmp-* entries, stat mode 600, stderr 'exported 0 memories and 0 links'; `export --help | grep -c -- --output` -> 1"
        status: pass
    human_judgment: false
  - id: D7
    description: "Export is a CLI subcommand only — no MCP tool and no REST route (D-01); no vector table is read; a storage fault reaches the CLI only as the redacted `export failed due to a storage error (details are in the server log)`"
    requirement: DIST-04
    verification:
      - kind: other
        ref: "grep -rn 'memory_export\\|/api/export' crates/agent-memory/src/mcp.rs crates/agent-memory/src/rest/ -> empty; grep -c 'redact_storage_error(\"export\"' service.rs -> 1; export_rows SELECTs name only meta, memories, memory_links"
        status: pass
    human_judgment: true
    rationale: "The D-01 absence and the redaction wiring are grep-proven, but no fault-injection test exercises the export path end to end (plan 05-01's redaction.rs covers the nine prior ops; adding `export` there is a cheap follow-up for plan 03/04). A reviewer should confirm the op string wiring reads correctly."

# Metrics
duration: 13min
completed: 2026-10-25
status: complete
---

# Phase 5 Plan 02: `agent-memory export` — format v1 JSONL, streamed, atomic and private on disk Summary

**`agent-memory export [--output PATH]` ships: format v1 is defined once in `export.rs` as a strict tagged `Record` enum with golden-tested wire order, every memory and link streams one line at a time from ONE SQLite read snapshot through a boxed writer, and `--output` publishes the file via a 0600 `create_new` temp beside the target that is synced and renamed into place only after the stream has flushed.**

## Performance

- **Duration:** 13 min
- **Started:** 2026-10-25T11:44:48Z
- **Completed:** 2026-10-25T11:57:52Z
- **Tasks:** 2
- **Files modified:** 8 (3 created, 5 modified; +1306 / −0 lines)

## Accomplishments

- **Format v1 frozen in one Rust type (D-03/D-04/D-05).** `crates/agent-memory-core/src/export.rs` holds `FORMAT_NAME = "agent-memory-export"`, `FORMAT_VERSION = 1`, and `enum Record { Header, Memory, Link }` with `#[serde(tag = "record", rename_all = "lowercase", deny_unknown_fields)]`. The module doc is the format specification (a table per record type, the derived-data exclusion and why, the `key`-is-export-local rule, link placement and canonical endpoints, the canonical-output rule, the strict reader rules). Nine in-module tests pin the exact serialized strings for all three records (tag first, declaration order), round-trip each, and probe strictness: unknown field → `unknown field`, unknown tag → `unknown variant`, `"key":"1"` and `"version":1.5` rejected, omitted optionals → `None`. `type`/`kind` stay `String` so the importer converts through `MemoryType::try_from` / `LinkKind::try_from` (D-09). The header carries `exported_rows`/`exported_links` (orchestrator resolution 1), decided now so it lives inside v1.
- **One snapshot, zero buffering (D-02/D-06, T-05-19).** `Store::export_rows(visit)` takes one pooled read connection and opens one read transaction with `conn.unchecked_transaction()` (RESEARCH assumption A1 confirmed against rusqlite 0.39 — the `BEGIN`/`COMMIT` fallback was not needed). Inside it, in order: the `meta` pin (`embedding_model`, `embedding_dim` parsed from TEXT to `u32`, a non-integer being `Internal("meta.embedding_dim is not an integer (…)")`), `count(*)` of both tables → one `ExportEvent::Snapshot`; then `SELECT id, mem_type, content, tags, source, scope, created_at, last_accessed, access_count, expires_at FROM memories ORDER BY id` and `SELECT from_id, to_id, kind, created_at FROM memory_links ORDER BY from_id, to_id, kind`, each stepped lazily through `query_map` and handed to the visitor per row. No `base_weight`, `decay_score`, `embedding_status`, no `vec_memories` read. The FK guarantees every link endpoint is a memory in the same snapshot, so a concurrent `link` can never produce a dangling key.
- **`MemoryService::export(out, agent_memory_version) -> ExportReport` (D-14).** One `spawn_blocking` hop moves the `Box<dyn Write + Send>`, the store and the version; the `Snapshot` event builds `Record::Header` (clock → `exported_at`, `LATEST_SCHEMA_VERSION` → `schema_version`), every `Record` event is one `serde_json::to_writer` + `\n`, and `out.flush()` runs inside the task before `Ok(report)` so a `BufWriter` never loses its tail. Sink failures are the crate-authored `Internal("export write failed: …")`; storage faults go through `redact_storage_error("export", e)` — the tenth op string.
- **CLI `agent-memory export`.** `Command::Export { --output }` and `run_export` wire the store/service exactly like `run_import` (the embedder is constructed, never called). Default sink: `Box::new(BufWriter::new(std::io::stdout()))` — the research's `stdout().lock()` was corrected because `StdoutLock` is `!Send`. The version is the binary's `env!("CARGO_PKG_VERSION")` (Pitfall 9). The report `exported N memories and M links` is ALWAYS on stderr, since stdout may be the file. Real-binary check on a fresh DB: exactly one stdout line starting `{"record":"header","format":"agent-memory-export","version":1,` and the stderr report.
- **`--output` is private and atomic (D-02, T-05-02/T-05-03).** `crates/agent-memory/src/export_file.rs` is the one place touching export-file permissions: `export_temp_path` → `{target}.export-tmp-{pid}` in the target's own directory (rename never crosses a mount point; a shared tmp dir is a different trust domain); `begin` → `OpenOptions::write(true).create_new(true).mode(0o600)` under `cfg(unix)` (umask can only remove bits; a stale temp is an explicit `AlreadyExists`, never a truncate), the `cfg(not(unix))` arm being the Phase 6 Windows seam; `PendingExport::writer` (`try_clone`), `commit` (`sync_all` then `rename`, temp removed on either failure), `abort` (best-effort remove, `tracing::warn!` only on non-`NotFound`). Five unit tests cover temp placement + 0600 mode, replace-and-no-temp, abort-keeps-target, create_new refusal, and path derivation. Real binary: `--output` exits 0, leaves exactly one line, zero `*.export-tmp-*` entries, file mode 600.
- **Proof file for the phase.** `tests/export_import.rs` carries the relations.rs-style harness plus `export_to_string` (real file sink under the temp dir) and `lines` helpers, and the five export proofs listed in the coverage block. Plan 03 appends the import half.
- **Nothing else moved:** `sql/` diff empty, `LATEST_SCHEMA_VERSION = 3`, `Cargo.lock` byte-identical under `--locked`, no workflow touched, no new dependency; `grep -rn "memory_export\|/api/export"` over `mcp.rs` and `rest/` is empty (D-01).

## Task Commits

Each task was committed atomically (fmt + clippy `-D warnings` + full workspace suite green before each):

1. **Task 1 (tracer): `agent-memory export` streams a valid format-v1 JSONL document to stdout — one path, every layer** — `6524cf1` (feat)
2. **Task 2: `export --output PATH` writes a private temp file in PATH's directory and renames it into place** — `cffe7a2` (feat)

**Plan metadata:** see the `docs(05-02)` commit that follows this summary.

## Files Created/Modified

- `crates/agent-memory-core/src/export.rs` (new) — format v1 module-doc spec; `FORMAT_NAME`, `FORMAT_VERSION`; `Record { Header, Memory, Link }`; `ExportSnapshot`, `ExportEvent`; 9 golden/strictness tests
- `crates/agent-memory-core/src/lib.rs` — `pub mod export;` between `embed` and `import`
- `crates/agent-memory-core/src/store/mod.rs` — `fn export_rows(&self, visit: &mut dyn FnMut(ExportEvent) -> Result<(), MemoryError>)` with the one-snapshot contract, column list, ordering and the `embedding_dim` parse rule in its doc
- `crates/agent-memory-core/src/store/sqlite.rs` — `export_rows` impl: `unchecked_transaction`, two `FROM meta WHERE key` reads, two counts, memories scan `ORDER BY id`, links scan `ORDER BY from_id, to_id, kind`, lazy visitor calls, `tx.commit()`
- `crates/agent-memory-core/src/service.rs` — `pub struct ExportReport { memories, links }`, private `write_record_line`, `pub async fn export` + `async fn export_unredacted`; new imports (`std::io::Write`, `crate::export::*`, `LATEST_SCHEMA_VERSION`)
- `crates/agent-memory-core/tests/export_import.rs` (new) — harness, `export_to_string`, `lines`, five `#[tokio::test]` export proofs
- `crates/agent-memory/src/export_file.rs` (new) — `PendingExport`, `export_temp_path`, `begin`, `writer`, `commit`, `abort`, `discard_temp`, `create_private` (both cfg arms), 5 unit tests
- `crates/agent-memory/src/main.rs` — `mod export_file;`, `Command::Export { output: Option<PathBuf> }`, dispatch arm, `run_export(db, ollama_url, output)` with the stdout / pending-file sink match and the stderr report

## Decisions Made

- **Record field order as shipped** (tag first, then declaration order — serde-verified by the goldens): header `record, format, version, exported_at, embedding_model, embedding_dim, schema_version, agent_memory_version, exported_rows, exported_links`; memory `record, key, type, content, tags, source, scope, created_at, last_accessed, access_count, expires_at`; link `record, from, to, kind, created_at`. The D-03 one-way rating is recorded here as pre-approved (05-CONTEXT.md + the orchestrator's unattended-run instruction); no `checkpoint:decision` was emitted.
- **`unchecked_transaction`, not `BEGIN`/`COMMIT`.** `rusqlite::Connection::unchecked_transaction(&self)` exists in the pinned 0.39 and works on the pooled `&Connection`; the read-only `tx.commit()` simply ends the snapshot.
- **`StdoutLock` is `!Send`** — the research's `BufWriter::new(stdout().lock())` cannot be boxed as `dyn Write + Send`; shipped `BufWriter::new(std::io::stdout())` (the `Stdout` handle is `Send + Sync`).
- **Deterministic link order `(from_id, to_id, kind)`** beyond D-05's "after all memories", so re-exports are byte-identical and the plan 03 round-trip diff has a stable baseline.
- **One abort covers every failure between `begin` and `commit`.** The plan wrote `pending.writer().context(..)?` directly, which would leak a 0-byte temp if `try_clone` failed; `run_export` instead runs `writer()` + `service.export` inside one `async {}` result and matches on it, keeping exactly one `pending.commit()` and one `pending.abort()` (the acceptance greps) while making a leaked temp impossible.
- **Tracer feedback gate:** auto mode off, `human_verify_mode: end-of-phase`, verify fully automated → re-ran it on the committed tree (lib `export::` 9/9, `--test export_import` 5/5, workspace clippy clean, real binary header line + stderr report) and continued to Task 2 rather than returning a mid-plan `checkpoint:human-verify`. Recorded so the orchestrator can route end-of-phase UAT.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Destructured the report counts so the stderr `eprintln!` stays on one line**
- **Found during:** Task 1 (acceptance-criteria gate)
- **Issue:** The criterion `grep -c 'eprintln!("exported' main.rs` → `1` requires the string literal on the macro's line, but rustfmt's `fn_call_width` (60) wraps `eprintln!("exported {} memories and {} links", report.memories, report.links)` onto four lines (the args exceed 60 columns), and the hook re-applies rustfmt on every write.
- **Fix:** `let (memories, links) = (report.memories, report.links); eprintln!("exported {memories} memories and {links} links");` — same output, fits rustfmt's width.
- **Files modified:** `crates/agent-memory/src/main.rs`
- **Verification:** grep → 1; `cargo fmt --all -- --check` clean; e2e stderr grep → 1
- **Committed in:** `6524cf1`

**2. [Rule 1 - Bug] `PendingExport::temp_path` was dead code in the non-test build**
- **Found during:** Task 2 (clippy gate, `-D warnings` → `-D dead-code`)
- **Issue:** I added a `temp_path()` accessor (not in the plan's contract) so the `begin_creates_a_private_temp_beside_the_target` test can assert the exact temp path; production code never calls it, so clippy failed the bin with `method temp_path is never used`.
- **Fix:** Gated the accessor with `#[cfg(test)]` and documented it as test-only.
- **Files modified:** `crates/agent-memory/src/export_file.rs`
- **Verification:** `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `export_file::` 5/5
- **Committed in:** `cffe7a2`

### Process notes (not code deviations)

- **TDD RED/GREEN without a separate RED commit.** Both tasks are `tdd="true"`. RED was executed and observed: Task 1's `tests/export_import.rs` failed to compile against the pre-implementation tree (`unresolved import ExportReport`, `no method named export`), and Task 2's `export_file` test module was written first and failed with `cannot find function begin / export_temp_path`; each task's `feat(05-02)` commit then carries tests + implementation together, because the project operating rules forbid committing failing tests and the plan calls for one commit per task. The `export.rs` goldens were authored alongside the type they pin (a type and its serialization goldens are one unit) and passed on first run.
- **Pre-bash safety hook.** The repo's `.claude/hooks/pre-bash-rust.sh` blocked two gate invocations because `rm -f <log>` and a `memory.db` path appeared in the same command (its regex is `rm -[rfRF]+ .*\.db`); re-issued with a `: > log` truncation and no temp-dir removal. No code impact.

---

**Total deviations:** 2 auto-fixed (1 Rule 3 plan-internal criterion conflict, 1 Rule 1 clippy dead-code on a test-only accessor); no scope or behaviour change.
**Impact on plan:** None on scope. Both fixes exist only to make the plan's own gates (acceptance grep, `-D warnings`) pass as written.

## Issues Encountered

None beyond the two auto-fixes. Every targeted `<verify>` passed on the first post-GREEN run; the full suite passed on every run (246 after Task 1, 251 after Task 2); no fix-attempt budget was consumed on behaviour.

## Verification

Plan-level `<verification>` re-run after the second commit:

- `cargo fmt --all -- --check` — clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` — exit 0; `cargo test --workspace --locked` — **20 suites, 251 passed, 0 failed, 2 ignored** (232 before this plan → 246 after Task 1: +9 `export::` goldens, +5 `export_import` → 251 after Task 2: +5 `export_file::`).
- `cargo test -p agent-memory-core --lib export::` — 9 passed; `cargo test -p agent-memory-core --test export_import` — 5 passed; `cargo test -p agent-memory --bin agent-memory export_file::` — 5 passed (incl. the `cfg(unix)` `mode & 0o777 == 0o600` assertion).
- Real binary, fresh DB: `export` → 1 stdout line matching `^{"record":"header","format":"agent-memory-export","version":1,`, stderr `exported 0 memories and 0 links`; `export --output out.jsonl` → exit 0, 1 line, 0 `*.export-tmp-*` entries, `stat` mode `600`, same stderr line; `export --help | grep -c -- --output` → 1.
- `grep -rn "memory_export\|/api/export" crates/agent-memory/src/mcp.rs crates/agent-memory/src/rest/` — empty (D-01); `git diff --stat -- crates/agent-memory-core/sql/ Cargo.lock .github/` — empty; `LATEST_SCHEMA_VERSION: i64 = 3`.
- No `unwrap()`/`expect()` outside `#[cfg(test)]` in any touched production file.

## Known Stubs

None — no placeholder values, TODO/FIXME markers, skipped tests, or unwired data paths were introduced.

## Threat Flags

None new. The plan's register is implemented as mitigated: T-05-02 (0600 at create, temp beside the target, unit-tested), T-05-03 (flush → sync_all → rename, temp removed on every failure, unit-tested), T-05-04 (no vector field exists in `Record`; `export_rows` never reads `vec_memories`; 11-key test), T-05-11 (CLI only; transport grep empty), T-05-19 (one read transaction spans counts and both scans), T-05-20 (`redact_storage_error("export", _)`; writer faults are crate-authored text), T-05-SC (zero new deps, `--locked`). T-05-12 (stdout in scrollback) stays accepted; the README sentence pointing at `--output` is plan 04's.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- **Plan 05-03 (import tracer)** compiles against a frozen `Record` enum with `deny_unknown_fields` and pinned goldens; `import::jsonl` can `serde_json::from_str::<Record>` each line, convert `type`/`kind` through the Phase 3 seam, and map `key → id` via `find_by_key` / `insert_imported` (plan 01). `tests/export_import.rs` already has the harness, `export_to_string` and `lines` helpers for the round-trip kill-test; the header's `exported_rows`/`exported_links` are available as the truncation check.
- **Plan 05-04 (docs + CLI realism)** needs the README "Export & import" section (point it at the `export.rs` module doc, which is the spec), the T-05-12 `--output` recommendation, and may add `export` to `tests/redaction.rs` for an end-to-end fault-injection proof of the tenth op string.
- No blockers. `.planning/config.json` and the untracked harness files were left unstaged as instructed.

---
*Phase: 05-portable-export-import*
*Completed: 2026-10-25*

## Self-Check: PASSED

All 8 key files exist on disk; task commits 6524cf1 and cffe7a2 are present in git history.
