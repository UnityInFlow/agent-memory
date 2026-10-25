---
phase: 05-portable-export-import
plan: 05
subsystem: testing, validation-evidence
tags: [rust, sqlite, wal, snapshot-isolation, mpsc, export, import, ollama, nomic-embed-text, hybrid-search, gap-closure]

# Dependency graph
requires:
  - phase: 05-portable-export-import
    provides: "plan 05-02: Store::export_rows on ONE pooled connection + ONE read transaction (unchecked_transaction), ExportEvent/ExportSnapshot stream, tests/export_import.rs harness; the backstop truth 2.8 this plan turns into evidence"
  - phase: 05-portable-export-import
    provides: "plan 05-03: parse_jsonl strict reader, MemoryService::import_jsonl, `import --from jsonl` six-count + pending lines; D-13 rows land pending and the sweep re-embeds them"
  - phase: 05-portable-export-import
    provides: "plan 05-04: real-binary CLI round trip + README; 05-VERIFICATION.md (human_needed, 32/33) naming the two items closed here"
provides:
  - "tests/export_import.rs: `a_concurrent_link_committed_mid_export_cannot_produce_a_dangling_key` — the held-out, channel-ordered race proof for the one-snapshot export (DIST-04 backstop truth 2.8); Harness exposes `store: Arc<SqliteStore>`"
  - "Recorded live-Ollama realism run (05-05-realism.sh, exit 0): A → move.jsonl → B lands 3 pending, first serve-rest sweep tick `backfilled=3`, `/health` embedder ready, paraphrase `POST /api/search` → `search_mode: hybrid` with the imported memory at rank 0 (INTEROP-02 criterion 3)"
  - "05-VALIDATION.md: 05-05-T1 / 05-05-T2 green; Manual-Only live re-embed row `Result: PASS`; concurrency row retired as manual"
affects: [05 re-verification (both human_verification items now have explicit evidence), phase-6 release notes]

# Actuals (#2632) — same estimateTokens scale as the plan's `estimate` (chars/4 over the realized diff)
actuals:
  tokens: 8100
  tasks: 2
  commits: 2

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Channel-ordered race test: a std::thread writer on the store's writer lane and the exporter's read-pool snapshot hand off through two mpsc channels (go: exporter→writer on the first memory record; done: writer→exporter with the new id); every wait is `recv_timeout(10 s)` so a blocked write fails with a named message instead of hanging the suite — no timed waits decide ordering"
    - "Realism run as a committed script, not a cargo test: live-service evidence (Ollama) lives in `.planning/phases/.../05-05-realism.sh` with fixed checks, counter-bounded waits, a trap-killed daemon on an ephemeral loopback port, and an evidence block pasted verbatim into the SUMMARY"

key-files:
  created: []
  modified:
    - crates/agent-memory-core/tests/export_import.rs
    - .planning/phases/05-portable-export-import/05-VALIDATION.md

key-decisions:
  - "Gap 1 closed with a test, not a code change: WAL snapshot isolation was already the mechanism; the race test drives `Store::export_rows` directly with a pausing visitor and proves the committed row is visible to a fresh connection yet absent from the open snapshot, the raced file parses strictly with exact header counts, and the next export carries both the memory and the `caused_by` edge"
  - "Gap 2 closed by running the real binary against the real `nomic-embed-text` on this host and recording the raw outputs — never a fake embedder, never a loosened check; the fake-embedder mechanism proof (`imported_rows_land_pending_and_the_sweep_backfills_them`) stays as the CI gate"
  - "05-02-PLAN.md left untouched (its `verification: backstop` marker stays); this plan's plain-string truth plus the new test is what the re-verifier reconciles truth 2.8 against"
  - "TDD RED observed as a compile failure (`no field store on type Harness` + four missing imports) before the harness change; GREEN on first run; test and harness change committed together because the project rule forbids committing failing tests"

patterns-established:
  - "Held-out proof for a `verification: backstop` truth: restate the truth as a plain string in the gap-closure plan, add the exact test the verifier named, leave the original plan's marker in place"

requirements-completed: [DIST-04, INTEROP-02]

