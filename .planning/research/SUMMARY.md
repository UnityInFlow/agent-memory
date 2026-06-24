# Project Research Summary

**Project:** agent-memory (Tool 10, UnityInFlow ecosystem)
**Domain:** Local-first Rust daemon — cross-runtime persistent agent memory (MCP server + REST API over embedded SQLite, local Ollama embeddings, decay/TTL lifecycle)
**Researched:** 2026-06-24
**Confidence:** HIGH

## Executive Summary

agent-memory is a single-binary, zero-cloud, MCP-native memory layer for AI agents — the kind of typed, semantically-searchable memory that Mem0/Letta/Zep offer, but delivered as one `brew install` Rust daemon with no cloud account and no separate database process. Experts in this space converge on a clear shape: an MCP server (the primary interface) plus a thin REST mirror, both backed by embedded SQLite, with semantic recall done locally. The decisive build choices are the **official `rmcp` MCP SDK**, **`rusqlite` (bundled)** + **`sqlite-vec`** for storage and vector search in one file, **`reqwest` direct to a local Ollama** for embeddings, and **`axum`** so the MCP HTTP transport and REST endpoints share one router, one listener, and one `tokio` runtime. This matches the ecosystem's existing Rust conventions (mcp-hub already pins `axum 0.8`, `clap 4`, `tokio 1`, `thiserror 2`) and keeps the tool a self-contained, zero-config binary.

The recommended approach is a strict bottom-up build: domain types + SQLite schema (with WAL, write-serialization, and the decay/TTL/forget distinction baked in from day one) → service layer (CRUD, FTS5 keyword search, decay engine) → MCP server (4 tools over stdio) → Ollama semantic search layered onto the same query path → REST/HTTP transport → GSD import → cross-platform release. The single most important design rule is that **keyword/FTS5 search must ship before and work without semantic search**, so the tool is useful on a fresh machine and degrades gracefully when Ollama is absent. The differentiator is "Mem0-grade typed memory in one local Rust binary over MCP" — explicitly *not* a knowledge graph, *not* LLM-on-write extraction, *not* cloud sync.

The key risks are well-characterized and front-loadable. Four pitfalls must be resolved in the foundation phase or they become expensive retrofits: (1) **stdout pollution corrupts the MCP stdio transport** — all logging must go to stderr; (2) **blocking SQLite/Ollama calls stall the async runtime** — DB work goes through `spawn_blocking` / a single writer, embeddings use async reqwest; (3) **decay must only re-rank, never delete** — decay, TTL, and explicit forget are three orthogonal mechanisms the schema must separate; (4) **`sqlite-vec` static linking + the cross-compile matrix** — static-link via the crate's `cc` build (no runtime `.so`), and prove the C cross-toolchain on all target triples on the `orangepi`/zigbuild path early, given the recurring Hetzner X64 fleet outages. Get these right in Phase 1 and the rest is layered enhancement.

## Key Findings

### Recommended Stack

A single `tokio` runtime hosts everything. The MCP server is the primary surface via the **official `rmcp` SDK** (stdio for editor launch, streamable-HTTP nestable into axum). Storage is **`rusqlite` with the `bundled` feature** (SQLite compiled into the binary — zero system dependency, identical behavior on every target) plus **`sqlite-vec`** for in-database vector KNN (the dependency-free successor to the deprecated `sqlite-vss`). Embeddings come from a **direct `reqwest` call to local Ollama `/api/embed`** (`nomic-embed-text`, 768-dim) — one call, no wrapper. **`axum`** serves the REST mirror and hosts rmcp's `StreamableHttpService` in the same router. Release tooling is **`cargo-dist`** (generates binaries + Homebrew formula) with a mandatory custom-runner override, paired with the ecosystem-proven **`cargo-zigbuild` on `orangepi`** for cross-compilation. Confidence is HIGH — core crate versions were verified against live crates.io and integration nuances against upstream docs/issues. See [STACK.md](./STACK.md).

**Core technologies:**
- **`rmcp` 1.8** (`server, transport-io, transport-streamable-http-server, macros`): the MCP server — official SDK, stdio + HTTP transports, `#[tool]` macros remove JSON-RPC plumbing
- **`rusqlite` 0.40 (`bundled`) + `sqlite-vec` 0.1.9**: embedded store + in-DB vector search in one self-contained binary — no cloud, no separate vector DB
- **`axum` 0.8 + `tokio` 1**: rmcp's HTTP service nests into the same axum router — MCP-over-HTTP and REST share one runtime, one listener
- **`reqwest` 0.12 (pin, rustls-tls)**: direct Ollama embedding client — avoids a system OpenSSL dependency in prebuilt binaries
- **`rusqlite_migration` 2, `clap` 4, `serde`, `thiserror` 2 / `anyhow`, `tracing`, `chrono`, `dirs`, `zerocopy`, `uuid`**: ecosystem-standard supporting cast

