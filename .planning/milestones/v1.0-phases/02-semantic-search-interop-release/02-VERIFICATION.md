---
phase: 02-semantic-search-interop-release
verified: 2026-07-03T12:10:04Z
status: passed
score: 15/15 must-haves verified
human_signoff:
  date: 2026-07-12
  gaps_confirmed:
    - "Gap 1 (CR-01) closure confirmed by user — tag-filter bind in keyword fallback, proven by live regression test (fallback 4/4)"
    - "Gap 2 (WR-05 + WR-04) closure confirmed by user — two-tier error taxonomy proven live at core/handler/MCP/HTTP layers"
  waived:
    - "Second-machine cross-arch install check — explicitly optional per plan 02-04; SC5 VERIFIED under its own definition; no Linux/Intel-Mac machine available (recorded as skipped-with-reason in 02-UAT.md)"
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 2/2 previously-failed items re-checked (13/15 initial)
  gaps_closed:
    - "With Ollama stopped or unreachable, search automatically degrades to keyword/FTS5 results instead of erroring or returning empty (SC2 / SEARCH-03) — keyword path now binds the tag filter (CR-01 closed, commit 39be828)"
    - "DELETE of an unknown id returns 404 with a clean JSON body; an invalid memory type returns 400 — never a 500 for bad input (plan 02-02 truth 3) — malformed FTS5 queries now map to InvalidQuery -> REST 400 / MCP invalid_params (WR-05 + WR-04 closed, commit e491443)"
  gaps_remaining: []
  regressions: []
human_verification:
  - test: "On a Linux (x86_64 or aarch64) or Intel-Mac machine: `brew install unityinflow/tap/agent-memory && agent-memory --version`, then confirm an MCP client (Claude Code/Cursor) launches the brewed binary from .mcp.json"
    expected: "Version prints 0.0.1; MCP server initializes"
    why_human: "This darwin-arm64 machine cannot execute the x86_64-darwin or Linux binaries; CI smoke was host-arch-aware (aarch64-linux only). Plan 02-04 marks this check optional — SC5 was defined as proven locally for darwin-arm64 + brew, structurally for the other triples, and is VERIFIED under that definition."
---

# Phase 2: Semantic Search, Interop & Release — Verification Report (Re-verification)

**Phase Goal:** The same memory store gains local semantic recall that gracefully falls back to keyword when Ollama is absent, a REST mirror for non-MCP clients, one-command import of GSD STATE.md, and an installable cross-platform release.
**Verified:** 2026-07-03T12:10:04Z
**Status:** passed (all 15 must-haves verified; human sign-off recorded 2026-07-12 — both gap closures confirmed, optional cross-arch install check waived with reason, see frontmatter `human_signoff`)
**Re-verification:** Yes — after gap-closure plan 02-05 (commits 39be828, e491443)
**Mode:** mvp (User Story goal carried by all five plans; validated against the `As a …, I want to …, so that …` shape manually — gsd-tools not installed on this machine)

## Re-verification Focus

Initial verification (2026-07-03T08:20:00Z) scored 13/15 with 2 gaps. Both were re-checked with full 3-level + behavioral verification against the current codebase; the 13 previously-passed items received regression checks (targeted test suites re-run, artifact existence, release assets re-queried via `gh`).

### Gap 1 (CR-01) — CLOSED, verified by code trace + live test

- `SqliteStore::search` WHERE clause now carries `AND (?10 IS NULL OR m.tags LIKE '%' || ?10 || '%')` (sqlite.rs:414) and `args.tag` is bound as the 10th `params![]` element (sqlite.rs:435) — the exact predicate shape `knn_search` uses at `?5` (sqlite.rs:279). Grep gate: `IS NULL OR m.tags LIKE` count == 2, as the plan specified.
- Regression test `keyword_fallback_honors_tag_filter` (tests/fallback.rs:83-130) is substantive: dead embedder, two rows sharing FTS keyword "pipeline" with disjoint tags, asserts all three of `SearchMode::Keyword`, `results.len() == 1`, and result id == the alpha-tagged row. **Run live in this verification: PASS** (fallback suite 4/4, 0.03s).

