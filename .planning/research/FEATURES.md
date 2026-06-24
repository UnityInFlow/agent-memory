# Feature Research

**Domain:** Cross-runtime persistent agent memory (local-first MCP memory server)
**Researched:** 2026-06-24
**Confidence:** HIGH (grounded in named comparables: Mem0, Letta/MemGPT, Zep/Graphiti, OpenAI Memory, MCP reference knowledge-graph server, sqlite-vec hybrid search)

## Orientation: the comparables

| Product | Storage model | Retrieval | Forgetting | Surface | Local? |
|---------|---------------|-----------|------------|---------|--------|
| **MCP reference memory server** | Knowledge graph (entities/relations/observations) in a `memory.jsonl` file | `search_nodes` over names/types/observation text (substring) | None — manual delete only | MCP tools: `create_entities`, `create_relations`, `add_observations`, `delete_*`, `read_graph`, `search_nodes`, `open_nodes` | Yes (JSONL file) |
| **Mem0** | Managed vector store + extracted facts | Hybrid: semantic + BM25 + entity matching, fused; rich `filters` (AND/OR/NOT, `in/gte/lte/icontains`) | "Latest truth wins" dedup/contradiction resolution on `add`; no time decay | SDK `add`/`search`/`get_all`/`delete`; scope via `user_id`/`agent_id`/`run_id`; `metadata` + custom `categories` | Self-host possible, cloud-default |
| **Letta / MemGPT** | 3 tiers: core (in-context blocks), recall (message history), archival (vector store) | `archival_memory_search` (semantic); recall is searchable history | Agent self-edits blocks (`core_memory_append/replace`); no auto-decay | Agent-callable memory functions | Self-host (server) |
| **Zep / Graphiti** | Bi-temporal knowledge graph; facts have valid-time + ingestion-time | Graph + semantic; sub-200ms | Facts **invalidated, not deleted** when contradicted (history preserved) | API + Graphiti OSS | Self-host (Neo4j/FalkorDB) |
| **OpenAI / ChatGPT Memory** | "Saved memories" (explicit, auditable) + "chat history" (implicit, "dreaming") | Opaque relevance injection | User delete; implicit memories auto-curated | UI toggles, no dev API for the store | No |

**The gap agent-memory fills:** none of these is a *single-binary, zero-cloud, MCP-native* memory shared across runtimes (Claude Code, Cursor, etc.) with GSD/dev-workflow typing. The MCP reference server is local but has no decay, no embeddings, and a graph model that is awkward for "remember this decision." Mem0/Letta/Zep have the smart features but pull in a cloud or a heavy DB. **The differentiator is "Mem0-grade typed memory in one local Rust binary over MCP."**

## Feature Landscape

### Table Stakes (Users Expect These)

Missing these = the product feels broken or untrustworthy versus the MCP reference server and Mem0.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **MCP server with store/search/list/forget tools** | This is the product's entire reason to exist; every MCP memory server exposes a mutation+query surface | MEDIUM | Use a maintained Rust MCP SDK (rmcp / official). Tool naming below. |
| **`memory_store`** — write a memory | Without write there is no memory | LOW | Args: `content` (req), `type` (enum), `tags[]`, `source`, `scope`/`project`, `ttl`, optional `id` for upsert. Return the stored record + `id`. |
| **`memory_search`** — relevance query | The single most-called tool; agents inject results into context | MEDIUM | Args: `query` (req), `type?`, `scope?`, `tags?`, `limit` (default ~5–10), `min_score?`. Return ranked records with score. |
| **`memory_list`** — browse/filter without a query | Auditability; users must be able to see what's stored (OpenAI's lesson: only auditable memory is trusted) | LOW | Args: `type?`, `scope?`, `tags?`, `limit`, `offset`, `sort` (recency/score). Deterministic, no embedding call. |
| **`memory_forget`** — delete by id or filter | Trust + GDPR-style control; OpenAI/Mem0 both expose delete | LOW | Args: `id` OR a filter (`type`/`scope`/`tags`). Confirm count deleted. Hard delete for v0.0.1. |
| **Keyword/substring search baseline** | The reference server ships substring search; semantic must not be the *only* path (works before Ollama is up) | LOW | SQLite FTS5 (BM25). Also the fallback when embeddings unavailable. |
| **Persistence across sessions** | "Survives session boundaries" is the core problem statement | LOW | SQLite file in a stable location (e.g. `~/.agent-memory/memory.db`). |
| **Memory typing** (DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT) | Mem0 has categories, Letta has labeled blocks; typing is what makes a *dev* memory useful vs a chat blob | LOW | Stored enum column; filterable in search/list. Already in the spec. |
| **Metadata: tags, source, scope/project, timestamps** | Mem0 scopes by `user_id`/`agent_id`/`run_id`; without scope, memories from project A leak into project B | LOW–MEDIUM | `created_at`, `updated_at`, `last_accessed_at`, `access_count`, `source`, `scope`/`project`, `tags[]`. `scope` is the cross-runtime-but-per-project key. |
| **Stable JSON record shape** | Agents and the REST API both parse it; breaking shape breaks every consumer | LOW | One canonical serde struct used by MCP, REST, and CLI. |
| **README with copy-paste MCP config** | MCP servers live or die on a 30-second install; reference server and every popular server lead with the JSON snippet | LOW | Claude Code / Cursor `mcpServers` block + Homebrew one-liner. |