### Expected Features

agent-memory fills a real gap: none of Mem0, Letta, Zep, or the MCP reference memory server is a *single-binary, zero-cloud, MCP-native* memory shared across runtimes with dev-workflow typing. The MVP must match the reference server + Mem0 on the basics while winning on locality and graceful degradation. See [FEATURES.md](./FEATURES.md).

**Must have (table stakes):**
- MCP server with **`memory_store` / `memory_search` / `memory_list` / `memory_forget`** — the product's reason to exist (spec-mandated 4 tools)
- **Persistence across sessions** (SQLite file in a stable location) — the core problem statement
- **Memory typing** (DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT) + metadata (tags, source, scope/project, timestamps) — what makes it a *dev* memory; scope prevents project-A leaking into project-B
- **FTS5 keyword search baseline** — works before/without Ollama; the resilience cornerstone
- **README with copy-paste MCP config** — MCP servers live or die on a 30-second install

**Should have (competitive differentiators):**
- **Single static Rust binary, zero cloud, zero account** — *the* wedge vs Mem0/Zep/Letta
- **Local Ollama semantic search** with **graceful FTS5 fallback** — Mem0-grade recall, privacy by architecture
- **Decay scoring (down-rank only) + TTL (delete)** with relevance surfaced in results — no comparable local server forgets
- **GSD STATE.md import** — instant seed data + concrete cross-tool interop proof
- **REST API mirroring the MCP tools** — cross-runtime reach for non-MCP clients

**Defer (v1.x / v2+):**
- Hybrid RRF fusion tuning, per-type decay rates, `memory_update`, `memory_stats` (v1.x — add when validated)
- Lightweight relations, local-LLM summarization, encrypted self-sync, ANN index (v2+)
- **Explicit anti-features:** full knowledge graph, cloud/hosted backend, LLM-on-write extraction, implicit "dreaming" capture, web UI, multi-tenant auth — all break the local-first/zero-cloud thesis

### Architecture Approach

A Cargo **workspace** with a `-core` library (`thiserror`, no `anyhow`) and a `-bin` binary (`anyhow` at the edges) — honoring the ecosystem error convention and keeping transports thin. Interfaces (MCP stdio, MCP-over-HTTP, REST, CLI) are thin adapters over a shared service layer; services reach adapters through `Store` and `Embedder` traits so the DB and Ollama can be faked in tests. One `Arc<AppState>` (connection pool + Ollama client + config) is shared across both transports on one `tokio` runtime. Vectors live in a `sqlite-vec` `vec0` sidecar table joined by rowid to the authoritative `memories` table; decay is hybrid (computed-on-read for correctness, materialized by a background sweep for cheap `ORDER BY`). Confidence HIGH. See [ARCHITECTURE.md](./ARCHITECTURE.md).

**Major components:**
1. **Interface layer (bin):** MCP stdio + MCP-HTTP (rmcp) + REST (axum) + CLI (clap) — thin adapters, no business logic
2. **Service layer (core lib):** `MemoryService` (CRUD), `SearchService` (semantic + keyword fallback, rank by similarity × decay), `DecayEngine` (score + TTL sweep on a tokio interval), `ImportService` (STATE.md parser)
3. **Adapters (core lib):** `SqliteStore` (rusqlite bundled + WAL + single-writer + vec0/FTS5), `OllamaClient` (reqwest, health-check + degrade)

### Critical Pitfalls

1. **Logging to stdout corrupts the MCP stdio transport** — route ALL logs to stderr, install a stderr panic hook, audit deps for stray prints, and add a CI test that pipes `initialize` and asserts every stdout line is valid JSON-RPC. (Phase 1, hard gate.)
2. **Blocking the async event loop with SQLite/Ollama** — `rusqlite` is synchronous; wrap all DB work in `spawn_blocking` / a single writer lane, use async reqwest for Ollama, keep decay off the request path. (Phase 1, architectural rule from the first handler.)
3. **Decay that deletes useful memories / conflated decay–TTL–forget** — decay only re-ranks (never deletes); TTL and `memory_forget` are the only deletion paths; per-type policy + a pin flag protect CONSTRAINT/ARCHITECTURE/DECISION. The schema must separate `decay_score`, `expires_at`, and `last_accessed` from day one. (Phase 1 schema.)
4. **`sqlite-vec` portability + cross-compile matrix** — static-link via the `sqlite-vec` crate's `cc` build (no runtime `.so` per arch); `bundled` rusqlite + sqlite-vec compile C, so prove the zig C cross-toolchain on ALL target triples early. Decide the Windows story up front (support-and-gate, or formally defer like mcp-hub). (Phase 1 decision + Release verification.)
5. **Silent embedding failure + dimension/model drift** — never store a fake/zero vector when Ollama is down; fail loud with an actionable message (`ollama pull nomic-embed-text`); store `embedding_model`/`embedding_dim` and assert dimension at startup; provide a `reindex` path. Also normalize vectors consistently (cosine on un-normalized vectors silently mis-ranks). (Phase 2.)

