---
phase: 5
slug: portable-export-import
# status lifecycle: draft (seeded by plan-phase) → planned (filled by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: planned
nyquist_compliant: true
wave_0_complete: false
created: 2026-10-25
---

# Phase 5 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in) + `#[tokio::test]`; `FakeEmbedder` / `TestClock` injected via the `test-clock` feature self-dev-dependency (`crates/agent-memory-core/Cargo.toml`) |
| **Config file** | none — workspace `Cargo.toml`; no framework install this phase |
| **Quick run command** | `cargo test -p agent-memory-core --test export_import --test import --test redaction` |
| **Full suite command** | `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` |
| **Coverage gate** | `cargo llvm-cov --workspace --fail-under-lines 80` (CI-enforced, `.github/workflows/ci.yml:44`; `cargo-llvm-cov 0.8.7` is installed locally) |
| **Estimated runtime** | quick: ~30–60 s incremental; full: several minutes on a cold build (executors run clippy + full tests in the background with a log file and poll — watchdog ~600 s) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p agent-memory-core --test export_import --test import --test redaction` plus `cargo clippy -p <touched crate> --all-targets --locked -- -D warnings` (every task's `<verify>` names its own narrower command too)
- **After every plan wave:** Run `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`
- **Before `/gsd-verify-work`:** Full suite green AND `cargo llvm-cov --workspace --fail-under-lines 80` green
- **Max feedback latency:** ~60 s (quick command, incremental build)

---

## Per-Task Verification Map

> Wave 0 note: this phase uses task-level TDD (Phase 3/4 precedent) — each new test file is created RED by the owning task's `<behavior>` block before its implementation lands. No separate Wave 0 plan.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 05-01-T1 | 01 | 1 | INTEROP-02 (D-14, 04-REVIEW IN-01) | T-05-01, T-05-14 | storage faults on store/list/import/forget/link/unlink/update/sweep redacted to `<op> failed due to a storage error (…)`; `/health`, `POST /api/memories`, `memory_store` carry no `sqlite` substring; `SEARCH_STORAGE_FAILURE` byte-identical | unit (service.rs) + integration (core, RENAME fault injection) + in-process REST/MCP | `cargo test -p agent-memory-core --lib redact_storage_error && cargo test -p agent-memory-core --test redaction && cargo test -p agent-memory --bin agent-memory storage_failure` | ❌ `tests/redaction.rs` created RED by 05-01-T1; handlers.rs/mcp.rs test modules extended | ⬜ pending |
| 05-01-T2 | 01 | 1 | INTEROP-02 (D-07, D-12) | T-05-15 | NULL-safe id-returning dedup probe (`source IS ?1`); `link` reports `created` from `tx.changes()`; wire JSON unchanged | integration (core) + in-process transports | `cargo test -p agent-memory-core --test store find_by_key && cargo test -p agent-memory-core --test relations && cargo test -p agent-memory-core --test import && cargo test -p agent-memory --bin agent-memory link` | ✅ extends `tests/store.rs`, `tests/relations.rs` | ⬜ pending |
| 05-01-T3 | 01 | 1 | INTEROP-02 (D-10, D-13) | T-05-15, T-05-16 | `insert_imported`: `embedding_status = 0`, no vector row, `last_accessed = now`, provenance preserved; exactly one `memories` INSERT behind `insert_row` | integration (core, raw-connection reads) | `cargo test -p agent-memory-core --test store && cargo test -p agent-memory-core --test ttl && cargo test -p agent-memory-core --test backfill_race && cargo test -p agent-memory-core --test update` | ✅ extends `tests/store.rs` | ⬜ pending |
| 05-02-T1 | 02 | 2 | DIST-04 (D-01, D-03, D-04, D-05, D-06; edges: empty, ordering, concurrency-backstop) | T-05-04, T-05-11, T-05-19, T-05-20 | header from `meta` + injected clock; 11-key memory records, no vectors/derived fields; one read snapshot; export faults redacted; no MCP/REST route | unit (export.rs goldens) + integration (core) + real-binary e2e | `cargo test -p agent-memory-core --lib export:: && cargo test -p agent-memory-core --test export_import` + the `<verify>` stdout header check | ❌ `tests/export_import.rs` + `export.rs` tests created RED by 05-02-T1 | ⬜ pending |
| 05-02-T2 | 02 | 2 | DIST-04 (D-02) | T-05-02, T-05-03 | `create_new` + `mode(0o600)` temp beside the target; `sync_all` + `rename` after flush; temp removed on failure; existing target replaced | unit (binary crate, `#[cfg(unix)]` perms) + real-binary e2e | `cargo test -p agent-memory --bin agent-memory export_file::` + the `<verify>` `--output` check | ❌ `export_file.rs` tests created RED by 05-02-T2 | ⬜ pending |
| 05-03-T1 | 03 | 3 | INTEROP-02, DIST-04 (D-01, D-07, D-08, D-09, D-10, D-11, D-12, D-13) | T-05-05, T-05-06, T-05-08, T-05-13, T-05-17 | `parse_jsonl` validates the whole file before `SqliteStore::open`; keys index a map, never ids; no embedder call; provenance preserved, `last_accessed` refreshed; skew warning; import faults redacted | unit (jsonl.rs, service.rs) + integration (core round trip + idempotency) + real-binary e2e | `cargo test -p agent-memory-core --lib import::jsonl && cargo test -p agent-memory-core --lib clock_skew && cargo test -p agent-memory-core --test export_import round_trip && cargo test -p agent-memory-core --test export_import idempotent && cargo test -p agent-memory-core --test import` | ❌ `jsonl.rs` tests created RED by 05-03-T1; extends `tests/export_import.rs`, `tests/import.rs` | ⬜ pending |
| 05-03-T2 | 03 | 3 | INTEROP-02, DIST-04 (D-07, D-08, D-10, D-11, D-13; edges: both adjacency, both empty, INTEROP ordering) | T-05-04, T-05-05, T-05-07, T-05-08, T-05-09 | overlapping-rowid kill-test (`edges_touching(pre_existing) == 0`, `orphan_edges == 0`); dedup-to-existing adjacency; in-file duplicates collapse; `expires_at <= now` skipped + `links_dropped`; pending → sweep → hybrid; header-only / zero-byte; file-order independence; ≥ 18-case malformed table; hostile input never panics; nothing written on a failed parse | integration (core) + unit (jsonl.rs) | `cargo test -p agent-memory-core --test export_import && cargo test -p agent-memory-core --lib import::jsonl` | ✅ extends `tests/export_import.rs`, `jsonl.rs` tests | ⬜ pending |
| 05-04-T1 | 04 | 4 | DIST-04, INTEROP-02 (docs: D-01..D-13) | T-05-11, T-05-18 | README states CLI-only rationale, 0600/atomic `--output`, strict reader + upgrade message, timestamp policy, pending-embedding cadence, the six counts, backup-vs-export | docs (grep gates) | `test "$(grep -c '^## Export & import' README.md)" = 1 && test "$(grep -c 'VACUUM INTO' README.md)" -ge 1 && test "$(grep -c 'upgrade agent-memory' README.md)" -ge 1` | ✅ README exists | ⬜ pending |
| 05-04-T2 | 04 | 4 | DIST-04, INTEROP-02 (D-01, D-02, D-11, D-12) | T-05-07, T-05-11 | malformed file → non-zero exit naming the line AND the `--db` target is never created; invalid UTF-8 refused without `PANIC`; `--from bogus` / `--scope` with jsonl refused; D-01 transport grep empty | realism (spawned binary, zero coverage by design) | `cargo test -p agent-memory --test cli` | ❌ `tests/cli.rs` created by 05-04-T2 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Satisfied structurally by task-level TDD (RED test files land inside the owning task, before implementation):

