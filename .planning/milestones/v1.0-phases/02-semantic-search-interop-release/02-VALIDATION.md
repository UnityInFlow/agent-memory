---
phase: 2
slug: semantic-search-interop-release
status: planned
nyquist_compliant: true
wave_0_complete: false
created: 2026-07-02
---

# Phase 2 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in) + tokio::test; binary integration via `CARGO_BIN_EXE_agent-memory` (Phase-1 pattern) |
| **Config file** | none — workspace Cargo.toml; `test-clock` feature auto-enabled via self dev-dependency (also gates FakeEmbedder) |
| **Quick run command** | `cargo test -p agent-memory-core` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30–60 seconds |

---

## Sampling Rate

- **After every task commit:** `cargo test -p agent-memory-core && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`
- **After every plan wave:** `cargo test --workspace`
- **Before `/gsd-verify-work`:** full suite green + `cargo llvm-cov --workspace --fail-under-lines 80`
- **Max feedback latency:** ~60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 2-01-01 | 01 | 1 | SEARCH-02/03 | T-02-01 | vec0 migration validates; extension registered pre-open; RED tests written | unit+integration | `cargo test -p agent-memory-core --lib --test store --test ttl` | ❌ W0 (semantic.rs, fallback.rs) | ⬜ pending |
| 2-01-02 | 01 | 1 | SEARCH-02/03 | T-02-01/03/05 | KNN fully parameterized; fallback never empty-on-outage; vec sync on forget/sweep | integration | `cargo test -p agent-memory-core --test semantic --test fallback` | ❌ W0 | ⬜ pending |
| 2-01-03 | 01 | 1 | SEARCH-03, MCP-05 | T-02-02 | stdout pure with dead Ollama URL; probe never blocks startup | integration | `cargo test -p agent-memory --test stdio_purity` | ✅ (extend) | ⬜ pending |
| 2-02-01 | 02 | 2 | API-01 | T-02-11 | REST contract as failing e2e test (400 invalid type, 404 not-found) | integration (RED) | `cargo test -p agent-memory --test rest` | ❌ W0 (rest.rs) | ⬜ pending |
| 2-02-02 | 02 | 2 | API-01 | T-02-10/11/12 | loopback-only default bind; typed DTOs; zero SQL in handlers | integration | `cargo test -p agent-memory --test rest && cargo test --workspace` | ❌ W0 | ⬜ pending |
| 2-03-01 | 03 | 3 | INTEROP-01 | T-02-20/22 | tolerant parse (fence trap, malformed skip-count, 8KiB cap); RED e2e | unit+integration (RED) | `cargo test -p agent-memory-core --lib` | ❌ W0 (import.rs + fixture) | ⬜ pending |
| 2-03-02 | 03 | 3 | INTEROP-01 | T-02-20/23 | parameterized dedup; idempotent re-run via real binary | integration+CLI | `cargo test -p agent-memory-core --test import` + double-run CLI check | ❌ W0 | ⬜ pending |
| 2-04-01 | 04 | 4 | DIST-01 | T-02-31 | pinned toolchain versions; no ubuntu-latest; musl-only continue-on-error | grep gates + full local suite | see plan verify (grep + cargo gates) | n/a | ⬜ pending |
| 2-04-02 | 04 | 4 | DIST-01 | T-02-30/32/33 | spike green pre-tag; asset validation; local checksum + exec proof | CLI (gh + shasum) | `gh release view v0.0.1 --json assets` asset assertions | n/a (needs orangepi runner) | ⬜ pending |
| 2-04-03 | 04 | 4 | DIST-02 | T-02-30 | formula sha256 pinned from SHA256SUMS; real brew install | CLI (brew) | `brew install unityinflow/tap/agent-memory && brew test agent-memory` | n/a (needs published release) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/agent-memory-core/src/embed/mod.rs` — `FakeEmbedder` deterministic test double (plan 02-01 task 1)
- [ ] `crates/agent-memory-core/tests/semantic.rs` — SEARCH-02 (incl. one `#[ignore]`d live-Ollama test)
- [ ] `crates/agent-memory-core/tests/fallback.rs` — SEARCH-03 kill-test
- [ ] `crates/agent-memory/tests/rest.rs` — API-01
- [ ] `crates/agent-memory-core/tests/import.rs` + `tests/fixtures/STATE.md` — INTEROP-01
- [ ] `.github/workflows/spike-cross-compile.yml` — DIST-01 de-risk (created plan 02-01; dispatched plan 02-04)
- Framework install: none — cargo test is built-in

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Live semantic ranking against real Ollama | SEARCH-02 | CI runners may lack Ollama; deterministic FakeEmbedder is the CI gate | `cargo test -p agent-memory-core --test semantic -- --ignored` (dev machine, Ollama + nomic-embed-text running) |
| 6-triple cross-compile | DIST-01 | needs orangepi runner's zig toolchain | dispatch `spike-cross-compile.yml`, all 4 required legs green |
| brew install on non-arm64-darwin hosts | DIST-01/02 | this Mac executes only aarch64-apple-darwin | end-of-phase human check: install + `--version` on a second machine (plan 02-04 task 3 human-check) |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 60s (release-plan gh/brew steps excepted — externally bounded)
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** planned 2026-07-02 (gsd-planner)