# Coverage metadata (#1602)
coverage:
  - id: D1
    description: "A memory and a link committed by a concurrent writer while `Store::export_rows` is mid-stream are invisible to that export (header counts == record counts, no dangling link key, raced file accepted by `parse_jsonl`) and present in the next export; the write commits while the snapshot is open"
    requirement: DIST-04
    verification:
      - kind: integration
        ref: "crates/agent-memory-core/tests/export_import.rs#a_concurrent_link_committed_mid_export_cannot_produce_a_dangling_key (5 consecutive runs green; file 20/20)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Cross-database move with the live embedder: export from A, import into B lands pending, first serve-rest sweep tick re-embeds all three rows against the local nomic-embed-text, paraphrase search answers search_mode hybrid with the imported memory in results"
    requirement: INTEROP-02
    verification:
      - kind: other
        ref: "bash .planning/phases/05-portable-export-import/05-05-realism.sh (2026-10-25T13:39:00Z, exit 0, `PASS: live Ollama realism run — search_mode hybrid, imported memory id 1 at rank 0`)"
        status: pass
    human_judgment: false

# Metrics
duration: 6 min
completed: 2026-10-25
status: complete
---

# Phase 5 Plan 05: Gap closure — one-snapshot race proof + live-Ollama cross-database re-embed Summary

**A channel-ordered race test proves the export's WAL snapshot cannot straddle a concurrent memory+link commit, and a recorded run of the real binary against the live `nomic-embed-text` moves three memories A → JSONL → B, backfills them on the first sweep tick and answers a paraphrase search in hybrid mode — closing both `human_verification` items of 05-VERIFICATION.md without touching production code.**

## Performance

- **Duration:** 6 min
- **Started:** 2026-10-25T13:35:24Z
- **Completed:** 2026-10-25T13:42:16Z
- **Tasks:** 2
- **Files modified:** 2 (one test file, one planning doc)

## Accomplishments

- `a_concurrent_link_committed_mid_export_cannot_produce_a_dangling_key` (tests/export_import.rs): a `std::thread` writer owning `Arc<SqliteStore>` inserts "delta arrived mid-export" and links it `caused_by` → `a` while the exporter is paused inside the visitor on the first memory record. A fresh raw connection sees 4 rows / 3 edges while the export's read transaction is still open; the snapshot still reports 3 / 2 and no record carries the new id; the raced file (header mirrored from the snapshot as the service does, plus every visited record) is accepted by `parse_jsonl` with `memories.len() == 3`, `links.len() == 2`; the next `export_to_string` reports `{ memories: 4, links: 3 }` and carries both the new memory and the `caused_by` edge. Ordering is decided only by two `mpsc` hand-offs, every wait bounded by `recv_timeout(10 s)`; no `thread::sleep`. Stable over 5 consecutive runs; `--test export_import` is 20/20.
- `Harness` gained `store: Arc<SqliteStore>` (the one argument now shared with `MemoryService::new`); module doc updated; nothing else in the 19 existing tests changed.
- Live realism run (`05-05-realism.sh`, exit 0, 2026-10-25T13:39:00Z) recorded verbatim below: six-count import line, `3 rows pending embedding`, `embedded pending memories on sweep backfilled=3 skipped_stale=0` about 50 ms after `REST listening on 127.0.0.1:57875`, `/health` `"embedder":"ready"`, `"search_mode":"hybrid"` with the SQLite-engine memory (id 1) at rank 0 for a paraphrase that shares almost no keyword with it.
- 05-VALIDATION.md: `05-05-T1` and `05-05-T2` flipped to green with the run date; the Manual-Only live re-embed row carries `Result: PASS …`; the concurrency Manual-Only row was already retired in favour of the automated test.
- Gates per commit: `cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0, `cargo test --workspace --locked` **287 passed / 0 failed / 2 ignored** (baseline 286 + the new test).

## Task Commits

Each task was committed atomically:

1. **Task 1: Held-out race test — a memory and link committed mid-export are invisible to that snapshot and present in the next** - `6cb8893` (test)
2. **Task 2: Live-Ollama realism run — export from A, import into B, first-sweep backfill, hybrid paraphrase search (recorded evidence)** - `d52c1ff` (docs)

**Plan metadata:** see the final `docs(05-05)` commit (this SUMMARY + STATE/ROADMAP/REQUIREMENTS).

_Task 1 was `tdd="true"`: RED was observed as a compile failure (`error[E0609]: no field `store` on type `Harness`` plus the four planned missing imports) before the harness change; GREEN on the first run after it. Test and harness change were committed together because the project rule forbids committing failing tests._

## Files Created/Modified

- `crates/agent-memory-core/tests/export_import.rs` - `store: Arc<SqliteStore>` on `Harness`; imports `Store`, `ExportEvent`, `ExportSnapshot`, `mpsc`, `Duration`; `record_lines` helper; the new race test under the banner "Export half — gap closure 05-05: one-snapshot concurrency proof"
- `.planning/phases/05-portable-export-import/05-VALIDATION.md` - 05-05-T1 / 05-05-T2 Status cells green; Manual-Only live re-embed `Result: PASS` with the evidence pointer

No production file changed: `git diff --stat 30085a2 HEAD -- crates/agent-memory-core/src crates/agent-memory/src Cargo.lock .github/ crates/agent-memory-core/sql/` is empty. `05-02-PLAN.md` was left untouched — its `verification: backstop` marker is reconciled by the re-verifier against this plan's plain-string truth and the new test.

## Live Ollama realism evidence

Command run from the repo root (background + log, as the plan directs): `bash .planning/phases/05-portable-export-import/05-05-realism.sh > /tmp/p0505/realism.log 2>&1` — exit 0; last line `PASS: live Ollama realism run — search_mode hybrid, imported memory id 1 at rank 0 (evidence: /var/folders/jr/lwzz65cx5pndqqdgzhnym1pc0000gn/T/tmp.Ws0nq17M6o/EVIDENCE.md)`. Precondition probe before the run: `curl -s --max-time 5 http://localhost:11434/api/tags | grep -c '"name":"nomic-embed-text'` → `1`; `/api/version` → `{"version":"0.35.0"}`. The script's checks were not edited. The evidence block below is the script's output verbatim (`EVIDENCE.md`):

