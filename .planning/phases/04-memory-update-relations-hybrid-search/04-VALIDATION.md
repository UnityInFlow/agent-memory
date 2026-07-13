---
phase: 4
slug: memory-update-relations-hybrid-search
status: planned
nyquist_compliant: true
wave_0_complete: false
created: 2026-07-12
---

# Phase 4 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in) + tokio::test; FakeEmbedder/TestClock injection via the `test-clock` feature self-dev-dependency |
| **Config file** | none — workspace Cargo.toml |
| **Quick run command** | `cargo test -p agent-memory-core` |
| **Full suite command** | `cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check` |
| **Estimated runtime** | ~30 seconds (quick) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p agent-memory-core`
- **After every plan wave:** Run `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Before `/gsd-verify-work`:** Full suite + `cargo fmt --check` must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

> Wave 0 note: this phase uses task-level TDD (Phase 3 precedent) — each new test file is created RED by the owning task's behavior block before its implementation lands, satisfying the test-before-code requirement without a separate Wave 0 plan.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 04-01-T1 | 04-01 | 1 | CR-01/WR-02/WR-01 (03-REVIEW) | T-04-04 | source:None re-import → skipped_duplicates; Internal → 500 tier both mappers; MAX_KNN_K = MAX_LIMIT*4 | unit (core) + mapper units | `cargo test -p agent-memory-core --test import` | ✅ (extends tests/import.rs) | ⬜ pending |
| 04-01-T2 | 04-01 | 1 | STORE-05 | T-04-01/02/03/05 | fixture divergence, backup created/retained-one-per-version, too-new friendly refusal, fresh-DB no-backup | unit (on-disk tempfiles) | `cargo test -p agent-memory-core --test migration_hygiene` | ❌ created RED by 04-01-T2 | ⬜ pending |
| 04-02-T1 | 04-02 | 2 | MCP-06 | T-04-06/07/08 | patch semantics, re-embed kill-test, embed-outage → status 0 + sweep backfill, unknown id not-found, validate-before-embed | unit (core) + MCP unit | `cargo test -p agent-memory-core --test update` | ❌ created RED by 04-02-T1 | ⬜ pending |
| 04-02-T2 | 04-02 | 2 | MCP-06 | T-04-07 | absent/null/value matrix on both transports, PATCH 404 / 400-never-500 | integration (in-process handler + MCP unit) | `cargo test -p agent-memory --bins` | partial (extends handlers.rs/mcp.rs suites) | ⬜ pending |
| 04-03-T1 | 04-03 | 3 | MCP-07 | T-04-10/12/13 | link/unlink idempotency, self-link 400, unknown-kind 400, cascade on forget AND sweep, zero orphans | unit (core) + transports | `cargo test -p agent-memory-core --test relations` | ❌ created RED by 04-03-T1 | ⬜ pending |
| 04-03-T2 | 04-03 | 3 | MCP-07 | T-04-11/14 | expansion cap + most-recent-win, reverse labels, envelope byte-compat, no neighbor bumps | unit (core) + transports | `cargo test -p agent-memory-core --test relations` | extends relations.rs | ⬜ pending |
| 04-04-T1 | 04-04 | 4 | SEARCH-04, SEARCH-05 | T-04-16/18 | RRF hand-computed sums, dup-id summing, oversample recall, fresh-outranks-stale golden, hybrid label | unit (core) | `cargo test -p agent-memory-core --test hybrid` | ❌ created RED by 04-04-T1 | ⬜ pending |
| 04-04-T2 | 04-04 | 4 | SEARCH-04, SEARCH-05 | T-04-15/17 | degrade decision table, InvalidQuery propagation in hybrid, bump only post-truncation ids, envelope compat | unit + integration | `cargo test -p agent-memory-core --test hybrid` | extends hybrid.rs | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Satisfied structurally by task-level TDD (RED test files land inside the owning task, before implementation):

- [x] `crates/agent-memory-core/tests/update.rs` — created RED by 04-02-T1 (MCP-06)
- [x] `crates/agent-memory-core/tests/relations.rs` — created RED by 04-03-T1 (MCP-07)
- [x] `crates/agent-memory-core/tests/hybrid.rs` — created RED by 04-04-T1 (SEARCH-04/05 golden fusion fixture corpus)
- [x] `crates/agent-memory-core/tests/migration_hygiene.rs` + `tests/fixtures/v0.0.1.db` — created by 04-01-T2 (STORE-05; fixture generated once from the frozen SQL, committed as binary, never regenerated)
- [x] Framework install: none — cargo test is built-in

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Live Ollama hybrid search end-to-end | SEARCH-04 | Real embedder unavailable in CI; FakeEmbedder covers logic | With Ollama running locally: `memory_search` a paraphrased query, confirm `search_mode: "hybrid"` and fused ordering; stop Ollama, re-run, confirm `"keyword"` degrade with unchanged envelope (end-of-phase check, plan 04-04 Task 2 human-check) |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references (via task-level TDD RED phases)
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** planner sign-off 2026-07-13 (execution pending)
