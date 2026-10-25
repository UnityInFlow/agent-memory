---
phase: 05-portable-export-import
plan: 03
subsystem: core-service, import, cli
tags: [rust, serde, jsonl, sqlite, import, idempotency, validation, two-pass, key-remap]

# Dependency graph
requires:
  - phase: 05-portable-export-import
    provides: "plan 05-01: Store::find_by_key -> Option<i64> (D-07), LinkOutcome::Linked { created } (D-12), ImportedMemory + Store::insert_imported (D-10/D-13), redact_storage_error(op, e) + the *_unredacted inner-fn shape (D-14)"
  - phase: 05-portable-export-import
    provides: "plan 05-02: export.rs Record enum (deny_unknown_fields, tag = record) + FORMAT_NAME / FORMAT_VERSION, header exported_rows / exported_links, Store::export_rows + MemoryService::export, CLI export / --output, tests/export_import.rs harness + export_to_string"
  - phase: 03-api-hardening-toolchain-spikes
    provides: "MemoryError::InvalidArgument client tier at the MemoryService seam; MemoryType::try_from / LinkKind::try_from / validate_link_endpoints as the one validation seam (D-04/D-06, D-09)"
  - phase: 04-memory-update-relations-hybrid-search
    provides: "Store::link with endpoint pre-check + INSERT OR IGNORE on (from_id, to_id, kind); relates_to canonical (min, max); sweep backfill (BACKFILL_BATCH = 128) that re-embeds pending rows"
provides:
  - "import/jsonl.rs: pure strict parser parse_jsonl(text) -> Result<JsonlFile, MemoryError> — lenient header probe before the strict Record parse, the importer's own 1-based line counter, every rejection InvalidArgument(\"line N: …\"), link keys resolved order-independently, declared counts enforced; 25-row malformed table + hostile-input probes"
  - "service.rs: ImportReport + skipped_expired / links_imported / links_skipped_duplicates / links_dropped (Default-derived; gsd path reports zeros); CLOCK_SKEW_TOLERANCE_SECS + clock_skew_message; MemoryService::import_jsonl(file) running pass 2 (expiry skip -> find_by_key dedup -> insert_imported, building key -> id) and pass 3 (Store::link through the map with the record's created_at) in ONE spawn_blocking hop, never touching the embedder, redacted as op = \"import\""
  - "CLI `agent-memory import --from jsonl PATH`: parses and validates the whole file BEFORE the database is resolved or opened, prints the six-count line plus the pending-embedding line, rejects --scope; gsd-state arm output byte-identical; shared open_service / read_import_file helpers"
  - "tests/export_import.rs import half (11 tests): round trip into an empty target, idempotent re-import, overlapping-rowid phantom-edge kill-test, dedup-to-existing adjacency, in-file duplicates collapse, expiry boundary fixture + ancient-row refresh, pending -> sweep -> hybrid, header-only no-op, zero-byte rejection, file-order independence, nothing-written-on-failure"
affects: [05-04 README Export & import section + spawned-binary CLI round trip, phase-6 release notes]

# Actuals (#2632) — same estimateTokens scale as the plan's `estimate` (chars/4 over the realized diff)
actuals:
  tokens: 20000
  tasks: 2
  commits: 2

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Two-pass validate-before-write import: a pure parser returns a fully typed file or an InvalidArgument naming the line; the service method's ONLY input type is that parsed file, so a malformed file structurally cannot reach a write"
    - "Lenient-probe-then-strict header parse: a serde struct WITHOUT deny_unknown_fields reads record/format/version first so a newer-version file reaches the upgrade message instead of failing on an unknown field"
    - "Importer-owned line numbers: `text.lines().enumerate()` is the only line counter; serde_json's ` at line 1 column N` suffix is stripped from every re-wrapped message because serde sees one line at a time"
    - "Seam re-wrap without double prefix: an InvalidArgument from the shared validators is lifted out of its variant before `line N:` is prepended; other variants keep their full Display text"
    - "Export-local keys live only in a HashMap<key, id> built during the memories pass; the links pass looks both endpoints up and drops-and-counts a miss — a file key is never used as a target id"
    - "Table-driven contract tests under #[rustfmt::skip]: one (name, input, expected-prefix) tuple per line so the rejection table reads as the spec"