**Live Ollama realism evidence (2026-10-25T13:39:00Z)** — the script's own heading line, demoted to bold so the section heading above stays the only `## Live Ollama realism evidence` heading

- host: Darwin 27.0.0 arm64; Ollama at http://localhost:11434, version {"version":"0.35.0"}; model: {"name":"nomic-embed-text:latest"
- binary: target/debug/agent-memory (agent-memory 0.0.1); workdir: /var/folders/jr/lwzz65cx5pndqqdgzhnym1pc0000gn/T/tmp.Ws0nq17M6o (kept, not removed)

```text
$ target/debug/agent-memory import --from gsd-state --scope realism --db /var/folders/jr/lwzz65cx5pndqqdgzhnym1pc0000gn/T/tmp.Ws0nq17M6o/a.db /var/folders/jr/lwzz65cx5pndqqdgzhnym1pc0000gn/T/tmp.Ws0nq17M6o/STATE.md
imported: 3, skipped duplicates: 0, malformed lines skipped: 0
$ target/debug/agent-memory export --db /var/folders/jr/lwzz65cx5pndqqdgzhnym1pc0000gn/T/tmp.Ws0nq17M6o/a.db --output /var/folders/jr/lwzz65cx5pndqqdgzhnym1pc0000gn/T/tmp.Ws0nq17M6o/move.jsonl
exported 3 memories and 0 links
move.jsonl line 1: {"record":"header","format":"agent-memory-export","version":1,"exported_at":1792935539,"embedding_model":"nomic-embed-text","embedding_dim":768,"schema_version":3,"agent_memory_version":"0.0.1","exported_rows":3,"exported_links":0}
$ target/debug/agent-memory import --from jsonl --db /var/folders/jr/lwzz65cx5pndqqdgzhnym1pc0000gn/T/tmp.Ws0nq17M6o/b.db /var/folders/jr/lwzz65cx5pndqqdgzhnym1pc0000gn/T/tmp.Ws0nq17M6o/move.jsonl
imported: 3, skipped duplicates: 0, skipped expired: 0, links imported: 0, links skipped duplicates: 0, links dropped: 0
3 rows pending embedding — the sweep backfills them on the next serve/serve-rest start (128 per tick)
$ RUST_LOG=info target/debug/agent-memory serve-rest --db /var/folders/jr/lwzz65cx5pndqqdgzhnym1pc0000gn/T/tmp.Ws0nq17M6o/b.db --addr 127.0.0.1:0   (pid 99888, stderr → daemon.log)
2026-10-25T13:38:59.490867Z  INFO agent_memory: REST listening on 127.0.0.1:57875
2026-10-25T13:38:59.495966Z  INFO agent_memory: semantic search ready (nomic-embed-text)
2026-10-25T13:38:59.539602Z  INFO agent_memory_core::service: embedded pending memories on sweep backfilled=3 skipped_stale=0
2026-10-25T13:38:59.539644Z  INFO agent_memory: lifecycle sweep complete expired=0 rescored=3
$ curl http://127.0.0.1:57875/health
{"embedder":"ready","status":"ok"}
$ curl -X POST http://127.0.0.1:57875/api/search -d '{  "query": "which database did we choose so that nothing needs a hosted service",  "limit": 3}'
{"search_mode":"hybrid","results":[{"id":1,"content":"SQLite is the embedded storage engine so the memory daemon runs with no cloud account and no server process","type":"DECISION","tags":["gsd"],"scope":"realism","decay_score":0.999999955430352,"created_at":1792935539,"last_accessed":1792935539},{"id":2,"content":"Release tarballs carry a signed SLSA provenance attestation built on GitHub-hosted runners","type":"DECISION","tags":["gsd"],"scope":"realism","decay_score":0.999999955430352,"created_at":1792935539,"last_accessed":1792935539},{"id":3,"content":"Clippy must pass with -D warnings and cargo fmt must be clean before every commit","type":"DECISION","tags":["gsd"],"scope":"realism","decay_score":0.999999955430352,"created_at":1792935539,"last_accessed":1792935539}]}
paraphrase target rank (0 = first): 0; imported memory id in b.db: 1
```

