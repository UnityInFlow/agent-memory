---
phase: 03-api-hardening-toolchain-spikes
plan: 02
subsystem: ci
tags: [github-actions, cargo-zigbuild, cargo-xwin, mingw-w64, musl, windows, cross-compile, sqlite-vec]

# Dependency graph
requires:
  - phase: 02 (v1.0 milestone)
    provides: v1.0 spike workflow's pinned-version + host-arch-aware-smoke discipline (zig official-tarball install block reused verbatim); the sqlite-vec BSD u_int*_t typedef diagnosis behind the musl CFLAGS shim
provides:
  - Live CI gate on this public repo -- ci.yml on ubuntu-latest, secretless, contents: read (D-12)
  - Hosted 4-leg cross-compile spike workflow (workflow_dispatch, fail-fast: false) proving Windows + musl feasibility
  - Recorded D-11 toolchain verdict (workflow header comment + this SUMMARY) -- Phase 6's toolchain decision input
affects: [phase-6 DIST-03 windows binary, phase-6 DIST-05 musl binaries, phase-6 release.yml runner decision, all phase 3+ CI coverage]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - GitHub-hosted secretless CI (permissions contents: read) as the sanctioned D-02 exception on this public repo -- OPS-02 makes self-hosted unreachable here
    - target-suffixed CFLAGS_<triple-with-underscores> env shims for per-target C flags -- never bare CFLAGS (macros must not bleed across legs)
    - tool-discriminated matrix include ({target, tool}) with per-tool conditional install steps and a case-dispatch build step

key-files:
  created: []
  modified:
    - .github/workflows/ci.yml
    - .github/workflows/spike-cross-compile.yml

key-decisions:
  - "ci.yml migration kept every step byte-identical -- only runner targeting, permissions, and the policy comment changed (D-12's trivially-cheap scope)"
  - "spike-cross-compile.yml rewritten in place (discretion resolved per RESEARCH): one file, one dispatch = full verdict; git history preserves the v1.0 darwin shape"
  - "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER set unconditionally on the Build step (cargo only reads it for the windows-gnu target) -- simpler than matrix-conditional env, zero risk"
  - "Phase 6 Windows leg: cargo-xwin 0.23.0 / x86_64-pc-windows-msvc adopted (first ladder rung, msvc GREEN); mingw-w64/gnu is a proven fallback, not needed"

patterns-established:
  - "Runner policy for this public repo: ubuntu-latest + contents: read, documented in-file with D-10/OPS-02/D-02 citations -- supersedes the inner CLAUDE.md's 'never ubuntu-latest'"
  - "Spike verdicts live as workflow-header comments citing the concrete run URL + date (auditable, D-11/T-03-07)"

requirements-completed: []
requirements-informed: [DIST-03, DIST-05]

# Metrics
duration: 30min
completed: 2026-07-12
---

# Phase 3 Plan 02: Toolchain Spikes + CI Revival Summary

**CI is verifiably alive again on this public repo (ubuntu-latest, secretless, green run on push), and the 4-leg Windows+musl cross-compile spike ran green on all legs — Phase 6 starts with cargo-xwin/msvc adopted and the musl CFLAGS shim proven**

## The D-11 Toolchain Verdict (Phase 6 reads this)

Identical text lives in the `.github/workflows/spike-cross-compile.yml` header comment (D-11 requires both locations):

```
# VERDICT (Phase 3, D-11, recorded 2026-07-12, run https://github.com/UnityInFlow/agent-memory/actions/runs/29196226289):
#   x86_64-unknown-linux-musl (zigbuild): GREEN — cargo-zigbuild 0.23.0 + zig 0.14.1;
#     CFLAGS_x86_64_unknown_linux_musl='-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int32_t=uint32_t -Du_int64_t=uint64_t';
#     static binary executed `--version` on the x86_64 runner (strongest smoke signal)
#   aarch64-unknown-linux-musl (zigbuild): GREEN — same -Du_int*_t shim (aarch64-suffixed var); presence-only (foreign arch)
#   x86_64-pc-windows-msvc (xwin): GREEN — cargo-xwin 0.23.0 + rustup llvm-tools; `cargo xwin build`
#     compiled bundled sqlite3.c + sqlite-vec.c via clang-cl; agent-memory.exe present
#   x86_64-pc-windows-gnu (mingw): GREEN — apt gcc-mingw-w64-x86-64,
#     CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc; agent-memory.exe present
#   Phase 6: Windows leg = cargo-xwin 0.23.0 msvc (first rung of the D-11 ladder — msvc green,
#   gnu also green as proven fallback); musl legs = zigbuild 0.23.0 + CFLAGS -Du_int*_t shim: GREEN (both)
```

All 4 legs green in a **single dispatch** — no re-dispatch, no A1/A2/A3 fixes needed. The milestone's only feasibility unknown is answered on the ladder's first rung:

