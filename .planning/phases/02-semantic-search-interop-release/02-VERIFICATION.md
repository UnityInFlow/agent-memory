---
phase: 02-semantic-search-interop-release
verified: 2026-07-03T08:20:00Z
status: gaps_found
score: 13/15 must-haves verified
overrides_applied: 0
gaps:
  - truth: "With Ollama stopped or unreachable, search automatically degrades to keyword/FTS5 results instead of erroring or returning empty (SC2 / SEARCH-03)"
    status: partial
    reason: "The degrade path works and is proven live for untagged searches, but the FTS5 keyword fallback silently drops the `tag` filter that both transports document and that the semantic path honors — a tag-filtered memory_search / POST /api/search returns an unfiltered superset exactly when Ollama is down (review CR-01, confirmed unfixed by direct code trace)"
    artifacts:
      - path: "crates/agent-memory-core/src/store/sqlite.rs"
        issue: "fn search (lines 384-413) binds only MATCH/mem_type/scope; args.tag is never referenced or bound — knn_search (line 258) binds it at ?5, so the two modes diverge on filter fidelity"
    missing:
      - "Add the tag predicate to the keyword search WHERE clause and bind args.tag (mirror knn_search's ?5 predicate)"
      - "Regression test: two rows with different tags, dead embedder, tag-filtered search returns only the tagged row with search_mode 'keyword'"
  - truth: "DELETE of an unknown id returns 404 with a clean JSON body; an invalid memory type returns 400 — never a 500 for bad input (plan 02-02 truth 3)"
    status: partial
    reason: "404-on-unknown-id and 400-on-invalid-type are verified (tests + code trace), but the 'never a 500 for bad input' clause is falsified: a malformed FTS5 query string (e.g. a lone double-quote) surfaces as MemoryError::Sqlite -> ApiError::Internal -> HTTP 500 in keyword mode (review WR-05), contradicting rest/mod.rs line 30's own written contract. Mode-dependent: the same request succeeds when Ollama is up (query only embedded, never FTS5-parsed)"
    artifacts:
      - path: "crates/agent-memory/src/rest/handlers.rs"
        issue: "map_memory_error (lines 86-95) maps every MemoryError::Sqlite to Internal (500); no FTS5-syntax-error -> client-error mapping exists anywhere in the codebase despite two comments claiming it does"
      - path: "crates/agent-memory/src/mcp.rs"
        issue: "Related same-seam defect (WR-04): all four MCP tools map every MemoryError to invalid_params — internal DB failures are misreported as client errors, so the two transports diverge at the error tier"
    missing:
      - "Detect FTS5 syntax errors at the service/store seam (e.g. an InvalidQuery variant) and map to 400 (REST) / invalid_params (MCP)"
      - "Shared two-tier error mapping in mcp.rs mirroring map_memory_error (closes WR-04 with the same fix)"
      - "REST test posting a malformed query with a dead embedder expecting 400, never 500"
---

# Phase 2: Semantic Search, Interop & Release — Verification Report

**Phase Goal:** The same memory store gains local semantic recall that gracefully falls back to keyword when Ollama is absent, a REST mirror for non-MCP clients, one-command import of GSD STATE.md, and an installable cross-platform release.
**Verified:** 2026-07-03T08:20:00Z
**Status:** gaps_found
**Re-verification:** No — initial verification
**Mode:** mvp (User Story goal carried by all four plans; validated against the `As a …, I want to …, so that …` shape manually — gsd-tools not installed on this machine)

## User Flow Coverage (MVP mode)

User story: *As a developer running AI agents across editors and tools, I want to install agent-memory with one command and have it recall my memories by meaning — falling back to keyword search when Ollama is absent, reachable over REST, and able to import my GSD project state, so that every agent session starts with the right context, fully locally with no cloud.*

