---
phase: 05-portable-export-import
plan: 04
subsystem: docs, cli, testing
tags: [rust, readme, jsonl, export, import, spawned-binary, realism-test, format-v1]

# Dependency graph
requires:
  - phase: 05-portable-export-import
    provides: "plan 05-02: export.rs module-doc field tables (the authoritative format-v1 spec), `agent-memory export [--output]`, export_file::PendingExport (0600 temp beside the target + rename), the stderr `exported N memories and M links` report"
  - phase: 05-portable-export-import
    provides: "plan 05-03: import::jsonl strict parser + rejection table, MemoryService::import_jsonl, the three-arm `run_import` with its six-count line, pending-embedding line, `--scope` refusal and `unsupported --from value` message"
  - phase: 05-portable-export-import
    provides: "plan 05-01: README redaction sentence widened to every surface (not restated here); ImportedMemory / insert_imported timestamp semantics the Timestamps table documents"
provides:
  - "README `## Export & import` (between 'Import from GSD' and 'Lifecycle: TTL & decay'): usage, CLI-only rationale, what travels / what is derived, the three format-v1 field tables in wire order, strict-reader rules + version policy, dedup and non-empty-target safety, per-column timestamp table, pending embeddings + 128-per-tick hourly sweep, the six-count report with a meaning table, private atomic --output, backup-vs-export tip (D-01..D-13)"
  - "README Lifecycle item 3 cross-reference for rows restored by `import --from jsonl`; Run section `agent-memory export --output memories.jsonl` example"
  - "crates/agent-memory/tests/cli.rs: five realism-only spawned-binary tests (zero coverage by design, like tests/rest.rs) — export → import → re-import round trip across two databases with the exact report strings, 0600 file mode, no temp sibling; malformed file names `line 2:` and never creates the --db; `--from bogus`, `--scope` with jsonl and invalid UTF-8 refused with the documented messages and no PANIC"
  - "Phase-end gates recorded: D-01 transport grep empty; `cargo llvm-cov --workspace --fail-under-lines 80` → 95.61% lines; sql/, Cargo.lock and .github/ unchanged across all phase-5 commits"
affects: [phase-6 release notes and Homebrew/Windows distribution, end-of-phase UAT for phase 5, any future transport for export/import (must update the README CLI-only paragraph)]

# Actuals (#2632) — same estimateTokens scale as the plan's `estimate` (chars/4 over the realized diff)
actuals:
  tokens: 5300
  tasks: 2
  commits: 2

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Spawned-binary realism tests are realism-only: `Command::new(env!(\"CARGO_BIN_EXE_agent-memory\"))`, `--db` under a fresh tempdir, `AGENT_MEMORY_OLLAMA_URL=http://127.0.0.1:9`, `.output()` captured; the module doc says they contribute zero coverage (a child never flushes LLVM profile data) and names where the coverage-bearing proofs live"
    - "Hand-written seed files use export-local keys that cannot coincide with target ids (10 and 20, not 1 and 2), so a remapping bug would surface as a wrong count rather than pass by coincidence"
    - "README restates the `Record` declaration order verbatim in three tables and points at `export.rs` as the authoritative definition — the doc never contradicts the code because it copies the code's order"
    - "A new integration test file is drafted outside the crate until the previous task's gate has run and committed, so an in-flight `cargo test --workspace` never compiles an uncommitted test target"

key-files:
  created:
    - crates/agent-memory/tests/cli.rs
  modified:
    - README.md

