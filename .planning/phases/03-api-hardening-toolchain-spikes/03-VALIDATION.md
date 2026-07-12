---
phase: 3
slug: api-hardening-toolchain-spikes
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-07-12
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Derived from 03-RESEARCH.md "## Validation Architecture" — the planner refines the per-task map.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) + existing integration suites (fallback.rs, rest.rs, store.rs) |
| **Config file** | Cargo.toml (workspace) — no Wave 0 install needed |
| **Quick run command** | `cargo test -p agent-memory-core` |
| **Full suite command** | `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check` |
| **Estimated runtime** | ~30-60 seconds (quick), ~2-3 min (full) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p agent-memory-core`
- **After every plan wave:** Run the full suite command
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 180 seconds

---

## Per-Task Verification Map

Filled by the planner per task. The validation layers from RESEARCH:

| Behavior | Requirement | Threat Ref | Test Type | Automated Command | Status |
|----------|-------------|------------|-----------|-------------------|--------|
| Out-of-bounds limit → InvalidArgument at core | API-02 | T-03-xx | unit (core) | `cargo test -p agent-memory-core validation` | ⬜ pending |
| Out-of-bounds ttl_secs (0, negative, overflow) → InvalidArgument on store AND import surfaces | API-02 | T-03-xx | unit (core) | `cargo test -p agent-memory-core validation` | ⬜ pending |
| REST 400 (never 500) for out-of-bounds inputs, in-process handler | API-02 | T-03-xx | integration | `cargo test -p agent-memory rest` | ⬜ pending |
| MCP invalid_params tier for out-of-bounds inputs | API-02 | T-03-xx | unit (mcp) | `cargo test -p agent-memory map_mcp_error` | ⬜ pending |
| Real-HTTP boundary assertions in spawned-binary e2e | API-02 | T-03-xx | e2e (realism-only, no coverage) | `cargo test -p agent-memory --test rest` | ⬜ pending |
| Valid boundary values (limit=1, limit=200, ttl=1) still succeed | API-02 | — | unit | `cargo test -p agent-memory-core validation` | ⬜ pending |
| `rustling` no longer matches `tag=rust` on all 3 predicate sites (knn, list, keyword-fallback) | API-03 | T-02G-02 regression | unit/integration per site | `cargo test -p agent-memory-core --test fallback --test semantic --test store` | ⬜ pending |
| NULL tag still disables the filter on all 3 sites | API-03 | — | unit | same suites | ⬜ pending |
| Spike workflow: 4 legs (musl x2, msvc, gnu) run on ubuntu-latest with recorded verdict | DIST de-risk (Phase 6 input) | T-02-31 pattern | CI dispatch | `gh workflow run` + `gh run watch` | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements — cargo test framework, fallback/rest/store suites, and the 02-05 four-layer test template are already in place. No Wave 0 installs.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Spike verdict interpretation (which Windows toolchain Phase 6 adopts) | DIST-03 input | Verdict is a human-recorded decision from CI logs, not an assertion | Read the 4 leg outcomes in the dispatched run; record verdict in SUMMARY + workflow comment |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 180s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