key-files:
  created:
    - crates/agent-memory-core/src/import/jsonl.rs
  modified:
    - crates/agent-memory-core/src/import/mod.rs
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory/src/main.rs
    - crates/agent-memory-core/tests/export_import.rs
    - crates/agent-memory-core/tests/import.rs

key-decisions:
  - "add-alongside (assumption delta): `jsonl` is a second importer beside `gsd-state` with its own parser module and the dedicated ImportedMemory insert shape; no shared Importer trait, because the two tolerance policies (tolerant scrape vs strict contract) are the point of D-11. A third import format is what would force a promote to an ImportSource enum/trait"
  - "HeaderProbe reads `format` and `version` as Option so a file whose first line is a memory record is refused as `first record must be the header` (the contract's intended message) rather than `missing field format`; a header lacking them says `(missing)`"
  - "serde_json's ` at line 1 column N` suffix is stripped from re-wrapped messages — surfacing it would contradict the rule that the importer's counter is the only line number"
  - "An InvalidArgument from MemoryType/LinkKind/validate_link_endpoints is unwrapped before re-wrapping, so `kind must be one of …` reads `invalid argument: line 3: kind must be one of …` and not `invalid argument: line 3: invalid argument: kind …`"
  - "Per-record writer transactions (the insert / link shape) in one blocking hop: a storage fault mid-pass leaves a partial import that a re-run completes idempotently; parse_jsonl is the all-or-nothing gate"
  - "main.rs factors open_service / read_import_file so both --from arms share the wiring; run_export is untouched"
  - "Tracer feedback gate resolved by re-running the fully automated <verify> after the Task 1 commit (same resolution as 05-01 / 05-02: auto mode off, config human_verify_mode: end-of-phase, no human-judgment step in the verify)"

patterns-established:
  - "redact_storage_error op = \"import\" now covers two service methods (import, import_jsonl); the acceptance grep for `redact_storage_error(\"import\"` is 2"
  - "Import CLI output contract: `imported: N, skipped duplicates: N, skipped expired: N, links imported: N, links skipped duplicates: N, links dropped: N` then `N rows pending embedding — the sweep backfills them on the next serve/serve-rest start (128 per tick)`"

requirements-completed: [INTEROP-02, DIST-04]