key-decisions:
  - "The README section is one `##` with ten `###` subsections in the plan's order (Usage → What travels → Format v1 → The reader is strict → Idempotent → Timestamps → Embeddings → The report → --output → Backup vs export), so each topic is linkable and the field tables are not buried in prose"
  - "The sweep cadence is documented as '128 rows per tick, the first tick at serve/serve-rest start and then hourly' — `SWEEP_INTERVAL` is 3600 s in main.rs, so 'hourly' is the shipped number, not an approximation"
  - "The usage paragraph states the `--scope` refusal for jsonl (each memory carries its own scope) — not in the plan's ten topics, but it is the one CLI flag a gsd-state user would reach for and the message is pinned by the CLI test"
  - "The refusal tests additionally assert the `--db` path is never created for `--from bogus`, `--scope` and invalid UTF-8 (the plan required this only for the malformed-file test) — all three bail or fail before `open_service`, and the extra assertion costs nothing while pinning that order"
  - "Coverage gate run once at phase end as the plan prescribed: `cargo llvm-cov --workspace --fail-under-lines 80` passes at 95.61% lines (regions 93.28%, functions 88.81%); tests/cli.rs runs under it but, as designed, adds no coverage"

patterns-established:
  - "README field tables mirror the serde declaration order of the wire enum and say so"
  - "Realism tests pin exact user-visible strings (report lines, error substrings) rather than parsing them, so a wording change is a deliberate README + test change together"

requirements-completed: [DIST-04, INTEROP-02]

# Coverage metadata (#1602)
coverage:
  - id: D1
    description: "README `## Export & import` section documents CLI usage, the CLI-only rule and its reason, what travels and what is derived, the three format-v1 field tables in wire order, the strict-reader rules and version policy, dedup/idempotency and non-empty-target safety, the per-column timestamp policy, the pending-embedding behaviour and sweep cadence, the six-count report, the private atomic --output, and the backup-vs-export tip; Lifecycle item 3 and Run cross-references present"
    requirement: DIST-04
    verification:
      - kind: other
        ref: "grep -c '^## Export & import' README.md → 1; section sits between '## Import from GSD' (l.527) and '## Lifecycle: TTL & decay' (l.742); every Record field name present; 'skipped expired', 'last_accessed', '128 per tick', 'no MCP tool', 'VACUUM INTO', '0600', 'export-tmp-', 'links dropped', 'upgrade agent-memory' all ≥ 1; Lifecycle 'import --from jsonl' → 1; Run 'agent-memory export' → 1"
        status: pass
    human_judgment: true
    rationale: "The greps prove every topic is present; whether a user who has never read the code can follow the section and understand every count is a legibility judgment no grep makes"
  - id: D2
    description: "The shipped binary round-trips a hand-written seed through two databases: first import prints the exact six-count line and `2 rows pending embedding`; `--output` writes a 4-line 0600 file with no `.export-tmp-` sibling and the stderr report; the stdout export carries identical records; the file restores into a second database and a re-import reports only duplicates"
    requirement: INTEROP-02
    verification:
      - kind: e2e
        ref: "cargo test -p agent-memory --test cli export_import_round_trip_over_the_real_binary"
        status: pass
    human_judgment: false
  - id: D3
    description: "The binary refuses bad input cleanly: a malformed file exits non-zero with `line 2:` on stderr and the --db path is never created; `--from bogus` lists the supported formats; `--scope` with jsonl prints the documented message; invalid UTF-8 is refused via the `reading` step with no PANIC"
    requirement: INTEROP-02
    verification:
      - kind: e2e
        ref: "cargo test -p agent-memory --test cli a_malformed_file_exits_non_zero_naming_the_line_and_creates_no_database"
        status: pass
      - kind: e2e
        ref: "cargo test -p agent-memory --test cli an_unsupported_from_value_still_bails"
        status: pass
      - kind: e2e
        ref: "cargo test -p agent-memory --test cli scope_is_rejected_for_jsonl"
        status: pass
      - kind: e2e
        ref: "cargo test -p agent-memory --test cli invalid_utf8_is_rejected_cleanly"
        status: pass
    human_judgment: false
  - id: D4
    description: "Export and import are CLI-only — no MCP tool and no REST route exist (D-01 prohibition, re-checked at phase end)"
    requirement: DIST-04
    verification:
      - kind: other
        ref: "grep -rn 'memory_export\\|memory_import\\|/api/export\\|/api/import' crates/agent-memory/src/mcp.rs crates/agent-memory/src/rest/ → no output (exit 1)"
        status: pass
    human_judgment: false
  - id: D5
    description: "Phase-level gates: fmt, clippy -D warnings, full workspace suite, llvm-cov ≥ 80% lines; sql/, Cargo.lock and .github/ unchanged across the phase"
    verification:
      - kind: other
        ref: "cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo test --workspace --locked → 279 passed / 0 failed / 2 ignored; cargo llvm-cov --workspace --fail-under-lines 80 → TOTAL 95.61% lines; git diff --stat 00d8080^..HEAD -- crates/agent-memory-core/sql/ Cargo.lock .github/ → empty"
        status: pass
    human_judgment: false

