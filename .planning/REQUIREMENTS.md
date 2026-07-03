# Requirements: agent-memory

**Defined:** 2026-06-24
**Core Value:** An agent can persist a structured memory and retrieve the right one later — across sessions and tools — over a standard MCP interface, with no cloud.

## v1 Requirements

Requirements for the v0.0.1 release. Each maps to a roadmap phase.

### Storage (STORE)

- [x] **STORE-01**: Agent can store a memory with a type (DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT), content, and metadata (tags, source, scope) in embedded SQLite
- [x] **STORE-02**: Stored memories survive process restarts — durable local SQLite, no cloud and no account required
- [x] **STORE-03**: Each memory carries a decay score that decreases over time based on recency/usage (exponential decay), and the score is surfaced in results
- [x] **STORE-04**: A memory can be given a TTL after which it expires and is removed; decay only down-ranks and never deletes (TTL and explicit forget are the only removal paths)

### MCP Interface (MCP)

- [x] **MCP-01**: Agent can call `memory_store` to persist a typed memory and receive its id
- [x] **MCP-02**: Agent can call `memory_search` to retrieve relevant memories ranked by relevance/decay
- [x] **MCP-03**: Agent can call `memory_list` to enumerate memories with filters (type, tag, scope, limit)
- [x] **MCP-04**: Agent can call `memory_forget` to delete a memory by id
- [x] **MCP-05**: The MCP server runs over stdio without corrupting the protocol — all logging goes to stderr and every stdout line is valid JSON-RPC

### Search (SEARCH)

- [x] **SEARCH-01**: Agent can find memories by keyword/full-text search (FTS5) with no embedding model required
- [x] **SEARCH-02**: Agent can find memories by semantic similarity using local Ollama embeddings (`nomic-embed-text`)
- [x] **SEARCH-03**: When Ollama is unavailable, search degrades gracefully to keyword search instead of failing or returning empty

### REST API (API)

- [x] **API-01**: A REST API exposes store/search/list/forget for non-MCP integrations

### Interop (INTEROP)

- [x] **INTEROP-01**: User can import memories from a GSD STATE.md file (`agent-memory import --from gsd-state .planning/STATE.md`)

### Distribution (DIST)

- [x] **DIST-01**: Pre-built binaries are published for macOS (arm64/x86_64) and Linux (x86_64/aarch64)
- [x] **DIST-02**: A Homebrew formula installs the binary

## v2 Requirements

Acknowledged but deferred — not in the current roadmap.

### Search

- **SEARCH-04**: Hybrid search — reciprocal-rank fusion of keyword + semantic results

### Interface

- **MCP-06**: `memory_update` and relation/link tools

### Distribution

- **DIST-03**: Pre-built Windows binaries (deferred per the `mcp-hub` `cfg(unix)` cross-compile precedent — needs a CLI refactor)
- **DIST-04**: Export memories to a portable file

## Out of Scope

Explicitly excluded. Anti-features from research recorded here to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Cloud/hosted backend or required account | Defeats the zero-dependency, local-first core value |
| Remote embedding providers (OpenAI/Anthropic) | Ollama keeps it fully local; remote is at most a later opt-in |
| Full knowledge-graph model (entities/relations) | The MCP reference server's graph shape confuses agents; flat typed records are the right call |
| LLM-on-write fact extraction (Mem0 `add` pipeline) | Heavy, non-local, and slow on store; out of scope for a local daemon |
| ANN vector index | Brute-force exact KNN is sufficient at local-memory scale |
| JVM/Python runtime dependency | Rust chosen for a dependency-free, low-footprint daemon |
| Web UI / RBAC | Backend + CLI + MCP first; no frontend until backend is tested |

## Traceability

Each v1 requirement maps to exactly one phase.

| Requirement | Phase | Status |
|-------------|-------|--------|
| STORE-01 | Phase 1 | Complete |
| STORE-02 | Phase 1 | Complete |
| STORE-03 | Phase 1 | Complete |
| STORE-04 | Phase 1 | Complete |
| MCP-01 | Phase 1 | Complete |
| MCP-02 | Phase 1 | Complete |
| MCP-03 | Phase 1 | Complete |
| MCP-04 | Phase 1 | Complete |
| MCP-05 | Phase 1 | Complete |
| SEARCH-01 | Phase 1 | Complete |
| SEARCH-02 | Phase 2 | Complete |
| SEARCH-03 | Phase 2 | Complete |
| API-01 | Phase 2 | Complete |
| INTEROP-01 | Phase 2 | Complete |
| DIST-01 | Phase 2 | Complete |
| DIST-02 | Phase 2 | Complete |

**Coverage:**

- v1 requirements: 16 total
- Mapped to phases: 16 (Phase 1: 10, Phase 2: 6)
- Unmapped: 0 ✓

---
*Requirements defined: 2026-06-24*
*Last updated: 2026-06-24 after roadmap creation (traceability populated)*
