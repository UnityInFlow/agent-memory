# Phase 1: Core Memory Foundation — Specification

**Created:** 2026-06-24
**Ambiguity score:** 0.13 (gate: ≤ 0.20)
**Requirements:** 9 locked

## Goal

An agent can store typed memories and retrieve them by keyword over an MCP stdio server, with the data surviving process restarts, decay down-ranking stale memories, and TTL expiring them — entirely locally, with no Ollama and no cloud.

## Background

The repository is greenfield: no `Cargo.toml` and no `*.rs` files exist yet. The harness (RTK, Superpowers, GSD, memtrace, Rust hooks, `.mcp.json`) and the planning docs (`PROJECT.md`, `REQUIREMENTS.md`, `ROADMAP.md`, research in `.planning/research/`) are in place. Every Phase 1 deliverable below describes something that does not exist today. This phase builds the foundation that Phase 2 (semantic search + REST + import + release) strictly enhances. Per the research, the hard-to-retrofit decisions (stdout purity, WAL + single-writer concurrency, injectable Clock/UTC timestamps, decay-never-deletes) must be locked here.

## Requirements

1. **Typed memory storage**: A memory record persists a type, content, optional metadata, and timestamps in embedded SQLite.
   - Current: No storage layer exists
   - Target: A `memories` table stores `id`, `type` ∈ {DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT}, `content`, `tags`, `source`, optional `scope`, `created_at`, `last_accessed`, `access_count`, `decay_score`, `expires_at` (TTL). `rusqlite` with the `bundled` feature (SQLite compiled in — no system dependency)
   - Acceptance: Storing a memory of each of the 6 types persists a row readable after reopen; an invalid type is rejected with an error (no panic)

2. **Durable local persistence**: Stored memories survive a full process restart with zero cloud and zero account.
   - Current: Nothing persists
   - Target: SQLite database file in a local data dir; WAL journal mode; opening an existing DB returns prior data
   - Acceptance: Store N memories, kill the process, restart, and `memory_list`/`memory_search` return all N — with no network call made at any point (verifiable: works offline)

3. **Exponential decay scoring**: Each memory carries a decay score that decreases with age/disuse and is surfaced in results.
   - Current: No scoring exists
   - Target: `decay_score` computed by exponential decay over time-since-`last_accessed`; recency bump (`last_accessed`, `access_count`) on every retrieval; decay computed via an injectable `Clock` over UTC timestamps so it is deterministic in tests
   - Acceptance: With an injected clock advanced by T, an un-accessed memory's score is strictly lower than a just-accessed one; search results expose the score; a unit test asserts the decay curve deterministically

4. **TTL expiry, separate from decay**: A memory may carry a TTL after which it is removed; decay never deletes.
   - Current: No expiry exists
   - Target: Optional `expires_at`; a sweep removes expired rows; decay only down-ranks. Removal happens ONLY via TTL expiry or explicit `memory_forget`
   - Acceptance: A memory past its TTL is gone after a sweep; a memory with a near-zero decay score but no TTL is still retrievable (proves decay ≠ deletion)

5. **MCP `memory_store` tool**: An agent stores a typed memory and receives its id.
   - Current: No MCP server exists
   - Target: `memory_store({ content, type, tags?, source?, scope?, ttl? })` inserts a row and returns its id; `scope` is explicit and optional (NULL = global)
   - Acceptance: A `tools/call` for `memory_store` returns a valid id; the row exists with the supplied fields; omitting `scope` stores NULL

6. **MCP `memory_search` tool (keyword/FTS5)**: An agent retrieves relevant memories by keyword with no embedding model required.
   - Current: No search exists
   - Target: `memory_search({ query, type?, tag?, scope?, limit? })` runs SQLite FTS5 over content, ranked by a combination of match relevance and decay score; returns content, type, metadata, and decay score
   - Acceptance: A query matching stored content returns those memories ranked sensibly, with Ollama not installed/running; an empty/no-match query returns an empty list (not an error)

7. **MCP `memory_list` tool**: An agent enumerates memories with filters.
   - Current: No list exists
   - Target: `memory_list({ type?, tag?, scope?, limit? })` returns memories matching the filters, newest first by default
   - Acceptance: Listing with each filter returns exactly the matching rows; `limit` caps the count

8. **MCP `memory_forget` tool**: An agent deletes a memory by id.
   - Current: No delete exists
   - Target: `memory_forget({ id })` deletes the row and returns success/notfound
   - Acceptance: Forgetting an existing id removes it (subsequent search/list omit it); forgetting an unknown id returns a not-found result without error

9. **MCP stdio protocol purity**: The server speaks JSON-RPC over stdio without corruption.
   - Current: No server exists
   - Target: MCP server over stdio (official `rmcp` SDK, `transport-io`); ALL logging goes to stderr; nothing but JSON-RPC is ever written to stdout; panics do not leak to stdout
   - Acceptance: Piping an `initialize` request to stdin yields only lines that parse as valid JSON-RPC on stdout; a CI test asserts stdout purity while logs appear on stderr