# Coverage metadata (#1602)
coverage:
  - id: D1
    description: "A format-v1 file imports into an empty OR a non-empty target with every memory and link recreated on the target's own ids; export-local keys are remapped through the pass-2 map and no pre-existing row ever gains a phantom edge (D-08, PITFALLS 18)"
    requirement: DIST-04
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/export_import.rs#round_trip_into_an_empty_database_recreates_memories_and_links, #importing_into_a_non_empty_db_with_overlapping_rowids_never_attaches_phantom_edges, #link_endpoints_that_dedup_to_existing_rows_attach_to_those_rows"
        status: pass
      - kind: e2e
        ref: "cargo run -q -p agent-memory -- export --db A --output x.jsonl && import --from jsonl x.jsonl --db B -> exit 0, `imported: 0, … links dropped: 0`, `0 rows pending embedding — …`"
        status: pass
    human_judgment: false
  - id: D2
    description: "Re-running the same import reports imported 0 / skipped_duplicates N / links_imported 0 / links_skipped_duplicates M with row and edge counts unchanged; two source rows identical on (source, mem_type, content) collapse to one; source is preserved verbatim (D-07)"
    requirement: INTEROP-02
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/export_import.rs#re_importing_the_same_file_is_idempotent, #identical_rows_collapse_to_one_on_import_and_count_a_duplicate (+ the second import inside the phantom-edge test)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Imported rows land embedding_status = 0 with no vector row; the importer never calls the embedder; one sweep backfills them and they are hybrid-searchable (D-13)"
    requirement: INTEROP-02
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/export_import.rs#imported_rows_land_pending_and_the_sweep_backfills_them"
        status: pass
      - kind: other
        ref: "sed -n '/async fn import_jsonl_unredacted/,/^    }/p' service.rs | grep -c self.embedder -> 0"
        status: pass
    human_judgment: false
  - id: D4
    description: "created_at / access_count / expires_at preserved, last_accessed refreshed to import-time now, expires_at <= now skipped and counted (boundary exact), dependent links dropped and counted, link created_at preserved, exported_at > now + 3600 warns and continues (D-10)"
    requirement: INTEROP-02
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/export_import.rs#expired_rows_are_skipped_and_counted_and_their_links_dropped (target clock 1_050 -> {2,0,1,1,0,1}; 1_049 -> {3,0,0,2,0,0}; 400-day-old row -> last_accessed == 7_777 through the skew-warning path), #round_trip_into_an_empty_database_recreates_memories_and_links (provenance columns)"
        status: pass
      - kind: unit
        ref: "crates/agent-memory-core/src/service.rs#clock_skew_message_fires_only_beyond_the_tolerance"
        status: pass
    human_judgment: false
  - id: D5
    description: "Every malformed-file class is rejected as InvalidArgument naming the 1-based line and nothing is written; wire type/kind strings go through MemoryType::try_from / LinkKind::try_from; hostile input never panics (D-09, D-11)"
    requirement: INTEROP-02
    verification:
      - kind: unit
        ref: "crates/agent-memory-core/src/import/jsonl.rs#malformed_files_are_rejected_naming_the_line (25 cases), #hostile_input_never_panics, #a_newer_version_with_extra_fields_reaches_the_upgrade_message, #serde_position_suffix_is_stripped_from_messages (+ 7 more)"
        status: pass
      - kind: integration
        ref: "crates/agent-memory-core/tests/export_import.rs#nothing_is_written_when_validation_fails, #a_zero_byte_file_is_rejected_at_line_one, #a_header_only_file_imports_nothing_and_reports_zeros, #links_are_applied_after_all_memories_regardless_of_file_order"
        status: pass
      - kind: e2e
        ref: "real binary: a file with `{\"record\":\"memo\"}` on line 2 exits non-zero with `invalid argument: line 2: unknown variant `memo`…` and the --db path is never created"
        status: pass
    human_judgment: false
  - id: D6
    description: "ImportReport carries six counts; the gsd-state path reports zeros for the four new ones; the CLI prints all six on one line plus the pending-embedding line; --scope is rejected for jsonl; unsupported --from names both formats (D-12)"
    requirement: INTEROP-02
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/import.rs#import_loads_typed_searchable_memories_idempotently (four zeros assertion)"
        status: pass
      - kind: e2e
        ref: "real binary: six-count line + pending line; `--scope s` -> `--scope applies only to --from gsd-state …`; `--from yaml` -> `unsupported --from value \"yaml\" (supported formats: gsd-state, jsonl)`"
        status: pass
    human_judgment: true
    rationale: "Counts and exit codes are machine-checked; whether the two CLI lines read clearly to a user restoring a corpus is editorial — plan 04's spawned-binary round trip and README section are where a reviewer sees them in context"

# Metrics
duration: 16min
completed: 2026-10-25
status: complete
---

# Phase 5 Plan 03: `agent-memory import --from jsonl` — strict two-pass restore with key→id remapping Summary

**A format-v1 file now restores on another machine: `parse_jsonl` validates the whole file into typed records (every defect refused as `InvalidArgument` naming the 1-based line, nothing written), `MemoryService::import_jsonl` runs the memories pass (expiry skip → `find_by_key` dedup → `insert_imported`, building `key → id`) and the links pass through that map in one blocking hop without ever touching the embedder, and the CLI arm parses before it opens the database and prints the six-count report plus the pending-embedding line.**

