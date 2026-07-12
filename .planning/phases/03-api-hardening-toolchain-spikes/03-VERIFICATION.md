---
phase: 03-api-hardening-toolchain-spikes
verified: 2026-07-12T15:35:00Z
status: passed
score: 7/7 must-haves verified
overrides_applied: 0
---

# Phase 3: API Hardening + Toolchain Spikes Verification Report

**Phase Goal:** As an agent or REST client, I want invalid inputs rejected cleanly at one shared seam and tag filters to match tags exactly, so that bad requests never surface as server faults or over-matched results on any transport.
**Verified:** 2026-07-12T15:35:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

Merged from ROADMAP success criteria (SC1–SC4) + plan frontmatter must_haves (deduplicated).

| #   | Truth | Status | Evidence |
| --- | ----- | ------ | -------- |
| 1 | SC1: Out-of-bounds `limit`/`ttl_secs` (zero, negative, absurd extremes) → REST 400 / MCP `invalid_params` with descriptive message; never 500, never silent clamp; valid boundaries succeed | ✓ VERIFIED | `domain.rs:165-188` validate_limit/validate_ttl with D-05 messages; `.max(0)` clamp gone (grep = 0 in service.rs); boundary matrix incl. valid boundaries in `tests/validation.rs` (4 tests, ran green); 3 in-process handler 400 tests (handlers.rs:444/476/500, `search_with_zero_limit_returns_400_never_500` ran green); real-HTTP 400 assertions in `tests/rest.rs:234-264` |
| 2 | SC2: `tag=rust` matches only exactly-`rust`-tagged memories on all three query paths — `rustling` no longer matches; behavioral change recorded for v0.1.0 release notes | ✓ VERIFIED | 3 `FROM json_each(` EXISTS predicates at sqlite.rs:284 (knn ?5), 377 (list ?3), 425 (keyword ?10); `tags LIKE` count = 0; all 3 named regression tests (semantic.rs:244, fallback.rs:133, store.rs:141) ran green with rust/rustling/Rust fixture; README: "Behavioral changes" ×1, "exact tag" ×5, "tag substring" ×0 |
| 3 | SC3: Same invalid input → same client-error classification on MCP and REST because validation lives once at the `MemoryService` seam via `MemoryError::InvalidArgument` | ✓ VERIFIED | Validation only in service.rs method tops (store:120, search:146, list:174, import:274 — `crate::domain::validate_*`, no transport-side checks); both mappers exhaustive with named arms, `InvalidArgument` in the client tier (mcp.rs:36 → invalid_params; handlers.rs:91 → BadRequest); `map_mcp_error_splits_client_and_internal_tiers` asserts InvalidArgument code == invalid_params code, ran green |
| 4 | SC4: Windows (xwin/msvc vs mingw/gnu) and musl CFLAGS-shim cross-compile spikes ran in CI with a recorded toolchain verdict | ✓ VERIFIED | VERDICT block in spike-cross-compile.yml:15-25 (per-leg GREEN + Phase 6 conclusion: cargo-xwin 0.23.0 msvc adopted, gnu proven fallback, both musl legs green); run 29196226289 independently confirmed via `gh run view`: status completed, all 4 jobs conclusion `success`; no "pending" placeholder remains |
| 5 | 03-01 extra: `import()` validates each draft's ttl_secs through the same shared helper (bypass closed for Phase 5 importer) | ✓ VERIFIED | service.rs:274 per-draft `crate::domain::validate_ttl(draft.ttl_secs)?`; `out_of_bounds_ttl_rejected_on_store_and_import` covers the import path explicitly, ran green |
| 6 | 03-02 extra: CI runs on this public repo again — push to main triggers ci.yml on ubuntu-latest and completes fmt→clippy→build→test→coverage | ✓ VERIFIED | ci.yml: `runs-on: ubuntu-latest`, `permissions: contents: read`, all six steps intact incl. `fail-under-lines 80`; zero `arc-runner|orangepi|matrix`; `gh run list --workflow=ci.yml`: latest two main runs (29196632534, 29196396239) both conclusion `success` |
| 7 | 03-02 extra: Spike workflow dispatchable on ubuntu-latest with 4 matrix legs, fail-fast: false, secretless, target-suffixed CFLAGS shims (never bare CFLAGS) | ✓ VERIFIED | spike-cross-compile.yml: `workflow_dispatch` only, `contents: read`, `fail-fast: false`, 4-leg include matrix (2× musl zigbuild / msvc xwin / gnu mingw), pinned ZIG 0.14.1 / zigbuild 0.23.0 / xwin 0.23.0, `CFLAGS_x86_64_unknown_linux_musl` + aarch64 variant (no bare CFLAGS), `.exe`-aware smoke step |