- [x] `crates/agent-memory-core/tests/redaction.rs` — created RED by 05-01-T1 (D-14 on the eight non-search paths; harness copied from `tests/relations.rs:33-104`)
- [x] `crates/agent-memory-core/tests/export_import.rs` — created RED by 05-02-T1 (export half), extended by 05-03-T1/T2 (import half, kill-tests)
- [x] `crates/agent-memory-core/src/export.rs` `#[cfg(test)]` — created by 05-02-T1 (golden field order + `deny_unknown_fields` probes)
- [x] `crates/agent-memory/src/export_file.rs` `#[cfg(test)]` — created by 05-02-T2 (0600 / commit / abort / create_new / same-dir)
- [x] `crates/agent-memory-core/src/import/jsonl.rs` `#[cfg(test)]` — created by 05-03-T1 (basic set), completed by 05-03-T2 (≥ 18-case malformed table + hostile input)
- [x] `crates/agent-memory/tests/cli.rs` — created by 05-04-T2 (realism only)
- [x] In-process redaction tests appended to `rest/handlers.rs` and `mcp.rs` test modules — 05-01-T1 (`test_state` / `test_server` harnesses already exist)
- [x] Framework install: none — cargo test is built-in

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| DIST-04 concurrency edge: memories and links are read inside ONE read transaction so a concurrent `link` cannot produce a dangling key (`must_haves.truths` backstop item in 05-02) | DIST-04 | A deterministic interleaving test would need a second process writing mid-export; the guarantee rests on WAL snapshot semantics + the `memory_links` FK | Code review of `SqliteStore::export_rows` (plan 05-02 T1): confirm a single `unchecked_transaction()` (or `BEGIN`…`COMMIT`) spans the `meta` reads, both `count(*)`s, the memories SELECT and the links SELECT on ONE pooled connection, and that no row is collected into a `Vec`. The round-trip tests (05-03) additionally prove the exporter's own output always imports cleanly. |
| Coverage gate at phase end | all | CI runs it; locally it takes minutes | `cargo llvm-cov --workspace --fail-under-lines 80` once after plan 05-04 (record the figure in 05-04-SUMMARY.md) |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies (9/9 tasks carry an `<automated>` command)
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references (via task-level TDD RED phases)
- [x] No watch-mode flags
- [x] Feedback latency ≈ 60 s on the quick command (cargo incremental build; documented, accepted — matches the Phase 4 contract)
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** planner sign-off 2026-10-25 (execution pending)
