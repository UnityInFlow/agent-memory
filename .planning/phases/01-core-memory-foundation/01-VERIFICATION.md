---
phase: 01-core-memory-foundation
verified: 2026-06-25T22:40:00Z
status: passed
score: 19/19 must-haves verified
overrides_applied: 0
re_verification:
  previous_status: none
  previous_score: n/a
---

# Phase 1: Core Memory Foundation Verification Report

**Phase Goal:** An agent can store typed memories and retrieve them by keyword over an MCP stdio server, surviving restarts, with decay down-ranking and TTL expiry — entirely locally, no Ollama, no cloud.
**Verified:** 2026-06-25T22:40:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

The phase goal is observably true in the codebase. The four-tool MCP surface was driven live over real stdio (behavioral spot-check below): `initialize` → `memory_store(DECISION)` → returns id `1`; `memory_search("coroutines")` → returns the stored row with `decay_score: 1.0`, `scope: null`; `memory_store(type=BOGUS)` → clean JSON-RPC `-32602` error (no panic); `tools/list` → all four tools with rich JSON Schema. All stdout output is valid JSON-RPC. No Ollama, no network — the non-dev dependency tree contains no HTTP client.

### Observable Truths

| #  | Truth (source) | Status | Evidence |
|----|----------------|--------|----------|
| 1  | `agent-memory serve` starts an rmcp stdio server answering `initialize` with valid JSON-RPC (P01) | VERIFIED | Live spot-check: `initialize` returned `{"jsonrpc":"2.0","id":1,"result":{...serverInfo:rmcp 1.8.0...}}`; `tests/stdio_purity.rs` green |
| 2  | `memory_store` returns a numeric id; omitting scope stores NULL (P01) | VERIFIED | Spot-check returned id `1`; `tests/tools.rs:160-166` asserts persisted `scope IS NULL` when omitted |
| 3  | Each of the 6 types persists a row readable after kill+restart (P01) | VERIFIED | `tests/store.rs:45` `six_typed_rows_survive_store_drop_reopen` — store→drop→reopen same temp-file path → 6 rows (green) |
| 4  | Invalid memory type rejected with a clean error, never a panic (P01) | VERIFIED | Spot-check `type=BOGUS` → `error -32602 invalid_params`; `domain.rs:66` returns `Err(InvalidType)`; `mcp.rs:116-117` maps to `McpError::invalid_params` |
| 5  | `memory_list` returns stored memories newest-first with type/scope/limit filters (P01) | VERIFIED | `sqlite.rs:160-186` `ORDER BY created_at DESC, id DESC` + parameterized filters + `LIMIT`; `tests/store.rs` asserts order/scope/limit |
| 6  | Every stdout line is JSON-RPC; all logs go to stderr (P01, MCP-05) | VERIFIED | `tests/stdio_purity.rs` asserts every non-empty stdout line parses as jsonrpc=2.0 and "Service initialized"/"Database migrated" appear on stderr; `main.rs:56-62` stderr-only subscriber + panic hook |
| 7  | No network call on store/list path — works offline (P01) | VERIFIED | `cargo tree -e no-dev` has no reqwest/hyper/ollama/tonic; core crate has no http dep |
| 8  | D-03: scope is a logical column on one shared store; list filters by column, not by switching DBs (P01) | VERIFIED | Schema `scope TEXT` (one DB); `sqlite.rs:170` `(?2 IS NULL OR scope = ?2)`; single `SqliteStore` per process |
| 9  | D-05: rich `memory_store` schema, only content+type required, rest default (P01) | VERIFIED | `tools/list` schema: required `[content,type]`, defaults tags=`[]`, source/scope/ttl_secs `null`; bare `{content,type}` call succeeded live |
| 10 | `memory_search` returns FTS5 results blended with decay, no Ollama/network (P02, MCP-02/SEARCH-01) | VERIFIED | `sqlite.rs:207-223` FTS5 MATCH JOIN with `(-bm25)*w_rel + exp(decay)*w_decay` ORDER BY; spot-check returned the match offline |
| 11 | A no-match query returns an empty list, not an error (P02) | VERIFIED | `service.rs:87` returns `Ok(vec![])`; `tests/tools.rs:213` `...then_empty_on_no_match`; `tests/decay.rs` no-match case |
| 12 | Each result exposes decay score; just-accessed outranks older un-accessed (P02, STORE-03) | VERIFIED | `apply_decay` (`service.rs:99-101`) recomputes on read; `tests/decay.rs` ranking assertion green; live result carried `decay_score` |
| 13 | Retrieving bumps last_accessed/access_count (recency bump) (P02) | VERIFIED | `service.rs:106-112` fire-and-forget `bump_access`; `sqlite.rs:258-280` writer-lane parameterized UPDATE |
| 14 | `memory_forget`: existing id deleted (search/list omit it); unknown id clean not-found, not error (P02, MCP-04) | VERIFIED | `sqlite.rs:251-256` returns `changes()>0`; `service.rs:119` returns bool; `mcp.rs` maps `Ok(false)` to a not-found result; `tests/tools.rs` forget cases green |
| 15 | TTL-given memory is gone after the sweep once expires_at is past (P03, STORE-04) | VERIFIED | `sqlite.rs:282-292` `sweep_expired` bounded DELETE; `tests/ttl.rs:38` `sweep_removes_expired_rows_from_list_and_search` green |
| 16 | Near-zero-decay no-TTL memory still retrievable — decay never deletes (P03, STORE-04) | VERIFIED | `tests/ttl.rs:91` `decay_never_deletes_low_score_no_ttl_memory_stays_retrievable` (20 half-lives, ~1e-6) green; `materialize_decay` is UPDATE-only |
| 17 | Sweep materializes each decay_score; pinned types decay slower (P03) | VERIFIED | `sqlite.rs:294-311` bulk UPDATE with pinned CASE; `tests/ttl.rs` `materialized_decay_matches_on_read_recompute_and_pinned_outranks` green |
| 18 | Background sweep runs on an interval while serving, using injectable clock (P03) | VERIFIED | `main.rs:112-127` `spawn_sweep_task` on `tokio::time::interval(SWEEP_INTERVAL)` before `serve(stdio())`, `service.sweep(clock.now())`; `service.sweep` deterministic under TestClock |
| 19 | CI runs fmt/clippy -D warnings/test/stdout-purity + >80% coverage on self-hosted runners, never ubuntu-latest (P03) | VERIFIED | `.github/workflows/ci.yml` matrix `[arc-runner-unityinflow, orangepi]`, no ubuntu-latest; steps fmt-check → clippy → build → test → llvm-cov `--fail-under-lines 80`; coverage gate **executed locally: exit 0, 92.42% line coverage** |

