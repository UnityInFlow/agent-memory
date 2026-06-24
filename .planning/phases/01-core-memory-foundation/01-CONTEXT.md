# Phase 1: Core Memory Foundation - Context

**Gathered:** 2026-06-24
**Status:** Ready for planning

<domain>
## Phase Boundary

A local Rust MCP daemon that stores, searches, lists, and forgets typed memories over embedded SQLite — with FTS5 keyword search, exponential decay, and TTL expiry — working entirely offline (no Ollama, no cloud). Semantic search, REST, GSD import, and release are Phase 2.

</domain>

<spec_lock>
## Requirements (locked via SPEC.md)

**9 requirements are locked.** See `01-SPEC.md` for full requirements, boundaries, and acceptance criteria.

Downstream agents MUST read `01-SPEC.md` before planning or implementing. Requirements are not duplicated here.

**In scope (from SPEC.md):**
- Rust workspace: `agent-memory-core` library (`thiserror`) + `agent-memory` binary (`anyhow` at edges)
- SQLite schema (`memories` table + FTS5 index), migrations, WAL + single-writer connection strategy
- Memory CRUD over the 6 types with explicit optional `scope`
- Exponential decay scoring with injectable `Clock`; TTL expiry sweep
- MCP stdio server exposing `memory_store`, `memory_search`, `memory_list`, `memory_forget`
- FTS5 keyword search (works with no Ollama)
- Tests to >80% coverage on core logic; stdout-purity CI test
- macOS + Linux targets

**Out of scope (from SPEC.md):**
- Semantic search / Ollama / `sqlite-vec` vec0 table — Phase 2 (schema stays relational+FTS5; embedding column added later via migration)
- REST API — Phase 2
- GSD STATE.md import + CLI subcommands beyond the MCP server — Phase 2
- Pre-built binaries + Homebrew — Phase 2
- Windows support — deferred to v2 (DIST-03)
- Hybrid ranking fusion, knowledge-graph relations, LLM-on-write extraction, ANN index, web UI — out of scope

</spec_lock>

<decisions>
## Implementation Decisions

### DB Location & Cross-Runtime Sharing
- **D-01:** Default to a **single global database** at the OS data dir resolved via the `dirs` crate — `<data_dir>/agent-memory/memory.db` (e.g. `~/Library/Application Support/agent-memory/memory.db` on macOS, `$XDG_DATA_HOME/agent-memory/memory.db` on Linux). Create the directory if missing.
- **D-02:** Override precedence: `--db <path>` flag > `AGENT_MEMORY_DB` env var > global default. This is the mechanism that lets a user point multiple runtimes (Claude Code, Cursor) at the same brain, or isolate per-project when they want.
- **D-03:** `scope` is a **logical column filter inside the one store**, NOT a separate DB file per project. One shared store; `scope` partitions logically.

### Search Ranking
- **D-04:** `memory_search` ranks by a **blend of FTS5/BM25 relevance and `decay_score`** so stale/unused memories sink and fresh/used ones rise. Pinned types (D-08) are floated up. Results expose both the relevance and the decay score so the agent (and tests) can see the ordering rationale. Exact blend formula is a planner/implementer tuning detail; the requirement is "relevance combined with decay, pinned types respected."

### MCP Tool Schema
- **D-05:** **Rich tool schema with sensible defaults.** `memory_store({ content, type, tags?, source?, scope?, ttl? })` — only `content` and `type` are required; the rest default (tags=[], source/scope=NULL, ttl=none) so a bare `{content, type}` call works.
- **D-06:** `memory_search`/`memory_list` results return `{ id, content, type, tags, scope, decay_score, created_at, last_accessed }`. `memory_forget({ id })` returns a deleted/not-found result.
- **D-07:** Tool input schemas must be explicit JSON Schema (rmcp) so agents get good argument hints; invalid `type` is rejected with a clean error, never a panic.

### Decay Defaults & Pinning
- **D-08:** **Pin high-value types.** DECISION, ARCHITECTURE, and CONSTRAINT decay much slower than (or are exempt from) normal decay; TODO, ERROR, PATTERN decay at the normal rate.
- **D-09:** Ship a **sensible default half-life (~30 days)**; both the half-life and which types are pinned are **configurable** (env/config), but the defaults must be good out of the box. Decay still only down-ranks — never deletes (per SPEC STORE-04).