## Implications for Roadmap

Dependencies flow strictly upward (**domain → store → service → transport → release**), which yields a natural phasing. The cleanest split for this project is a **two-phase milestone** (matching the spec's own discuss/plan/execute ×2 workflow), with the foundation phase carrying the heavy, hard-to-retrofit pitfalls and the second phase layering semantic search + interop + release. A finer-grained 4-phase breakdown is also viable if the roadmapper prefers smaller units.

### Phase 1: Core Memory Foundation (storage, CRUD, decay, MCP)
**Rationale:** Everything depends on the schema and the concurrency/error/logging conventions; these are the pitfalls that are expensive to retrofit. Keyword search must exist before semantic so the MCP `search` tool works without Ollama.
**Delivers:** Cargo workspace (`-core` lib / `-bin`); SQLite schema with WAL + single-writer + `decay_score`/`expires_at`/`last_accessed` separated; `MemoryService` (store/list/forget); FTS5 keyword search; `DecayEngine` (lazy compute + background sweep + TTL, injectable `Clock`, UTC timestamps); MCP server with the 4 tools over stdio.
**Addresses:** MCP 4-tool surface, memory typing + metadata, FTS5 baseline, persistence, decay/TTL (all P1 table stakes).
**Avoids:** Pitfalls 1 (stdout→stderr), 2 (blocking loop), 3/7 (decay≠delete, schema separation), 8 (Clock trait + UTC + lazy decay), 11 (WAL + write serialization); and the Pitfall 3/10 *decision* (static-link sqlite-vec, Windows support-vs-defer, dep gating).

### Phase 2: Semantic Search, Interop & Release
**Rationale:** Semantic search is the latest-arriving feature and must not block the MCP surface; it's a strict enhancement on the same query path. Import, REST, and release naturally follow once the core is proven.
**Delivers:** `Embedder` trait + `OllamaClient` (health probe, graceful degrade); `vec0` table + KNN; hybrid re-rank (semantic + decay, FTS5 fallback); REST API + MCP-over-HTTP on the shared axum router; GSD STATE.md import (idempotent); prebuilt binaries (self-hosted runners) + Homebrew; README/CONTRIBUTING/LICENSE/CI; >80% core coverage.
**Uses:** `rmcp` HTTP transport, `axum`, `reqwest`→Ollama, `sqlite-vec`, `cargo-dist`/`cargo-zigbuild`.
**Implements:** `SearchService` semantic path, `ImportService`, the REST/HTTP transport, the release matrix.
**Avoids:** Pitfalls 4 (Ollama-down → loud, recoverable), 5 (dim/model metadata + reindex), 6 (normalized cosine + golden-set test), 9 (hybrid FTS5+vector recall), 10 (cross-compile verification on all triples).

### Phase Ordering Rationale
- **Strict upward dependency** (domain → store → service → transport → release) means the schema and concurrency model cannot be deferred — they shape every later layer.
- **Keyword-before-semantic** is the load-bearing rule: the MCP `search` tool ships functional in Phase 1 and Ollama is a pure enhancement, preserving "zero cloud, just works."
- **Foundation phase front-loads the irreversible pitfalls** (stdout purity, blocking boundary, WAL/writer, decay≠delete, Clock/UTC, static-link decision) because each is a migration or data-loss event if discovered late.
- **Release is last** because it depends on the full target matrix building — and the recurring Hetzner-fleet/orangepi cross-compile risk wants the C-toolchain link proven before committing to the release path.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2 (semantic search internals):** `--research-phase` recommended for the exact `sqlite-vec` + `rusqlite 0.40` registration path (the API changed at rusqlite 0.34 — `RawAutoExtension`, not `transmute`), the cosine-normalization convention, and the hybrid FTS5+vec0 fusion query. These are HIGH-confidence-but-fiddly integration details where copying stale snippets will break the build.
- **Phase 2 (release/cross-compile):** lightweight research on proving `cc`-compiled `sqlite-vec` cross-builds for every target triple via zigbuild on `orangepi`, and confirming the cargo-dist custom-runner override (default `ubuntu-latest`/`macos-latest` is org-banned). This is the single biggest release risk.

Phases with standard patterns (skip research-phase):
- **Phase 1 (schema, CRUD, MCP stdio, decay):** well-documented, ecosystem precedent exists (mcp-hub Cargo.toml, rmcp official examples). Pitfalls already enumerated; patterns are established. Standard execution.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Core crate versions verified against live crates.io; integration nuances against upstream docs/issues; strong in-repo precedent (mcp-hub, injection-scanner) |
| Features | HIGH | Grounded in named comparables (Mem0, Letta/MemGPT, Zep/Graphiti, OpenAI Memory, MCP reference server) with documented sources |
| Architecture | HIGH | All stack pieces current and verified; one MEDIUM area (exact rmcp HTTP-into-axum nesting, sourced from a vendor blog rather than official docs) |
| Pitfalls | HIGH | MCP stdio + sqlite-vec findings verified against upstream issues; ecosystem CI pitfalls confirmed from CLAUDE.md Decisions Log |

**Overall confidence:** HIGH

### Gaps to Address
- **rmcp version churn:** rmcp had breaking changes across 0.7→0.8→1.x. Pin a specific `1.x` and read release notes before any bump; verify the `StreamableHttpService`-into-axum nesting against the actual pinned version during Phase 2 planning (the nesting pattern is sourced from a MEDIUM-confidence vendor blog).
- **`sqlite-vec` cross-compile (C toolchain):** the static-link + zigbuild path is plausible (injection-scanner shipped apple-darwin this way) but unproven for `sqlite-vec`'s bundled C across all triples. Validate on `orangepi` on day one of the release work, not at release time. Documented Plan B: BLOB + in-Rust cosine for any target that fails.
- **Windows support decision:** unresolved support-vs-defer. mcp-hub precedent (defer to v2, gate `cfg(unix)` deps properly) is the safe default; decide in Phase 1 so `dirs`/path deps are gated consistently throughout.
- **Hetzner X64 fleet availability:** recurring ecosystem blocker — plan the release matrix for orangepi-only serial builds with host-arch-aware smoke tests as the working assumption.

## Sources

### Primary (HIGH confidence)
- crates.io live API — verified `rmcp 1.8`, `rusqlite 0.40`, `sqlite-vec 0.1.9`, `axum 0.8.9`, `reqwest`, `sqlx 0.9`, `libsql 0.9.30`
- modelcontextprotocol/rust-sdk (rmcp) README + docs.rs/rmcp — official SDK, feature flags, stdio + streamable-HTTP transports, axum integration
- asg017/sqlite-vec + "Using sqlite-vec in Rust" (Alex Garcia) — vec0 virtual table, brute-force exact KNN, sqlite3_auto_extension registration, zerocopy Vec<f32>
- sqlite-vec issue #206 — rusqlite 0.34 RawAutoExtension registration change
- ollama.com/library/nomic-embed-text — 768-dim, 8192 ctx, /api/embed input format
- axodotdev/cargo-dist releases/CHANGELOG — v0.32.0, Homebrew formula generation, custom runners
- MCP reference knowledge-graph memory server; Mem0 docs/repo; Letta/MemGPT memory blog; Zep/Graphiti (arXiv 2501.13956); OpenAI ChatGPT memory FAQ — feature comparables
- SQLite hybrid search (FTS5 BM25 + sqlite-vec + RRF) — Alex Garcia blog, Simon Willison
- MCP stdio stdout-corruption issues (dirmacs/daedra #4, ruvnet/claude-flow #835); rusqlite README (bundled + WAL)
- UnityInFlow CLAUDE.md Decisions Log — mcp-hub Windows cfg(unix) failure, cargo-zigbuild on orangepi, Hetzner fleet offline; in-repo Cargo.toml precedents

### Secondary (MEDIUM confidence)
- Shuttle blogs — stdio MCP server + Streamable HTTP MCP server in Rust (axum-nesting pattern for rmcp)
- ChatForest MCP Debugging Guide; MCPcat "Build MCP Servers in Rust"
- FadeMem (arXiv 2601.18642) + co-r-e.com — decay/forgetting & recency·frequency·similarity scoring

### Tertiary (LOW confidence)
- (none — all findings backed by at least one verified or community-consensus source)

---
*Research completed: 2026-06-24*
*Ready for roadmap: yes*
