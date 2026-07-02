# Roadmap: agent-memory

## Overview

agent-memory ships as a single local Rust binary that gives AI agents a persistent, typed memory layer over MCP — store a decision in Claude Code, recall it in Cursor, survive every restart, no cloud. The journey is a strict bottom-up build in two vertical slices. **Phase 1** lays the irreversible foundation — embedded SQLite schema (with WAL, single-writer, and the decay/TTL/forget distinction baked in), typed memory CRUD, exponential decay + TTL lifecycle, FTS5 keyword search, and the MCP stdio server with all four tools — yielding a fully usable local memory tool that needs no Ollama. **Phase 2** layers the enhancements that turn it into the shipped product: local Ollama semantic search with graceful keyword fallback, a REST mirror, GSD STATE.md import, and the cross-platform release (binaries + Homebrew). Each phase is an end-to-end usable capability; the first is the load-bearing resilience baseline, the second is pure enhancement and distribution.

## Phases

**Phase Numbering:**

- Integer phases (1, 2): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked INSERTED)

- [x] **Phase 1: Core Memory Foundation** - Local SQLite-backed typed memory with decay/TTL, keyword search, and an MCP stdio server — works with zero cloud and zero Ollama (completed 2026-06-25)
- [ ] **Phase 2: Semantic Search, Interop & Release** - Local Ollama semantic search (graceful keyword fallback), REST API, GSD STATE.md import, and cross-platform binaries + Homebrew

## Phase Details

### Phase 1: Core Memory Foundation

**Goal**: An agent can store typed memories and retrieve them by keyword over MCP, surviving restarts, with decay down-ranking and TTL expiry — entirely locally, with no Ollama and no cloud.
**Mode:** mvp
**Depends on**: Nothing (first phase)
**Requirements**: STORE-01, STORE-02, STORE-03, STORE-04, MCP-01, MCP-02, MCP-03, MCP-04, MCP-05, SEARCH-01
**Success Criteria** (what must be TRUE):

  1. An agent calls `memory_store` over MCP stdio with a type (DECISION/PATTERN/ERROR/TODO/ARCHITECTURE/CONSTRAINT), content, and metadata (tags, source, scope) and receives a memory id back.
  2. After the daemon process is killed and restarted, `memory_search` (keyword/FTS5) and `memory_list` (filtered by type/tag/scope/limit) still return the previously stored memories — durable local SQLite, no cloud or account.
  3. Search results expose a decay score that decreases for older/unused memories; decay only down-ranks and never deletes, while `memory_forget` by id and TTL expiry are the only ways a memory is removed.
  4. Piping an `initialize` request into the server's stdin yields only valid JSON-RPC on stdout (all logs go to stderr) — the stdio MCP transport is never corrupted.**Plans**: 3 plans (coarse, vertical MVP — walking skeleton first)

**Wave 1**

- [x] 01-01-PLAN.md — Walking skeleton: Cargo workspace + SQLite store (schema/WAL/FTS5 mirror) + rmcp stdio server with memory_store/memory_list; Wave-0 test harness (STORE-01/02, MCP-01/03/05) ✅ 2026-06-25

**Wave 2** *(blocked on Wave 1 completion)*

- [x] 01-02-PLAN.md — FTS5 keyword search (bm25×decay) + memory_forget + decay surfacing (MCP-02/04, SEARCH-01, STORE-03)

**Wave 3** *(blocked on Wave 2 completion)*

- [x] 01-03-PLAN.md — TTL sweep + decay materialization engine + background task + CI/coverage gate + README (STORE-04, STORE-03)

### Phase 2: Semantic Search, Interop & Release

**Goal**: The same memory store gains local semantic recall that gracefully falls back to keyword when Ollama is absent, a REST mirror for non-MCP clients, one-command import of GSD STATE.md, and an installable cross-platform release.
**Mode:** mvp
**Depends on**: Phase 1
**Requirements**: SEARCH-02, SEARCH-03, API-01, INTEROP-01, DIST-01, DIST-02
**Success Criteria** (what must be TRUE):

  1. With local Ollama (`nomic-embed-text`) running, `memory_search` returns memories ranked by semantic similarity (combined with decay), finding relevant memories that share no exact keywords.
  2. With Ollama stopped or unreachable, search automatically degrades to keyword/FTS5 results instead of erroring or returning empty — the tool still works on a fresh machine.
  3. A non-MCP client can store, search, list, and forget memories through the REST API hitting the same store as the MCP tools.
  4. Running `agent-memory import --from gsd-state .planning/STATE.md` loads memories from a GSD STATE.md file and they become searchable; re-running it does not duplicate them.
  5. A user installs the tool from a pre-built binary (macOS arm64/x86_64, Linux x86_64/aarch64) or via `brew install` and the MCP server launches successfully.

**Plans**: 4 plans (coarse, vertical MVP slices — serial waves: every slice shares `main.rs`/`service.rs`/`Cargo.toml` ownership)

**Wave 1**

- [x] 02-01-PLAN.md — Semantic search via sqlite-vec + Ollama embeddings with graceful keyword fallback, search_mode envelope, sweep backfill, spike workflow file (SEARCH-02, SEARCH-03)

**Wave 2** *(blocked on Wave 1 completion)*

- [ ] 02-02-PLAN.md — REST mirror: `serve-rest` axum adapter over the same store, loopback-guarded, shared SearchOutcome envelope (API-01)

**Wave 3** *(blocked on Wave 2 completion)*

- [ ] 02-03-PLAN.md — Idempotent GSD STATE.md import: tolerant parser + `import --from gsd-state` subcommand + batch embed (INTEROP-01)

**Wave 4** *(blocked on Wave 3 completion)*

- [ ] 02-04-PLAN.md — Cross-platform release: publish repo, darwin cross-compile spike gate, zigbuild release workflow, v0.0.1 + Homebrew tap (DIST-01, DIST-02)

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Core Memory Foundation | 3/3 | Complete    | 2026-06-25 |
| 2. Semantic Search, Interop & Release | 1/4 | In Progress|  |