| Step | Expected | Evidence in codebase / live check | Status |
|---|---|---|---|
| Install with one command | `brew install unityinflow/tap/agent-memory` yields a working binary | `brew list --versions` → agent-memory 0.0.1 at `/opt/homebrew/bin/agent-memory`; Formula/agent-memory.rb live in UnityInFlow/homebrew-tap; Release v0.0.1 has 4 tarballs + SHA256SUMS.txt (verified via `gh` during this verification) | ✓ |
| Recall by meaning | Semantic ranking finds memories with zero keyword overlap | Live ignored Ollama test run in this verification: `live_ollama_semantic_recall_without_shared_keywords ... ok` (0.76s); deterministic FakeEmbedder golden set also green | ✓ |
| Fallback when Ollama absent | Keyword/FTS5 results, never error/empty | `fallback` suite run live: 2/2 pass; degrade seam traced in service.rs:212-240 | ⚠️ works, but tag filter silently dropped in this mode (gap 1 / CR-01) |
| Reachable over REST | store/search/list/forget over HTTP, same store | rest/ module wired to MemoryService (State\<Arc\<AppState\>\>), tests/rest.rs (264 lines, spawned-binary HTTP) + 6 in-process handler tests; zero SQL in handlers | ✓ |
| Import GSD project state | One command, idempotent | Run live through the brewed binary against a scratch DB: run 1 `imported: 9, skipped duplicates: 0`, run 2 `imported: 0, skipped duplicates: 9` | ✓ |
| Fully local, no cloud | SQLite embedded, Ollama optional | MCP initialize answered by brewed binary with pure JSON-RPC on stdout, logs on stderr, dead-Ollama import succeeded best-effort | ✓ |

## Goal Achievement

### Observable Truths

Merged from ROADMAP Success Criteria (SC1-SC5, the contract) + PLAN frontmatter truths (deduplicated).

