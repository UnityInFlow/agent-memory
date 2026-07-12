---
phase: 01-core-memory-foundation
plan: 03
subsystem: lifecycle-ttl-decay-ship
tags: [rust, sqlite, ttl, decay, sweep, tokio-interval, ci, llvm-cov, self-hosted-runners, readme]

# Dependency graph
requires:
  - 01-01 walking skeleton (Store trait, SqliteStore writer lane, MemoryService spawn_blocking, FTS5 mirror + triggers, expires_at/decay_score columns, injectable Clock/TestClock)
  - 01-02 search + forget + on-read decay (decay_score(), DecayConfig, apply_decay, registered exp() scalar fn, pinned half-life CASE)
provides:
  - Store::sweep_expired (writer-lane DELETE WHERE expires_at IS NOT NULL AND expires_at < now — the ONLY sweep delete)
  - Store::materialize_decay (writer-lane UPDATE-only decay_score recompute, same exp/half-life math as on-read search)
  - decay.rs DecayEngine + SweepReport — orchestrates TTL-delete THEN re-score; structurally enforces decay-never-deletes (STORE-04)
  - MemoryService::sweep(now) over spawn_blocking — deterministic under TestClock, exposed for the bg task + tests/ttl.rs
  - background tokio::time::interval sweep task spawned before serve(stdio()), stderr-only SweepReport logging
  - .github/workflows/ci.yml — self-hosted matrix [arc-runner-unityinflow, orangepi], fmt/clippy/build/test + llvm-cov --fail-under-lines 80
  - README.md — problem statement, .mcp.json cross-runtime sharing, the four tools, offline/no-Ollama v0.0.1 boundary
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Two-phase sweep enforced by code structure: sweep_expired (only delete) THEN materialize_decay (UPDATE-only) — decay physically cannot remove a row (STORE-04 / Pitfall 7)"
    - "materialize_decay UPDATE uses the SAME inline exp/half-life/CASE math as the on-read search ORDER BY, so the materialized column == the recompute-on-read value bit-for-bit"
    - "detached tokio::time::interval lifecycle task: first tick immediate, errors logged-and-continue (a transient DB hiccup never kills the loop), SweepReport to stderr only"

key-files:
  created:
    - .github/workflows/ci.yml
    - README.md
  modified:
    - crates/agent-memory-core/src/store/mod.rs
    - crates/agent-memory-core/src/store/sqlite.rs
    - crates/agent-memory-core/src/decay.rs
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory-core/tests/ttl.rs
    - crates/agent-memory/src/main.rs

key-decisions:
  - "materialize_decay is a single bulk UPDATE (no per-row iteration) computing decay_score inline with the registered exp() fn + pinned-half-life CASE — matches the search query's math exactly so on-read and materialized values agree (STORE-03), and is one short writer transaction per sweep (T-03-03 writer-contention mitigation)"
  - "Sweep order is TTL-delete FIRST, then re-score survivors — the survivor set is final before materialization and no work is wasted re-scoring rows about to vanish"
  - "Hourly default sweep interval (SWEEP_INTERVAL = 3600s): TTL is coarse and search recomputes decay on read regardless, so a tighter interval buys nothing while adding writer churn"
  - "cargo-llvm-cov is NOT installed on the local toolchain; per execution guidance the >80% coverage gate is wired into ci.yml (cargo install + cargo llvm-cov --fail-under-lines 80) and documented in the README as a CI-enforced gate rather than blocking the plan on a long local install"

requirements-completed: [STORE-04, STORE-03]

# Metrics
duration: 18min
completed: 2026-06-25
---

# Phase 1 Plan 03: TTL Sweep + Decay Materialization + Ship Gate Summary

**A background `tokio::time::interval` sweep that deletes TTL-expired rows then materializes every survivor's decay score (decay NEVER deletes — only TTL and `memory_forget` remove), wired before `serve(stdio())` with stderr-only logging, plus the self-hosted-runner CI workflow (fmt/clippy/test + >80% llvm-cov gate) and a README — completing and shipping the Phase-1 memory foundation.**

## Performance

- **Duration:** ~18 min
- **Completed:** 2026-06-25
- **Tasks:** 2 (Task 1 `tdd`; Task 2 `auto`)
- **Files modified/created:** 8 (2 created, 6 modified)

## Accomplishments