Reading: the gsd-state seed of A reports the gsd-path three-count line (zeros for the link counters are implicit there — D-12's additive report prints the six-count form only on the JSONL path); the export header is format v1 with `exported_rows: 3` / `exported_links: 0`; B's import prints the exact six-count line and the pending line; the daemon's first sweep tick embedded all three rows (`backfilled=3 skipped_stale=0`) 49 ms after bind, before `lifecycle sweep complete expired=0 rescored=3`; `/health` reports the embedder ready; the paraphrase ("which database did we choose so that nothing needs a hosted service") shares essentially no keyword with the target ("SQLite is the embedded storage engine so the memory daemon runs with no cloud account and no server process"), so the KNN leg carried rank 0 — `search_mode: "hybrid"` with the imported memory present is INTEROP-02 criterion 3 on the live embedder. The temp directory was kept (the repo's pre-bash hook forbids removing `.db` files from the shell).

## Decisions Made

- The gap was closed with evidence only (one test, one recorded run, planning rows) — no production, schema, lock or workflow change, as the plan's prohibitions require.
- The writer in the race test is a `std::thread`, not a tokio task: `Store` methods are synchronous and `#[tokio::test]` is a current-thread runtime that the exporter blocks while waiting on `done_rx`.
- The realism run stays a script and is not promoted to a cargo test (Ollama is absent in CI; the fake-embedder proof remains the CI gate).

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None. The `echo ====` separators in early read commands were rewritten by the local RTK hook into failing commands; switched to `printf` separators — no effect on the work.

## TDD Gate Compliance

Task 1 (`tdd="true"`): RED observed as a compile failure against the unchanged harness (`no field store on type Harness`), GREEN on the first run after adding `store: Arc<SqliteStore>`. Per the project rule "never commit failing tests", the RED state was not committed; a single `test(05-05)` commit (`6cb8893`) carries the test and the harness change. No refactor commit was needed.

## User Setup Required

None - no external service configuration required (the run used the already-running local Ollama).

## Next Phase Readiness

- Both 05-VERIFICATION.md `human_verification` items now have explicit, re-runnable evidence on this host: truth 2.8 via the race test (CI-gated), the cross-database live re-embed via the recorded script run. The orchestrator re-runs phase verification next; this plan does not mark the phase complete.
- Phase 5 plan progress after this plan: 5/5 plans executed.
- Workspace gate at `d52c1ff`: fmt clean, clippy clean, 287 passed / 0 failed / 2 ignored.

---
*Phase: 05-portable-export-import*
*Completed: 2026-10-25*

## Self-Check: PASSED

- files present: 05-05-SUMMARY.md, 05-VALIDATION.md, tests/export_import.rs, 05-05-realism.sh
- commits present: 6cb8893 (Task 1), d52c1ff (Task 2)
- Task 2 acceptance greps on this file: section heading 1, `"search_mode":"hybrid"` ≥1, `backfilled=3` ≥1, six-count line ≥1; VALIDATION `Result: PASS` ≥1