# Metrics
duration: 9 min
completed: 2026-10-25
status: complete
---

# Phase 5 Plan 04: Document the contract and prove it over the real binary Summary

**The README now carries a complete "Export & import" section — format v1 field tables in wire order, the strict-reader and version rules, the per-column timestamp policy, the pending-embedding sweep, every count of the six-count report, the private atomic `--output`, the CLI-only rationale and a `VACUUM INTO` backup tip — and five spawned-binary realism tests prove the exact shipped `agent-memory` round-trips a memory file between two databases with that documented output and refuses malformed, mis-flagged and non-UTF-8 input before touching the target database.**

## Performance

- **Duration:** 9 min
- **Started:** 2026-10-25T12:25:28Z
- **Completed:** 2026-10-25T12:34:30Z
- **Tasks:** 2
- **Files modified:** 2 (1 created, 1 modified; README +199 / −1, cli.rs 326 lines)

## Accomplishments

- **README `## Export & import` (D-01..D-13 for users).** Inserted between "Import from GSD" and "Lifecycle: TTL & decay", ten `###` subsections in the plan's order. *Usage* shows the three commands and states the CLI-only rule with both reasons (arbitrary-file-read primitive on the unauthenticated daemon; the corpus must not cross the JSON-RPC stdout channel) plus the `--scope` refusal. *What travels / what does not* names the four derived fields and why vectors are re-created locally. *Format v1* restates `export.rs`'s header (10 fields), memory (11) and link (5) tables in the exact `Record` declaration order, with the ordering/canonical-output paragraph (`relates_to` smaller key first, explicit `null`s, byte-identical re-exports). *The reader is strict* contrasts the tolerant `gsd-state` importer, lists every rejection class, shows the `invalid argument: line 42: …` shape and the `upgrade agent-memory` version policy. *Idempotent, and safe on a non-empty database* documents the `(source, type, content)` key, verbatim `source`, twin collapse and key remapping. *Timestamps* is the five-row per-column table plus the one-hour clock-skew warning. *Embeddings* states 128 per tick, first tick at `serve`/`serve-rest` start, then hourly, keyword-only until backfill. *The report* shows both output lines and a six-row meaning table (`links dropped` is "not an error"). *`--output` is private and atomic* names `PATH.export-tmp-<pid>`, `0600` at creation, sync + rename, replace-existing, never-partial, and the Windows seam. *Backup vs export* gives `sqlite3 memory.db "VACUUM INTO 'memory-backup.db'"` with the stop-every-process caveat and points at the pre-migration backup under Upgrading.
- **Cross-references.** Lifecycle item 3 ends with "Rows restored by `agent-memory import --from jsonl` take this path too."; Run gains `agent-memory export --output memories.jsonl   # portable JSONL export (see Export & import)`. The "Import from GSD" section and the plan-01 redaction sentence are untouched.
- **`crates/agent-memory/tests/cli.rs` (realism-only, five tests, all green first run).** Helpers `binary_path`, `run` (dead-Ollama env, `Stdio::null()` stdin, `.output()`), `stdout`/`stderr` (lossy), `s` (path → `&str`), `header(rows, links)` and `seed_jsonl()` built with `serde_json::json!` (keys 10 and 20 so they can never coincide with target ids), `export_temp_entries`. `export_import_round_trip_over_the_real_binary`: import seed → A prints exactly `imported: 2, skipped duplicates: 0, skipped expired: 0, links imported: 1, links skipped duplicates: 0, links dropped: 0` then `2 rows pending embedding…`; `export --output out.jsonl` → stderr `exported 2 memories and 1 links`, 4 lines, line 1 starts `{"record":"header"`, Unix mode `& 0o777 == 0o600`, zero `.export-tmp-` siblings; stdout export lines 2–4 equal the file's; import into B → the same first line; import again → `imported: 0, skipped duplicates: 2, skipped expired: 0, links imported: 0, links skipped duplicates: 1, links dropped: 0` and `0 rows pending embedding…`. `a_malformed_file_exits_non_zero_naming_the_line_and_creates_no_database`: header + `{"record":"memo"}` → non-zero, stderr contains `line 2:`, `never.db` absent. `an_unsupported_from_value_still_bails` (`supported formats: gsd-state, jsonl`), `scope_is_rejected_for_jsonl` (`--scope applies only to --from gsd-state`), `invalid_utf8_is_rejected_cleanly` (`[0xff, 0xfe, 0x0a]` → stderr contains `reading`, not `PANIC`) — each also asserts the `--db` path was never created.
- **Phase-end gates (plan `<verification>`).** D-01 transport grep over `mcp.rs` and `rest/` is empty. `cargo llvm-cov --workspace --fail-under-lines 80` → **COV OK, TOTAL 95.61% lines** (regions 93.28%, functions 88.81%; `export.rs` 99.02%, `import/jsonl.rs` 98.57%, `export_file.rs` 93.55%). `git diff --stat 00d8080^..HEAD -- crates/agent-memory-core/sql/ Cargo.lock .github/` is empty across all eleven phase-5 commits. No new dependency; `--locked` throughout.

