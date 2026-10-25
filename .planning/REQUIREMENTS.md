# Requirements: agent-memory — Milestone v1.1 Hardening & Interop

**Defined:** 2026-07-12
**Core Value:** An agent can persist a structured memory and retrieve the right one later — across sessions and tools — over a standard MCP interface, with no cloud.
**Ships as:** product release v0.1.0

## v1.1 Requirements

Each maps to a roadmap phase. IDs continue from the v1.0 numbering (archived in `milestones/v1.0-REQUIREMENTS.md`); the four deferred v2 IDs (SEARCH-04, MCP-06, DIST-03, DIST-04) are picked up here.

### Search (SEARCH)

- [x] **SEARCH-04**: With Ollama available, `memory_search` fuses the keyword and semantic legs via reciprocal-rank fusion (rank-based, k=60, duplicate ids sum contributions from both legs); results carry `search_mode: "hybrid"`. With Ollama unreachable, search degrades to keyword exactly as in v1.0 — the envelope shape is unchanged and existing `search_mode == "keyword"` checks keep working
- [x] **SEARCH-05**: Hybrid ranking remains decay-aware — a post-fusion decay blend so a fresh memory outranks a stale one at equal fused rank, and access bumps apply only to post-truncation returned ids

### MCP Interface (MCP)

- [x] **MCP-06**: Agent can call `memory_update` to patch an existing memory (content, tags, scope, TTL) by id; a content update re-mirrors FTS and invalidates/re-embeds the vector **in the same writer transaction** (embed-outage falls back to `embedding_status = 0` + existing sweep backfill); unknown id returns the clean not-found shape
- [x] **MCP-07**: Agent can link two memories with a flat typed relation (`memory_link` / `memory_unlink`: from_id, to_id, kind) and search/list results can expand 1-hop related memories; forgetting a memory cascades its links (no orphan edges, no graph/entity model)

### REST & Input Hardening (API)

- [x] **API-02**: Out-of-bounds `limit` and `ttl_secs` (zero, negative, absurd extremes) are rejected at the shared core seam as invalid input → REST 400 / MCP `invalid_params` — never a 500, never a silent clamp — across all v1.0 **and** new v1.1 surfaces
- [x] **API-03**: Tag filtering matches exact tags (json_each equality) instead of LIKE substring over-match — `tag=rust` no longer matches `rustling` (behavioral change, called out in release notes)

### Storage & Migration Hygiene (STORE)

- [x] **STORE-05**: Schema migration 0003 (relations) ships with migration hygiene: shipped 0001/0002 SQL frozen (fixture divergence test against a v0.0.1 database), pre-migration backup, and a friendly error when a newer DB meets an older binary

### Portability (DIST / INTEROP)

- [x] **DIST-04**: User can export all memories to a versioned JSONL file (header records format version + embedding model/dim; embeddings excluded as derived data; relation edges included with export-local key remapping)
- [x] **INTEROP-02**: User can import that JSONL on another machine — idempotent re-run, imported rows land `embedding_status = 0` and re-embed via the existing sweep backfill, expired items skipped and counted

### Distribution (DIST)

- [ ] **DIST-06**: Product release v0.1.0 published: checksummed tarballs for all supported targets + updated Homebrew formula, with release notes covering the tag-match behavioral change. *Re-scoped 2026-10-25 (issue #7): "all supported targets" = the six `release.yml` builds (four required + the best-effort musl pair); also covers the crates.io publish of `agent-memory-core` + `agent-memory-mcp` (#9) and CHANGELOG/SECURITY.md (#8).*

*DIST-05 (hard musl) and DIST-03 (Windows) moved to v0.2.0 on 2026-10-25 by the owner decision on issue #7 — see Future Requirements.*

## Future Requirements

Acknowledged but deferred — not in this milestone.

- **DIST-05** *(moved from v1.1 Phase 6 to v0.2.0, 2026-10-25, issue #7)*: Linux musl binaries (x86_64/aarch64) as a **hard** release target via a target-suffixed CFLAGS typedef shim (`-Du_int*_t=uint*_t`) on the existing zigbuild pipeline — removable when sqlite-vec PR #199 ships in a pinned release. Until then the musl legs stay best-effort, as in v0.0.1. Toolchain verdict already recorded (Phase 3 D-11).
- **DIST-03** *(moved from v1.1 Phase 6 to v0.2.0, 2026-10-25, issue #7)*: A Windows x86_64 binary is published — cargo-xwin 0.23.0 / `x86_64-pc-windows-msvc` (Phase 3 D-11 verdict; mingw-w64/gnu proven fallback); artifact-presence smoke only, best-effort label in release notes.
- **SEARCH-06**: Request-level `mode` override (`keyword | semantic | hybrid`) — cheap additive, revisit if agents need deterministic keyword testing
- **MCP-08**: LLM auto-arbitration on write (mem0 AUDN-style) — heavy, non-local
- Bi-temporal invalidation (Zep-style) — complexity not justified at local scale

## Out of Scope

| Feature | Reason |
|---------|--------|
| Entity/observation knowledge-graph model (9-tool MCP graph shape) | Field-documented agent-confusion source; flat typed links on stable ids are the v1.1 answer |
| Exporting embedding vectors by default | Model-pinned derived data; re-embed-on-import via existing backfill is safer and smaller |
| Validation crate (validator/garde) | Three manual bounds checks at one seam; a crate is dependency surface for nothing |
| Windows runtime CI testing | No Windows runner in the fleet; presence smoke + best-effort label for the first Windows release |
| sqlite-vec 0.1.10-alpha bump | Pre-release; the libsqlite3-sys 0.37 pin triangle (rusqlite 0.39 / rusqlite_migration 2.5 / r2d2_sqlite 0.34) must not move this milestone |

## Traceability

Mapped by roadmap creation 2026-07-12. Coverage: 12/12 v1.1 requirements mapped to Phases 3-6; re-scoped 2026-10-25 (issue #7): 10 stay in v1.1, DIST-05 and DIST-03 moved to v0.2.0.

| Requirement | Phase | Status |
|-------------|-------|--------|
| API-02 | Phase 3 | Complete |
| API-03 | Phase 3 | Complete |
| MCP-06 | Phase 4 | Complete |
| MCP-07 | Phase 4 | Complete |
| STORE-05 | Phase 4 | Complete |
| SEARCH-04 | Phase 4 | Complete |
| SEARCH-05 | Phase 4 | Complete |
| DIST-04 | Phase 5 | Complete |
| INTEROP-02 | Phase 5 | Complete |
| DIST-05 | v0.2.0 (moved from Phase 6, 2026-10-25, #7) | Deferred |
| DIST-03 | v0.2.0 (moved from Phase 6, 2026-10-25, #7) | Deferred |
| DIST-06 | Phase 6 (v0.1.0 release, #7) | Pending |

---
*Requirements defined: 2026-07-12 from approved v2 backlog + 4-dimension research (research/SUMMARY.md)*
*Traceability filled: 2026-07-12 by roadmap creation (Phases 3-6)*
*Traceability updated: 2026-10-25 — Phase 6 re-scoped to the v0.1.0 release (owner decision on issue #7); DIST-05/DIST-03 → v0.2.0*
