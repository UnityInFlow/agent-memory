---
phase: 02-semantic-search-interop-release
plan: 03
subsystem: interop
tags: [rust, gsd, import, markdown-parser, idempotency, sqlite, clap]

# Dependency graph
requires:
  - phase: 02-01
    provides: Embedder seam (batch /api/embed, FakeEmbedder), embedding_status pending rows + sweep backfill, SearchOutcome envelope
  - phase: 02-02
    provides: main.rs subcommand dispatch shape (serve/serve-rest wiring to reuse)
  - phase: 01 (foundation)
    provides: SqliteStore writer/read-pool, FTS5 keyword search, MemoryService seams, NewMemory domain type
provides:
  - agent_memory_core::import::gsd_state::parse_gsd_state — tolerant single-pass line scanner (fence tracking, section mapping, deferred-table rows, skip-counts)
  - ParsedStateFile { drafts, skipped } + MAX_IMPORT_CONTENT_BYTES = 8192 cap (T-02-22)
  - Store::exists(source, mem_type, content) — parameterized INTEROP-01 dedup probe
  - MemoryService::import(drafts) -> ImportReport { imported, skipped_duplicates } — one dedup hop, ONE batch embed (best-effort), standard insert path
  - CLI: agent-memory import --from gsd-state <path> [--scope <s>] with .planning-ancestor default scope and stdout report
affects: [02-04 release (README documents import), future importers (import/ module pattern)]

# Tech tracking
tech-stack:
  added: []
  patterns: [tolerant-importer (skip+count, never hard-error), exact-key idempotent import, one-batch-embed-per-import]

key-files:
  created:
    - crates/agent-memory-core/src/import/mod.rs
    - crates/agent-memory-core/src/import/gsd_state.rs
    - crates/agent-memory-core/tests/fixtures/STATE.md
    - crates/agent-memory-core/tests/import.rs
  modified:
    - crates/agent-memory-core/src/lib.rs
    - crates/agent-memory-core/src/store/mod.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory/src/main.rs

key-decisions:
  - "Dedup key is exact (source, mem_type, content): edited bullets import as NEW memories by design — success criterion 4 requires only no-duplication on unchanged re-import"
  - "Import embeds in ONE batch call and NEVER fails on embed errors — rows land embedding_status=0 and the 02-01 sweep backfills later (SEARCH-03 seam rule)"
  - "Default --scope resolves the parent of the nearest .planning ancestor (canonicalized), else the cwd name — deterministic and documented in the flag help"

patterns-established:
  - "import/ module: tolerant, idempotent, data-only importers (module doc states the invariants); new formats add a sibling of gsd_state.rs"
  - "CLI stdout is sanctioned for subcommand reports; MCP-05 stdout purity applies only to serve (stdio_purity regression stayed green)"

requirements-completed: [INTEROP-01]

# Metrics
duration: ~40 min (active; wall clock spanned an overnight pause + 4 failed subagent dispatch attempts)
completed: 2026-07-03
---

# Phase 02 Plan 03: GSD STATE.md Import (Idempotent) Summary

**One-command GSD STATE.md import: tolerant line-scanner parser → typed gsd-tagged memories, exact-key dedup proven idempotent through the real binary (second run `imported: 0`), with one best-effort batch embed**

## Performance

- **Duration:** ~40 min active (wall clock spanned an overnight pause; execution ran inline in the orchestrator after 4 subagent stream stalls)
- **Started:** 2026-07-02T20:25:27Z
- **Completed:** 2026-07-03T06:07:00Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments

- INTEROP-01 shipped: `agent-memory import --from gsd-state <path> [--scope <s>]` loads Decisions → DECISION, Blockers/Concerns → CONSTRAINT, Pending Todos → TODO, and Deferred Items table rows → TODO+`deferred` tag; every row carries `source=gsd-state`, tag `gsd`, and the scope (T-02-23 provenance)
- Idempotency proven twice: integration test (re-import reports `imported == 0`, `skipped_duplicates == 9`) AND the real binary double-run against one DB (`imported: 9` → `imported: 0`)
- Tolerance rules hold: fenced fake `### Decisions` heading never derails section detection; `None yet.` placeholder yields zero todos; lone `-` malformed bullet and >8192-byte items are skipped and counted; heading-less garbage yields zero drafts without panicking
- Import embeds all new drafts in ONE `/api/embed` call, best-effort: with Ollama dead the import still succeeds (rows land pending, warning to stderr) and imported content is immediately findable via the FTS5 keyword path

## Task Commits

Each task was committed atomically:

1. **Task 1: Fixture + tolerant parser + failing import integration test** - `0d0715e` (test, TDD RED — parser unit suite GREEN, integration test compile-fails on the missing service surface)
2. **Task 2: Idempotent import path + CLI subcommand** - `e529ba4` (feat, TDD GREEN)

**Plan metadata:** this commit (docs(02-03))

## Files Created/Modified

- `crates/agent-memory-core/src/import/gsd_state.rs` - Tolerant single-pass line scanner: fence state, heading-text section mapping, bullet + deferred-table extraction, 8192-byte cap, 9 inline unit tests
- `crates/agent-memory-core/src/import/mod.rs` - Importer module invariants (tolerant, idempotent, data-only) + gsd_state re-export
- `crates/agent-memory-core/tests/fixtures/STATE.md` - Realistic GSD STATE.md incl. fence trap, placeholder, malformed bullet, deferred table
- `crates/agent-memory-core/tests/import.rs` - INTEROP-01 proof: typed counts, keyword searchability with a dead embedder, idempotent re-run
- `crates/agent-memory-core/src/store/{mod,sqlite}.rs` - `Store::exists` dedup probe (parameterized SELECT 1 … LIMIT 1 on the read pool)
- `crates/agent-memory-core/src/service.rs` - `ImportReport` + `MemoryService::import` (one dedup spawn_blocking hop → one batch embed → standard inserts, one clock stamp)
- `crates/agent-memory/src/main.rs` - `Import` subcommand, `run_import` wiring (same constructor shape as serve), `.planning`-ancestor default scope, stdout report
- `crates/agent-memory-core/src/lib.rs` - registers `pub mod import`

## Decisions Made

- Dedup checks the store only (not within-batch) — exactly per the plan contract; the fixture and real STATE.md files have unique items per section
- `Store::exists` binds `source` as `""` for a draft with no source (parser always sets `gsd-state`, so this is a defensive no-match, never an unwrap)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- Not a code issue: 4 consecutive gsd-executor subagent dispatches died on API stream stalls (`Connection closed mid-response` / 600s watchdog) with zero side effects each time. Execution completed inline in the orchestrator per the workflow's sanctioned stall-recovery path. All TDD gates, verify commands, and acceptance criteria were still enforced.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Import is the last code slice; plan 02-04 (release) can document `import` in the README and ship v0.0.1
- Workspace fully green for the release gate: 56 tests + 1 ignored (live Ollama), clippy `-D warnings` clean, fmt clean

## Self-Check: PASSED

- All 4 created key files exist on disk; `git log --grep="02-03"` returns 2 task commits
- Task 1 acceptance: lib tests green (26), fixture literals present, MAX_IMPORT_CONTENT_BYTES=8192, RED confirmed before Task 2
- Task 2 acceptance: `--test import` green, CLI double-run printed `imported: 9` then `imported: 0`, bogus `--from` exits 1 naming gsd-state, `SELECT 1 FROM memories` fully parameterized, workspace/clippy/fmt green, stdio_purity green

---
*Phase: 02-semantic-search-interop-release*
*Completed: 2026-07-03*
