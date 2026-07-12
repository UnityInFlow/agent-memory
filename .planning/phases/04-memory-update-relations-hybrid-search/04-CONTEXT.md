# Phase 4: Memory Update, Relations & Hybrid Search - Context

**Gathered:** 2026-07-12
**Status:** Ready for planning

<domain>
## Phase Boundary

Agents can update existing memories in place (`memory_update` partial patch with same-transaction FTS re-mirror + vector re-embed), link related memories with flat typed relations (`memory_link`/`memory_unlink`, 1-hop expansion in search/list, cascade on forget/TTL), and get decay-aware hybrid keyword+semantic recall (RRF, k=60, `search_mode: "hybrid"`, keyword-only degradation with unchanged envelope). Migration 0003 ships with hygiene: frozen 0001/0002 (fixture divergence test), pre-migration backup, friendly newer-schema error. Requirements: MCP-06, MCP-07, STORE-05, SEARCH-04, SEARCH-05. No graph/entity model, no export (Phase 5), no request-level mode override (SEARCH-06, deferred).

</domain>

<decisions>
## Implementation Decisions

### Relation kind vocabulary (MCP-07)
- **D-01:** `kind` is a **closed enum** validated at the `MemoryService` seam — v0.1.0 ships exactly three kinds: **`relates_to`, `supersedes`, `caused_by`**. Unknown kind returns `InvalidArgument` listing the allowed values (Phase 3 D-05 message shape: field + allowed set + offending value). Extending the enum later is additive; removing is a contract break, hence the minimal start.
- **D-02:** Edge cases: **duplicate link (same from/to/kind) is a no-op success** (idempotent, agent-retry-friendly), enforced by a UNIQUE constraint in migration 0003; **self-link (from_id == to_id) is `InvalidArgument`**; **unlink of a nonexistent edge returns the clean not-found shape** (same contract as `memory_update` with unknown id).
- **D-03:** Links are **stored directed** (`from_id`, `to_id`, `kind`) so `supersedes`/`caused_by` keep meaning, but **1-hop expansion follows edges in both directions**, annotating each neighbor with kind + direction (reverse view reads naturally, e.g. `superseded_by`). One edge row, no duplicate bookkeeping.

