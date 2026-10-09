# Feature Research

**Domain:** Local-first agent memory (Rust + SQLite + MCP) — v1.1 Hardening & Interop milestone
**Researched:** 2026-07-12
**Confidence:** MEDIUM (hybrid-RRF and MCP tool-shape findings cross-verified across official docs, LanceDB API docs via Context7, and the canonical sqlite-vec hybrid-search reference; export-format and REST-validation findings are convergent community practice, LOW-MEDIUM)

**Scope note:** This covers ONLY the new v1.1 features (SEARCH-04 hybrid RRF, MCP-06 update/relations, REST input hardening, DIST-03 Windows, musl, DIST-04 portable export). Shipped v0.0.1 features (store/search/list/forget, types, decay, TTL, FTS5, Ollama semantic + fallback, GSD import) are treated as existing substrate and not re-researched. The prior (2026-06-24) v1.0 feature research already flagged "add a lightweight relate only if v1 validates demand" and "export pairs with import" — this milestone is that follow-through.

## How Comparable Tools Do It (Question-by-Question)

### 1. Hybrid keyword+vector search with RRF

**Expected behavior — this is a solved, converged pattern:**

- **Formula:** `score(d) = w_fts * 1/(k + rank_fts) + w_vec * 1/(k + rank_vec)`. Ranks only — RRF exists specifically to sidestep the BM25-vs-cosine score-scale mismatch. Never fuse raw scores.
- **k constant:** **60 is the industry standard** (original Cormack/Clarke paper; LanceDB `RRFReranker.create(k)` defaults to 60; every SQLite implementation surveyed uses 60). The optimum is flat anywhere in k ∈ [20, 100], so expose it as config but do not agonize — ship 60.
- **Weighting:** per-list multipliers (`w_fts`, `w_vec`), both defaulting to 1.0. qmd double-weights results from the original (un-expanded) query; that only matters if you do query expansion (we don't).
- **Dedup:** union of unique memory IDs across both lists. A memory appearing in both lists gets *both* reciprocal-rank contributions — that consensus boost **is** the fusion; do not "dedup by keeping one score" (qmd issue #331 flags naive dedup as a bug source). In SQL this is the canonical sqlite-vec pattern: two CTEs with `row_number()` ranks, `FULL OUTER JOIN` on id, `coalesce(1.0/(k+rank), 0.0)` for rows present in only one list.
- **Fallback interaction (critical for us):** hybrid must degrade exactly like semantic search does today. Ollama absent/unreachable → the vector CTE is empty → RRF over one list is mathematically just keyword rank order. The clean design: hybrid is a *fusion layer over the two existing retrievers*, and the existing SC2 keyword-fallback envelope (`SearchOutcome`) reports which legs actually ran. LanceDB, sqlite-rag, and llama-stack all implement hybrid as exactly this composition.
- **Candidate depth:** fetch top-N from each leg with N larger than the requested limit (2–4× is common; qmd fuses then reranks top 30) so fusion has material to work with, then truncate to `limit` after fusion.

**Where decay fits:** comparable tools *lack* recency/decay in hybrid ranking and users ask for it (qmd issue #331 requests temporal decay with configurable half-life as a missing feature). agent-memory already has exponential decay — applying the existing decay multiplier to the fused RRF score (down-rank only, never delete, per STORE-03/04) is a genuine differentiator, not scope creep. It must remain a post-fusion multiplier so the RRF math stays pure ranks.

### 2. memory_update and relation/link tools over MCP

**What agents actually use well — flat, id-addressed, string-level operations:**

- **mem0:** the agent/client-facing shape is `update(memory_id, new_content)` (single or batch). The clever ADD/UPDATE/DELETE/NOOP ("AUDN") arbitration is *server-side LLM inference* — it is not part of the tool schema agents call. Lesson: keep the tool dumb and deterministic; don't put "decide whether to update" inside the server.
- **Letta/MemGPT:** `core_memory_replace(label, old_text, new_text)` and `core_memory_append` — the most battle-tested memory-editing shape in the field. Agents reliably target a known memory and supply replacement text. The design principle: *update rather than accumulate* when a fact changes.
- **Official MCP memory server** (`@modelcontextprotocol/server-memory`): full knowledge graph — entities, directed relations (active-voice strings), observations, across a 9-tool surface (`create_entities`, `create_relations`, `add_observations`, `delete_entities`, `delete_observations`, `delete_relations`, `read_graph`, `search_nodes`, `open_nodes`). Relations are flat triples `{from, to, relationType}` keyed on *entity names as strings*. Known failure modes: entity-name drift (agent creates "Jiri", "Jiří", "the user" as three nodes), tool-choice confusion across 9 tools, and agents dumping everything into observations while ignoring relations. The *triple shape itself* is fine — the entity/observation ceremony around it is what confuses agents.
- **Zep/Graphiti:** bi-temporal edges (`valid_at`/`invalid_at` + system timestamps), LLM-driven fact invalidation. State of the art for enterprise memory, but requires an LLM extraction pipeline and a graph DB — everything interesting happens server-side, invisible to the agent. Directly conflicts with our zero-cloud, no-LLM-in-daemon constraint.

**Verdict on the quality-gate question — can relations stay flat? Yes, and they should.** The winning shape for us: `memory_link(from_id, to_id, relation)` producing flat typed edges *between existing memory IDs* (stable ids, not name strings — this kills the entity-name-drift failure mode outright). Retrieval surfaces links as a `related` field on search/get results (1 hop). No entities, no observations, no traversal query language, no multi-hop. This is the official MCP server's proven triple shape minus everything that confuses agents — consistent with the v1.0 decision to reject full knowledge-graph modeling.

**Update semantics that matter:**
- Partial update by id: `memory_update(id, {content?, memory_type?, tags?, ttl_secs?})` — reject empty patch with invalid_params.
- **Content change ⇒ re-embed** (and refresh the FTS row). If Ollama is down, follow the existing store-path behavior for missing embeddings (keyword-searchable immediately, embedding absent per SC2) — never fail the update because embedding failed.
- Updating should touch `last_accessed`/decay inputs consistently with store semantics (an updated memory is "fresh").
- Deletion cascade: `memory_forget` and the TTL sweep must delete edges referencing the removed memory (foreign keys with `ON DELETE CASCADE` — cheap in SQLite, mandatory for integrity).

### 3. Portable export/import formats

**Convergent practice (no universal standard exists):**

- **JSONL, one memory per line**, is the de-facto interchange shape (ChromaDB Data Pipes exports collections to `.jsonl`; the MCP reference server persists its whole graph as JSONL). Raw DB copy (`VACUUM INTO`) is a *backup*, not a portable export — it pins schema version, drags FTS/vec shadow tables along, and is useless to other tools. `.dump` SQL is migration plumbing. JSONL is diffable, greppable, streamable, and survives schema evolution.
- **Header/metadata line first:** format version, tool version, embedding model + dimension, export timestamp. Versioning the format from day one (`"format": "agent-memory/1"`) is the cheapest insurance in the whole milestone.
- **Embeddings: exclude by default, offer `--include-embeddings`.** Embeddings are model-specific (nomic-embed-text, 768-dim); an export consumed by any other tool or a future model version needs re-embedding anyway. ChromaDB Data Pipes supports both include-embeddings export and re-embed-on-import; the re-embed path is the one that actually makes exports portable. Default-exclude also keeps files small and human-readable.
- **Round-trip import is what makes export meaningful.** Import must be idempotent (same contract as the existing GSD STATE.md import — deterministic by-id skip/overwrite) and must re-embed via local Ollama when embeddings are absent or the model tag mismatches, with the same graceful degradation (no Ollama → import succeeds, keyword-searchable, embeddings absent).
- **Everything semantic goes in:** id, type, content, tags, created/last-accessed timestamps, TTL/expiry, decay-relevant fields, and — once MCP-06 lands — relations (inline per record or a second record kind `{"kind":"link",...}`). Losing timestamps silently resets decay; that's data corruption from the user's perspective.

### 4. REST input-validation table stakes (local single-user daemon)

- **Bad input is 400, never 500** — already our locked two-tier taxonomy (InvalidQuery → 400/invalid_params). The hardening work is extending coverage to boundary values: negative/zero/overflow `limit`, absurd `ttl_secs` (0, negative, > ~100 years, i64 overflow), non-numeric strings, oversized bodies.
- **`limit`: default + hard max, one documented policy.** Industry accepts either silent-clamp or 400-reject; the requirement is *consistency and documentation*. Given the existing taxonomy already makes invalid input loudly visible, **reject-with-400 above the max** is the more self-consistent choice for agent clients (silent clamping hides bugs in agent code); clamping is defensible only if documented. Pick one, test both edges.
- **Exact tag-match semantics (WR-07):** tag filters must be exact-match (no substring surprise); an unknown tag yields an empty result, not an error.
- **Implementation shape:** for ~6 endpoints, manual validation via bounded serde newtypes / a small custom extractor is idiomatic (the axum repo's own validator example is a manual extractor); `validator`/`garde`/`axum-valid` only pay off with many DTOs. No new dependency needed.
- Loopback guard already exists; auth/rate-limiting remain out of scope for a single-user localhost daemon (adding them would be ceremony, not security — STRIDE already closed 25/25).

## Feature Landscape

### Table Stakes (Users Expect These)

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| RRF fusion with k=60 default | Every hybrid implementation surveyed (LanceDB, sqlite-vec canonical, sqlite-rag, llama-stack, qmd) uses rank-based RRF, k=60 | MEDIUM | Single SQL: two ranked CTEs + FULL OUTER JOIN + coalesce. Reuses existing FTS5 + vec legs untouched |
| Dedup-by-fusion (memory in both lists gets both contributions) | This is the definition of RRF; naive "keep one" dedup is a documented bug (qmd #331) | LOW | Falls out of the JOIN + coalesce pattern automatically |
| Hybrid degrades to keyword-only without Ollama | Existing SC2 contract; hybrid must not break the zero-dependency promise | MEDIUM | Empty vec leg ⇒ RRF reduces to keyword order; report degraded mode in the existing `SearchOutcome` envelope |
| Over-fetch candidates then truncate post-fusion | All implementations fetch 2–4× limit per leg before fusing | LOW | Otherwise fusion has nothing to reorder |
| `memory_update(id, partial_patch)` | mem0 `update(id, data)` and Letta `core_memory_replace` prove this is the shape agents use; "update rather than accumulate" is the field norm | MEDIUM | Content change ⇒ re-embed + FTS refresh; empty patch ⇒ invalid_params; Ollama-down follows store-path degradation |
| Edge cleanup on forget/TTL | Dangling relations = corrupt reads | LOW | `ON DELETE CASCADE` FK on the links table; covers both removal paths in one place |
| 400 on all malformed boundary input (limit/ttl extremes, overflow) | REST norm: invalid client input never reads as server fault; extends our locked two-tier taxonomy | LOW | Bounded newtypes/manual checks; no validation crate needed at this surface size |
| Documented limit max with one consistent policy | API-design consensus: default + hard max + explicit clamp-or-reject choice | LOW | Recommend 400-reject to match the existing loud-error taxonomy |
| Exact tag-match, unknown tag ⇒ empty result | WR-07; substring matching surprises agents | LOW | |
| JSONL export, one memory per line, version header | ChromaDB Data Pipes precedent; MCP reference server persists as JSONL; raw DB copy is backup not export | MEDIUM | Header line: format version, tool version, embedding model+dim, timestamp |
| Export carries timestamps + TTL + type + tags | Dropping timestamps silently resets decay = data corruption | LOW | Include relations once MCP-06 lands (second record kind) |
| Idempotent round-trip import | Export without import is a dead end; matches the existing GSD-import idempotency contract | MEDIUM | Deterministic by-id skip/overwrite; re-embed when embeddings absent/model-mismatched |
| Windows + musl binaries checksummed like existing targets | Distribution parity promise from the acceptance criteria | HIGH (Windows) / LOW–MEDIUM (musl) | Windows = cfg(unix) refactor per mcp-hub precedent (known multi-spot slog). musl = sqlite-vec upstream PR #199 (removes BSD typedef block) or CFLAGS/vendored shim if the fix isn't in the pinned release |

### Differentiators (Competitive Advantage)

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Decay-aware hybrid ranking | Comparable tools lack recency bias in hybrid search and users explicitly request it (qmd #331 asks for temporal decay); we already have the decay engine | LOW | Apply existing decay multiplier post-fusion; down-rank only, preserves STORE-03/04 kill-test invariants |
| Flat id-addressed relations (`memory_link(from_id, to_id, relation)`) | The official MCP memory server's triple shape *minus* the entity/observation ceremony that demonstrably confuses agents; stable-id keys kill entity-name drift | MEDIUM | One table, 2–3 tools (`memory_link`, `memory_unlink`, links surfaced in results); no graph query language |
| `related` memories surfaced inline in search/get results (1 hop) | Agents get graph value without issuing graph queries — zero new retrieval concepts to misuse | LOW | Bounded fan-out (cap the related list) to protect context windows |
| Exposed `k`, `w_fts`, `w_vec` config with sane defaults | LanceDB exposes k; power users tune, everyone else never touches it | LOW | Config/env only — do NOT add per-call tuning params to the MCP tool schema (schema bloat raises agent error rates) |
| Embeddings-optional export with re-embed-on-import | ChromaDB Data Pipes is the only comparable with this; makes exports genuinely portable across embedding model versions | LOW | Default exclude; `--include-embeddings` flag tags model+dim |
| Fully-local hybrid search in a single static binary | mem0/Zep need cloud or LLM pipelines; qmd needs Bun + model downloads; we do BM25+vector+RRF+decay in one brew-installable binary | — | The milestone's headline positioning, not extra work |

### Anti-Features (Commonly Requested, Often Problematic)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Entity/observation knowledge graph (official MCP memory server shape) | "Real" memory graphs look impressive; the reference server does it | 9-tool surface causes tool-choice confusion; string entity names drift ("Jiri"/"Jiří"/"the user"); agents dump into observations and ignore relations. The project already rejected full KG modeling in v1 — research confirms that call | Flat id-addressed links between existing memories; 2–3 tools max |
| LLM-driven auto-arbitration on write (mem0 AUDN: LLM decides add/update/delete) | "Smart" dedup and contradiction handling | Requires an LLM inside the daemon — violates zero-cloud/no-runtime-dep constraint; non-deterministic writes are untestable against our kill-test culture | Deterministic `memory_update(id, patch)`; let the *calling agent* (which already has an LLM) decide, like Letta does |
| Bi-temporal fact invalidation (Zep valid_at/invalid_at edges) | State-of-the-art memory papers showcase it | Needs LLM extraction + contradiction-detection pipeline; massive complexity for a single-user local store | Decay + TTL + explicit update/forget already cover "facts go stale" for our use case |
| Cross-encoder re-ranking stage (qmd `query` mode) | Measurably better final ordering | Second model dependency beyond the Ollama embed model; latency; qmd needed position-aware blending hacks to make it behave | RRF + decay multiplier; leave reranking as a possible future opt-in behind Ollama |
| Score-normalization fusion instead of RRF (normalized BM25 + cosine weighted sum) | Feels more "precise" than rank-only | BM25/vector score scales are incompatible; normalization is corpus-dependent and brittle — the exact problem RRF was invented to avoid (and what qmd #331 users trip on) | Rank-only RRF |
| `VACUUM INTO` / raw DB copy as the export feature | One-line implementation, "it's already SQLite" | Pins schema version, includes FTS/vec shadow tables, useless to any other tool — a backup, not a portable contract | Versioned JSONL export; optionally *also* document `VACUUM INTO` as a backup tip in the README (zero code) |
| Graph traversal / multi-hop query tools ("find path from A to B") | Comes free with graph framing | Agents misuse open-ended traversal; unbounded result explosion; no demonstrated agent win beyond 1 hop | 1-hop `related` field inline in results |
| Per-call RRF tuning params in the MCP tool schema (`k`, weights as tool args) | Power-user flexibility | Every extra schema param increases agent tool-call error rate; agents will cargo-cult bad values | Server config/env vars; keep tool schemas minimal |
| Auth/API-keys/rate limiting on the REST API | "Hardening" sounds like auth | Single-user loopback-guarded localhost daemon; auth adds ceremony without a threat-model change (STRIDE 25/25 closed) | Keep the loopback guard; hardening = input validation only |
| Silent limit clamping without documentation | "Friendlier" than erroring | Hides bugs in agent client code; inconsistent with the locked loud-error (400/invalid_params) taxonomy | 400-reject above the documented max (or clamp — but documented and tested; do not mix policies) |

## Feature Dependencies

```
SEARCH-04 hybrid RRF
    └──requires──> FTS5 keyword leg (shipped)
    └──requires──> Ollama vector leg + SC2 fallback (shipped)
    └──requires──> over-fetch depth per leg (new, trivial)
[decay multiplier post-fusion] ──enhances──> SEARCH-04 (shipped engine, new wiring)

MCP-06 memory_update
    └──requires──> re-embed pipeline on content change (shipped embed path, new trigger)
    └──requires──> two-tier error taxonomy (shipped) for empty-patch/bad-id
MCP-06 memory_link / relations
    └──requires──> stable memory IDs (shipped)
    └──requires──> cascade delete on forget + TTL sweep (new FK)

DIST-04 export/import
    └──requires──> relations schema settled first (export format must include links)
    └──requires──> idempotent import machinery (shipped GSD-import pattern, generalize)
    └──requires──> re-embed-on-import via Ollama + SC2 degradation (shipped pattern)

REST hardening ── independent ── (extends the shipped 400/invalid_params taxonomy)

musl binaries ──requires──> sqlite-vec BSD-typedef fix (upstream PR #199 or CFLAGS shim)
DIST-03 Windows ──requires──> cfg(unix) audit/refactor (mcp-hub precedent)
[Windows + musl] ──enhance──> DIST-04 (portable export matters more once binaries run everywhere)

[Per-call tuning params] ──conflicts──> minimal MCP tool schemas (tuning lives in config)
[Raw-DB-copy export] ──conflicts──> versioned JSONL contract (backup ≠ export)
```

### Dependency Notes

- **Hybrid RRF requires both shipped legs unchanged:** it is a fusion layer, not a new retriever. The SC2 fallback contract transfers automatically if the vector leg's emptiness is handled with `coalesce` — and the `SearchOutcome` envelope should report `mode: hybrid | keyword_only` so agents and tests can assert degradation.
- **Relations must land before the export format freezes:** the JSONL v1 contract should include link records from day one; otherwise format v2 arrives one phase later. Sequence MCP-06 before DIST-04 (or at minimum settle the links schema first).
- **memory_update reuses the store path's embedding degradation:** update-with-Ollama-down must behave exactly like store-with-Ollama-down (succeed, keyword-searchable, no embedding) — a divergence here would create a new class of inconsistency.
- **Cascade delete touches both removal paths:** TTL sweep and explicit forget are the only removal paths (locked decision); both must now clean edges. FK `ON DELETE CASCADE` handles both in one place.
- **musl fix is likely upstream already:** sqlite-vec PR #199 removes the BSD typedef block (confirmed on Alpine musl + Ubuntu glibc); verify whether it's in the pinned release, else vendored patch/CFLAGS shim (both LOW effort). Windows is the expensive one — cfg(unix) audit across daemon paths, per the documented mcp-hub precedent.

## MVP Definition

### Launch With (v1.1 milestone / product v0.1.0)

- [ ] Hybrid RRF search (k=60, w=1.0/1.0, over-fetch, FULL-OUTER-JOIN dedup, decay multiplier post-fusion, keyword-only degradation reported in `SearchOutcome`) — the headline recall-quality feature
- [ ] `memory_update(id, partial_patch)` with re-embed on content change — completes CRUD; field-proven shape (mem0/Letta)
- [ ] `memory_link` / `memory_unlink` + `related` in results + cascade delete — flat relations, settled before the export format
- [ ] REST boundary hardening (limit default+max with 400-reject, ttl bounds, exact tag match, overflow → 400) — closes WR-01/WR-02/WR-07
- [ ] JSONL export with version header + idempotent import with re-embed — export without import is a dead end
- [ ] musl binaries (typedef fix) — cheap, unblocks Alpine/containers
- [ ] Windows binaries (cfg(unix) refactor) — acceptance-criteria parity; the known HIGH-effort item

### Add After Validation (v1.x)

- [ ] Configurable k / w_fts / w_vec via config file/env — after real usage shows defaults falling short
- [ ] `--include-embeddings` export flag with model+dim tagging — when a same-model restore use case shows up
- [ ] Relation-type vocabulary guidance in tool descriptions (suggested set, free string accepted) — after observing what relation strings agents actually write

### Future Consideration (v2+)

- [ ] Optional local re-ranking stage (Ollama-served reranker) — only if RRF+decay recall proves insufficient; second model dependency
- [ ] Cross-tool import adapters (mem0/Chroma JSONL dialects) — no universal standard exists; wait for demand
- [ ] Multi-hop relation queries — no evidence agents use them well; revisit only with concrete transcripts showing 1-hop insufficiency

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| Hybrid RRF (k=60 + dedup + fallback) | HIGH | MEDIUM | P1 |
| Decay multiplier on fused score | HIGH | LOW | P1 |
| memory_update | HIGH | MEDIUM | P1 |
| Flat memory_link/unlink + related-in-results | MEDIUM | MEDIUM | P1 |
| Cascade delete of edges | HIGH (integrity) | LOW | P1 |
| REST boundary hardening | MEDIUM | LOW | P1 |
| JSONL export + idempotent import | HIGH | MEDIUM | P1 |
| musl binaries | MEDIUM | LOW–MEDIUM | P1 |
| Windows binaries | MEDIUM | HIGH | P2 (in-milestone, sequence last; known slog) |
| Config-exposed k/weights | LOW | LOW | P2 |
| --include-embeddings export | LOW | LOW | P2 |
| Local reranker stage | MEDIUM | HIGH | P3 |
| Multi-hop graph queries | LOW | HIGH | P3 (anti-feature until proven otherwise) |

## Competitor Feature Analysis

| Feature | mem0 | Letta/MemGPT | Zep/Graphiti | Official MCP memory | LanceDB / sqlite-vec | qmd | Our Approach |
|---------|------|--------------|--------------|--------------------|--------------------|-----|--------------|
| Hybrid search | vector-first fusion | archival embed search | hybrid over graph | none (substring over graph) | RRF reranker, k=60 default | BM25+vec+RRF (orig query 2×) + cross-encoder | RRF k=60, SQL CTE + FULL OUTER JOIN, decay post-fusion, no reranker |
| Recency/decay in ranking | no | no | temporal edges (heavyweight) | no | no | requested, unimplemented (#331) | shipped decay engine wired into fusion — differentiator |
| Memory update | `update(id, data)` + server-side LLM AUDN | `core_memory_replace(label, old, new)` | LLM invalidation pipeline | delete + recreate observations | n/a | n/a (read-only index) | deterministic `memory_update(id, patch)`, re-embed on content change, no LLM in daemon |
| Relations | optional graph add-on | none (blocks, no graph) | full bi-temporal KG | entities+relations+observations, 9 tools, name-string keys | n/a | n/a | flat id-keyed triples, 2–3 tools, 1-hop `related` in results |
| Export/import | cloud export API | agent-file export | n/a (DB-bound) | whole-graph JSONL file | Data Pipes JSONL ± embeddings, re-embed on import | index rebuildable from source files | versioned JSONL, embeddings excluded by default, idempotent re-embedding import |
| Input validation posture | cloud API (server-enforced) | server-enforced | server-enforced | minimal (local stdio) | n/a | n/a | 400/invalid_params taxonomy extended to boundary values; reject above max limit |
| Fully local, single binary | no (cloud or Python+LLM) | no (server + Postgres) | no | yes (Node) | library, not product | yes (Bun + local models) | yes — Rust static binary; brew, Windows, musl |

## Sources

Confidence tiers assigned via `gsd-tools query classify-confidence` (context7 → MEDIUM; websearch cross-verified → MEDIUM; single-source websearch/webfetch → LOW). Digests cached in `.planning/research/.cache` via the research-store seam.

**Hybrid RRF (MEDIUM — cross-verified):**
- [Alex Garcia — Hybrid full-text search and vector search with SQLite](https://alexgarcia.xyz/blog/2024/sqlite-vec-hybrid-search/index.html) (canonical sqlite-vec pattern: CTEs, FULL OUTER JOIN, coalesce, k=60, weights)
- LanceDB `RRFReranker` API docs via Context7 (`/lancedb/lancedb`) — `create(k)` default k=60
- [Reciprocal Rank Fusion explained](https://blog.serghei.pl/posts/reciprocal-rank-fusion-explained/), [Spice AI — RRF](https://spice.ai/learn/reciprocal-rank-fusion), [apxml — RRF fusion algorithms](https://apxml.com/courses/advanced-vector-search-llms/chapter-3-hybrid-search-approaches/rrf-fusion-algorithms), [MongoDB — RRF](https://www.mongodb.com/resources/basics/reciprocal-rank-fusion) (k=60 standard, flat optimum [20,100])
- [Simon Willison on sqlite-vec hybrid](https://simonwillison.net/2024/Oct/4/hybrid-full-text-search-and-vector-search-with-sqlite/), [sqliteai/sqlite-rag](https://github.com/sqliteai/sqlite-rag), [llama-stack hybrid search issue #1158](https://github.com/meta-llama/llama-stack/issues/1158)

**Update/relations tool shapes (MEDIUM — cross-verified):**
- [mem0 — Update Memory docs](https://docs.mem0.ai/core-concepts/memory-operations/update), [mem0 memory operations (DeepWiki)](https://deepwiki.com/mem0ai/mem0/3.3-history-and-storage-management) (AUDN cycle is server-side)
- [Letta — MemGPT agents docs](https://docs.letta.com/guides/legacy/memgpt_agents_legacy) (core_memory_replace/append, archival tools)
- [Official MCP memory server](https://github.com/modelcontextprotocol/servers/tree/main/src/memory), [npm @modelcontextprotocol/server-memory](https://www.npmjs.com/package/@modelcontextprotocol/server-memory) (entities/relations/observations, 9-tool surface)
- [Zep temporal KG paper (arXiv 2501.13956)](https://arxiv.org/html/2501.13956v1), [Graphiti overview (Neo4j blog)](https://neo4j.com/blog/developer/graphiti-knowledge-graph-memory/) (bi-temporal edges, invalidation)

**qmd + hybrid pain points (MEDIUM):**
- [tobi/qmd](https://github.com/tobi/qmd), [qmd search modes (DeepWiki)](https://deepwiki.com/tobi/qmd/3.2-search-modes-explained), [qmd issue #331 — ranking improvements](https://github.com/tobi/qmd/issues/331) (score mismatch, temporal-decay gap, dedup bugs)

**Export formats (LOW–MEDIUM):**
- [ChromaDB Data Pipes](https://datapipes.chromadb.dev/) (JSONL export ± embeddings, re-embed on import)
- [Exporting SQLite to CSV/JSON/SQL (Sling Academy)](https://www.slingacademy.com/article/exporting-sqlite-data-to-csv-json-and-sql-formats/), [High Performance SQLite — Exports](https://databaseschool.com/series/high-performance-sqlite/videos/71) (VACUUM INTO = backup, not interchange)

**REST validation (MEDIUM — convergent):**
- [Speakeasy — pagination best practices](https://www.speakeasy.com/api-design/pagination), [restfulapi.net — pagination/sorting/filtering](https://restfulapi.net/api-pagination-sorting-filtering/), [Vinay Sahni — pragmatic REST](https://www.vinaysahni.com/best-practices-for-a-pragmatic-restful-api) (default+max limit, clamp-vs-400 both accepted if documented, 400 never 500)
- [axum-valid](https://github.com/gengteng/axum-valid), [garde](https://github.com/jprochazk/garde), [axum official validator example](https://github.com/tokio-rs/axum/blob/main/examples/validator/src/main.rs) (manual extractor idiomatic at small surface)

**Distribution (MEDIUM):**
- [sqlite-vec PR #199 — musl typedef fix](https://github.com/asg017/sqlite-vec/pull/199), [Mozilla bug 1964446](https://bugzilla.mozilla.org/show_bug.cgi?id=1964446)

---
*Feature research for: agent-memory v1.1 Hardening & Interop (hybrid RRF, MCP update/relations, REST hardening, portable export, Windows/musl)*
*Researched: 2026-07-12*