**Score:** 19/19 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `Cargo.toml` | workspace root | VERIFIED | `[workspace]` with both member crates; edition 2021 |
| `crates/agent-memory-core/src/domain.rs` | MemoryType (6) + TryFrom + is_pinned + MemoryError | VERIFIED | 6 variants, UPPERCASE wire, `is_pinned` for Decision/Architecture/Constraint, thiserror enum |
| `crates/agent-memory-core/src/clock.rs` | Clock + SystemClock + TestClock | VERIFIED | 100% coverage; TestClock feature-gated |
| `crates/agent-memory-core/src/store/sqlite.rs` | WAL+single-writer+r2d2 pool, all CRUD+sweep | VERIFIED | All SQL parameterized; bm25 sign rule; sweep_expired only-delete; 91.33% line cov |
| `crates/agent-memory-core/sql/0001_init.sql` | memories table + FTS5 mirror + 3 triggers | VERIFIED | FTS5 external-content + ai/ad/au triggers; NO embedding column (Phase-2 boundary honored) |
| `crates/agent-memory-core/src/service.rs` | store/list/search/forget/sweep | VERIFIED | All over spawn_blocking; 100% function cov |
| `crates/agent-memory-core/src/decay.rs` | decay_score + DecayEngine + apply_decay | VERIFIED | 30-day default, pinned 6×, sweep delete-then-rescore; 100% function cov |
| `crates/agent-memory/src/mcp.rs` | 4 rmcp tools | VERIFIED | memory_store/list/search/forget present; thin adapters, no SQL |
| `crates/agent-memory/src/main.rs` | tokio main, clap, stderr logging, panic hook, sweep task | VERIFIED | All present + `serve(stdio())` |
| `crates/agent-memory/src/config.rs` | --db > env > data_dir, 0700 dir | VERIFIED | D-01/D-02 precedence; 0700 on Unix, Windows fallback isolated |
| `tests/stdio_purity.rs` | MCP-05 gate | VERIFIED | green |
| `.github/workflows/ci.yml` | self-hosted CI + coverage | VERIFIED | self-hosted only; coverage gate runs |
| `README.md` | tools + .mcp.json + offline note | VERIFIED | present, documents four tools |

