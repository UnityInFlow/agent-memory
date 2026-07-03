---
phase: 02-semantic-search-interop-release
plan: 04
subsystem: infra
tags: [rust, github-actions, cargo-zigbuild, zig, orangepi, homebrew, release, cross-compile, sha256]

# Dependency graph
requires:
  - phase: 02-01
    provides: spike-cross-compile.yml (Pitfall 1 gate workflow) + local darwin canary prior
  - phase: 02-02
    provides: REST surface documented in README (serve-rest, --allow-remote, 5 endpoints)
  - phase: 02-03
    provides: import subcommand documented in README; fully green workspace as release baseline
provides:
  - Public repo github.com/UnityInFlow/agent-memory (remote origin, main + tag v0.0.1 pushed)
  - .github/workflows/release.yml — tag-triggered orangepi zigbuild release (test+coverage gate → 6-triple matrix, musl best-effort only → DIST-01 asset validation + SHA256SUMS.txt + softprops/action-gh-release@v3)
  - GitHub Release v0.0.1 with 4 required tarballs (darwin arm64/x86_64, linux-gnu x86_64/aarch64) + SHA256SUMS.txt
  - LICENSE (MIT) + CONTRIBUTING.md (ecosystem pre-release requirement)
  - README covering the full Phase 2 surface (Install/brew first, semantic search + fallback, REST, GSD import)
  - UnityInFlow/homebrew-tap Formula/agent-memory.rb (per-arch url+sha256) — brew install unityinflow/tap/agent-memory proven on this Mac
  - In-process REST handler unit tests (coverage-bearing; spawned-binary tests flush no profile data)
affects: [phase-verification, future releases (v0.0.2+ reuse release.yml), homebrew-tap maintenance]

# Tech tracking
tech-stack:
  added: [cargo-zigbuild 0.23.0 (CI, pinned), zig 0.14.1 (CI, pinned), softprops/action-gh-release@v3, cargo-llvm-cov (local)]
  patterns: ["tar.gz-with-binary-at-root asset contract for Homebrew", "musl legs best-effort via matrix continue-on-error, DIST targets hard-fail", "coverage-bearing tests must run in-process — SIGKILL'd spawned binaries flush no LLVM profile data", "UTC-only chrono: default-features off, features=[now] — Local time is un-linkable on zig darwin cross"]

key-files:
  created:
    - .github/workflows/release.yml
    - LICENSE
    - CONTRIBUTING.md
  modified:
    - README.md
    - Cargo.toml
    - Cargo.lock
    - crates/agent-memory/src/rest/handlers.rs

key-decisions:
  - "chrono trimmed to default-features=false, features=[now]: the default clock feature pulls iana-time-zone → core-foundation-sys, whose -framework CoreFoundation zig cannot link without a macOS SDK. Codebase is UTC-only (one chrono call). Local time must never be reintroduced."
  - "v0.0.1 ships WITHOUT musl binaries: sqlite-vec.c uses BSD u_int8_t/u_int16_t/u_int64_t typedefs that musl headers lack — gnu covers Linux (musl legs were best-effort by plan). Deferred: upstream fix or CFLAGS shim in v2."
  - "Coverage gate is enforced by in-process handler tests: tests/rest.rs (spawned, SIGKILL'd binary) contributes 0% coverage by construction; the six direct handler tests are the coverage-bearing REST tests."
  - "Spike gate evaluated per-leg: the run-level conclusion is 'failure' (musl legs), but all 4 DIST-01-required legs were individually green — the plan's gate condition."

patterns-established:
  - "Release asset contract: agent-memory-<triple>.tar.gz with the single binary at archive root, SHA256SUMS.txt over all tarballs — the Homebrew formula consumes exactly this"
  - "Homebrew tap formula: on_macos/on_linux × Hardware::CPU.arm? with per-arch pinned sha256 from SHA256SUMS.txt (never placeholders)"

requirements-completed: [DIST-01, DIST-02]

# Metrics
duration: 1h 46m
completed: 2026-07-03
---