## Performance

- **Duration:** 16 min
- **Started:** 2026-10-25T12:04:27Z
- **Completed:** 2026-10-25T12:20:00Z
- **Tasks:** 2
- **Files modified:** 6 (1 created, 5 modified; +1681 / −53 lines)

## Accomplishments

- **Strict parser, one entry point to writes (D-08 pass 1, D-11).** `crates/agent-memory-core/src/import/jsonl.rs` exposes `parse_jsonl(text) -> Result<JsonlFile, MemoryError>` with `JsonlHeader`, `JsonlMemory { key, memory: ImportedMemory }`, `JsonlLink { from, to, kind: LinkKind, created_at }`. Line 1 is probed leniently (`HeaderProbe` without `deny_unknown_fields`: record → format → version-is-unsigned → `>= 1` → `<= FORMAT_VERSION`) before the strict `Record::Header` parse, so a `"version":2,"later":1` header reaches `file is format version 2 but this binary reads up to version 1 — upgrade agent-memory`. Every later line must start with `{`, parse as a `Record`, and pass the Phase 3 seam: `MemoryType::try_from`, non-empty content, unique key; `LinkKind::try_from`, `validate_link_endpoints`. Link endpoints are resolved against the complete key set after the loop (order-independent), and the record counts must equal the header's `exported_rows` / `exported_links`. `last_accessed` is validated as an integer and dropped (D-10 refresh). The module is pure — the CLI reads the file (D-01).
- **`import_jsonl` — passes 2 and 3, one hop, no embedder (D-07, D-08, D-10, D-12, D-13, D-14).** `ImportReport` gained `skipped_expired`, `links_imported`, `links_skipped_duplicates`, `links_dropped` (and `Default`); both gsd-state construction sites fill them with `..ImportReport::default()`. `import_jsonl_unredacted` reads the clock once, logs `clock_skew_message` when `exported_at > now + CLOCK_SKEW_TOLERANCE_SECS` (3600), then in ONE `spawn_blocking`: for each memory, `expires_at <= now` → `skipped_expired` (no map entry); else `find_by_key(source, mem_type, content)` → `Some(id)` maps the key to the EXISTING row and counts a duplicate, `None` → `insert_imported(memory, now)` and maps to the new id. For each link, both endpoints are looked up in the map: `Linked { created: true }` → `links_imported`, `Linked { created: false }` → `links_skipped_duplicates`, `NotFound` or an unmapped key → `links_dropped`; the edge time is the record's own `created_at`. `sed -n '/async fn import_jsonl_unredacted/,/^    }/p' service.rs | grep -c self.embedder` → 0. Errors leave through `redact_storage_error("import", _)` — the grep count is now 2.
- **CLI arm (D-01, D-12).** `run_import` is a three-arm `match from.as_str()`: `"gsd-state"` (behaviour and output line byte-identical), `"jsonl"` (rejects `--scope` with `--scope applies only to --from gsd-state (a jsonl file carries each memory's own scope)`; `parse_jsonl(&text)` at main.rs:282 runs BEFORE `open_service` → `SqliteStore::open` at :320, so a malformed file never creates a database; prints `imported: …, skipped duplicates: …, skipped expired: …, links imported: …, links skipped duplicates: …, links dropped: …` then `N rows pending embedding — the sweep backfills them on the next serve/serve-rest start (128 per tick)`), and `other` → `unsupported --from value {other:?} (supported formats: gsd-state, jsonl)`. The two arms share new `read_import_file` / `open_service` helpers. `--from` and `Import` docs name both formats and the dedup key.
- **Kill-tests, all green on first run.** `tests/export_import.rs` grew from 5 to 16 tests. Overlapping rowids: source keys 1..=3 vs a target whose ids 1..=3 are unrelated rows → `{3,0,0,1,0,0}`, `edges_touching(pre) == 0` for all three, `orphan_edges == 0`, 6 rows, and a second import of the same file → `{0,3,0,0,1,0}`. Dedup adjacency: a row identical to A already at id 2 while A's key is 1 → the edge attaches to id 2, never to id 1, row count +1. In-file twins → `exported_rows == 2`, import `{1,1,0,0,0,0}`, one row. Expiry: target clock exactly 1_050 → `{2,0,1,1,0,1}` with B and C listed and A absent; 1_049 → `{3,0,0,2,0,0}`; a row 400 days stale imports with `last_accessed == 7_777`, `created_at == 1_000`, and is listed (through the skew-warning path). Pending → sweep: with an UP `FakeEmbedder`, both rows land `status 0 / vec_rows 0`, one `sweep` makes them `1 / 1`, and `search("orchid")` returns `Hybrid` containing the row. Header-only → all zeros, counts unchanged; `""` and `"\n"` → `line 1:`; LINK-before-MEM parses and imports identically to canonical order with the edge carrying `created_at 5`; a valid line 2 followed by `{"record":"memo"}` writes nothing.
- **The D-11 table.** 25 `(name, input, expected-prefix)` rows in `jsonl.rs` under `#[rustfmt::skip]`, every one asserted `InvalidArgument` + `starts_with("invalid argument: line ")` + the needle; plus `hostile_input_never_panics` (300 nested arrays → `Err`; 4 MiB content → `Ok`; 1 MiB of `{` → `Err`; `\u0000` → `Ok`, stored as data).
- **Nothing else moved:** `git diff --stat` for `gsd_state.rs`, `export.rs`, `sql/`, `Cargo.lock`, `.github/` is empty; no new dependency; `--locked` throughout.