- **TTL sweep + decay materialization engine.** `Store` gained `sweep_expired` (writer-lane `DELETE … WHERE expires_at IS NOT NULL AND expires_at < ?1`, the ONLY sweep delete) and `materialize_decay` (writer-lane UPDATE-only decay recompute). `decay.rs` gained `DecayEngine` + `SweepReport` orchestrating **TTL-delete FIRST, then re-score** — the two-phase structure is what enforces the STORE-04 invariant: decay materialization is physically an UPDATE and cannot remove a row.
- **`MemoryService::sweep(now)`** wraps `DecayEngine::sweep` in `spawn_blocking`, driven by the injected clock so it is fully deterministic under `TestClock`, and exposed for both the background task and the test suite.
- **Background lifecycle task.** `main.rs` spawns a detached `tokio::time::interval` loop (hourly) **before** `serve(stdio())`; each tick runs the sweep and logs the `SweepReport` to **stderr** via `tracing` (never stdout) — proven by the still-green `stdio_purity.rs` running with the sweep active. A sweep error is logged and the loop continues.
- **`materialize_decay == on-read recompute`.** The UPDATE uses the same `exp()` + half-life + pinned-`CASE` math as the search `ORDER BY`, so the materialized `decay_score` column matches the recompute-on-read value exactly (asserted to `< 1e-9` in `tests/ttl.rs`) — STORE-03 holds materialized and on-read.
- **`tests/ttl.rs` (STORE-04, 4 tests):** (1) sweep removes a TTL-expired row from both list and search (FTS5 mirror synced via the delete trigger) while the no-TTL row survives; (2) a no-TTL memory aged 20 half-lives (decay ≈ 1e-6) is STILL listable and searchable after the sweep — decay ≠ delete; (3) materialized decay matches on-read recompute and pinned > unpinned at equal age; (4) `sweep_expired` deletes only `expires_at < now` rows and never touches NULL-expires rows.
- **Ship gate:** `.github/workflows/ci.yml` — `strategy.matrix.runner: [arc-runner-unityinflow, orangepi]`, `runs-on: ${{ matrix.runner }}` (never `ubuntu-latest`), steps fmt-check → clippy `-D warnings` → build → test → `cargo install cargo-llvm-cov` → `cargo llvm-cov --workspace --fail-under-lines 80`.
- **`README.md`:** problem statement (persistent cross-runtime typed memory, local/offline/no-cloud), install/build/run, the `.mcp.json` stdio config sharing one DB via `AGENT_MEMORY_DB` (the cross-runtime headline), the four tools with example args, the TTL+decay lifecycle (decay never deletes), and an explicit "v0.0.1 is keyword-only — works fully offline, no Ollama" boundary note.

## Task Commits

1. **Task 1 (tdd): TTL sweep + decay materialization engine (decay never deletes) + STORE-04 test** — `f97db43` (feat). TDD RED demonstrated: `tests/ttl.rs` written first, failed to compile (`no method named sweep found for MemoryService`), then GREEN after the trait/engine/service additions landed.
2. **Task 2 (auto): background sweep task + self-hosted CI + coverage gate + README** — `a466b78` (feat).

**Plan metadata:** _(this docs commit)_

## Files Created/Modified

- `crates/agent-memory-core/src/store/mod.rs` — `Store` trait: add `sweep_expired` + `materialize_decay` with the STORE-04 contract documented in the doc comments.
- `crates/agent-memory-core/src/store/sqlite.rs` — writer-lane `sweep_expired` (bound TTL DELETE) + `materialize_decay` (bulk UPDATE, inline exp/half-life/pinned-CASE).
- `crates/agent-memory-core/src/decay.rs` — `DecayEngine { cfg }` + `SweepReport { expired, rescored }`; `sweep` runs delete-then-rescore.
- `crates/agent-memory-core/src/service.rs` — `async fn sweep(now)` over `spawn_blocking`.
- `crates/agent-memory-core/tests/ttl.rs` — the four STORE-04 behaviors (new).
- `crates/agent-memory/src/main.rs` — `SWEEP_INTERVAL` + `spawn_sweep_task`, spawned before `serve(stdio())`, stderr-only logging.
- `.github/workflows/ci.yml` — self-hosted matrix CI + llvm-cov gate (new).
- `README.md` — tool surface, `.mcp.json`, lifecycle, offline boundary (new).

## Decisions Made