# Phase 02 Plan 04: Cross-Platform Release + Homebrew Tap Summary

**v0.0.1 publicly released: orangepi zigbuild pipeline shipped checksummed tarballs for all 4 DIST-01 triples after the darwin spike gate passed, and `brew install unityinflow/tap/agent-memory` installs a working MCP server on this Mac from the new formula**

## Performance

- **Duration:** 1h 46m (dominated by serial CI on the single ARM64 orangepi runner: 2 spike dispatches + CI + the 21m31s release run)
- **Started:** 2026-07-03T06:08:35Z
- **Completed:** 2026-07-03T07:55:06Z
- **Tasks:** 3
- **Files modified:** 7 (this repo) + 1 (homebrew-tap)

## Accomplishments

- **DIST-01 shipped:** GitHub Release v0.0.1 (https://github.com/UnityInFlow/agent-memory/releases/tag/v0.0.1) carries `agent-memory-{aarch64,x86_64}-apple-darwin.tar.gz`, `agent-memory-{aarch64,x86_64}-unknown-linux-gnu.tar.gz`, and `SHA256SUMS.txt` — built entirely on the org self-hosted orangepi runner via pinned zig 0.14.1 + cargo-zigbuild 0.23.0
- **DIST-02 proven end-to-end:** `Formula/agent-memory.rb` (tap commit `aa76440`) installs on this Mac; `$(brew --prefix)/bin/agent-memory --version` prints 0.0.1, `brew test agent-memory` passes, and the brewed binary answers an MCP `initialize` with a JSON-RPC frame on stdout
- **Pitfall 1 gate honored:** the darwin cross-compile spike (run 28643212871) was green on all 4 required triples BEFORE the tag was pushed (spike completed 07:01:29Z; tag pushed immediately after; release run 28644256471 started 07:02Z) — and the gate caught a real darwin blocker on its first dispatch
- **Local end-to-end proof (phase success criterion 5):** downloaded darwin-arm64 tarball checksum-verifies (`shasum -a 256 -c` OK), extracts, prints `agent-memory 0.0.1`, and answers an MCP initialize on stdout
- **Repo published:** `UnityInFlow/agent-memory` public with remote `origin`; pre-push secret scan clean; CI green on the tagged commit (including the >80% coverage gate)
- LICENSE (MIT) + CONTRIBUTING.md landed before the tag (ecosystem pre-release requirement); README now leads with Install (brew + binary table + checksum verification) and documents semantic search/fallback, the REST API with the loopback security posture, and GSD import

## Task Commits

Each task was committed atomically:

1. **Task 1: release.yml + LICENSE + CONTRIBUTING + README** - `15d9349` (feat)
2. **Task 2: publish, spike gate, tag, validate** - external actions + two deviation commits: `b5c0fec` (test), `3469054` (fix); tag `v0.0.1` on `3469054`
3. **Task 3: Homebrew tap formula + brew install proof** - `aa76440` in `UnityInFlow/homebrew-tap` (external repo; no changes in this repo)

**Plan metadata:** committed with this SUMMARY (docs)

## Files Created/Modified

- `.github/workflows/release.yml` - Tag-triggered release: test job (fmt/clippy/test/llvm-cov 80), 6-triple zigbuild matrix on `[orangepi]` (musl `continue-on-error: true` only), tar.gz packaging with binary at root, DIST-01 hard validation, SHA256SUMS.txt, softprops/action-gh-release@v3
- `LICENSE` - MIT, Copyright (c) 2026 Jiří Hermann
- `CONTRIBUTING.md` - Build/test/lint/coverage gates, commit conventions, no-unwrap + stdout-purity (MCP-05) rules, self-hosted CI note
- `README.md` - Keyword-only boundary note removed; new Install (brew first), Semantic search (optional Ollama), REST API (5 endpoints, loopback default, `--allow-remote` warning, never-expose note), Import from GSD sections; `.mcp.json` example kept
- `Cargo.toml` / `Cargo.lock` - chrono trimmed to `default-features = false, features = ["now"]` (darwin link fix)
- `crates/agent-memory/src/rest/handlers.rs` - Six in-process handler tests (tempdir store + dead-port embedder) restoring the coverage gate

## Release Record (plan output spec)

- **Spike run (gate, green):** https://github.com/UnityInFlow/agent-memory/actions/runs/28643212871 — required legs aarch64-apple-darwin / x86_64-apple-darwin / x86_64-unknown-linux-gnu / aarch64-unknown-linux-gnu all SUCCESS; musl pair failed (non-blocking, see decisions)
- **First spike run (caught the darwin blocker):** https://github.com/UnityInFlow/agent-memory/actions/runs/28642349482
- **Release run:** https://github.com/UnityInFlow/agent-memory/actions/runs/28644256471 (success, 21m31s)
- **Release:** https://github.com/UnityInFlow/agent-memory/releases/tag/v0.0.1
- **Formula sha256 values (from SHA256SUMS.txt):**
  - aarch64-apple-darwin: `b344ef40ce50361c641b0f66338163e4d8f6acef3a95c81257c5485d538d7405`
  - x86_64-apple-darwin: `5e58a9c4f7b2d4abf3944773a68cc162091bfd33d85e4608064bdbf4c7097bec`
  - aarch64-unknown-linux-gnu: `73d916d979a745ec4dfc2a6823350213687ae69313bf08f6a4ff20302d3cf08c`
  - x86_64-unknown-linux-gnu: `f06c4aaf3f7c36532cd51d530a8fd6872680918cf65469a5d984b6cd1d18487d`

## Decisions Made

- **chrono → `features = ["now"]` only** (see key-decisions): the darwin spike failure was a LINK failure (`unable to find framework 'CoreFoundation'`), NOT the bundled-C Pitfall-1 signature — sqlite3.c + sqlite-vec.c compiled clean under zig, so Plan B (dropping sqlite-vec) was correctly NOT triggered
- **musl deferred:** sqlite-vec.c's `u_int8_t`/`u_int16_t`/`u_int64_t` don't exist in musl headers; gnu binaries cover Linux for v0.0.1
- **Spike gate read per-leg**, matching the plan's rule "a musl-only failure does not block"
- Existing `UnityInFlow/homebrew-tap` repo reused (already existed since 2026-03); formula added on its default branch

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] CI coverage gate failed at 76.50% — added in-process REST handler tests**
- **Found during:** Task 2 (first-ever CI run after publishing the repo)
- **Issue:** `cargo llvm-cov --fail-under-lines 80` failed: `rest/handlers.rs` showed 0% coverage because `tests/rest.rs` exercises it via a spawned binary that the test harness SIGKILLs — a killed process never flushes LLVM profile data. The gate had never actually run before (no remote existed), and release.yml enforces the same gate, so tagging would have produced a failed release.
- **Fix:** Six direct handler tests (tempdir SQLite store, dead-loopback-port embedder → deterministic keyword mode) covering all 5 handlers, both error tiers, and type validation. Coverage: 76.50% → 84.82% lines locally; CI on the runner confirmed green.
- **Files modified:** crates/agent-memory/src/rest/handlers.rs
- **Verification:** `cargo llvm-cov --workspace --fail-under-lines 80` exit 0 locally; CI job `build-and-test (orangepi)` success on 3469054
- **Committed in:** b5c0fec