| # | Truth | Status | Evidence |
|---|---|---|---|
| 1 | SC1: With Ollama running, memory_search ranks by semantic similarity blended with decay, finding memories sharing no exact keywords | ✓ VERIFIED | Live-Ollama ignored test executed in this verification process: PASS. knn_search KNN CTE (sqlite.rs:249-259) + similarity×decay blend (service.rs:181-210) traced |
| 2 | SC2: With Ollama unreachable, search degrades to keyword/FTS5, never error/empty | ⚠️ PARTIAL | Degrade seam traced (service.rs:212-240, loud warn + SearchMode::Keyword); fallback suite 2/2 live pass. BUT keyword path drops the `tag` filter (gap 1, CR-01) — degraded results are wrong for tag-filtered callers |
| 3 | SC3: Non-MCP client can store/search/list/forget over REST against the same store | ✓ VERIFIED | handlers → MemoryService via State\<Arc\<AppState\>\> (handlers.rs:110,131); SearchOutcome serialized as-is (handlers.rs:159); ServeRest wired (main.rs:66,119); tests/rest.rs + in-process handler tests; UAT confirmed live |
| 4 | SC4: `import --from gsd-state` loads STATE.md memories, searchable, idempotent on re-run | ✓ VERIFIED | Run live in this verification via brewed binary: 9 imported → 0 on re-run; import suite 1/1 pass; main.rs:238 → parse_gsd_state → service.import traced |
| 5 | SC5: Install from pre-built binary (4 targets) or brew; MCP server launches | ✓ VERIFIED | Release v0.0.1 live with all 4 required tarballs + SHA256SUMS.txt (gh-verified now); brewed 0.0.1 binary answered MCP initialize with pure JSON-RPC on stdout in this verification. x86_64-darwin/Linux legs proven structurally (assets + per-leg-green spike) per the plan's own SC5 definition |
| 6 | memory_store succeeds with Ollama down; row lands embedding_status=0 | ✓ VERIFIED | fallback test `store_succeeds_with_failing_embedder_and_row_is_listable` live pass; best-effort embed traced in service store path |
| 7 | Forget/TTL removes the vector — deleted memory never resurfaces semantically | ✓ VERIFIED | Explicit `DELETE FROM vec_memories` in forget (sqlite.rs:435) and sweep_expired NOT-IN cleanup (sqlite.rs:478-481), same writer TX; semantic suite covers |
| 8 | stdout stays pure JSON-RPC with embedder active against dead Ollama | ✓ VERIFIED | stdio_purity test exists (dead-Ollama env); my live MCP initialize: JSON-RPC only on stdout, all logs on stderr. (WR-09: the test asserts an rmcp-internal log string — reliability warning, truth itself holds) |
| 9 | POST /api/search returns the shared {search_mode, results} envelope incl. keyword fallback | ✓ VERIFIED | search_handler returns SearchOutcome verbatim; tests/rest.rs asserts search_mode "keyword" under dead-Ollama env |
| 10 | DELETE unknown id → 404 clean JSON; invalid type → 400 — never a 500 for bad input | ⚠️ PARTIAL | 404/400 verified (tests + map_memory_error trace). Never-500 clause FALSIFIED: malformed FTS5 query → Sqlite error → 500 in keyword mode (gap 2, WR-05) |
| 11 | serve-rest binds loopback by default; non-loopback refused without --allow-remote | ✓ VERIFIED | ensure_bind_allowed in rest/mod.rs + unit tests + spawned-binary bind-guard test; review confirms |
| 12 | Malformed lines / unknown sections skipped and counted — importer never hard-errors | ✓ VERIFIED | Live run reported `malformed lines skipped: 1`; 9 parser unit tests; tolerant scanner traced (gsd_state.rs). (IN-02: empty deferred-table row imports as `" — "` — info-level edge) |
| 13 | Fenced code block with a fake heading never derails section detection | ✓ VERIFIED | Fence state tracking in gsd_state.rs; fixture contains the fence trap; import suite green live |
| 14 | Darwin cross-compile spike green on all required legs BEFORE tag push | ✓ VERIFIED | gh run 28643212871 (created 06:37:54Z, before ~07:02Z tag): all 4 DIST legs success, musl pair failure (best-effort per plan — non-blocking by the plan's per-leg gate rule) |
| 15 | LICENSE (MIT) + CONTRIBUTING.md exist before the v0.0.1 tag | ✓ VERIFIED | Both on disk (LICENSE contains "MIT License"; CONTRIBUTING.md 79 lines); committed in 15d9349, which precedes tag commit 3469054 |

**Score:** 13/15 truths verified (2 partial)

### Required Artifacts

| Artifact | Expected | Status | Details |
|---|---|---|---|
| `crates/agent-memory-core/src/embed/mod.rs` | Embedder trait + EmbedError + FakeEmbedder, ≥80 lines | ✓ VERIFIED | 247 lines; Arc\<dyn Embedder\> consumed by service.rs |
| `crates/agent-memory-core/src/embed/ollama.rs` | OllamaClient batch embed + health, ≥60 lines | ✓ VERIFIED | 135 lines; wired via main.rs --ollama-url |
| `crates/agent-memory-core/sql/0002_embeddings.sql` | vec0 table + embedding_status + meta pin | ✓ VERIFIED | contains `USING vec0`; migration applied live ("Database migrated to version 2"). WR-06: meta pin is write-only (dead schema) — warning |
| `crates/agent-memory-core/src/store/sqlite.rs` | register_vec_extension, knn_search, vec-synced ops | ✓ VERIFIED | 504 lines; all patterns present (embedding MATCH, register_auto_extension). CR-01 defect in fn search |
| `crates/agent-memory-core/src/service.rs` | SearchMode/SearchOutcome + fallback + import | ✓ VERIFIED | 406 lines; degrade seam + ImportReport + import fn traced |
| `crates/agent-memory-core/tests/semantic.rs` | SEARCH-02 proof | ✓ VERIFIED | 287 lines; 3 pass + 1 ignored live test (ran: pass) |
| `crates/agent-memory-core/tests/fallback.rs` | SEARCH-03 kill-test | ✓ VERIFIED | 120 lines; 2/2 pass live |
| `.github/workflows/spike-cross-compile.yml` | dispatch orangepi zigbuild spike | ✓ VERIFIED | 99 lines; workflow_dispatch; run 28643212871 executed |
| `crates/agent-memory/src/rest/mod.rs` | Router + ApiError + bind guard, ≥60 lines | ✓ VERIFIED | 111 lines |
| `crates/agent-memory/src/rest/handlers.rs` | 5 thin handlers, ≥80 lines | ✓ VERIFIED | 441 lines (incl. 6 in-process tests); zero SQL |
| `crates/agent-memory/tests/rest.rs` | API-01 e2e proof, ≥80 lines | ✓ VERIFIED | 264 lines; spawned binary, real HTTP |
| `crates/agent-memory-core/src/import/gsd_state.rs` | Tolerant parser, ≥80 lines | ✓ VERIFIED | 278 lines; 9 inline unit tests |
| `crates/agent-memory-core/tests/fixtures/STATE.md` | Realistic fixture incl. traps, ≥40 lines | ✓ VERIFIED | 73 lines |
| `crates/agent-memory-core/tests/import.rs` | INTEROP-01 proof, ≥60 lines | ✓ VERIFIED | 99 lines; 1/1 pass live |
| `.github/workflows/release.yml` | Tag-triggered zigbuild release | ✓ VERIFIED | 296 lines; softprops/action-gh-release@v3; runs-on [orangepi]; release run succeeded (assets live) |
| `LICENSE` | MIT | ✓ VERIFIED | "MIT License", Copyright 2026 |
| `CONTRIBUTING.md` | ≥30 lines | ✓ VERIFIED | 79 lines |
| `README.md` | Full Phase 2 surface + brew install | ✓ VERIFIED | contains `brew install unityinflow/tap/agent-memory` (line 31) |
| `UnityInFlow/homebrew-tap Formula/agent-memory.rb` (external) | Per-arch formula | ✓ VERIFIED | Present in tap repo (gh API); brew-installed binary works locally |

### Key Link Verification

| From | To | Via | Status | Details |
|---|---|---|---|---|
| service.rs | embed/mod.rs | Arc\<dyn Embedder\> injection | ✓ WIRED | 3 matches; embed() called in search/store/import/sweep paths |
| sqlite.rs | vec_memories | KNN CTE `embedding MATCH` | ✓ WIRED | line 251, bound blob + k |
| mcp.rs | SearchOutcome | envelope with top-level search_mode | ✓ WIRED | mcp.rs:176,191 |
| sqlite.rs | sqlite3_vec_init | register_auto_extension before open | ✓ WIRED | line 57 |
| rest/handlers.rs | MemoryService | State\<Arc\<AppState\>\> | ✓ WIRED | lines 110,131+ |
| rest/handlers.rs | SearchOutcome | shared envelope | ✓ WIRED | line 159 |
| main.rs | rest/mod.rs | ServeRest subcommand | ✓ WIRED | main.rs:66,119 |
| main.rs | import::gsd_state | Import → parse_gsd_state → service.import | ✓ WIRED | main.rs:123,224,238 |
| service.rs | sqlite.rs | Store::exists dedup probe | ✓ WIRED | store/mod.rs:53, sqlite.rs:317; idempotency proven live |
| gsd_state.rs | NewMemory | parser output = standard store input | ✓ WIRED | 5 matches |
| release.yml | orangepi runner | runs-on: [orangepi] | ✓ WIRED | 3 jobs, lines 54/91/204 |
| homebrew formula | Release v0.0.1 assets | per-arch url + sha256 | ✓ WIRED | Formula live in tap; installed binary = 0.0.1; assets gh-verified |
| spike workflow | release | spike-green-before-tag gate | ✓ WIRED | Run 28643212871: 4/4 required legs success, created 06:37Z; tag at ~07:02Z |
| sqlite.rs fn search | args.tag | keyword-mode tag filter | ✗ NOT_WIRED | **CR-01: tag predicate absent from keyword SQL — the one broken link found** |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|---|---|---|---|---|
| service.rs search | SearchOutcome.results | knn_search / store.search → real SQLite rows | Yes (live semantic + keyword runs) | ✓ FLOWING |
| rest search_handler | SearchOutcome | same service call as MCP | Yes (rest.rs asserts non-empty envelope) | ✓ FLOWING |
| import path | ImportReport counts | real inserts via standard store path | Yes (live: 9 → 0) | ✓ FLOWING |
| sweep backfill | pending_embeddings → insert_embedding | read pool → writer TX | Yes (traced; WR-08 race window is a warning) | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|---|---|---|---|
| Brewed binary version | `/opt/homebrew/bin/agent-memory --version` | `agent-memory 0.0.1` | ✓ PASS |
| MCP initialize, stdout purity | pipe initialize into `serve` (scratch DB) | Pure JSON-RPC frame on stdout; migration v2 + logs on stderr | ✓ PASS |
| Import idempotency (real binary, dead Ollama) | double `import --from gsd-state` on fixture | run1 `imported: 9`, run2 `imported: 0, skipped duplicates: 9` | ✓ PASS |
| SEARCH-03 kill-test | prebuilt `fallback` test binary | 2 passed, 0 failed | ✓ PASS |
| SEARCH-02 golden set + INTEROP-01 suite | prebuilt `semantic` + `import` test binaries | 3+1 passed | ✓ PASS |
| SC1 live semantic recall | `semantic --ignored` against local Ollama | `live_ollama_semantic_recall_without_shared_keywords ... ok` | ✓ PASS |
| Release assets | `gh release view v0.0.1` | 4 required tarballs + SHA256SUMS.txt | ✓ PASS |
| Spike gate per-leg | `gh run view 28643212871 --json jobs` | 4 DIST legs success; musl pair failure (documented best-effort) | ✓ PASS |
| REST live HTTP | — | Not re-run (verifier must not start servers); covered by tests/rest.rs + today's UAT | ? SKIP |

### Probe Execution

No `scripts/*/tests/probe-*.sh` probes exist and none are declared in any PLAN/SUMMARY — SKIPPED (no probes in this project's convention).

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|---|---|---|---|---|
| SEARCH-02 | 02-01 | Semantic similarity via local Ollama | ✓ SATISFIED | Live test pass + code trace |
| SEARCH-03 | 02-01 | Graceful degrade to keyword | ⚠️ SATISFIED WITH DEFECT | Core behavior proven; tag filter silently dropped in degraded mode (gap 1) |
| API-01 | 02-02 | REST store/search/list/forget | ⚠️ SATISFIED WITH DEFECT | All 4 ops + health wired and tested; never-500 contract falsified for malformed FTS5 queries (gap 2) |
| INTEROP-01 | 02-03 | GSD STATE.md import | ✓ SATISFIED | Live idempotent double-run |
| DIST-01 | 02-04 | Binaries: macOS arm64/x86_64 + Linux x86_64/aarch64 | ✓ SATISFIED | All 4 tarballs + SHA256SUMS live on Release v0.0.1 (musl was best-effort, deferred with rationale) |
| DIST-02 | 02-04 | Homebrew formula | ✓ SATISFIED | Formula in tap; brew-installed 0.0.1 answers MCP initialize on this machine |

No orphaned requirements: REQUIREMENTS.md maps exactly these 6 IDs to Phase 2 and every one is claimed by a plan.

### Anti-Patterns Found

No TBD/FIXME/XXX debt markers in any phase-modified file. No stub returns, no placeholder implementations, no console-log-only handlers. Findings below are from the code review (02-REVIEW.md), spot-confirmed where they bear on must-haves:

| File | Line | Pattern | Severity | Impact |
|---|---|---|---|---|
| store/sqlite.rs | 384-413 | CR-01: keyword search drops `tag` filter | 🛑 Blocker (gap 1) | Degraded mode returns rows the caller excluded — silent wrong results |
| rest/handlers.rs | 86-95 | WR-05: malformed FTS5 query → 500 | 🛑 Blocker (gap 2) | Falsifies plan 02-02 truth "never a 500 for bad input"; mode-dependent |
| mcp.rs | 138-222 | WR-04: all errors → invalid_params | ⚠️ Warning | Internal failures misreported; transports diverge at error tier (fix with gap 2) |
| store/sqlite.rs | 244,196 | WR-01: unvalidated limit/ttl_secs extremes | ⚠️ Warning | Overflow panic (debug) / wrap (release) |
| store/sqlite.rs | 374,399 | WR-02: negative limit unbounded on keyword path | ⚠️ Warning | Divergent extremes between modes |
| store/sqlite.rs | 6 sites | WR-03: poisoned mutex → NotFound (404) | ⚠️ Warning | Wrong error taxonomy |
| sql/0002_embeddings.sql | 21-26 | WR-06: meta model/dim pin write-only | ⚠️ Warning | Promised drift guard is dead schema |
| store/sqlite.rs | 258,348 | WR-07: tag LIKE substring over-match, unescaped | ⚠️ Warning | Loose filter semantics |
| store/sqlite.rs / service.rs | 278-299 | WR-08: backfill race can orphan a vector | ⚠️ Warning | Transient invariant violation, self-healing |
| tests/stdio_purity.rs | 69-72 | WR-09: asserts rmcp-internal log strings | ⚠️ Warning | MCP-05 gate fragile to rmcp upgrades |
| import/gsd_state.rs, service.rs, ollama.rs | various | IN-01..IN-07 | ℹ️ Info | See 02-REVIEW.md |

### Human Verification Required

Harvested from plan 02-04's end-of-phase `<human-check>` (the only deferred item). Today's 8-step UAT on this machine already covered semantic recall, keyword fallback, REST CRUD, import idempotency, brew binary MCP initialize, and cross-session MCP recall — the remaining item is the one thing this Mac cannot execute:

### 1. Second-machine install (other arch legs)

**Test:** On a Linux (x86_64 or aarch64) or Intel-Mac machine: `brew install unityinflow/tap/agent-memory && agent-memory --version`, then confirm an MCP client (Claude Code/Cursor) launches the brewed binary from `.mcp.json`.
**Expected:** Version prints 0.0.1; MCP server initializes.
**Why human:** This darwin-arm64 machine cannot execute the x86_64-darwin or Linux binaries; CI smoke was host-arch-aware (aarch64-linux only). The plan itself marks this optional — SC5 was defined as proven locally for darwin-arm64 + brew, structurally for the other triples.

### Gaps Summary

The phase goal is substantially achieved — semantic recall, keyword fallback, REST mirror, idempotent import, and the installable release all exist, are wired, and were re-proven live during this verification (including the live-Ollama semantic test and an import double-run through the brewed binary). Two must-have truths are only partially true, both confirmed by first-hand code trace, both small focused fixes at the same storage/error seams:

1. **CR-01 (gap 1):** the FTS5 keyword fallback ignores the `tag` filter the semantic path honors and both transports document. The degraded mode SEARCH-03 promises returns silently wrong (unfiltered) results for tag-filtered callers — e.g. the importer's own `gsd`/`deferred` tag partitioning breaks exactly when Ollama is down. One predicate + one bind + one regression test.
2. **WR-05 (gap 2):** a malformed FTS5 query string returns HTTP 500 in keyword mode, falsifying plan 02-02's "never a 500 for bad input" truth and rest/mod.rs's own written contract. Fixing it at the service seam also closes WR-04 (MCP's inverted error mapping), restoring the "one dialect" property API-01 claims.

Neither gap invalidates the shipped v0.0.1 release for its primary flows, but both are correctness defects inside behavior the phase's success criteria explicitly promise, so they block a clean pass. Recommend `/gsd-plan-phase --gaps` for a small gap-closure plan (both fixes are test-first one-seam changes), optionally folding in WR-01/WR-02 (integer clamps) which touch the same functions.

---

_Verified: 2026-07-03T08:20:00Z_
_Verifier: Claude (gsd-verifier)_
