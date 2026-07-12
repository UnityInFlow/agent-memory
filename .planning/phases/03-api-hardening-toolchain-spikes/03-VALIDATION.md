---
phase: 3
slug: api-hardening-toolchain-spikes
status: ready
nyquist_compliant: true
wave_0_complete: true
created: 2026-07-12
refined: 2026-07-12
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Derived from 03-RESEARCH.md "## Validation Architecture" — per-task map refined by the planner (plans 03-01, 03-02).

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) + existing integration suites (fallback.rs, rest.rs, store.rs, semantic.rs) |
| **Config file** | Cargo.toml (workspace) — no Wave 0 install needed |
| **Quick run command** | `cargo test -p agent-memory-core` |
| **Full suite command** | `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check` |
| **Estimated runtime** | ~30-60 seconds (quick), ~2-3 min (full) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p <touched crate>` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --check`
- **After every plan wave:** Run the full suite command
- **Before `/gsd-verify-work`:** Full suite + `cargo llvm-cov --workspace --fail-under-lines 80` green
- **Max feedback latency:** 180 seconds (spike run watch in 03-02 is CI wall-clock, exempt — dispatch feedback is per-leg via `gh run watch`)

---

## Per-Task Verification Map

| Behavior | Requirement | Threat Ref | Test Type | Automated Command | Status |
|----------|-------------|------------|-----------|-------------------|--------|
| Out-of-bounds limit → InvalidArgument at the seam (search + list): `out_of_bounds_limit_rejected_at_service_seam` | API-02 | T-03-01 | integration (core, plan 03-01 T1) | `cargo test -p agent-memory-core --test validation` | ⬜ pending |
| Out-of-bounds ttl_secs (0, negative, overflow extremes) → InvalidArgument on store AND import: `out_of_bounds_ttl_rejected_on_store_and_import` | API-02 | T-03-01 | integration (core, plan 03-01 T1) | `cargo test -p agent-memory-core --test validation` | ⬜ pending |
| Valid boundaries succeed (limit 1/200/None; ttl 1/3_155_760_000/None): `valid_limit_boundaries_succeed`, `valid_ttl_boundaries_succeed` | API-02 | — | integration (core, plan 03-01 T1) | `cargo test -p agent-memory-core --test validation` | ⬜ pending |
| REST 400 (never 500) for out-of-bounds inputs, in-process: `search_with_zero_limit_returns_400_never_500`, `store_with_out_of_bounds_ttl_returns_400_never_500`, `list_with_negative_limit_returns_400_never_500` | API-02 | T-03-02/T-03-04 | in-process handler (coverage-bearing, plan 03-01 T1) | `cargo test -p agent-memory --lib` | ⬜ pending |
| MCP invalid_params tier for InvalidArgument: extended `map_mcp_error_splits_client_and_internal_tiers` | API-02 | T-03-02 | unit (mcp, plan 03-01 T1) | `cargo test -p agent-memory --lib` | ⬜ pending |
| Real-HTTP 400s on the spawned binary (limit 0, ttl -1, limit 201) | API-02 | T-03-04 | e2e (realism-only, zero coverage, plan 03-01 T3) | `cargo test -p agent-memory --test rest` | ⬜ pending |
| `rustling` no longer matches `tag=rust` per site: `tag_filter_is_exact_on_knn_path` (semantic), `keyword_fallback_tag_filter_is_exact` (fallback), `list_tag_filter_is_exact_and_case_sensitive` (store) — each also locks `Rust` ≠ `rust` (D-08) | API-03 | T-03-03 (T-02G-02 regression) | integration per site (plan 03-01 T2) | `cargo test -p agent-memory-core --test semantic --test fallback --test store` | ⬜ pending |
| NULL tag still disables the filter on all 3 sites (asserted inside the three exact-tag tests) | API-03 | — | integration (plan 03-01 T2) | same suites | ⬜ pending |
| CI revived: push-triggered run on ubuntu-latest goes green | D-12 | T-03-05 | CI (plan 03-02 T2) | `gh run list --workflow=ci.yml --limit 1 --json conclusion -q '.[0].conclusion'` == success | ⬜ pending |
| Spike: 4 legs (musl ×2, msvc, gnu) run on ubuntu-latest, all conclusions recorded | DIST de-risk (Phase 6 input) | T-03-05/T-03-06 | CI dispatch (plan 03-02 T2/T3) | `gh workflow run spike-cross-compile.yml` + `gh run watch` + `gh run view --json jobs` | ⬜ pending |
| Verdict persisted for Phase 6 (workflow comment + SUMMARY) | DIST de-risk | T-03-07 | grep (plan 03-02 T3) | `grep -q "VERDICT (Phase 3, D-11" .github/workflows/spike-cross-compile.yml` | ⬜ pending |
| Phase coverage gate ≥80% lines (in-process layers carry the new branches) | CLAUDE.md gate | — | coverage (plan 03-01 T3) | `cargo llvm-cov --workspace --fail-under-lines 80` | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements — cargo test framework, fallback/rest/store/semantic suites, the 02-05 four-layer test template, and an authenticated `gh` CLI are already in place. No Wave 0 installs. (The spike workflow rewrite is plan 03-02 Task 2 — it is the deliverable, not a test-infra gap.)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Spike verdict interpretation (which Windows toolchain Phase 6 adopts) | DIST-03 input | The GREEN/RED → adoption mapping follows the pre-approved ladder (msvc → gnu → document-and-defer) applied to CI log evidence, not an assertion | Plan 03-02 T3: read the 4 leg conclusions via `gh run view --json jobs`; record verdict in the workflow comment + SUMMARY |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references (none — no gaps)
- [x] No watch-mode flags
- [x] Feedback latency < 180s (local suites; CI dispatch exempt by design)
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** planner, 2026-07-12 (plans 03-01, 03-02)