**2. [Rule 3 - Blocking] darwin zigbuild link failure — trimmed chrono default features**
- **Found during:** Task 2 (first spike dispatch, aarch64-apple-darwin leg)
- **Issue:** Link error `unable to find framework 'CoreFoundation'`: chrono's default `clock` feature (only needed for `Local` time) pulls `iana-time-zone` → `core-foundation-sys`, which emits `-framework CoreFoundation` — zig has no macOS SDK frameworks on the Linux runner. Not the bundled-C Pitfall-1 failure (the C objects compiled and were on the link line), so Plan B did not apply.
- **Fix:** `chrono = { version = "0.4", default-features = false, features = ["now"] }` — the codebase's single chrono call is `Utc::now().timestamp()`. Verified `cargo tree --target aarch64-apple-darwin -i core-foundation-sys` is empty; local zigbuild darwin canary green; re-dispatched spike green on all 4 required legs.
- **Files modified:** Cargo.toml, Cargo.lock
- **Verification:** spike run 28643212871 — both darwin legs SUCCESS on the orangepi runner
- **Committed in:** 3469054

---

**Total deviations:** 2 auto-fixed (2 × Rule 3 blocking)
**Impact on plan:** Both fixes were prerequisites for the release the plan orders; no scope creep. The spike gate worked exactly as designed — it caught the darwin blocker before any tag existed.