### 1-hop expansion surface (MCP-07)
- **D-04:** Expansion is **opt-in via an `expand_links` boolean** (default false) on `memory_search` and `memory_list`, both transports. The v1.0 envelope stays byte-identical when the flag is absent — existing `search_mode` checks and payload shapes unaffected. The flag routes through the Phase 3 validation seam like every new input.
- **D-05:** Expanded relations are **nested trimmed summaries** per result: `related: [{id, kind, direction, type, content snippet, tags}]` — NOT full MemoryViews. Enough for an agent to decide whether to fetch; bounded payload.
- **D-06:** **Expanded neighbors get NO access bumps** — bumps apply only to post-truncation returned result ids (roadmap criterion 4 taken literally). Expansion is a pure read; decay stays honest.
- **D-07:** Neighbor count per result is capped by a **fixed `pub` const** (e.g. `MAX_EXPANDED_LINKS = 10`; exact value Claude's discretion) living next to `DEFAULT_SEARCH_LIMIT`/`MAX_KNN_K`, documented in the README. Overflow: most-recent edges win. No caller-tunable knob in v0.1.0 (additive later if needed).

### memory_update patch semantics (MCP-06)
- **D-08:** **JSON-merge-patch style**: omitted field = unchanged; explicit `null` = clear where clearing is legal (`ttl` → no expiry, `tags` → empty, `scope` → default). `content` can never be null/empty → `InvalidArgument`. Rust side uses the double-Option (`Option<Option<T>>`) serde pattern. Sentinels were rejected because `ttl_secs: 0` was just made `InvalidArgument` by Phase 3 — overloading it as "clear" would contradict the seam contract.
- **D-09:** **Update bumps `last_accessed`** (reuses existing `bump_access`) — an update is the strongest freshness signal; an edited memory ranks as current. No separate `updated_at` column in 0003.
- **D-10:** `memory_update` returns the **full post-update MemoryView** (same shape as `memory_store`), including `embedding_status` so callers can observe "pending re-embed" after an Ollama outage falls back to `embedding_status = 0` + sweep backfill.

### Migration 0003 hygiene (STORE-05)
- **D-11:** Pre-migration backup is a **sibling timestamped file** (`memories.db.backup-pre-0003-{YYYYMMDD}` pattern) created via **SQLite's online backup API** before migrations run. Retention: **one backup per schema version** — re-running the same upgrade overwrites that version's backup; a different version's backup is never touched.
- **D-12:** Backup is a **generic mechanism standard for all future migrations** (trigger: pending migrations exist AND DB is non-empty) — 0003 is just the first beneficiary; Phases 5-6 inherit it structurally, mirroring Phase 3's validation-seam philosophy.
- **D-13:** Newer-schema DB met by an older binary → **refuse at startup with an upgrade hint**: detect schema version ahead of the binary's known max, exit cleanly with a message naming both versions and the fix (`brew upgrade unityinflow/tap/agent-memory`). No writes attempted, DB untouched. No read-only degraded mode.

### Claude's Discretion
- Exact `MAX_EXPANDED_LINKS` value and const naming/placement.
- Relations table/column naming and index layout in 0003 (UNIQUE(from_id, to_id, kind) required per D-02).
- Backup failure handling detail (abort migration vs proceed) — lean toward abort-with-message; decide during planning.
- RRF implementation details (leg ordering, tie-breaking) within the locked requirement mechanics (k=60, duplicate ids sum, post-fusion decay blend per SEARCH-05).
- Whether the snippet in D-05 truncates at a fixed char count or word boundary.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Milestone research (decides the patterns this phase implements)
- `.planning/research/SUMMARY.md` — synthesized v1.1 research; sequencing constraints (relations before export)
- `.planning/research/ARCHITECTURE.md` — service-seam validation pattern, exhaustive-mapper enforcement, migration structure
- `.planning/research/PITFALLS.md` — same-transaction re-embed rationale, migration hygiene traps
- `.planning/research/FEATURES.md` — hybrid RRF and relations feature research

### Prior phase contracts (locked, must be honored)
- `.planning/phases/03-api-hardening-toolchain-spikes/03-CONTEXT.md` — validation seam (D-03/D-06), `InvalidArgument` taxonomy, bounds-consts pattern all new inputs must follow
- `.planning/phases/03-api-hardening-toolchain-spikes/03-SECURITY.md` — informational note 1 (internal-tier message contract) and AR-03-01 (pin triangle must not move)
- `.planning/phases/03-api-hardening-toolchain-spikes/03-REVIEW.md` — CR-01 (import dedup NULL-source bug) and WR-01 (filter-after-knn-cut recall loss) sit directly on code this phase touches; WR-02 (poisoned mutex → NotFound → 404 mis-tier) argues for an `Internal` variant while the mappers are already being extended

### Code the phase extends
- `crates/agent-memory-core/src/store/migrations.rs` + `crates/agent-memory-core/sql/0001_init.sql` / `0002_embeddings.sql` — migration chain 0003 appends to; 0001/0002 must be provably frozen
- `crates/agent-memory-core/src/service.rs` — `SearchOutcome` envelope (`search_mode`), seam call sites, semantic/keyword legs to fuse
- `crates/agent-memory-core/src/decay.rs` — existing blend weights and `decay_score` the post-fusion decay blend builds on
- `crates/agent-memory-core/src/store/mod.rs` — Store trait surface gaining update/link/unlink/expansion methods
- `crates/agent-memory/src/mcp.rs` + `crates/agent-memory/src/rest/handlers.rs` — exhaustive mappers gaining new tool/endpoint arms

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `validate_limit`/`validate_ttl` + bounds consts (domain.rs): the pattern (and for ttl, the actual helper — update patches route through `validate_ttl`) new inputs follow; `import()` already proves the seam works for non-store entry points.
- `bump_access` (store): reused for D-09 update-bumps-decay.
- `SearchOutcome { search_mode, results }` envelope (service.rs:57-61): hybrid adds a `Hybrid` variant; keyword degradation keeps `Keyword`.
- 02-05 four-layer test pattern (core unit → in-process handler → MCP unit → spawned-binary HTTP): template for update/link/hybrid proofs.
- Writer-mutex single-writer lane (sqlite.rs): the same-transaction update (FTS re-mirror + vec invalidate/re-embed) lives inside one writer lock acquisition.

### Established Patterns
- Bad client input never surfaces as 500/internal_error (02-05 contract, extended by Phase 3) — link/update/expand inputs validate at the seam before any store call.
- Exhaustive match arms on `MemoryError` force compile-time coverage when new variants land.
- All SQL parameterized; relations queries keep bound parameters.
- Migrations are `M::up(include_str!(...))` entries in migrations.rs — 0003 appends `sql/0003_relations.sql`; the sqlite-vec module must be registered process-globally before migrations run (existing constraint, migrations.rs:23).

### Integration Points
- `service.rs:218/:247` — Semantic/Keyword `search_mode` assignment: hybrid fusion slots between the two legs.
- `store/mod.rs` trait — new methods: update, link, unlink, related-expansion, plus schema-version probe for D-13.
- `decay.rs` blend weights — SEARCH-05's post-fusion decay blend reuses `decay_score`; RRF replaces bm25 as the relevance input in hybrid mode.
- Known issue WR-01 (03-REVIEW.md): tag/scope filters apply after the global knn cut — hybrid's semantic leg inherits this; planner should decide whether the fix rides along.

</code_context>

<specifics>
## Specific Ideas

- Error strings stay greppable/stable-prefixed (Phase 3 D-05 convention) — e.g. `invalid argument: kind must be one of relates_to, supersedes, caused_by (got 'blocks')`.
- Reverse-direction expansion labels should read naturally (`superseded_by`, `caused`, `relates_to` symmetric) rather than exposing a raw `direction: reverse` flag alone — exact rendering is planner's call within D-03.
- The newer-schema refusal message names both versions and the exact brew upgrade command (D-13).

</specifics>

<deferred>
## Deferred Ideas

- Request-level `mode` override (`keyword|semantic|hybrid`) — already tracked as SEARCH-06 (future milestone candidate).
- Caller-tunable `expand_limit` parameter — additive later if agents need it (D-07 ships a fixed const).
- `updated_at` metadata column — rejected for 0003 (D-09); revisit only if a ranking path needs write-vs-read signals.
- Broader relation vocabulary (blocks, derived_from, contradicts) — additive enum extension once real usage data exists.
- Read-only degraded mode for newer-schema DBs — rejected (D-13); would need a designed forward-compat story.

</deferred>

---

*Phase: 4-Memory Update, Relations & Hybrid Search*
*Context gathered: 2026-07-12*