## Task Commits

Each task was committed atomically (fmt + clippy `-D warnings` + full workspace suite green before each):

1. **Task 1 (tracer): A format-v1 file imports into another database with memories, links and provenance intact — and importing it again is a no-op** — `9d937f4` (feat)
2. **Task 2: Overlapping rowids, duplicates, expiry, pending re-embed and every malformed-file class behave exactly as decided** — `c7bc811` (test)

**Plan metadata:** see the `docs(05-03)` commit that follows this summary.

## Files Created/Modified

- `crates/agent-memory-core/src/import/jsonl.rs` (new, 634 lines) — module doc (strict policy, error contract, header probe, timestamp handling); `JsonlHeader`, `JsonlMemory`, `JsonlLink`, `JsonlFile`; `HeaderProbe`; `invalid_line`, `rewrap`, `serde_message`, `check_header`, `parse_record`, `pub fn parse_jsonl`; 11 unit tests incl. the 25-row table and the hostile probes
- `crates/agent-memory-core/src/import/mod.rs` — `pub mod jsonl;`; invariants reworded: tolerance is per importer, idempotency names `Store::find_by_key` and both service entry points
- `crates/agent-memory-core/src/service.rs` — `ImportReport` +4 fields + `Default`; `CLOCK_SKEW_TOLERANCE_SECS`, `clock_skew_message`; `pub async fn import_jsonl` + `async fn import_jsonl_unredacted`; `clock_skew_message_fires_only_beyond_the_tolerance` test; `use crate::import::jsonl::{JsonlFile, JsonlLink, JsonlMemory}`
- `crates/agent-memory/src/main.rs` — `Import` / `--from` / `--scope` docs; `run_import` three-arm match; `read_import_file`; `open_service`
- `crates/agent-memory-core/tests/export_import.rs` — module doc extended; `db_path` exposed; raw helpers (`raw_conn`, `row_count`, `edge_count`, `edges_touching`, `orphan_edges`, `status`, `vec_rows`, `id_by_content`, `Provenance`/`provenance`, `edges`), `import`, `normalised`, `six`, `header`, `MEM1`/`MEM2`/`LINK12`, `file_from`, `seed_round_trip_source`; 11 import tests
- `crates/agent-memory-core/tests/import.rs` — four-zeros assertion on the gsd-state report