## Boundaries

**In scope:**
- A Rust workspace: `agent-memory-core` library (`thiserror`) + `agent-memory` binary (`anyhow` at edges)
- SQLite schema (`memories` table + FTS5 index), migrations, WAL + single-writer connection strategy
- Memory CRUD over the 6 types with explicit optional `scope`
- Exponential decay scoring with injectable `Clock`; TTL expiry sweep
- MCP stdio server exposing `memory_store`, `memory_search`, `memory_list`, `memory_forget`
- FTS5 keyword search (works with no Ollama)
- Unit/integration tests to >80% coverage on core logic; stdout-purity CI test
- macOS + Linux targets

**Out of scope:**
- Semantic search / Ollama embeddings / `sqlite-vec` vec0 table — Phase 2 (schema stays relational+FTS5; embedding column added later via migration)
- REST API — Phase 2
- GSD STATE.md import + CLI subcommands beyond the MCP server — Phase 2
- Pre-built binaries + Homebrew release — Phase 2
- Windows support — deferred to v2 (DIST-03), per the mcp-hub `cfg(unix)` precedent; path code need not be Windows-safe in Phase 1
- Hybrid (keyword+semantic) ranking fusion — v2 (SEARCH-04)
- Knowledge-graph relations, LLM-on-write extraction, ANN index, web UI — out of scope (REQUIREMENTS.md)

## Constraints

- Rust stable, edition 2021; JVM/Python runtime dependency forbidden
- `rusqlite` with `bundled` feature (no system SQLite); WAL mode; a single serialized writer lane (stores, recency bumps, decay sweep, TTL deletes) to avoid `SQLITE_BUSY`
- `clap` (derive) for the binary; `tokio` async runtime; `serde`/`serde_json`; `anyhow` (binary) / `thiserror` (library)
- No `unwrap()` in production code; exhaustive pattern matching (no catch-all `_` unless justified)
- Timestamps are UTC; time accessed through an injectable `Clock` so decay/TTL are deterministic under test
- Logging to stderr only — stdout is reserved for the MCP JSON-RPC channel
- `cargo clippy -- -D warnings` clean; `cargo fmt`; >80% coverage on core logic before phase completion
- MCP SDK: official `rmcp` (`transport-io`); MIT license

## Acceptance Criteria

- [ ] A Rust workspace builds (`cargo build`) with a `agent-memory-core` library and `agent-memory` binary; no `unwrap()` in non-test code; clippy clean
- [ ] Storing a memory of each of the 6 types persists and is readable after a process restart, with no network access
- [ ] `memory_store`, `memory_search`, `memory_list`, `memory_forget` are callable over MCP stdio and behave per their requirements
- [ ] `memory_search` returns keyword (FTS5) results with Ollama absent; no-match returns an empty list, not an error
- [ ] Decay score decreases for un-accessed memories under an injected clock and is surfaced in results; a near-zero-decay memory without TTL is still retrievable (decay never deletes)
- [ ] A memory past its TTL is removed by the sweep; `memory_forget` removes by id; unknown id is a clean not-found
- [ ] Piping `initialize` to the server's stdin yields only valid JSON-RPC on stdout while logs go to stderr (CI-asserted)
- [ ] Core-logic test coverage > 80%

## Ambiguity Report

| Dimension          | Score | Min  | Status | Notes                                            |
|--------------------|-------|------|--------|--------------------------------------------------|
| Goal Clarity       | 0.88  | 0.75 | ✓      | 9 falsifiable reqs, single precise goal          |
| Boundary Clarity   | 0.92  | 0.70 | ✓      | Schema/scope/Windows locked; Phase 2 split clear |
| Constraint Clarity | 0.85  | 0.65 | ✓      | WAL/single-writer, stdout purity, Clock/UTC      |
| Acceptance Criteria| 0.82  | 0.70 | ✓      | 8 pass/fail checks                               |
| **Ambiguity**      | 0.13  | ≤0.20| ✓      |                                                  |

Status: ✓ = met minimum, ⚠ = below minimum (planner treats as assumption)

## Interview Log

| Round | Perspective     | Question summary                          | Decision locked                                              |
|-------|-----------------|-------------------------------------------|--------------------------------------------------------------|
| 0     | (initial)       | Score from ROADMAP + REQUIREMENTS + research | Ambiguity 0.19 — already near gate; 3 boundary items open |
| 1     | Boundary Keeper | Phase-1 schema: vectors now or Phase 2?   | Relational + FTS5 only; embedding/vec0 added in Phase 2 migration |
| 1     | Boundary Keeper | How is `scope` determined?                | Explicit, optional; NULL = global (no auto-derive)           |
| 1     | Failure Analyst | Windows in Phase 1?                        | Deferred to v2 (DIST-03); path code need not be Windows-safe |

---

*Phase: 01-core-memory-foundation*
*Spec created: 2026-06-24*
*Next step: /gsd-discuss-phase 1 — implementation decisions (how to build what's specified above)*
