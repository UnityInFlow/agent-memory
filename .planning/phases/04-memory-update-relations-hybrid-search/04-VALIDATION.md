---
phase: 4
slug: memory-update-relations-hybrid-search
status: draft
nyquist_compliant: false
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

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| TBD | — | — | MCP-06 | TBD | patch semantics (absent/null/value), re-embed kill-test, embed-outage → status 0, unknown id not-found | unit (core) | `cargo test -p agent-memory-core --test update` | ❌ W0 | ⬜ pending |
| TBD | — | — | MCP-06 | TBD | PATCH 404 / MCP not-found shape, 400-never-500 for bad patches | integration | `cargo test -p agent-memory` | partial (extends handlers.rs/tools.rs) | ⬜ pending |
| TBD | — | — | MCP-07 | TBD | link/unlink idempotency, self-link 400, cascade on forget AND sweep, zero orphans, expansion cap + no-bump | unit (core) | `cargo test -p agent-memory-core --test relations` | ❌ W0 | ⬜ pending |
| TBD | — | — | SEARCH-04 | TBD | RRF hand-computed sums, dup-id summing, `search_mode` decision table, InvalidQuery propagation, envelope byte-compat | unit + integration | `cargo test -p agent-memory-core --test hybrid` | ❌ W0 | ⬜ pending |
| TBD | — | — | SEARCH-05 | TBD | fresh-outranks-stale-at-equal-fused-rank golden fixture; bump only post-truncation ids | unit (core) | `cargo test -p agent-memory-core --test hybrid` | ❌ W0 | ⬜ pending |
| TBD | — | — | STORE-05 | TBD | fixture divergence, backup created/retained-one-per-version, too-new friendly refusal, fresh-DB no-backup | unit (on-disk tempfiles) | `cargo test -p agent-memory-core --test migration_hygiene` | ❌ W0 | ⬜ pending |
| TBD | — | — | CR-01 (03-REVIEW) | TBD | source:None re-import → skipped_duplicates | unit (core) | `cargo test -p agent-memory-core --test import` | ✅ (extends tests/import.rs) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

*(Task IDs filled by the planner — map rows derive from 04-RESEARCH.md Validation Architecture.)*

---

## Wave 0 Requirements

- [ ] `crates/agent-memory-core/tests/update.rs` — stubs for MCP-06
- [ ] `crates/agent-memory-core/tests/relations.rs` — stubs for MCP-07
- [ ] `crates/agent-memory-core/tests/hybrid.rs` — stubs for SEARCH-04/05 (golden fusion fixture corpus)
- [ ] `crates/agent-memory-core/tests/migration_hygiene.rs` + `tests/fixtures/v0.0.1.db` — stubs for STORE-05 (fixture generated once from the frozen SQL, committed as binary)
- [ ] Framework install: none — cargo test is built-in

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Live Ollama hybrid search end-to-end | SEARCH-04 | Real embedder unavailable in CI; FakeEmbedder covers logic | With Ollama running locally: `memory_search` a paraphrased query, confirm `search_mode: "hybrid"` and fused ordering |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