## Decisions Made

- **`add-alongside`** (the plan's assumption-delta decision, confirmed in code): `jsonl` sits beside `gsd_state` as a second parser module with its own policy; both share the seam (`MemoryService`), the dedup probe (`find_by_key`) and the writer lane, not a type. Accepted debt: two `--from` arms, two parser modules, two insert shapes. A third format forces an `ImportSource` enum/trait.
- **Pass structure:** pass 1 (validation) is the all-or-nothing gate and lives entirely in `parse_jsonl`; passes 2 and 3 share one `spawn_blocking` hop with per-record writer transactions (the shape of `insert` / `link`), so a storage fault mid-pass leaves a partial import that an idempotent re-run completes.
- **Message wording changed from the interface contract (recorded per the plan's Task 2 step 4):**
  1. `HeaderProbe.format` / `.version` are `Option` — a first line that is not a header now says `line 1: first record must be the header` (the contract's intended message; with `String` fields it would have been `missing field format`), and a header lacking them says `format must be 'agent-memory-export' (missing)` / `version must be a positive integer (missing)`.
  2. serde_json's ` at line 1 column N` suffix is stripped from every re-wrapped message (`serde_message`) — the contract said "verbatim", but the suffix IS serde's line number, which the same contract forbids surfacing.
  3. Seam errors are re-wrapped through `rewrap`, which lifts an `InvalidArgument` payload out of its variant: `invalid argument: line 3: kind must be one of …`, not `invalid argument: line 3: invalid argument: kind …`. `InvalidType` keeps its full text (`invalid memory type 'MEMO' (expected one of …)`).
- **Tracer feedback gate:** auto mode is off, but `.planning/config.json` sets `human_verify_mode: "end-of-phase"` and the tracer's `<verify>` is wholly `<automated>`. I re-ran it end-to-end on the committed tree (lib `import::jsonl` 11/11, `clock_skew` 1/1, `--test export_import` round_trip + idempotent, `--test import` 2/2, workspace clippy, the real-binary e2e printing `1`) and continued to Task 2 rather than returning a mid-plan `checkpoint:human-verify` — the same resolution as 05-01 and 05-02, recorded so the orchestrator can route end-of-phase UAT.

## Final rejection-message table (one line per class)

| Class | Message (after `invalid argument: `) |
|---|---|
| zero-byte file | `line 1: missing or invalid header (empty file)` |
| line 1 not JSON | `line 1: missing or invalid header (<serde message>)` |
| line 1 not a header record | `line 1: first record must be the header` |
| wrong format | `line 1: format must be 'agent-memory-export' (got '<x>')` / `(missing)` |
| non-integer version | `line 1: version must be a positive integer (got <value>)` / `(missing)` |
| version 0 | `line 1: version must be >= 1 (got 0)` |
| version > 1 | `line 1: file is format version <v> but this binary reads up to version 1 — upgrade agent-memory` |
| header missing a strict field | `line 1: missing field \`<name>\`` |
| non-object line | `line N: expected a JSON object` |
| unknown record tag | `line N: unknown variant \`<tag>\`, expected one of \`header\`, \`memory\`, \`link\`` |
| unknown field | `line N: unknown field \`<name>\`, expected one of …` |
| wrong field type | `line N: invalid type: <got>, expected <wanted>` |
| invalid type | `line N: invalid memory type '<t>' (expected one of DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT)` |
| empty content | `line N: content must be a non-empty string` |
| duplicate memory key | `line N: duplicate memory key <k>` |
| invalid kind | `line N: kind must be one of relates_to, supersedes, caused_by (got '<k>')` |
| self-link | `line N: from_id and to_id must differ (got <id>)` |
| link key absent from file | `line N: link key <k> does not appear in the file` (N = the link's line) |
| memory count mismatch | `line <last>: file declares <r> memory records but contains <n>` |
| link count mismatch | `line <last>: file declares <l> link records but contains <n>` |
| second header | `line N: duplicate header` |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Double `invalid argument:` prefix on seam errors**
- **Found during:** Task 1 (the malformed table's `bad kind` case, run before the parser was finished — the RED observation)
- **Issue:** `LinkKind::try_from` and `validate_link_endpoints` already return `InvalidArgument`, whose Display is `invalid argument: …`; re-wrapping them through `Display` produced `invalid argument: line 3: invalid argument: kind must be one of …`.
- **Fix:** `rewrap(line_no, e)` lifts an `InvalidArgument` payload out before prefixing; other variants keep their Display text.
- **Files modified:** `crates/agent-memory-core/src/import/jsonl.rs`
- **Verification:** `cargo test -p agent-memory-core --lib import::jsonl` 11/11; the table asserts the exact `line 3: kind must be one of` prefix
- **Committed in:** `9d937f4`

**2. [Rule 3 - Blocking] rustfmt broke the table's one-tuple-per-line layout the acceptance grep counts**
- **Found during:** Task 2 (acceptance-criteria gate: `grep -c '^\s*("' jsonl.rs` ≥ 18 returned 13, then 2, for a 25-row table)
- **Issue:** rustfmt's 60-column short-argument width wraps any tuple wider than that onto four lines, so the literal grep undercounts however the inputs are factored.
- **Fix:** `#[rustfmt::skip]` on the `let cases` statement (with a comment saying why), the standard idiom for table-driven tests; the in-test `assert!(cases.len() >= 18)` guards the count independently of layout.
- **Files modified:** `crates/agent-memory-core/src/import/jsonl.rs`
- **Verification:** grep → 25; `cargo fmt --all -- --check` clean; clippy clean
- **Committed in:** `c7bc811`

### Process notes (not code deviations)

- **TDD RED/GREEN without a separate RED commit.** Both tasks are `tdd="true"`. RED was observed: Task 1's `tests/export_import.rs` and `tests/import.rs` additions reference `import_jsonl`, `parse_jsonl` and the four new `ImportReport` fields, so they failed to compile against the pre-task tree by construction, and the parser's table test failed on the double-prefix bug above before the parser was corrected. Each task's commit carries tests + implementation together because the project operating rules forbid committing failing tests and the plan calls for one commit per task.
- **Task 2's `jsonl.rs` tests landed in Task 1.** The malformed table and hostile probes were authored alongside the parser (a parser and its rejection table are one unit, like 05-02's goldens), so the Task 1 commit already contains them; the Task 2 commit reflows the table to one case per line. The nine `tests/export_import.rs` tests are Task 2's as planned.
- **Pre-bash safety hook.** Never tripped: logs were truncated with `: > file`, databases lived in `mktemp -d` dirs and were never removed by the shell.

---

**Total deviations:** 2 auto-fixed (1 Rule 1 message bug caught by the table before it shipped, 1 Rule 3 plan-internal criterion vs rustfmt); no scope or behaviour change beyond the three recorded wording choices.
**Impact on plan:** None on scope. The wording choices make the D-11 messages read as the contract intended; the formatting attribute exists only to make the plan's own grep pass as written.

## Issues Encountered

None beyond the two auto-fixes. Every targeted `<verify>` and the full workspace suite passed on the first post-GREEN run (265 after Task 1, 274 after Task 2); no fix-attempt budget was consumed on behaviour.

## Verification

Plan-level `<verification>` re-run after the second commit:

- `cargo fmt --all -- --check` — clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` — exit 0; `cargo test --workspace --locked` — **274 passed, 0 failed, 2 ignored** (251 before this plan → 265 after Task 1: +11 `import::jsonl`, +1 `clock_skew`, +2 `export_import` → 274 after Task 2: +9 `export_import`).
- `cargo test -p agent-memory-core --test export_import` — 16 passed (≥ 16 required); `cargo test -p agent-memory-core --lib import::jsonl` — 11 passed with the 25-row table (≥ 18 required, grep → 25); `cargo test -p agent-memory-core --test import` — 2 passed incl. the zeros assertion.
- Real binary: `export --db A --output x.jsonl` then `import --from jsonl x.jsonl --db B` → exit 0, exactly one line matching `^imported: 0, skipped duplicates: 0, skipped expired: 0, links imported: 0, links skipped duplicates: 0, links dropped: 0$` (grep → `1`) followed by `0 rows pending embedding — …`; a malformed file → non-zero exit, `invalid argument: line 2: unknown variant \`memo\`…`, and the `--db` file is never created; `--scope` with jsonl and `--from yaml` print their respective messages.
- Acceptance greps: `pub fn parse_jsonl` 1; `upgrade agent-memory` 4; `MemoryType::try_from` 3; `LinkKind::try_from` 4; `validate_link_endpoints` 4; `does not appear in the file` 3; `pub mod jsonl;` 1; `per importer` 1; `pub async fn import_jsonl` 1; `redact_storage_error("import"` 2; `links_skipped_duplicates` ≥ 3; `CLOCK_SKEW_TOLERANCE_SECS` 5; `"jsonl" =>` 1; `supported formats: gsd-state, jsonl` 1; `rows pending embedding` 1; `parse_jsonl(&text)` (main.rs:282) precedes the arm's `SqliteStore::open` (:320); `self.embedder` inside `import_jsonl_unredacted` 0.
- `git diff --stat -- crates/agent-memory-core/src/import/gsd_state.rs crates/agent-memory-core/src/export.rs crates/agent-memory-core/sql/ Cargo.lock .github/` — empty.
- No `unwrap()` / `expect()` outside `#[cfg(test)]` in any touched production file (`jsonl.rs` test module starts at line 393; `service.rs` at 1116).

## Known Stubs

None — no placeholder values, TODO/FIXME markers, skipped tests, or unwired data paths were introduced.

## Threat Flags

None new. The plan's register is implemented as mitigated: T-05-04 (no vector field in `Record`; `insert_imported` always `embedding_status = 0`; sweep proof), T-05-05 (keys only index the pass-2 map; overlapping-rowid kill-test), T-05-06 (seam conversion; `tags: Vec<String>`; `params![]` in the store), T-05-07 (`hostile_input_never_panics`; no `unwrap`), T-05-08 (`parse_jsonl` completes before `open_service`; `nothing_is_written_when_validation_fails`), T-05-09 (`<=` boundary test), T-05-11 (CLI only — no `memory_import` / `/api/import` added), T-05-13 (`clock_skew_message` + unit test, exercised by the ancient-row fixture), T-05-17 (`redact_storage_error("import", _)`), T-05-SC (zero new deps, `--locked`). T-05-10 (pass 1 holds the file in RAM) stays accepted; the 4 MiB content probe parses.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- **Plan 05-04 (docs + CLI realism)** has everything it documents: the format spec in `export.rs`'s module doc, the rejection table above (copy it into the README "Export & import" section), the six-count + pending-line CLI contract, the `--scope`-is-gsd-only rule, and the strict-vs-tolerant importer note from `import/mod.rs`. Its spawned-binary round trip can reuse the real-binary commands in Verification. Adding `export` and `import_jsonl` to `tests/redaction.rs` remains the cheap follow-up noted in 05-02.
- No blockers. `.planning/config.json` and the untracked harness files were left unstaged as instructed.

---
*Phase: 05-portable-export-import*
*Completed: 2026-10-25*

## Self-Check: PASSED

All 7 key files exist on disk; task commits 9d937f4 and c7bc811 are present in git history.