## Task Commits

Each task was committed atomically (fmt + clippy `-D warnings` + full workspace suite green before each):

1. **Task 1: The README tells a user how to move their memory and what every line of the report means** — `5a15d13` (docs)
2. **Task 2: The shipped binary round-trips a memory file between two databases and refuses bad input cleanly** — `6eb354a` (test)

**Plan metadata:** see the `docs(05-04)` commit that follows this summary.

## Files Created/Modified

- `README.md` — `## Export & import` (+~190 lines, l.548–741) with ten `###` subsections and five tables; Lifecycle item 3 sentence; Run example line
- `crates/agent-memory/tests/cli.rs` (new, 326 lines) — module doc (realism-only rationale, where coverage lives, dead-Ollama env), `FIRST_IMPORT` / `REIMPORT` constants, seven helpers, five `#[test]`s

## Decisions Made

- **Subsections, not one block.** The plan said "containing in this order"; the section is one `##` with ten `###` headings in exactly that order so each topic is linkable from issues and release notes and the three field tables stand on their own.
- **"Then hourly."** `SWEEP_INTERVAL` is `Duration::from_secs(3600)` in `main.rs`, so the README states the shipped cadence instead of "on the sweep interval".
- **The `--scope` refusal is documented in Usage** (one sentence) although it is not one of the plan's ten topics — it is the flag a `gsd-state` user would reach for, and the CLI test pins the message.
- **Extra "database never created" assertions** on the three refusal tests beyond the malformed-file one the plan required — `--from bogus` bails before any I/O, `--scope` bails before reading, and an unreadable file fails in `read_import_file`, all ahead of `open_service`; pinning that order is free.
- **Draft-outside-the-crate staging.** `cli.rs` was written to `/tmp` while Task 1's `cargo test --workspace` gate ran and copied into `tests/` only after `5a15d13`, so the Task 1 gate could not compile (or fail on) an uncommitted test target.

## Deviations from Plan

None - plan executed exactly as written. Both commits carry only the planned files; no code outside `README.md` and `tests/cli.rs` changed.

### Process notes (not code deviations)

- **TDD on an already-shipped behaviour.** Task 2 is `tdd="true"`, but it pins behaviour plans 05-02 and 05-03 already shipped, so a RED run cannot fail by construction — the five tests passed on their first run (`5 passed; 0 failed`), which is the expected outcome for a realism proof, not the "passes unexpectedly" fail-fast case (the feature is known to exist; the tests exist to pin its user-visible contract). The single `test(05-04)` commit is the plan's one-commit-per-task shape; `MVP_MODE=true` with `TDD_MODE=false` means the MVP+TDD RED-commit gate did not apply.
- **Pre-bash safety hook** never tripped: logs were truncated with `: > file`, databases lived in `tempfile::tempdir()` and were never removed by the shell.

