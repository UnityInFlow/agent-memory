# Project Retrospective

*A living document updated after each milestone. Lessons feed forward into future planning.*

## Milestone: v1.0 — MVP

**Shipped:** 2026-07-12 (product release v0.0.1 live 2026-07-03; milestone closed after gap-closure + human sign-off)
**Phases:** 2 | **Plans:** 8 | **Tasks:** 19

### What Was Built
- MCP stdio memory server (store/search/list/forget) over WAL SQLite with FTS5, typed memories, exponential decay (never deletes), and TTL sweep — fully offline, zero cloud.
- Local Ollama semantic search (sqlite-vec sidecar, similarity×decay blend) with loud-but-graceful keyword fallback carrying the same filters; `search_mode` on every result.
- REST mirror (axum, loopback-guarded), idempotent GSD STATE.md import, and a public v0.0.1 release: 4 checksummed target tarballs + Homebrew tap, built via orangepi zigbuild.

### What Worked
- Walking-skeleton-first vertical MVP slices: a usable no-Ollama tool existed after Phase 1; Phase 2 was pure enhancement — no retrofit of stdout purity, writer lane, or decay semantics.
- Plan-time STRIDE threat models in every plan meant the milestone security gate closed with a short-circuit (25/25, no auditor spawn).
- The verify → gap-closure loop: 02-VERIFICATION.md caught 2 real defects (dropped tag filter in fallback, 500-for-bad-input); plan 02-05 closed both in 8 minutes with regression tests at 4 layers.
- Front-loading cross-compile risk: the darwin zigbuild canary in 02-01 and the spike gate before tag push in 02-04 made the release pipeline boring.

### What Was Inefficient
- Phase 02-04 (release) took 1h46m vs ~10-40min for other plans — serial orangepi builds (Hetzner fleet offline, ecosystem-wide) plus musl/darwin cross-compile discovery on the critical path.
- STATE.md drifted twice (status "executing" while artifacts showed complete; "Last Activity Description" field missing for gsd-tools) — small manual reconciliations at close.
- The optional cross-arch install check sat as verification debt for 9 days before being explicitly waived; deciding its optionality at plan time would have avoided the carry.

### Patterns Established
- Decay never deletes — TTL sweep and explicit forget are the only removal paths, locked by kill-tests.
- Shared serde envelopes (SearchOutcome) and shared error taxonomies (two-tier client/internal) keep MCP and REST wire-compatible from one definition.
- Coverage-bearing tests run in-process (SIGKILL'd spawned binaries flush no LLVM profile data); spawned-binary e2e tests exist for realism, not coverage.
- chrono UTC-only (`default-features=false, features=["now"]`) as the price of zig darwin cross-compiles — Local time never reintroduced.

### Key Lessons
- Verify with live execution, not SUMMARY claims — both gaps were invisible at the summary level and trivially visible under a dead-embedder test.
- Pin the whole native-lib graph together (`rusqlite`/`libsqlite3-sys`/`r2d2_sqlite`) before writing code; `links =` conflicts are cheaper to avoid than to unwind.
- musl is not "Linux for free": vendored C (sqlite-vec) with BSD typedefs breaks it — declare per-libc support explicitly in DIST requirements next time.

### Cost Observations
- Sessions: ~8 plan executions across 2026-06-24 → 2026-07-12 (18 days wall clock; ~1.4h summed plan execution for the instrumented plans).
- Notable: gap-closure plan (02-05) was the cheapest and highest-leverage — 8 minutes to flip verification from 13/15 to 15/15.

## Cross-Milestone Trends

| Milestone | Phases | Plans | Wall clock | Verification score at first pass |
|-----------|--------|-------|------------|----------------------------------|
| v1.0 MVP | 2 | 8 | 18 days | 13/15 → 15/15 after one gap-closure plan |