- **Bulk SQL UPDATE for `materialize_decay`, not per-row iteration.** A single `UPDATE memories SET decay_score = <inline exp/half-life/CASE>` recomputes every survivor in one short writer transaction. It reuses the exact math of the search `ORDER BY`, so the materialized column equals the on-read recompute (STORE-03), and keeps the sweep to one transaction per interval (writer-contention threat T-03-03 mitigated).
- **Sweep order = TTL-delete first, then re-score.** The survivor set is finalized before materialization; no effort is spent scoring rows that are about to be deleted.
- **Hourly sweep interval.** TTL granularity is coarse and `memory_search` already recomputes decay on read, so a tight interval would only add writer churn for no ranking benefit.
- **Coverage gate is CI-enforced, not run locally.** `cargo-llvm-cov` is not installed on the local toolchain. Per the execution guidance (do not block on a long `cargo install`), the `--fail-under-lines 80` gate is wired into `ci.yml` and documented in the README; the local command is recorded for when the tool is present.

## Deviations from Plan

None — both tasks executed exactly as written. No auto-fixes (Rules 1–3) were needed; the build, the four ttl behaviors, clippy, and fmt all passed on the first GREEN run after the engine landed. The plan's signatures (`sweep_expired`, `materialize_decay`, `DecayEngine::sweep`, `SweepReport`, `MemoryService::sweep`, the interval task, the CI matrix, the README contents) were implemented verbatim.

## Threat Model Compliance

- **T-03-01 (sweep DELETE scope / data loss):** `sweep_expired` is the only delete and is bounded `WHERE expires_at IS NOT NULL AND expires_at < ?1` (parameterized). `materialize_decay` is structurally UPDATE-only. `tests/ttl.rs` asserts a near-zero-decay no-TTL row survives and that NULL-expires rows are never touched — STORE-04 proven by test.
- **T-03-02 (sweep log → stdout corruption):** `SweepReport` is logged via `tracing` to stderr only; `stdio_purity.rs` runs green with the background sweep active, proving stdout stays pure (MCP-05).
- **T-03-03 (writer contention):** the sweep is one short bulk-UPDATE + one bulk-DELETE per interval on the single serialized writer lane (WAL + busy_timeout from Plan 01), not per-row locking.
- **T-03-SC (CI toolchain):** `cargo install cargo-llvm-cov` is a pre-audited dev tool; CI pins `dtolnay/rust-toolchain@stable` and runs only on the org self-hosted runners (`arc-runner-unityinflow`/`orangepi`) — never `ubuntu-latest`.

## Known Stubs

None. The TTL sweep, decay materialization, background interval task, CI workflow, and README are all fully realized and verified. Semantic search (Ollama embeddings), GSD STATE.md import, TTL/REST extras, and Homebrew distribution are **Phase-2 / later-milestone** scope per the tool spec — explicitly out of Phase 1's foundation, not stubs in this plan.

## User Setup Required

None — the server creates its own SQLite DB on first run; no external service, no network, no Ollama in v0.0.1.

## Next Phase Readiness

- **Phase 1 (core memory foundation) is complete: 3/3 plans shipped.** The four-tool MCP surface (store/search/list/forget) plus the full lifecycle (TTL expiry + decay materialization, decay-never-deletes) is implemented and tested over real stdio with a self-hosted-runner CI gate.
- Phase 2 inherits: the materialized `decay_score` column + the sweep cadence (a place to also refresh embeddings), the `expires_at` TTL path, the `DecayEngine`/`SweepReport` seam, and the CI workflow (extend with an embeddings/Ollama job).
- **Carry-forward for release:** the `>80% llvm-cov` gate is CI-enforced but was not run locally (cargo-llvm-cov not installed) — the first CI run on the self-hosted runners is its first real execution; `CONTRIBUTING.md` and `LICENSE` (MIT) are the remaining pre-v0.0.1 ecosystem-required files (README landed here). The recurring Hetzner-X64-fleet-offline risk applies to this repo's CI too — the `orangepi` matrix leg is the documented fallback.

## Self-Check: PASSED

Both created files (`.github/workflows/ci.yml`, `README.md`) exist on disk; both task commits (`f97db43`, `a466b78`) are present in git history. Full workspace verification green: `cargo build --workspace`, `cargo test --workspace` (32 tests across 8 suites — core lib + decay 4 + store 4 + **ttl 4** + bin-unit + stdio_purity 1 + tools 4), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` all clean. No `unwrap()`/`expect()` in production code in any file this plan touched (all such occurrences live in `#[cfg(test)]` modules). The coverage gate is wired into CI (cargo-llvm-cov absent locally — CI-enforced as documented).

---
*Phase: 01-core-memory-foundation*
*Completed: 2026-06-25*