### Claude's Discretion
- Crate/workspace mechanics, exact dependency versions (research recommends `rmcp` + `rusqlite` bundled + `clap` + `tokio` + `thiserror`/`anyhow`), migration library, connection-pool approach (research recommends WAL + r2d2 read pool + single serialized writer), the precise decay/ranking math, the config-file format, and the coverage tool are all left to research/planner.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Locked requirements
- `.planning/phases/01-core-memory-foundation/01-SPEC.md` — Locked requirements, boundaries, acceptance criteria. MUST read before planning.

### Project & requirements
- `.planning/PROJECT.md` — product framing, core value, constraints, key decisions
- `.planning/REQUIREMENTS.md` — REQ-IDs mapped to Phase 1 (STORE-01..04, MCP-01..05, SEARCH-01)
- `.planning/ROADMAP.md` §"Phase 1" — goal + success criteria
- `CLAUDE.md` — ecosystem constraints (Rust edition 2021, no `unwrap()`, clap/serde/tokio/anyhow/thiserror, >80% coverage, self-hosted runners)

### Research (HIGH confidence, this milestone)
- `.planning/research/STACK.md` — crate choices + versions (rmcp 1.8 nested in axum, rusqlite 0.40 bundled, sqlite-vec for Phase 2, Ollama `/api/embed`)
- `.planning/research/ARCHITECTURE.md` — crate layout, SQLite schema, WAL + single-writer concurrency, build order
- `.planning/research/PITFALLS.md` — stdout purity, sqlite-vec static-link, decay≠delete, Clock injection, Windows `cfg(unix)` precedent
- `.planning/research/FEATURES.md` — flat-typed (not graph) MCP surface, pin concept, FTS5-before-semantic
- `.planning/research/SUMMARY.md` — synthesis + roadmap implications

### Source spec
- `10-agent-memory.md` — original feature spec (schema/decay/search notes, build todos)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- None in-repo (greenfield — no `Cargo.toml`/`src` yet). Sibling Rust tools `03-injection-scanner` and `07-mcp-hub` (own git repos, indexed in memtrace) are the closest reference patterns for CLI structure, error handling, and the self-hosted-runner release pipeline (reused in Phase 2).

### Established Patterns
- Ecosystem Rust conventions (CLAUDE.md): edition 2021, `clap` derive, `serde`/`serde_json`, `tokio`, `anyhow` (binary) / `thiserror` (library), no `unwrap()`, exhaustive matches, `cargo clippy -D warnings` + `cargo fmt`.
- Harness hooks already active: `cargo-fmt.sh` (rustfmt-on-write), `cargo-check.sh` (clippy+test on Stop, no-ops until `Cargo.toml` exists), `pre-bash-rust.sh` (blocks `rm *.db`, `cargo publish`, force-push). The first execute commit landing `Cargo.toml` activates these.

### Integration Points
- The MCP server is consumed by Claude Code / Cursor via `.mcp.json` stdio. The cross-runtime DB-sharing decisions (D-01..03) are what make one store reachable from multiple runtimes.

</code_context>

<specifics>
## Specific Ideas

- Cross-runtime sharing is the headline UX: "store a decision in Claude Code, recall it in Cursor." The global-DB-by-default decision (D-01) directly serves this.
- stdout is sacred: all logging to stderr; a CI test must assert stdout-only-JSON-RPC (SPEC MCP-05).

</specifics>

<deferred>
## Deferred Ideas

- **Hybrid (keyword+semantic) RRF ranking fusion** — v2 (SEARCH-04); Phase 1 blend is keyword-relevance × decay only.
- **Per-runtime / per-project DB auto-detection** beyond explicit `--db`/env — possible later UX; v0.0.1 keeps it explicit.
- **`memory_update` / relation tools** — v2 (MCP-06).

None of the above were scope creep — they were correctly recognized as later work and set aside.

</deferred>

---

*Phase: 1-core-memory-foundation*
*Context gathered: 2026-06-24*