### Differentiators (Competitive Advantage)

Aligned with PROJECT.md Core Value (local-first, observability, governance) and the cross-runtime story.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| **Single static Rust binary, zero cloud, zero account** | Mem0/Zep need a cloud or Neo4j; Letta needs a server. One `brew install` daemon is uniquely frictionless | MEDIUM | This is *the* wedge. SQLite embedded, no DB process. |
| **Local semantic search via Ollama (`nomic-embed-text`)** | Mem0-grade semantic recall with **no data leaving the machine** — privacy-by-architecture | MEDIUM–HIGH | Cosine similarity over stored embeddings. Must degrade gracefully to FTS5 when Ollama is down (see dependencies). |
| **Hybrid ranking: semantic + recency-decay + access frequency** | Stanford generative-agents / FadeMem-style scoring `score = α·similarity + β·recency + γ·frequency` — more relevant than pure cosine | MEDIUM | Combine cosine, exponential recency decay, and `access_count`. RRF or weighted sum over FTS5+vector (proven in sqlite-vec hybrid examples). |
| **Decay / forgetting model with exposed relevance** | No comparable local server forgets; Zep only invalidates. Exposing a `decay_score` makes "old, unused context fades" a feature, not silent loss | MEDIUM | Exponential decay on `last_accessed_at`; background pass updates scores. **Decay should down-rank, not auto-delete** (deletion = TTL's job). Surface `score`/`decay_score` in every result so agents/users see why something ranked. |
| **TTL / expiry** | Ephemeral memories (a TODO, a transient error) shouldn't linger forever | LOW | `expires_at` column; lazy purge on access + periodic sweep. |
| **GSD STATE.md import** | Stated requirement; turns the existing GSD ecosystem into instant seed data and is a concrete cross-tool interop proof | MEDIUM | `agent-memory import --from gsd-state .planning/STATE.md`. Parse sections → typed memories (Decisions Log → DECISION, etc.). |
| **Cross-runtime by being MCP-native + REST** | The whole pitch: same memory in Claude Code, Cursor, and any HTTP client | MEDIUM | REST mirrors the MCP tool surface for non-MCP consumers. |
| **Export (dump to JSON / portable file)** | Trust + portability; counters lock-in fear and pairs with import | LOW | `agent-memory export --format json`. Cheap once the record shape exists. |
| **Observability hooks (counts, last-access, decay stats)** | Ecosystem is "observability-first"; a `memory_stats` tool or `--stats` CLI fits the brand and aids debugging | LOW | Counts per type/scope, oldest/newest, embedding coverage. |

### Anti-Features (Commonly Requested, Often Problematic)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| **Full knowledge-graph (entities + relations) à la MCP reference / Zep** | "Graphs are powerful," familiar from the reference server | Doubles the data model + tool surface; `create_relations`/graph traversal is a research rabbit hole; not needed for "remember this decision." Zep needs Neo4j for it | Flat typed records + tags + `scope`. Add a lightweight `relate`/`related_to` only if v1 validates demand. |
| **Cloud sync / hosted multi-user backend** | "Use my memory on another machine" | Kills the zero-cloud/zero-account differentiator; introduces auth, infra, privacy/compliance burden — exactly what Mem0/Zep carry | Local file + `export`/`import` for portability. Sync is a v2+ opt-in plugin, never core. |
| **LLM-based fact extraction on write (Mem0's `add` pipeline)** | "Auto-summarize what's worth remembering" | Requires an LLM call per write, adds latency/cost/nondeterminism, and a cloud LLM breaks local-first. Mem0's contradiction-resolution is complex to match | Agent decides what to store (Letta's model); store verbatim `content`. Optional local-LLM summarize is v2+. |
| **Automatic implicit memory ("dreaming" / passive capture)** | ChatGPT does it; "I don't want to call store manually" | Opaque, untrustworthy (OpenAI's own auditability gap), and impossible to do well without an LLM in the loop | Explicit `memory_store` only. Auditable by design. |
| **Built-in embedding model in the binary** | "Don't make me run Ollama" | Bloats the binary, pins a model, complicates cross-platform builds; sqlite-lembed/bundled-transformer is fragile in Rust today | Depend on Ollama (already chosen); fall back to FTS5 keyword search when absent so the tool still works. |
| **ANN / vector index optimization** | "Won't full-scan cosine be slow?" | sqlite-vec itself only does full scans today; premature for a personal/per-project store (thousands, not millions, of rows) | Full-scan cosine is fine at expected scale. Revisit only if a user hits 100k+ memories. |
| **Web UI / dashboard** | "I want to see my memories" | Backend/CLI must work and be tested first (ecosystem rule: no frontend before backend); a UI is a separate tool's job (token-dashboard pattern) | `memory_list` + `--stats` CLI + `export` to JSON. UI is out of scope. |
| **Multi-tenant auth / RBAC on the REST API** | "Secure the API" | The product is local-first, single-user; auth is solving a problem that doesn't exist on `localhost` | Bind REST to localhost by default; document not exposing it publicly. |

## Feature Dependencies

```
SQLite schema (memories table: id, content, type, tags, source, scope,
   created_at, updated_at, last_accessed_at, access_count, expires_at,
   embedding, decay_score)
    ├──requires──> memory_store / memory_list / memory_forget   (CRUD, no embeddings)
    ├──requires──> FTS5 keyword search  ──requires──> memory_search (baseline path)
    └──requires──> TTL / expiry sweep

Ollama embedding client
    └──enables──> semantic search ──enhances──> memory_search (semantic path)

memory_search (semantic) + recency decay + access frequency
    └──compose──> hybrid ranking (RRF / weighted score)

Decay background pass ──reads/writes──> last_accessed_at, access_count, decay_score
    └──enhances──> hybrid ranking
    (TTL purge is SEPARATE from decay — decay down-ranks, TTL deletes)

Stable JSON record shape
    ├──requires──> MCP tool surface
    └──requires──> REST API  (mirror of MCP tools)

GSD STATE.md import parser ──requires──> memory_store + typing
Export ──requires──> stable JSON record shape
```

### Dependency Notes

- **memory_search (semantic) requires the Ollama client, but must NOT hard-require it at runtime:** if Ollama is unreachable, search falls back to FTS5 keyword/BM25. This keeps the binary useful on a fresh machine and is the single most important resilience decision.
- **Hybrid ranking enhances search but depends on both paths existing:** ship FTS5 first, add semantic, then fuse. Do not build fusion before both inputs work.
- **Decay and TTL are independent and must not be conflated:** decay changes *ranking* (a quiet memory sinks); TTL changes *existence* (an expired memory is gone). Implementing decay as deletion would silently lose data and break trust.
- **GSD import and Export both depend only on the stable record shape + store path** — cheap to add once CRUD exists; good early interop proof.
- **REST API is a thin mirror of the MCP tools over the same service layer** — build the service layer once, expose it twice. Don't write business logic in the MCP handlers.

## MVP Definition

### Launch With (v0.0.1)

- [ ] **SQLite schema + persistence** — the foundation everything else needs; zero-config local file.
- [ ] **MCP server: `memory_store`, `memory_search`, `memory_list`, `memory_forget`** — the product's reason to exist (spec-mandated 4 tools).
- [ ] **Memory typing + metadata** (type enum, tags, source, scope, timestamps, access_count) — what makes it a *dev* memory, and filtering depends on it.
- [ ] **FTS5 keyword search** — works before/without Ollama; baseline `memory_search` path.
- [ ] **Ollama semantic search** with **graceful fallback to FTS5** — the privacy differentiator; spec-mandated.
- [ ] **Exponential decay scoring** (down-ranks, background pass) + **TTL expiry** — spec-mandated; the "memories fade" feature with relevance surfaced in results.
- [ ] **GSD STATE.md import** (`import --from gsd-state`) — spec-mandated, instant seed + interop proof.
- [ ] **REST API mirroring the MCP tools** — spec-mandated non-MCP path; thin layer over shared service.
- [ ] **CLI** (`store`/`search`/`list`/`forget`/`import`/`export`/`stats`) + **README MCP config** + **prebuilt binaries + Homebrew** — install/adoption table stakes.

### Add After Validation (v1.x)

- [ ] **`memory_update`** (in-place edit by id) — trigger: users asking to revise rather than forget+re-store. (Letta's `core_memory_replace`.)
- [ ] **Hybrid RRF fusion tuning + per-type decay rates** — trigger: users report semantic-only or keyword-only ranking misses; important types (CONSTRAINT) should decay slower.
- [ ] **Export/import round-trip + more importers** (Cursor rules, Superpowers context, RTK db) — trigger: cross-runtime adoption requests.
- [ ] **`memory_stats` MCP tool** — trigger: debugging/observability demand beyond the CLI.

### Future Consideration (v2+)

- [ ] **Lightweight relations (`relate` / `related_to` tag)** — defer: only if flat-record model proves insufficient; avoid the full graph.
- [ ] **Optional local-LLM summarization on store** — defer: nondeterminism + latency; only when a clean local path exists.
- [ ] **Opt-in encrypted sync between a user's own machines** — defer: must never compromise zero-cloud default; plugin, not core.
- [ ] **ANN vector index** — defer: only when a real user exceeds full-scan-comfortable scale (100k+ rows).

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| SQLite schema + persistence | HIGH | LOW | P1 |
| MCP store/search/list/forget tools | HIGH | MEDIUM | P1 |
| Memory typing + metadata (incl. scope) | HIGH | LOW | P1 |
| FTS5 keyword search (+ fallback) | HIGH | LOW | P1 |
| Ollama semantic search | HIGH | MEDIUM | P1 |
| Decay scoring + TTL | MEDIUM | MEDIUM | P1 |
| GSD STATE.md import | MEDIUM | MEDIUM | P1 |
| REST API (mirror) | MEDIUM | LOW | P1 |
| CLI + Homebrew + README config | HIGH | LOW | P1 |
| Export to JSON | MEDIUM | LOW | P2 |
| Hybrid RRF ranking | MEDIUM | MEDIUM | P2 |
| memory_update | MEDIUM | LOW | P2 |
| memory_stats / observability | MEDIUM | LOW | P2 |
| Lightweight relations | LOW | MEDIUM | P3 |
| Local-LLM summarization | LOW | HIGH | P3 |
| Encrypted self-sync | MEDIUM | HIGH | P3 |
| Knowledge graph | LOW | HIGH | (anti-feature) |
| Cloud sync / hosted backend | LOW | HIGH | (anti-feature) |

**Priority key:** P1 must-have for launch · P2 add when possible · P3 future.

## Competitor Feature Analysis

| Feature | MCP reference server | Mem0 | Letta / Zep | Our Approach |
|---------|----------------------|------|-------------|--------------|
| Tool surface | Graph mutate + `search_nodes` (substring) | SDK `add`/`search`/`get_all`/`delete` | Agent memory functions / API | **4 MCP tools** (`memory_store/search/list/forget`) + REST mirror; flat typed records |
| Search | Substring over text | Hybrid semantic+BM25+entity, rich filters | Semantic (archival), graph (Zep) | FTS5 baseline → **Ollama semantic** → hybrid fusion (v1.x) |
| Typing/metadata | entity `type` + observations | `metadata` + custom categories, `user/agent/run_id` scope | Labeled blocks (Letta) | **6 fixed dev types** + tags + `source` + `scope`/project + timestamps |
| Forgetting | Manual delete only | Contradiction dedup ("latest wins") | Invalidate-not-delete (Zep); agent edits (Letta) | **Exponential decay (down-rank)** + **TTL (delete)**, both surfaced |
| Storage / locality | Local JSONL | Cloud-default | Server / Neo4j | **Single Rust binary + SQLite, zero cloud** |
| Import/interop | none | SDK import | API | **GSD STATE.md import** + JSON export |
| Embeddings | none | Cloud/provider | Provider | **Local Ollama `nomic-embed-text`**, FTS5 fallback |

## Sources

- MCP reference knowledge-graph memory server — tool surface & JSONL persistence: https://github.com/modelcontextprotocol/servers/tree/main/src/memory (HIGH)
- Mem0 add/search, hybrid retrieval, metadata/categories, scoping: https://docs.mem0.ai/api-reference/memory/search-memories , https://github.com/mem0ai/mem0 , https://github.com/mem0ai/mem0/blob/main/docs/core-concepts/memory-operations/add.mdx (HIGH)
- Letta/MemGPT core/recall/archival tiers & memory blocks: https://www.letta.com/blog/agent-memory/ (HIGH)
- Zep/Graphiti bi-temporal graph, fact invalidation vs deletion: https://arxiv.org/abs/2501.13956 , https://github.com/getzep/graphiti (HIGH)
- OpenAI ChatGPT saved-memories vs chat-history, auditability, user control: https://help.openai.com/en/articles/8590148-memory-faq , https://openai.com/index/memory-and-new-controls-for-chatgpt/ (HIGH)
- Decay/forgetting & relevance scoring (recency·frequency·similarity), forgetting curves: https://arxiv.org/html/2601.18642 (FadeMem) , https://co-r-e.com/method/agent-memory-forgetting (MEDIUM)
- SQLite hybrid search (FTS5 BM25 + sqlite-vec + RRF), full-scan limitation: https://alexgarcia.xyz/blog/2024/sqlite-vec-hybrid-search/index.html , https://simonwillison.net/2024/Oct/4/hybrid-full-text-search-and-vector-search-with-sqlite/ (HIGH)

---
*Feature research for: cross-runtime persistent agent memory (local-first MCP memory server)*
*Researched: 2026-06-24*
