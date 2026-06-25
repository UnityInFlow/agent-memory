# Walking Skeleton — agent-memory

**Phase:** 1
**Generated:** 2026-06-25

## Capability Proven End-to-End

An MCP client pipes `initialize`, then `tools/call memory_store {content, type}`, then `tools/call memory_list` to the `agent-memory serve` stdio binary, gets a numeric memory id back and the stored row back — over real stdio and a real on-disk SQLite database, with the row surviving a process restart and nothing but JSON-RPC ever reaching stdout.

(The skeleton is delivered by Plan 01-01; Plans 01-02 and 01-03 thicken it with search/forget, decay, and TTL on top of these decisions without changing them.)

## Architectural Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Language / edition | Rust stable, edition 2021 | Ecosystem rule (CLAUDE.md); dependency-free low-footprint local daemon |
| Crate layout | Cargo workspace: `agent-memory-core` (lib, `thiserror`) + `agent-memory` (bin, `anyhow`) | anyhow-at-edges / thiserror-in-lib split (CLAUDE.md); core is transport-agnostic and reused by all future transports (REST in Phase 2) + tests |
| MCP transport | Official `rmcp` 1.8 (`server`, `transport-io`, `macros`) over stdio; `#[tool_router]`/`#[tool]`/`Parameters<T>`/`#[tool_handler]` | Official SDK handles JSON-RPC framing, dispatch, JSON Schema gen; verified verbatim in RESEARCH Pattern 1/2. mcp-hub is an MCP *client* — its dispatcher is the inverse and is NOT reused |
| Data layer | embedded SQLite via `rusqlite` 0.40 `bundled` (SQLite compiled in); WAL journal; single serialized writer + `r2d2` read pool; all DB work in `spawn_blocking` | Zero system dependency, zero cloud (core value); WAL + single-writer avoids `SQLITE_BUSY` (Pitfall 11); rusqlite is sync so it must not block the tokio runtime (Pitfall 2) |
| Schema versioning | `rusqlite_migration` 2.6 (`user_version`), `M::up(include_str!("sql/0001_init.sql"))` | Ordered, idempotent, forward-compatible with the Phase-2 embedding-column ALTER |
| Search | SQLite FTS5 external-content mirror (`memories_fts`, trigger-synced), ranked by negated `bm25()` blended with `decay_score` | FTS5 is in bundled SQLite; works with no Ollama (SEARCH-01); semantic search is a Phase-2 enhancement |
| Time / decay | injectable `Clock` trait (`SystemClock` + `TestClock`), UTC epoch i64 everywhere; exponential decay (~30-day half-life, configurable), pinned types decay slower | Deterministic decay/TTL under test (Pitfall 8); decay only down-ranks, never deletes (STORE-04) |
| DB path | `--db` flag > `AGENT_MEMORY_DB` env > `dirs::data_dir()/agent-memory/memory.db`; dir created `0700` | One global brain shared across runtimes (D-01/D-02), per-project isolation when wanted (D-03 scope is a column, not a file); `0700` prevents other local users reading the corpus (Security V4) |
| Logging | `tracing` → `std::io::stderr` only, `with_ansi(false)`, panic hook → stderr | stdout is reserved for the JSON-RPC channel; any stray stdout byte corrupts the client (MCP-05, Pitfall 1) |
| CI / distribution | self-hosted runners `[arc-runner-unityinflow, orangepi]` only (never `ubuntu-latest`); `cargo-llvm-cov --fail-under-lines 80` | Ecosystem CI policy (CLAUDE.md); orangepi leg is the documented Hetzner-offline fallback |

## Stack Touched in Phase 1

- [x] Project scaffold — Cargo workspace, two crates, `cargo build`/`clippy`/`fmt`, test runner (`cargo test` + `cargo-llvm-cov`), CI workflow
- [x] Routing — the MCP tool surface: `memory_store`, `memory_search`, `memory_list`, `memory_forget` over stdio JSON-RPC
- [x] Database — real reads (list/search) AND real writes (store/forget/recency-bump/TTL-delete/decay-materialize) against on-disk SQLite
- [x] UI — the interactive surface is the MCP stdio protocol itself (an agent calling tools); proven by the piped-JSON-RPC integration tests
- [x] Deployment — documented local full-stack run command: `agent-memory serve --db <path>` wired via `.mcp.json` (binary release is Phase 2)

## Out of Scope (Deferred to Later Slices)

Explicit — to stop future phases re-litigating Phase 1's minimalism:

- Semantic search / Ollama embeddings / `sqlite-vec` vec0 table — Phase 2 (NO embedding column in the Phase-1 schema; added via a Phase-2 migration)
- REST API mirror — Phase 2
- GSD STATE.md import + any CLI subcommand beyond `serve` — Phase 2
- Pre-built binaries + Homebrew formula — Phase 2 (DIST-01/02)
- Windows support — v2 (DIST-03); platform path code centralized in `config.rs`, not scattered `cfg(unix)`
- Hybrid keyword+semantic RRF ranking fusion (SEARCH-04), `memory_update`/relation tools (MCP-06), portable export (DIST-04) — v2
- Knowledge-graph relations, LLM-on-write extraction, ANN index, web UI — out of scope (REQUIREMENTS.md)

## Subsequent Slice Plan

Each later phase adds one vertical slice on top of this skeleton without altering its architectural decisions:

- **Phase 1, Plan 02:** `memory_search` (FTS5 bm25×decay) + `memory_forget` + decay surfacing — completes the four-tool surface.
- **Phase 1, Plan 03:** TTL sweep + decay materialization engine + background task + CI/coverage gate + README — completes the resilience baseline and ships v0.0.1 shape.
- **Phase 2:** local Ollama semantic search (graceful keyword fallback), REST API mirror, `agent-memory import --from gsd-state`, and the cross-platform binary + Homebrew release.