- **DIST-03 (Windows):** adopt `cargo-xwin 0.23.0` / `x86_64-pc-windows-msvc` (with `rustup component add llvm-tools`). The mingw-w64 `x86_64-pc-windows-gnu` leg is a proven fallback if msvc ever regresses.
- **DIST-05 (musl):** adopt `cargo-zigbuild 0.23.0` + zig `0.14.1` with the target-suffixed `-Du_int*_t` CFLAGS shim — both musl legs green; the x86_64-musl static binary actually executed `--version` on the runner. No DIST-05 blocker signal.

## What was delivered

### Task 1 — CI revival (D-12), commit `8b0337e`

`.github/workflows/ci.yml` migrated off the dead self-hosted matrix (`arc-runner-unityinflow`/`orangepi` — permanently unreachable since the repo went public under OPS-02 `allows_public_repositories: false`) to plain `runs-on: ubuntu-latest` with top-level `permissions: contents: read`. All six steps byte-identical (fmt → clippy `-D warnings` → build → test → `cargo install cargo-llvm-cov --locked` → `cargo llvm-cov --workspace --fail-under-lines 80`). Policy comment rewritten to cite D-10/D-12/OPS-02/D-02.

**Proven live:** the push-triggered run [29196120702](https://github.com/UnityInFlow/agent-memory/actions/runs/29196120702) completed `success` — the first real CI run since publication. The verdict-commit push re-confirmed it (run 29196396239, also `success`, including the ≥80% coverage gate).

### Task 2 — Spike workflow rewrite + dispatch (D-10/D-11), commit `bd45b87`

`.github/workflows/spike-cross-compile.yml` rewritten in place: `workflow_dispatch`-only, `ubuntu-latest`, `contents: read`, `fail-fast: false`, 4-leg `include` matrix with a `tool` discriminator (zigbuild ×2 / xwin / mingw), pinned versions (`ZIG_VERSION: '0.14.1'`, `CARGO_ZIGBUILD_VERSION: '0.23.0'`, `CARGO_XWIN_VERSION: '0.23.0'`), target-suffixed musl CFLAGS shims, per-tool conditional install steps (zig official tarball block carried over verbatim; xwin: llvm-tools + implicit MSVC license acceptance; mingw: apt package + explicit linker env), case-dispatch build step, and a suffix-aware smoke step (`agent-memory.exe` presence-only on Windows legs; x86_64-musl runs `--version`).

Pushed to main, CI revival verified green, then dispatched: run [29196226289](https://github.com/UnityInFlow/agent-memory/actions/runs/29196226289).

### Task 3 — Verdict recorded (D-11), commit `bf591ca`

Watched the dispatched run (no `--exit-status` — red legs would have been data, not failure). All 4 jobs concluded `success`. Placeholder header line replaced with the final VERDICT block (above); committed and pushed.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Removed a leftover "orangepi" mention from a smoke-step comment**
- **Found during:** Task 2 (automated verification)
- **Issue:** a carried-over comment said "the v1.0 orangepi pipeline", violating the acceptance grep `orangepi|arc-runner|apple-darwin` == 0
- **Fix:** reworded to "the v1.0 ARM64 self-hosted pipeline"
- **Files modified:** .github/workflows/spike-cross-compile.yml
- **Commit:** bd45b87 (fixed before the task commit)

**2. [Rule 3 - Blocking] Cancelled the stale pre-migration CI run**
- **Found during:** Task 2 (after push)
- **Issue:** run 29194998488 (commit `6d4219c`, pre-migration ci.yml) was queued forever against the unreachable self-hosted labels — it would eventually surface as a noisy timeout failure
- **Fix:** `gh run cancel 29194998488`
- **Files modified:** none

No other deviations — plan executed as written; one dispatch sufficed (zero re-dispatches of the budgeted one).

## Scope respected

No Rust/Cargo/source changes, no `release.yml` change (Phase 6 owns the release-side runner decision, D-12), no D-02 split rework beyond the runner swap. Cargo installs are CI-side only, pinned, `--locked` (threat register T-03-06/T-03-SC honored; both workflows secretless with `contents: read`, T-03-05).

## Verification results

1. `ci.yml`: `runs-on: ubuntu-latest` ×1; no `arc-runner|orangepi|matrix`; `contents: read` present; all six steps + `fail-under-lines 80` intact — PASS
2. Latest ci.yml run conclusion `success` — PASS (D-12 proven live twice)
3. Spike workflow on main: 4 legs, pinned versions, target-suffixed shims only (bare `CFLAGS:` count 0), `fail-fast: false`, `.exe`-aware smoke — PASS
4. Spike run `completed`, all 4 job conclusions `success` (none skipped/cancelled) — PASS
5. VERDICT block present with per-leg GREEN lines + one Phase 6 conclusion line; "pending" placeholder gone — PASS

## Commits

| Task | Commit | Message |
| ---- | ------ | ------- |
| 1 | `8b0337e` | ci: migrate CI to GitHub-hosted ubuntu-latest (D-12, ecosystem D-02 exception) |
| 2 | `bd45b87` | ci: rewrite cross-compile spike as hosted 4-leg windows+musl matrix (D-10/D-11) |
| 3 | `bf591ca` | docs(ci): record Phase 3 cross-compile spike verdict (D-11) |

## Self-Check: PASSED

All modified files present on disk; all three task commits (8b0337e, bd45b87, bf591ca) verified in git log.