**Score:** 7/7 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
| -------- | -------- | ------ | ------- |
| `crates/agent-memory-core/src/domain.rs` | InvalidArgument variant + 4 pub bounds consts + validate helpers | ✓ VERIFIED | Lines 117-131 consts (MIN/MAX_LIMIT, MIN/MAX_TTL_SECS=3_155_760_000), 143-144 variant, 165-188 helpers; substantive, wired (called from service.rs) |
| `crates/agent-memory-core/src/service.rs` | Validation at tops of store/search/list/import; clamp removed | ✓ VERIFIED | 4 call sites (120/146/174/274); `max(0)` grep = 0 |
| `crates/agent-memory-core/src/store/sqlite.rs` | 3 json_each equality predicates; MAX_KNN_K = domain::MAX_LIMIT | ✓ VERIFIED | Sites at 284/377/425 with unchanged param indices; MAX_KNN_K defined via `crate::domain::MAX_LIMIT` (sqlite.rs:35); `tags LIKE` = 0 |
| `crates/agent-memory/src/mcp.rs` | InvalidArgument in invalid_params arm + exact-tag doc strings + extended mapper test | ✓ VERIFIED | Mapper arm mcp.rs:36 (exhaustive, no `_` catch-all); doc strings at 82/102; tier test extended (277-298) |
| `crates/agent-memory/src/rest/handlers.rs` | InvalidArgument → BadRequest + exact-tag doc strings + in-process 400 tests | ✓ VERIFIED | Mapper arm at 91 (exhaustive); doc strings at 55/72; 3 named 400 tests at 444/476/500 |
| `crates/agent-memory-core/tests/validation.rs` | Boundary matrix across search/list/store/import | ✓ VERIFIED | 4 named tests, invalid values [0,-1,201/3155760001,i64::MAX,i64::MIN] AND valid boundaries [1, 200 / 3_155_760_000, None]; ran green |
| `README.md` | Bounds + exact-tag semantics + behavioral-changes note | ✓ VERIFIED | `3155760000` ×4, "exact tag" ×5 (case-insensitive), "Behavioral changes" ×1, "tag substring" ×0 |
| `.github/workflows/ci.yml` | Revived CI on ubuntu-latest, contents: read, unchanged steps | ✓ VERIFIED | All plan acceptance greps pass; proven live via gh (success on main) |
| `.github/workflows/spike-cross-compile.yml` | Hosted 4-leg spike + D-11 VERDICT comment | ✓ VERIFIED | VERDICT block present with per-leg GREEN lines + Phase 6 conclusion; matches SUMMARY text |

### Key Link Verification

| From | To | Via | Status | Details |
| ---- | --- | --- | ------ | ------- |
| service.rs | domain.rs | validate_limit/validate_ttl at method tops before embed/store hops | ✓ WIRED | Fully qualified `crate::domain::validate_*` — 2× ttl (store, import), 2× limit (search, list); store validates before the embed call (line 120) |
| sqlite.rs | args.tag | json_each equality EXISTS with same bound parameter indices | ✓ WIRED | ?5/?3/?10 preserved; NULL-disables-filter convention intact (locked by tag=None assertions in tests) |
| handlers.rs | MemoryError::InvalidArgument | map_memory_error client arm → ApiError::BadRequest → 400 | ✓ WIRED | handlers.rs:88-98, exhaustive named arms; end-to-end proven by in-process 400 tests + tests/rest.rs real-HTTP assertions |
| mcp.rs | MemoryError::InvalidArgument | map_mcp_error client arm → McpError::invalid_params | ✓ WIRED | mcp.rs:33-42, exhaustive named arms; locked by tier unit test |
| ci.yml | GitHub-hosted runners | runs-on: ubuntu-latest (D-02 exception, OPS-02) | ✓ WIRED | Live green runs on main confirmed via gh API |
| spike-cross-compile.yml | musl typedef shim / cargo-xwin | target-suffixed CFLAGS vars; pinned CARGO_XWIN_VERSION | ✓ WIRED | Both patterns present; run 29196226289 all 4 legs success (gh-verified) |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| -------- | ------- | ------ | ------ |
| Boundary matrix rejects/accepts at the seam | `cargo test -p agent-memory-core --test validation` | 4 passed | ✓ PASS |
| List tag filter exact + case-sensitive | `cargo test -p agent-memory-core --test store list_tag_filter_is_exact_and_case_sensitive` | 1 passed | ✓ PASS |
| KNN-path tag filter exact | `cargo test -p agent-memory-core --test semantic tag_filter_is_exact_on_knn_path` | 1 passed | ✓ PASS |
| Keyword-fallback tag filter exact | `cargo test -p agent-memory-core --test fallback keyword_fallback_tag_filter_is_exact` | 1 passed | ✓ PASS |
| REST returns 400 (not 500) for limit=0 | `cargo test -p agent-memory --bins search_with_zero_limit_returns_400_never_500` | 1 passed | ✓ PASS |
| MCP maps InvalidArgument to invalid_params | `cargo test -p agent-memory --bins map_mcp_error_splits_client_and_internal_tiers` | 1 passed | ✓ PASS |
| Spike run conclusions (not SUMMARY trust) | `gh run view 29196226289 --json status,conclusion,jobs` | completed/success; all 4 jobs success | ✓ PASS |
| CI alive on main | `gh run list --workflow=ci.yml --limit 2` | 29196632534 + 29196396239 both success | ✓ PASS |