## Issues Encountered

- **musl legs fail (expected-tier):** sqlite-vec 0.1.9's C source uses BSD `u_int*_t` typedefs absent from musl headers. Best-effort per plan; v0.0.1 ships gnu-only for Linux. Logged as deferred (fix upstream or via a CFLAGS `-Du_int8_t=uint8_t` shim in v2).
- **Runner-group precondition already satisfied:** the org runner group has `allows_public_repositories: true`, so CI picked up the public repo immediately — the plan's PATCH/dashboard fallback was never needed.
- The `arc-runner-unityinflow` CI legs queue forever (Hetzner X64 fleet offline — known ecosystem carry-forward); the orangepi legs are the effective CI. Superseded/doomed runs were cancelled to unclog the serial runner.

## Authentication Gates

None — `gh` was already authenticated (`hermanngeorge15`) with repo/workflow/admin:org scopes.

## Known Stubs

None — all shipped artifacts are real and verified (no placeholder sha256 values, no REPLACE_ markers; `grep -c REPLACE_` = 0 on the formula).

## Threat Flags

None beyond the plan's threat model. Mitigations applied as registered: SHA256SUMS.txt published + formula pins per-arch sha256 (T-02-30); zig/cargo-zigbuild/action versions pinned (T-02-31); pre-push secret scan run before `gh repo create`, untracked local files (.mcp.json, CLAUDE.md, .claude/) left untracked (T-02-32); Release created only by the tag-triggered workflow's GITHUB_TOKEN (T-02-33).

## User Setup Required

None — the plan's single conditional dashboard task (enable `allows_public_repositories` on the runner group) never triggered: the group already allows public repositories.

**Optional second-machine check (end-of-phase human verify, from the plan):** on a Linux or Intel-Mac machine, run `brew install unityinflow/tap/agent-memory && agent-memory --version`, and confirm your MCP client launches the brewed binary from `.mcp.json` — this covers the arch legs this Mac cannot execute.

## Next Phase Readiness

- Phase 2 is complete: all 4 plans have SUMMARYs; v0.0.1 is publicly released and installable via brew
- Deferred for v2: musl static binaries (sqlite-vec u_int8_t), Windows (DIST-03, pre-existing), hybrid RRF (SEARCH-04), memory_update tools (MCP-06), portable export (DIST-04)
- `release.yml` is reusable as-is for v0.0.2+ (bump version, tag)

## Self-Check: PASSED

- Created files exist on disk: .github/workflows/release.yml, LICENSE, CONTRIBUTING.md (+ README modified)
- 3 task/deviation commits present: 15d9349, b5c0fec, 3469054; tag v0.0.1 on origin at 3469054
- Plan verify re-run green: `gh release view v0.0.1` lists exactly the 4 required tarballs + SHA256SUMS.txt; darwin-arm64 tarball checksum OK, `--version` prints 0.0.1, MCP initialize answered with JSON-RPC on stdout; `brew install unityinflow/tap/agent-memory` + `brew test agent-memory` + brewed-binary MCP smoke all PASS
- Local gates green at HEAD: 62 tests + 1 ignored, clippy -D warnings, fmt --check, llvm-cov ≥80%

---
*Phase: 02-semantic-search-interop-release*
*Completed: 2026-07-03*