**Total deviations:** 0.
**Impact on plan:** None.

## Issues Encountered

None. Every grep in both tasks' acceptance criteria passed on the first check; both gates were green on the first run (274 → 279 tests).

## Verification

Plan-level `<verification>` re-run after the second commit:

- `cargo test -p agent-memory --test cli` — 5 passed (`export_import_round_trip_over_the_real_binary`, `a_malformed_file_exits_non_zero_naming_the_line_and_creates_no_database`, `an_unsupported_from_value_still_bails`, `scope_is_rejected_for_jsonl`, `invalid_utf8_is_rejected_cleanly`).
- README: `grep -c '^## Export & import'` → 1, between `## Import from GSD` (l.527) and `## Lifecycle: TTL & decay` (l.742); `exported_rows` 2, `skipped expired` 2, `last_accessed` 4, `128 per tick` 2, `no MCP tool` 1, `VACUUM INTO` 1, `0600` ≥ 1, `export-tmp-` ≥ 1, `links dropped` ≥ 1, `upgrade agent-memory` ≥ 1; every `Record` field name present; Lifecycle `import --from jsonl` → 1; Run `agent-memory export` → 1.
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `cargo test --workspace --locked` — **279 passed, 0 failed, 2 ignored** (274 after 05-03 + 5 CLI tests).
- `cargo llvm-cov --workspace --fail-under-lines 80` — **COV OK**, TOTAL 95.61% lines / 93.28% regions / 88.81% functions.
- D-01: `grep -rn "memory_export\|memory_import\|/api/export\|/api/import" crates/agent-memory/src/mcp.rs crates/agent-memory/src/rest/` → empty.
- `git diff --stat 00d8080^..HEAD -- crates/agent-memory-core/sql/ Cargo.lock .github/` → empty; `git diff --diff-filter=D` on both task commits → no deletions.
- `grep -c '#\[test\]' crates/agent-memory/tests/cli.rs` → 5; `CARGO_BIN_EXE_agent-memory` → 1; `"--from", "jsonl"` → 5.

## Known Stubs

None — no placeholder values, TODO/FIXME markers, skipped tests, or unwired data paths were introduced.

## Threat Flags

None new. The plan's register is implemented as mitigated or accepted as recorded: T-05-11 (CLI-only — README states the rule and both reasons; the transport grep is empty), T-05-18 (every count defined in the README table, `links dropped` explicitly "not an error"; the CLI test pins the exact wording), T-05-12 (README shows `--output` as the private alternative and warns that stdout may land in scrollback), T-05-10 (validate-before-write documented; no cap added), T-05-SC (zero new dependencies, `--locked`).

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- **Phase 5 is fully executed (4/4 plans).** The orchestrator runs phase verification next; `human_verify_mode: end-of-phase` means the three tracer gates (05-01, 05-02, 05-03) and D1 above route to end-of-phase UAT together. The phase is not marked complete here.
- **For Phase 6 (release):** the README section is the user-facing changelog source for DIST-04 / INTEROP-02; the "Behavioral changes in v0.1.0" list may want one line pointing at it. The Windows `--output` permission sentence ("follows its directory's permissions") is the one README statement Phase 6's DIST-03 work must revisit.
- **Cheap follow-up carried from 05-02/05-03:** adding `export` and `import_jsonl` to `tests/redaction.rs` remains open.
- No blockers. `.planning/config.json` and the untracked harness files were left unstaged as instructed.

---
*Phase: 05-portable-export-import*
*Completed: 2026-10-25*

## Self-Check: PASSED

Both key files and the SUMMARY exist on disk; task commits 5a15d13 and 6eb354a are present in git history; the D-01 transport grep is empty; the llvm-cov gate passed at 95.61% lines.