### Key Link Verification

| From | To | Via | Status |
|------|----|----|--------|
| `mcp.rs` | `service.rs` | tools call `self.state.service.store/list/search/forget` | WIRED |
| `service.rs` | `store/sqlite.rs` | spawn_blocking → Store trait methods | WIRED |
| `main.rs` | `config.rs` | `resolve_db_path` then open SqliteStore | WIRED |
| `main.rs` | `service.rs` | bg interval → `service.sweep(clock.now())` | WIRED |
| `decay.rs` | `store/sqlite.rs` | `DecayEngine::sweep` → `sweep_expired` + `materialize_decay` | WIRED |
| `sqlite.rs` | `memories_fts` | FTS5 MATCH joined, ordered by negated-bm25 × decay | WIRED |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Server answers initialize | piped JSON-RPC to `agent-memory serve` | valid jsonrpc=2.0 result | PASS |
| memory_store returns id | tools/call memory_store {content,type:DECISION} | id `1` | PASS |
| memory_search FTS5 offline | tools/call memory_search {query:coroutines} | 1 match, decay_score 1.0, scope null | PASS |
| invalid type clean error | tools/call memory_store {type:BOGUS} | error -32602, no panic | PASS |
| tools/list rich schema | tools/list | 4 tools, JSON Schema, required=[content,type] | PASS |
| offline (no http dep) | cargo tree -e no-dev | no reqwest/hyper/ollama | PASS |

### Probe Execution

| Probe | Command | Result | Status |
|-------|---------|--------|--------|
| Workspace tests | `cargo test --workspace` | 32 passed / 8 suites (lib 13, decay 4, store 4, ttl 4, bin-unit 2, stdio_purity 1, tools 4) | PASS |
| Lint gate | `cargo clippy --workspace --all-targets -- -D warnings` | clean (exit 0) | PASS |
| Format gate | `cargo fmt --check` | clean (exit 0) | PASS |
| Coverage gate | `cargo llvm-cov --workspace --fail-under-lines 80` | exit 0; TOTAL 92.42% lines (core: clock 100%, decay 100%, service 100%, domain 98.31%, sqlite 91.33%) | PASS |

### Requirements Coverage

| Requirement | Source Plan | Status | Evidence |
|-------------|-------------|--------|----------|
| STORE-01 typed storage | 01-01 | SATISFIED | 6-type insert + invalid-type reject; `tests/store.rs` |
| STORE-02 durable persistence | 01-01 | SATISFIED | store→drop→reopen 6 rows; no http dep |
| STORE-03 decay scoring surfaced | 01-02/01-03 | SATISFIED | `apply_decay` on read + `materialize_decay`; `tests/decay.rs`/`ttl.rs` |
| STORE-04 TTL ≠ decay-delete | 01-03 | SATISFIED | `sweep_expired` only delete; near-zero-decay survives; `tests/ttl.rs` |
| MCP-01 memory_store | 01-01 | SATISFIED | live id returned; `tests/tools.rs` |
| MCP-02 memory_search ranked | 01-02 | SATISFIED | FTS5 bm25×decay; live + `tests/decay.rs`/`tools.rs` |
| MCP-03 memory_list filters | 01-01 | SATISFIED | parameterized filters + order + limit; `tests/store.rs` |
| MCP-04 memory_forget | 01-02 | SATISFIED | delete + clean not-found; `tests/tools.rs` |
| MCP-05 stdio purity | 01-01 | SATISFIED | `tests/stdio_purity.rs` + live stdout all-JSON-RPC |
| SEARCH-01 keyword FTS5 no embeddings | 01-02 | SATISFIED | FTS5 only, no embedding column, offline |