### Gap 2 (WR-05 + WR-04) — CLOSED, verified by code trace + live tests at all four layers

- `MemoryError::InvalidQuery(String)` exists (domain.rs:125).
- `map_fts_query_error` (sqlite.rs:199-206) classifies post-bind FTS5 parse failures (`fts5: syntax error` OR `unterminated string` — the lone-quote reproducer) as `InvalidQuery`; only statement-step/row-iteration errors are routed through it (sqlite.rs:446,449); `conn.prepare` failures stay internal, per the plan's seam rule. The previously stale comment now correctly states the mapping lives at the store seam (sqlite.rs:440-444).
- REST: `map_memory_error` maps `InvalidType | InvalidQuery` → `ApiError::BadRequest` (handlers.rs:89-91), exhaustive match, no catch-all. **In-process test `search_with_malformed_query_returns_400_never_500`: PASS (live).**
- MCP: shared exhaustive `map_mcp_error` (mcp.rs:31-42) — `InvalidType | InvalidQuery` → invalid_params; `Sqlite | Pool | Join | Migration | NotFound` → internal_error. All four tool service calls use `.map_err(map_mcp_error)` (mcp.rs:156,186,221,240 — grep count == 4); the only remaining `McpError::invalid_params` closures are the up-front `MemoryType::try_from` validations, exactly as the plan requires. **Unit test `map_mcp_error_splits_client_and_internal_tiers`: PASS (live).**
- Core: `malformed_fts5_query_maps_to_invalid_query_in_keyword_mode` (tests/fallback.rs:133-162) asserts `matches!(err, MemoryError::InvalidQuery(_))` with a dead embedder. **PASS (live).**
- Real HTTP: the malformed-query 400 block appended to `rest_end_to_end_store_list_search_forget_health` (tests/rest.rs:205-211, spawned binary, dead-Ollama env). **PASS (live, 0.85s).**
- The independent post-gap-closure code review (02-REVIEW.md, 2026-07-03T12:02:47Z, 26 files) independently confirms all three findings closed: **critical: 0**.

## User Flow Coverage (MVP mode)

User story: *As a developer running AI agents across editors and tools, I want to install agent-memory with one command and have it recall my memories by meaning — falling back to keyword search when Ollama is absent, reachable over REST, and able to import my GSD project state, so that every agent session starts with the right context, fully locally with no cloud.*

| Step | Expected | Evidence in codebase / live check | Status |
|---|---|---|---|
| Install with one command | `brew install unityinflow/tap/agent-memory` yields a working binary | Release v0.0.1 re-queried via `gh` in this re-verification: all 4 tarballs + SHA256SUMS.txt still live; tag v0.0.1 unchanged at 3469054; brew install + MCP initialize proven in initial verification/UAT | ✓ |
| Recall by meaning | Semantic ranking finds memories with zero keyword overlap | Deterministic FakeEmbedder golden set green (semantic suite re-run live); live-Ollama proof from initial verification stands (code paths untouched by 02-05) | ✓ |
| Fallback when Ollama absent | Keyword/FTS5 results, never error/empty, honoring the same filters | fallback suite now 4/4 live — including the new tag-filter fidelity test. **The initial verification's ⚠️ (tag filter dropped) is resolved** | ✓ |
| Reachable over REST | store/search/list/forget over HTTP, same store | agent-memory package 20/20 live incl. tests/rest.rs e2e (real HTTP) with the new 400-not-500 assertion | ✓ |
| Import GSD project state | One command, idempotent | import suite 1/1 re-run live; live brewed-binary double-run proven in initial verification (importer untouched by 02-05) | ✓ |
| Fully local, no cloud | SQLite embedded, Ollama optional | All 02-05 regression tests run with a dead embedder, no network; stdio_purity green in the 20/20 run | ✓ |