### Probe Execution

No `scripts/*/tests/probe-*.sh` probes exist or are declared by this phase — SKIPPED. The plan-declared verification is test/grep/gh-based and was executed above.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ----------- | ----------- | ------ | -------- |
| API-02 | 03-01 | Out-of-bounds limit/ttl_secs rejected at shared core seam → 400 / invalid_params, never 500, never silent clamp | ✓ SATISFIED | Truths 1, 3, 5; REQUIREMENTS.md marks `[x]` Complete / Phase 3 |
| API-03 | 03-01 | Tag filtering exact via json_each; behavioral change in release notes | ✓ SATISFIED | Truth 2; README "Behavioral changes in v0.1.0" section feeds DIST-06 |
| DIST-03 | 03-02 (informed, not delivered) | Windows binary — toolchain decided by early spike | ✓ INFORMED (per plan contract) | Plan 03-02 explicitly declares itself decision input, not delivery; verdict adopts cargo-xwin 0.23.0/msvc; REQUIREMENTS.md correctly still Pending / Phase 6 |
| DIST-05 | 03-02 (informed, not delivered) | musl binaries via CFLAGS typedef shim on zigbuild | ✓ INFORMED (per plan contract) | Both musl legs GREEN with the shim; x86_64-musl binary executed `--version`; still Pending / Phase 6 |

Orphan check: REQUIREMENTS.md maps exactly API-02 and API-03 to Phase 3 — both claimed by plan 03-01. No orphaned requirements.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |
| — | — | None | — | No TBD/FIXME/XXX/TODO debt markers in any phase-modified file (the only "TODO"/"placeholder" grep hits are the TODO memory type wire name and a SQL `placeholders` variable — legitimate domain code, not stubs) |

### Human Verification Required

None. All four success criteria are programmatically verifiable and were verified: behavior is test-locked at four layers (core seam, in-process handlers, MCP mapper unit, spawned-binary real HTTP), and the CI/spike run conclusions were independently confirmed via the GitHub API rather than trusting SUMMARY claims. No `<human-check>` blocks exist in either plan.

### Gaps Summary

No gaps. Both plans delivered exactly what the phase goal requires:

- The validation seam exists once in `MemoryService` (`domain.rs` helpers, four call sites in `service.rs`), with the silent `.max(0)` clamp removed and both transport mappers routing `MemoryError::InvalidArgument` to the client tier through exhaustive named-arm matches.
- Exact tag matching replaced all three LIKE predicates with `json_each` equality, with per-site regression fixtures locking the rust/rustling/Rust matrix and the behavioral change documented in README for the v0.1.0 release notes.
- The toolchain spikes ran on hosted CI (run 29196226289, all 4 legs green — gh-verified) with the D-11 verdict recorded in both the workflow header and the plan SUMMARY; Phase 6 starts with cargo-xwin/msvc adopted and the musl shim proven.
- Ride-along D-12: CI is verifiably alive again on this public repo (two green main runs, gh-verified).

---

_Verified: 2026-07-12T15:35:00Z_
_Verifier: Claude (gsd-verifier)_