All 10 declared Phase-1 requirement IDs accounted for across plan frontmatter and REQUIREMENTS.md (Phase-1 mapping: STORE-01..04, MCP-01..05, SEARCH-01). No orphaned requirements. (Note: REQUIREMENTS.md traceability table still marks STORE-01/02, MCP-01/03/05 as "Pending" — a stale status field, not a code gap; all are implemented and tested. Recommend the orchestrator flip these to Complete.)

### SPEC Boundary & CLAUDE.md Compliance

| Check | Status | Evidence |
|-------|--------|----------|
| No embedding/sqlite-vec/vec0 column (Phase-2 boundary) | VERIFIED | `0001_init.sql` has no embedding column; comment confirms Phase-2 deferral |
| FTS5 keyword-only search | VERIFIED | `memories_fts` FTS5 MATCH; no vector search |
| Works with no Ollama / no cloud | VERIFIED | no http client in non-dev tree; live search offline |
| No `unwrap()`/`expect()` in production code | VERIFIED | all occurrences inside `#[cfg(test)]` modules (domain.rs:140, config.rs:56, migrations.rs:13); sqlite.rs uses only `unwrap_or_*` |
| Edition 2021 | VERIFIED | workspace `edition = "2021"`, members inherit |
| Self-hosted CI runners, never ubuntu-latest | VERIFIED | matrix `[arc-runner-unityinflow, orangepi]` only |
| thiserror in lib / anyhow at edges | VERIFIED | core `MemoryError` (thiserror); binary uses `anyhow::Context` |
| Parameterized SQL (no injection) | VERIFIED | all queries bound; `bump_access` builds placeholder count only, ids bound |

### Context Decisions D-01..D-09

All honored: D-01 (dirs data_dir default), D-02 (--db > env > default precedence, config.rs), D-03 (scope = logical column, one DB), D-04 (bm25×decay blend), D-05 (rich schema, content+type required), D-06 (MemoryView return shape), D-07 (clean invalid-type error, live-proven), D-08 (pinned types Decision/Architecture/Constraint, 6× slower), D-09 (~30-day half-life default, configurable).

### Anti-Patterns Found

None. The only grep hits for "PLACEHOLDER" were the SQL variable name `placeholders` (parameter binding), not a debt marker. No TBD/FIXME/XXX/HACK/unimplemented! in source.

### Human Verification Required

None. All truths were verifiable programmatically — including the coverage gate, which was installed and executed (not merely CI-wired), and the four-tool MCP surface, which was driven live over real stdio.

### Notes (non-blocking)

- `CONTRIBUTING.md` and `LICENSE` are absent. These are ecosystem pre-v0.0.1 release artifacts that SPEC explicitly defers to Phase 2 ship ("Pre-built binaries + Homebrew release — Phase 2"); 01-03-SUMMARY flags them as remaining release items. Not a Phase-1 must-have — no gap.
- REQUIREMENTS.md traceability statuses for STORE-01/02, MCP-01/03/05 read "Pending" despite being complete and tested — a documentation lag worth correcting.

### Gaps Summary

No gaps. Every Phase-1 must-have across all three plans is verified against source and by execution. The walking skeleton (store/list), the search/forget slice, and the TTL/decay-sweep + ship gate are all real, wired, and behaviorally proven. The phase goal — store typed memories, retrieve by keyword over MCP stdio, survive restarts, decay down-ranks, TTL expires, fully local with no Ollama/cloud — is achieved.

---

_Verified: 2026-06-25T22:40:00Z_
_Verifier: Claude (gsd-verifier)_