## Goal Achievement

### Observable Truths

Merged from ROADMAP Success Criteria (SC1-SC5, the contract) + PLAN frontmatter truths (02-01…02-05, deduplicated).

| # | Truth | Status | Evidence |
|---|---|---|---|
| 1 | SC1: With Ollama running, memory_search ranks by semantic similarity blended with decay, finding memories sharing no exact keywords | ✓ VERIFIED | Regression: semantic suite 3 pass + 1 ignored live-test (re-run this verification); knn_search/blend code untouched by 02-05 (commit diff: sqlite.rs +2 lines in fn search only for Task 1) |
| 2 | SC2: With Ollama unreachable, search degrades to keyword/FTS5, never error/empty — honoring the same filters as the semantic path | ✓ VERIFIED | **Previously PARTIAL, now closed.** ?10 tag predicate + args.tag bind (sqlite.rs:414,435); `keyword_fallback_honors_tag_filter` PASS live; both original fallback tests still pass (4/4) |
| 3 | SC3: Non-MCP client can store/search/list/forget over REST against the same store | ✓ VERIFIED | Regression: agent-memory 20/20 live incl. tests/rest.rs e2e; handlers → MemoryService wiring unchanged |
| 4 | SC4: `import --from gsd-state` loads STATE.md memories, searchable, idempotent on re-run | ✓ VERIFIED | Regression: import suite 1/1 re-run live; importer untouched by 02-05; initial live double-run (9 → 0) stands |
| 5 | SC5: Install from pre-built binary (4 targets) or brew; MCP server launches | ✓ VERIFIED | Release v0.0.1 assets re-verified live via `gh` (4 tarballs + SHA256SUMS.txt); tag unchanged at 3469054; per the plan's SC5 definition (darwin-arm64 + brew proven locally, other triples structurally). Optional second-machine check → human item |
| 6 | memory_store succeeds with Ollama down; row lands embedding_status=0 | ✓ VERIFIED | `store_succeeds_with_failing_embedder_and_row_is_listable` in the 4/4 fallback run |
| 7 | Forget/TTL removes the vector — deleted memory never resurfaces semantically | ✓ VERIFIED | Regression: store/semantic suites green; forget vec-delete code untouched (sqlite.rs:454+) |
| 8 | stdout stays pure JSON-RPC with embedder active against dead Ollama | ✓ VERIFIED | stdio_purity in the agent-memory 20/20 live run; initial live MCP-initialize proof stands |
| 9 | POST /api/search returns the shared {search_mode, results} envelope incl. keyword fallback | ✓ VERIFIED | tests/rest.rs asserts search_mode "keyword" under dead-Ollama env — green in live e2e run |
| 10 | DELETE unknown id → 404 clean JSON; invalid type → 400 — never a 500 for bad input | ✓ VERIFIED | **Previously PARTIAL, now closed.** InvalidQuery variant + store-seam mapping + BadRequest arm; proven live at core (`malformed_fts5_query_maps_to_invalid_query_in_keyword_mode`), in-process handler (400 + error body), and real HTTP (spawned binary, `{"query": "\""}` → 400) layers |
| 11 | serve-rest binds loopback by default; non-loopback refused without --allow-remote | ✓ VERIFIED | Bind-guard tests in the agent-memory 20/20 live run; rest/mod.rs untouched by 02-05 |
| 12 | Malformed lines / unknown sections skipped and counted — importer never hard-errors | ✓ VERIFIED | Regression: import suite green; gsd_state.rs untouched by 02-05 |
| 13 | Fenced code block with a fake heading never derails section detection | ✓ VERIFIED | Regression: import suite green; fixture + fence tracking unchanged |
| 14 | Darwin cross-compile spike green on all required legs BEFORE tag push | ✓ VERIFIED | Historical fact, unchanged: gh run 28643212871 (4 DIST legs success) preceded the ~07:02Z tag |
| 15 | LICENSE (MIT) + CONTRIBUTING.md exist before the v0.0.1 tag | ✓ VERIFIED | Both still on disk (re-checked); commit 15d9349 precedes tag commit 3469054 |
| — | 02-05 truth: MCP tools report invalid client input as invalid_params, internal DB failures as internal_error — REST and MCP agree at the error tier | ✓ VERIFIED | Exhaustive `map_mcp_error` (mcp.rs:31-42) + 4 `.map_err(map_mcp_error)` sites; `map_mcp_error_splits_client_and_internal_tiers` PASS live (subsumed under truth 10's seam — closes WR-04) |

**Score:** 15/15 truths verified (previously 13/15; both partials flipped)

### Required Artifacts

Previously-verified artifacts (02-01…02-04) regression-checked for existence and suite-level sanity — all still present and green. New/modified 02-05 artifacts verified at all levels:

| Artifact | Expected | Status | Details |
|---|---|---|---|
| `crates/agent-memory-core/src/store/sqlite.rs` | Keyword-search tag predicate (bound ?10) + FTS5 parse-error → InvalidQuery at the store seam | ✓ VERIFIED | Contains `?10 IS NULL OR m.tags LIKE` (line 414); `map_fts_query_error` (199-206) routing post-bind errors (446,449); stale comment corrected (440-444) |
| `crates/agent-memory-core/src/domain.rs` | `MemoryError::InvalidQuery(String)` variant | ✓ VERIFIED | Line 125, thiserror message starts `invalid search query`; client tier alongside InvalidType |
| `crates/agent-memory/src/mcp.rs` | Shared two-tier `fn map_mcp_error` used by all four tool service calls | ✓ VERIFIED | mcp.rs:31-42, exhaustive (no `_` arm); 4 call sites (156,186,221,240); doc-commented as mirror of map_memory_error; unit test at 276 |
| `crates/agent-memory/src/rest/handlers.rs` | InvalidQuery → BadRequest arm + in-process 400-not-500 test | ✓ VERIFIED | Arm at 89-91 (exhaustive match); test `search_with_malformed_query_returns_400_never_500` at 401 |
| `crates/agent-memory-core/tests/fallback.rs` | Regression tests for both gaps | ✓ VERIFIED | `keyword_fallback_honors_tag_filter` (83) + `malformed_fts5_query_maps_to_invalid_query_in_keyword_mode` (133); both dead-embedder, no network; 4/4 live |
| `crates/agent-memory/tests/rest.rs` | Real-HTTP malformed-query 400 assertion | ✓ VERIFIED | `{"query": "\""}` → assert 400 block at 205-211 inside the e2e test; PASS live |
| All 19 artifacts from initial verification (embed/, ollama.rs, 0002_embeddings.sql, service.rs, semantic.rs, workflows, rest/mod.rs, gsd_state.rs, fixtures, import.rs, release.yml, LICENSE, CONTRIBUTING.md, README.md, tap formula) | Unchanged | ✓ VERIFIED (regression) | Existence re-checked; release assets re-queried via gh; v0.0.1 tag unchanged; crates working tree clean |

### Key Link Verification

| From | To | Via | Status | Details |
|---|---|---|---|---|
| sqlite.rs fn search | args.tag | bound ?10 LIKE predicate (same shape as knn_search ?5) | ✓ WIRED | **The one previously NOT_WIRED link — now wired** (sqlite.rs:414 + 435); locked by dead-embedder regression test |
| rest/handlers.rs | MemoryError::InvalidQuery | map_memory_error BadRequest arm → HTTP 400 | ✓ WIRED | handlers.rs:89-91; proven in-process AND over real HTTP |
| mcp.rs | map_mcp_error | .map_err(map_mcp_error) on all four tool service calls | ✓ WIRED | grep count == 4; remaining invalid_params closures are only the up-front type validations, per plan |
| sqlite.rs fn search | map_fts_query_error | post-bind rows?/row? routed through helper; prepare errors stay internal | ✓ WIRED | sqlite.rs:446,449; seam rule honored |
| All 13 previously-WIRED links (service↔embedder, KNN CTE, envelopes, REST/MCP wiring, import path, release/formula/spike) | — | — | ✓ WIRED (regression) | Suites re-run green; none of these seams touched by 02-05 (commit diffs confined to the 6 planned files) |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|---|---|---|---|---|
| fn search keyword path | tag-filtered result rows | real SQLite rows through the new ?10 bind | Yes — regression test proves the beta row is excluded and the alpha row returned by id | ✓ FLOWING |
| REST error body | `{"error": ...}` from InvalidQuery | e.to_string() (`invalid search query …`) surfaced verbatim | Yes — in-process test asserts non-empty string error field; real-HTTP test asserts body carries `error` | ✓ FLOWING |
| MCP error tier | McpError code | map_mcp_error over real MemoryError variants | Yes — unit test compares actual `.code` values against invalid_params/internal_error | ✓ FLOWING |
| All Level-4 traces from initial verification (search outcome, REST envelope, import counts, sweep backfill) | — | — | Unchanged, suites green | ✓ FLOWING |

### Behavioral Spot-Checks

All run live in this re-verification, no Ollama, no network:

| Behavior | Command | Result | Status |
|---|---|---|---|
| Gap-1 regression + gap-2 core + both original fallback tests | `cargo test -p agent-memory-core --test fallback` | 4 passed, 0 failed (0.03s) | ✓ PASS |
| In-process 400-not-500 | `cargo test -p agent-memory search_with_malformed_query_returns_400_never_500` | 1 passed | ✓ PASS |
| MCP two-tier split | `cargo test -p agent-memory map_mcp_error_splits_client_and_internal_tiers` | 1 passed | ✓ PASS |
| Real-HTTP e2e incl. malformed-query 400 | `cargo test -p agent-memory --test rest rest_end_to_end_store_list_search_forget_health` | 1 passed (0.85s) | ✓ PASS |
| Regression: semantic/import/store | `cargo test -p agent-memory-core --test semantic --test import --test store` | 8 passed, 1 ignored (live-Ollama test, proven in initial verification) | ✓ PASS |
| Regression: full agent-memory package | `cargo test -p agent-memory` | 20 passed (4 suites) | ✓ PASS |
| Lint gate | `cargo clippy --workspace --all-targets -- -D warnings` | clean | ✓ PASS |
| Format gate | `cargo fmt --check` | clean | ✓ PASS |
| Release assets | `gh release view v0.0.1 --json assets` | 4 tarballs + SHA256SUMS.txt live | ✓ PASS |
| Commits exist | `git show 39be828` / `git show e491443` | Both present, diffs confined to the 6 planned files | ✓ PASS |

### Probe Execution

No `scripts/*/tests/probe-*.sh` probes exist and none are declared in any PLAN/SUMMARY — SKIPPED (no probes in this project's convention).

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|---|---|---|---|---|
| SEARCH-02 | 02-01 | Semantic similarity via local Ollama | ✓ SATISFIED | Initial live-Ollama proof stands; deterministic golden set re-run green; code untouched by 02-05 |
| SEARCH-03 | 02-01, 02-05 | Graceful degrade to keyword | ✓ SATISFIED | **Defect removed** — degraded mode now honors the tag filter; fallback suite 4/4 live |
| API-01 | 02-02, 02-05 | REST store/search/list/forget | ✓ SATISFIED | **Defect removed** — never-500-for-bad-input contract now factually true, proven at core/handler/HTTP layers; MCP error-tier parity restored (WR-04) |
| INTEROP-01 | 02-03 | GSD STATE.md import | ✓ SATISFIED | Import suite re-run green; initial live idempotent double-run stands |
| DIST-01 | 02-04 | Binaries: macOS arm64/x86_64 + Linux x86_64/aarch64 | ✓ SATISFIED | All 4 tarballs + SHA256SUMS re-verified live on Release v0.0.1 |
| DIST-02 | 02-04 | Homebrew formula | ✓ SATISFIED | Formula in tap (initial verification); release assets it points to re-verified now |

No orphaned requirements: REQUIREMENTS.md maps exactly these 6 IDs to Phase 2; every one is claimed by a plan (02-05 re-claims SEARCH-03 and API-01 for gap closure).

### Anti-Patterns Found

No TBD/FIXME/XXX debt markers in any 02-05-modified file (all `TODO` grep hits are the `MemoryType::Todo` domain literal, not markers). No stub returns, no placeholder paths. The post-gap-closure code review (02-REVIEW.md, 26 files, run after commits 39be828/e491443) reports **critical: 0, warning: 6, info: 5** — the two former blockers (CR-01, WR-05) and WR-04 are confirmed closed by that independent review as well. Remaining warnings are recorded review findings, not verification gaps:

| File | Pattern | Severity | Impact |
|---|---|---|---|
| store/sqlite.rs | Unvalidated limit/ttl_secs extremes (WR-01/WR-02 family) | ⚠️ Warning | Boundary-value overflow/bypass; deliberately out of 02-05 scope per plan's scope discipline |
| store/sqlite.rs | tag LIKE substring over-match (WR-07 family) | ⚠️ Warning | Loose filter semantics — the new ?10 predicate intentionally keeps the existing LIKE shape (plan: exact-match semantics is WR-07's separate concern) |
| various | Remaining review warnings/infos | ⚠️/ℹ️ | See 02-REVIEW.md — none release-blocking, none contradicting a must-have |

### Human Verification Required

One item, carried forward from plan 02-04's end-of-phase `<human-check>` (explicitly marked optional by the plan; SC5 is VERIFIED under the plan's own definition). All other UAT items were covered live in the initial verification on this machine.

### 1. Second-machine install (other arch legs)

**Test:** On a Linux (x86_64 or aarch64) or Intel-Mac machine: `brew install unityinflow/tap/agent-memory && agent-memory --version`, then confirm an MCP client (Claude Code/Cursor) launches the brewed binary from `.mcp.json`.
**Expected:** Version prints 0.0.1; MCP server initializes.
**Why human:** This darwin-arm64 machine cannot execute the x86_64-darwin or Linux binaries; CI smoke was host-arch-aware (aarch64-linux only). The plan marks this optional — SC5 was defined as proven locally for darwin-arm64 + brew, structurally for the other triples.

### Gaps Summary

No gaps remain. Both partial truths from the initial verification are now observably true in the codebase, confirmed by first-hand code trace and live test execution in this verification process — not by SUMMARY claims:

1. **Gap 1 (CR-01) closed:** the FTS5 keyword fallback binds `args.tag` via a parameterized `?10` predicate mirroring the semantic path's `?5`; a tag-filtered search with a dead embedder returns only the tagged row in keyword mode, locked by a regression test run live (4/4).
2. **Gap 2 (WR-05 + WR-04) closed:** malformed FTS5 input is classified at the store seam as `MemoryError::InvalidQuery` and surfaces as REST 400 / MCP invalid_params; internal DB failures report as 500 / internal_error on both transports. Proven live at four layers: core, in-process handler, MCP unit, and real HTTP through a spawned binary. The rest/mod.rs "bad input can never surface as a 500" contract is now factually true.

No regressions: all previously-passing suites re-run green (fallback 4/4, semantic/import/store 8+1-ignored, agent-memory 20/20), clippy `-D warnings` and `fmt --check` clean, v0.0.1 tag and release assets unchanged and live. The only outstanding item is the optional second-machine install check this darwin-arm64 machine physically cannot execute — hence `human_needed` rather than `passed`.

---

_Verified: 2026-07-03T12:10:04Z_
_Verifier: Claude (gsd-verifier)_
