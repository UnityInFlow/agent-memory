---
phase: 1
slug: core-memory-foundation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-25
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` / `#[tokio::test]` + integration tests under `crates/*/tests/` |
| **Config file** | none (cargo standard); `cargo-llvm-cov` optional config in `Cargo.toml` |
| **Quick run command** | `cargo test -p agent-memory-core` |
| **Full suite command** | `cargo test --workspace` |
| **Coverage command** | `cargo llvm-cov --workspace --fail-under-lines 80` |
| **Estimated runtime** | ~30–60 seconds (workspace incl. stdio integration tests) |

---

## Sampling Rate

- **After every task commit:** `cargo test -p agent-memory-core` + `cargo clippy -- -D warnings` + `cargo fmt --check`
- **After every plan wave:** `cargo test --workspace` (includes piped-stdio integration tests)
- **Before `/gsd-verify-work`:** `cargo llvm-cov --workspace --fail-under-lines 80` green AND the stdout-purity test green
- **Max feedback latency:** ~60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 01-xx | TBD | 0 | — | — | `clock.rs` Clock+TestClock harness | unit | `cargo test -p agent-memory-core clock` | ❌ W0 | ⬜ pending |
| 01-xx | TBD | 1+ | STORE-01 | T-V5 | invalid type → `Err(InvalidType)`, no panic | unit | `cargo test -p agent-memory-core store` | ❌ W0 | ⬜ pending |
| 01-xx | TBD | 1+ | STORE-02 | — | N memories survive reopen, offline | integration | `cargo test -p agent-memory-core --test store` | ❌ W0 | ⬜ pending |
| 01-xx | TBD | 1+ | STORE-03 | — | decay(un-accessed) < decay(just-accessed), deterministic | unit | `cargo test -p agent-memory-core --test decay` | ❌ W0 | ⬜ pending |
| 01-xx | TBD | 1+ | STORE-04 | — | TTL sweep removes expired; low-decay+no-TTL still listed | integration | `cargo test -p agent-memory-core --test ttl` | ❌ W0 | ⬜ pending |
| 01-xx | TBD | 2+ | MCP-01 | T-V5 | `memory_store` returns id; omit scope → NULL | integration | `cargo test -p agent-memory --test tools` | ❌ W0 | ⬜ pending |
| 01-xx | TBD | 2+ | MCP-02 | — | FTS5 ranks sensibly; no-match → empty (not error) | integration | `cargo test -p agent-memory --test tools` | ❌ W0 | ⬜ pending |
| 01-xx | TBD | 2+ | MCP-03 | — | `memory_list` filters + limit cap | integration | `cargo test -p agent-memory --test tools` | ❌ W0 | ⬜ pending |
| 01-xx | TBD | 2+ | MCP-04 | — | `memory_forget` deletes; unknown id → clean not-found | integration | `cargo test -p agent-memory --test tools` | ❌ W0 | ⬜ pending |
| 01-xx | TBD | 2+ | MCP-05 | T-DoS | every stdout line is JSON-RPC; logs on stderr | integration (CI gate) | `cargo test -p agent-memory --test stdio_purity` | ❌ W0 | ⬜ pending |
| 01-xx | TBD | 2+ | SEARCH-01 | — | keyword search works, no localhost:11434 call | integration | `cargo test -p agent-memory --test tools` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/agent-memory-core/src/clock.rs` — `Clock` trait + `TestClock` (blocks STORE-03/04 determinism)
- [ ] `crates/agent-memory-core/tests/store.rs` — persistence + restart (STORE-01/02)
- [ ] `crates/agent-memory-core/tests/decay.rs` — deterministic decay curve + pinning (STORE-03, D-08)
- [ ] `crates/agent-memory-core/tests/ttl.rs` — sweep removes expired, decay≠delete (STORE-04)
- [ ] `crates/agent-memory/tests/stdio_purity.rs` — piped `initialize`, stdout-JSON-RPC-only assertion (MCP-05) — highest-priority gate
- [ ] `crates/agent-memory/tests/tools.rs` — four tools over piped JSON-RPC (MCP-01..04, SEARCH-01)
- [ ] `cargo install cargo-llvm-cov` on CI runner
- [ ] `.github/workflows/ci.yml` — `runs-on: [arc-runner-unityinflow]`: fmt-check, clippy -D warnings, test --workspace, llvm-cov fail-under 80, stdout-purity

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Real cross-runtime recall (store in Claude Code, recall in Cursor) | STORE-02 / MCP-* | Requires two real MCP clients | Configure both runtimes' `.mcp.json` at the same `AGENT_MEMORY_DB`; store in one, search in the other |

*All other phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have automated verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter (set by planner once tasks carry automated verify)

**Approval:** pending
