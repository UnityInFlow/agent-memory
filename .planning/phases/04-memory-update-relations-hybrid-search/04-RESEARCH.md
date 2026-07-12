# Phase 4: Memory Update, Relations & Hybrid Search - Research

**Researched:** 2026-07-12
**Domain:** Rust + SQLite (rusqlite/sqlite-vec/FTS5) local memory daemon — in-place update with re-embed, flat typed relations, RRF hybrid search, migration hygiene
**Confidence:** HIGH (every integration point verified against the live v1.1 codebase this session; upstream facts verified against docs.rs)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Relation kind vocabulary (MCP-07)**
- **D-01:** `kind` is a **closed enum** validated at the `MemoryService` seam — v0.1.0 ships exactly three kinds: **`relates_to`, `supersedes`, `caused_by`**. Unknown kind returns `InvalidArgument` listing the allowed values (Phase 3 D-05 message shape: field + allowed set + offending value). Extending the enum later is additive; removing is a contract break, hence the minimal start.
- **D-02:** Edge cases: **duplicate link (same from/to/kind) is a no-op success** (idempotent, agent-retry-friendly), enforced by a UNIQUE constraint in migration 0003; **self-link (from_id == to_id) is `InvalidArgument`**; **unlink of a nonexistent edge returns the clean not-found shape** (same contract as `memory_update` with unknown id).
- **D-03:** Links are **stored directed** (`from_id`, `to_id`, `kind`) so `supersedes`/`caused_by` keep meaning, but **1-hop expansion follows edges in both directions**, annotating each neighbor with kind + direction (reverse view reads naturally, e.g. `superseded_by`). One edge row, no duplicate bookkeeping.

**1-hop expansion surface (MCP-07)**
- **D-04:** Expansion is **opt-in via an `expand_links` boolean** (default false) on `memory_search` and `memory_list`, both transports. The v1.0 envelope stays byte-identical when the flag is absent — existing `search_mode` checks and payload shapes unaffected. The flag routes through the Phase 3 validation seam like every new input.
- **D-05:** Expanded relations are **nested trimmed summaries** per result: `related: [{id, kind, direction, type, content snippet, tags}]` — NOT full MemoryViews. Enough for an agent to decide whether to fetch; bounded payload.
- **D-06:** **Expanded neighbors get NO access bumps** — bumps apply only to post-truncation returned result ids (roadmap criterion 4 taken literally). Expansion is a pure read; decay stays honest.
- **D-07:** Neighbor count per result is capped by a **fixed `pub` const** (e.g. `MAX_EXPANDED_LINKS = 10`; exact value Claude's discretion) living next to `DEFAULT_SEARCH_LIMIT`/`MAX_KNN_K`, documented in the README. Overflow: most-recent edges win. No caller-tunable knob in v0.1.0 (additive later if needed).

**memory_update patch semantics (MCP-06)**
- **D-08:** **JSON-merge-patch style**: omitted field = unchanged; explicit `null` = clear where clearing is legal (`ttl` → no expiry, `tags` → empty, `scope` → default). `content` can never be null/empty → `InvalidArgument`. Rust side uses the double-Option (`Option<Option<T>>`) serde pattern. Sentinels were rejected because `ttl_secs: 0` was just made `InvalidArgument` by Phase 3 — overloading it as "clear" would contradict the seam contract.
- **D-09:** **Update bumps `last_accessed`** (reuses existing `bump_access`) — an update is the strongest freshness signal; an edited memory ranks as current. No separate `updated_at` column in 0003.
- **D-10:** `memory_update` returns the **full post-update MemoryView** (same shape as `memory_store`), including `embedding_status` so callers can observe "pending re-embed" after an Ollama outage falls back to `embedding_status = 0` + sweep backfill.

**Migration 0003 hygiene (STORE-05)**
- **D-11:** Pre-migration backup is a **sibling timestamped file** (`memories.db.backup-pre-0003-{YYYYMMDD}` pattern) created via **SQLite's online backup API** before migrations run. Retention: **one backup per schema version** — re-running the same upgrade overwrites that version's backup; a different version's backup is never touched.
- **D-12:** Backup is a **generic mechanism standard for all future migrations** (trigger: pending migrations exist AND DB is non-empty) — 0003 is just the first beneficiary; Phases 5-6 inherit it structurally, mirroring Phase 3's validation-seam philosophy.
- **D-13:** Newer-schema DB met by an older binary → **refuse at startup with an upgrade hint**: detect schema version ahead of the binary's known max, exit cleanly with a message naming both versions and the fix (`brew upgrade unityinflow/tap/agent-memory`). No writes attempted, DB untouched. No read-only degraded mode.

### Claude's Discretion
- Exact `MAX_EXPANDED_LINKS` value and const naming/placement.
- Relations table/column naming and index layout in 0003 (UNIQUE(from_id, to_id, kind) required per D-02).
- Backup failure handling detail (abort migration vs proceed) — lean toward abort-with-message; decide during planning.
- RRF implementation details (leg ordering, tie-breaking) within the locked requirement mechanics (k=60, duplicate ids sum, post-fusion decay blend per SEARCH-05).
- Whether the snippet in D-05 truncates at a fixed char count or word boundary.

### Deferred Ideas (OUT OF SCOPE)
- Request-level `mode` override (`keyword|semantic|hybrid`) — already tracked as SEARCH-06 (future milestone candidate).
- Caller-tunable `expand_limit` parameter — additive later if agents need it (D-07 ships a fixed const).
- `updated_at` metadata column — rejected for 0003 (D-09); revisit only if a ranking path needs write-vs-read signals.
- Broader relation vocabulary (blocks, derived_from, contradicts) — additive enum extension once real usage data exists.
- Read-only degraded mode for newer-schema DBs — rejected (D-13); would need a designed forward-compat story.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| MCP-06 | `memory_update` patches content/tags/scope/TTL by id; content update re-mirrors FTS + invalidates/re-embeds vector in the SAME writer tx; embed outage → `embedding_status = 0` + existing sweep backfill; unknown id → clean not-found | Update Transaction pattern (§Pattern 2): FTS `memories_au` trigger (verified in `sql/0001_init.sql:42-45`) makes FTS free; vec0 ignores triggers so explicit `DELETE FROM vec_memories` + conditional re-insert inside the writer `Mutex` tx; `Ok(Option<T>)` not-found shape mirrors `forget`. Double-Option serde pitfall documented (§Pitfall 4). |
| MCP-07 | `memory_link`/`memory_unlink` flat typed relations; 1-hop expansion in search/list; forget/TTL cascade — no orphans, no graph model | Relations schema (§Pattern 3): `memory_links` in `sql/0003_relations.sql`, PK/UNIQUE(from_id,to_id,kind), FK `ON DELETE CASCADE` (verified: `foreign_keys=ON` per-connection in `apply_pragmas`, sqlite.rs:78) + belt-and-braces explicit deletes in `forget`/`sweep_expired`; `LinkKind` closed enum → `InvalidArgument` (D-01); expansion query + reverse labels (§Pattern 5). |
| STORE-05 | Migration 0003 hygiene: 0001/0002 frozen (fixture divergence test), pre-migration backup, friendly newer-DB error | Migration hygiene (§Pattern 6): 0001/0002 verified byte-identical to the v0.0.1 tag this session (`git diff v0.0.1` empty) — fixture can be generated now and committed; rusqlite `backup` feature (`Backup::run_to_completion`) verified available on the pinned 0.39 [CITED: docs.rs/rusqlite/0.39.0]; `PRAGMA user_version` pre-probe + `DatabaseTooFarAhead` backstop verified [CITED: docs.rs/rusqlite_migration/2.5.0]. |
| SEARCH-04 | Hybrid RRF (rank-based, k=60, dup ids sum), `search_mode: "hybrid"`; Ollama down → keyword degrade with unchanged envelope | Fusion pattern (§Pattern 1): service-level RRF over two oversampled legs; `SearchMode::Hybrid` additive variant on the verified `SearchOutcome` envelope (service.rs:47-65); degrade decision table; `InvalidQuery` propagates in all modes (map_fts_query_error seam verified sqlite.rs:202-209). |
| SEARCH-05 | Decay-aware hybrid: post-fusion decay blend; fresh outranks stale at equal fused rank; bumps only on post-truncation returned ids | Post-fusion multiplicative decay on pure-relevance leg ranks (§Pattern 1, avoids double-count Pitfall 1); `spawn_bump(results…)` after `take(limit)` — same as both v1.0 paths (service.rs:216, 245); kill-test spec included. |
</phase_requirements>

## Summary

This phase is **zero new runtime dependencies** — everything plugs into seams that already exist and were verified in source this session. Hybrid search is a pure-Rust RRF combinator in `MemoryService::search` over the two existing store legs (`knn_search` distance-ordered, plus a bm25-ordered keyword-candidates leg); `memory_update` is a new `Store` method reusing the writer-mutex single-tx pattern that `insert`/`forget` already prove; relations are one appended migration (`0003_relations.sql`) plus four Store/service/tool/handler surfaces; migration hygiene is a `PRAGMA user_version` pre-probe + `rusqlite::backup` (a **feature-flag addition on the already-pinned rusqlite 0.39** — versions unmoved, AR-03-01 pin triangle intact).

The three open review findings from Phase 3 sit directly on files this phase edits and should ride along: **CR-01** (change `Store::exists` to `Option<&str>` + SQLite `IS` — the trait is being extended anyway, and Phase 5 JSONL import inherits the fix), **WR-02** (add `MemoryError::Internal(String)` while both mappers are already gaining arms — new writer-lock sites in `update`/`link`/`unlink` must not repeat the `NotFound` mis-tier), and **WR-01** partially (decouple `MAX_KNN_K` from `MAX_LIMIT` to `MAX_LIMIT * 4` — one const change that directly improves the hybrid semantic leg's recall; full filter-aware KNN retry stays deferred).

Two contract-level traps need explicit planner attention: (1) **D-10 vs the wire shape** — `MemoryView` has no `embedding_status` field, so "return the full MemoryView including embedding_status" requires either a dedicated update-response struct or an additive field on `MemoryView` (recommend the dedicated struct; see Open Questions); (2) **the double-Option serde pattern** requires a custom `deserialize_with` helper — plain `Option<Option<T>>` deserializes JSON `null` to outer `None`, silently collapsing "clear" into "unchanged" (serde issue #1042; §Pitfall 4).

**Primary recommendation:** Structure plans as two parallel tracks that share only `domain.rs` error variants: Track A (update + relations + migration 0003 + hygiene) and Track B (hybrid RRF fusion, `service.rs` only) — landing the shared `MemoryError::Internal` variant and the WR-01 const decoupling first, in whichever plan executes first.

## Project Constraints (from CLAUDE.md)

- Rust stable, edition 2021; `serde`/`serde_json`; `tokio` for async; `thiserror` in the library, `anyhow` at binary edges
- `cargo fmt` before every commit; `cargo clippy -- -D warnings` must pass
- **No `unwrap()` in production code** — `?` or handled errors
- **Pattern match exhaustively** — no catch-all `_` unless truly needed (this is the compile-time mapper safety net the phase relies on)
- Test coverage >80% on core logic; coverage-bearing tests run **in-process** (spawned binaries flush no LLVM profile data)
- Zero cloud dependency — SQLite embedded, Ollama local
- No secrets committed
- GSD workflow enforcement: file changes go through `/gsd-execute-phase` (this phase)
- Ecosystem pin (STATE.md 01-01, re-affirmed AR-03-01): **rusqlite 0.39 / rusqlite_migration 2.5 / r2d2_sqlite 0.34 / libsqlite3-sys 0.37 must not move**; sqlite-vec stays 0.1.9; chrono stays `default-features=false, features=["now"]` (UTC only — `Local` breaks the zig darwin cross-link)

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| RRF fusion + decay blend + degrade decision | Service (`service.rs`) | — | v1.0 locked rule: the SEARCH-03 degrade seam lives in `MemoryService`, never in transports; fusion is a pure function over two candidate lists |
| Patch validation (kind enum, content non-empty, ttl bounds, self-link) | Service seam / `domain.rs` | — | Phase 3 D-06 contract: all validation at the `MemoryService` seam through `InvalidArgument`; transports stay thin |
| Update transaction (memories UPDATE + vec invalidate/re-insert) | Store (`sqlite.rs`, writer lane) | Service (pre-embeds content async) | Same split as `store()`: embed OUTSIDE `spawn_blocking`, one writer tx inside |
| Relations persistence + cascade | Store + SQL migration | — | FK CASCADE in schema; explicit deletes in `forget`/`sweep_expired` mirror the existing vec0 idiom |
| 1-hop expansion query | Store (read pool) | Service (attaches summaries, enforces cap) | Read-only JOIN; no bumps (D-06) |
| Backup + version probe | `SqliteStore::open` | `main.rs` (surfaces friendly error via anyhow/stderr) | Open-time is the only hook; stdout purity (MCP-05) means the refusal must reach stderr/MCP error, never stdout |
| New tool/route arms + DTOs | Transports (`mcp.rs`, `rest/handlers.rs`) | — | Deserialize → one service call → map error; zero business logic (verified existing rule) |

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rusqlite | 0.39 (pinned, unchanged) | SQLite driver; **add `backup` feature flag** for D-11 | Feature add only — same crate version, same libsqlite3-sys 0.37; `rusqlite::backup::Backup` wraps the SQLite Online Backup API [CITED: docs.rs/rusqlite/0.39.0/rusqlite/backup] |
| rusqlite_migration | 2.5 (pinned, unchanged) | Migration 0003 append; too-new detection backstop | `to_latest` returns `Error::MigrationDefinition(MigrationDefinitionError::DatabaseTooFarAhead)` when DB version exceeds defined migrations; `current_version` returns `SchemaVersion::{NoneSet,Inside,Outside}` [CITED: docs.rs/rusqlite_migration/2.5.0] |
| sqlite-vec | 0.1.9 (pinned, unchanged) | vec0 KNN leg — reused unmodified | Existing `knn_search` is the semantic leg of hybrid |
| serde / serde_json | 1 (unchanged) | Patch DTOs (double-Option), link DTOs, expansion payload | Already pinned |
| tokio | 1 (unchanged) | `tokio::join!` of the two hybrid legs; `spawn_blocking` per v1.0 rule | Already pinned |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| rmcp + schemars | 1.8 (unchanged) | `UpdateArgs`/`LinkArgs`/`UnlinkArgs` tool DTOs | New tool arms; `Option<Option<T>>` derives `JsonSchema` via the generic `Option` impl [ASSUMED — verify the generated schema is sane during implementation] |
| axum | 0.8 (unchanged) | `PATCH /api/memories/{id}`, link routes | New handler arms; `{id}` path syntax (0.8 form) |
| tempfile | 3 (dev, unchanged) | Migration/backup tests on real files | Backup + fixture tests need on-disk DBs (in-memory DBs have no sibling file) |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| rusqlite `backup` feature (`Backup::run_to_completion`) | `VACUUM INTO ?1` (zero manifest change; rusqlite docs themselves suggest it as the simpler alternative) | D-11 **locks** "SQLite's online backup API", so `Backup` is the decision-faithful choice; the feature flag does not move any version in the pin triangle. If the planner reads D-11's intent as "a consistent WAL-safe snapshot", `VACUUM INTO` satisfies it with no manifest delta — flag at plan-check if substituting. |
| Service-level RRF in Rust | Single SQL CTE (`FULL OUTER JOIN` of knn + fts, sqlite-vec blog pattern) | Rejected by milestone ARCHITECTURE research: duplicates every filter predicate, entangles `map_fts_query_error` with vec0 errors, and the keyword-only fallback path must still exist separately |
| New internal bm25-ordered keyword-candidates store method | Reusing public `Store::search` as the keyword leg | `Store::search` is decay-blended in SQL and fetches exactly `limit` — reusing it double-counts decay against the locked post-fusion blend AND starves fusion (Pitfall 5). Public `search` must stay untouched for the keyword-only degrade path. |

**Installation:** No new crates. One manifest edit:

```toml
# workspace Cargo.toml — feature add only, version unchanged
rusqlite = { version = "0.39", features = ["bundled", "functions", "backup"] }
```

**Version verification:** performed this session — pins verified in `/Users/jirihermann/Documents/workspace-1-ideas/unity-in-flow-ai/10-agent-memory/Cargo.toml` (rusqlite 0.39, rusqlite_migration 2.5, r2d2_sqlite 0.34, sqlite-vec 0.1.9); the `backup` feature confirmed present on rusqlite 0.39 [CITED: docs.rs/rusqlite/0.39.0/rusqlite/backup/index.html].

## Package Legitimacy Audit

**No new packages are installed this phase.** The only manifest change is enabling the `backup` cargo feature on the already-pinned, already-audited `rusqlite 0.39` (same crate, same version, same `libsqlite3-sys 0.37`). This mirrors AR-03-01's zero-new-dependency posture; no `package-legitimacy check` run was required.

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Architecture Patterns

### System Architecture Diagram

```
                 MCP stdio client                REST client
                       │                             │
              ┌────────▼─────────┐        ┌──────────▼──────────┐
              │ mcp.rs tool arms │        │ rest/handlers.rs    │   NEW arms: memory_update,
              │ (deserialize +   │        │ (PATCH /memories/id,│   memory_link, memory_unlink,
              │  map_mcp_error)  │        │  link routes,       │   expand_links flag on
              └────────┬─────────┘        │  map_memory_error)  │   search/list
                       └───────┬──────────┴─────────┬───────────┘
                               ▼                    │
                    ┌─────────────────────┐         │  validation at the seam:
                    │   MemoryService     │◄────────┘  LinkKind, patch fields,
                    │  (service.rs)       │            self-link, ttl bounds
                    │                     │
                    │  search(args):      │
                    │   embed(query)──────┼──► Ollama (async, outside spawn_blocking)
                    │   ├─ Ok ──► HYBRID: │       │ embed Err/empty
                    │   │  tokio::join!   │       ▼
                    │   │   knn leg ──────┼──► Store::knn_search (distance-ordered, k=limit×4)
                    │   │   kw leg ───────┼──► Store::keyword_candidates (bm25-ordered, limit×4)  [NEW]
                    │   │  RRF k=60, sum dup ids → ×decay → take(limit) → bump returned ids
                    │   │  → search_mode: "hybrid"
                    │   └─ Err ──► UNCHANGED keyword path → search_mode: "keyword"
                    │                     │
                    │  update(id, patch): │
                    │   content changed? ─┼──► embed new content best-effort
                    │   └────────────────►│ Store::update — ONE writer tx:
                    │                     │   UPDATE memories (FTS trigger fires)
                    │                     │   DELETE vec row; re-insert or status=0
                    │  link/unlink/expand─┼──► Store::{link,unlink,related}
                    └──────────┬──────────┘
                               ▼
              ┌────────────────────────────────┐
              │ SqliteStore (sqlite.rs)        │
              │ writer Mutex + r2d2 read pool  │
              │ open(): user_version probe ────┼──► too new? friendly refusal (D-13)
              │         pending + non-empty? ──┼──► Backup API sibling file (D-11)
              │         then migrations 0001→3 │
              └────────────────┬───────────────┘
                               ▼
        memories ── triggers ──► memories_fts (FTS5)
            │  ── explicit Rust ─► vec_memories (vec0 — ignores triggers)
            │  ── FK CASCADE + explicit Rust ─► memory_links  [NEW, migration 0003]
```

### Recommended Project Structure (delta only)

```
crates/agent-memory-core/
├── sql/0003_relations.sql          # NEW — memory_links table (CONTEXT names this file)
├── src/domain.rs                   # + LinkKind enum, MemoryError::Internal, RelatedLink summary type
├── src/service.rs                  # + rrf fusion fn, SearchMode::Hybrid, update/link/unlink/expansion orchestration
├── src/store/mod.rs                # + update, link, unlink, related, keyword_candidates; exists → Option<&str>
├── src/store/sqlite.rs             # + impls; MAX_KNN_K decoupled; backup + version probe in open()
├── src/store/migrations.rs         # + M::up(include_str!("../../sql/0003_relations.sql")); LATEST const
└── tests/
    ├── update.rs                   # NEW — patch semantics, re-embed kill-test, not-found
    ├── relations.rs                # NEW — link/unlink/cascade (forget AND sweep), expansion
    ├── hybrid.rs                   # NEW — golden fusion fixtures, degrade table, bump scope
    ├── migration_hygiene.rs        # NEW — fixture divergence, backup, too-new refusal
    └── fixtures/v0.0.1.db          # NEW — committed v0.0.1-schema database (see Pattern 6)
crates/agent-memory/
├── src/mcp.rs                      # + memory_update/link/unlink tools; expand_links; description updates
├── src/rest/mod.rs                 # + PATCH /api/memories/{id}, link routes
└── src/rest/handlers.rs            # + handlers; mapper arm for Internal
```

### Pattern 1: Service-level RRF fusion with post-fusion decay (SEARCH-04/05)

**What:** On embed success, run both legs concurrently, fuse by rank, blend decay after fusion, truncate, bump only returned ids.

**Leg design (critical for SEARCH-05 correctness):** both legs must be **pure relevance** orderings because the requirement locks a *post-fusion* decay blend — reusing decay-blended leg orderings would double-count recency (milestone Pitfall 1 explicitly forbids "both").
- Semantic leg: existing `Store::knn_search` **as-is** — its return order is distance-ascending (pure relevance); ignore the service-side decay blend used by the semantic-only path.
- Keyword leg: **new internal store method** (e.g. `keyword_candidates`) — same JOIN/filters/`map_fts_query_error` as `search` but `ORDER BY bm25(memories_fts) ASC` (bm25 smaller = better; ascending order = best first — do not re-introduce the sign trap) and an oversampled `LIMIT` (limit×4 capped). Public `Store::search` stays byte-identical for the keyword-only degrade path.

**Fusion (RRF, locked mechanics):**

```rust
// service.rs — pure function, TDD against hand-computed values first
/// RRF smoothing constant (Cormack & Clarke convention). NOT the KNN oversample k.
const RRF_K: f64 = 60.0;

fn rrf_fuse(legs: &[Vec<i64>]) -> Vec<(i64, f64)> {
    let mut scores: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
    for leg in legs {
        for (idx, id) in leg.iter().enumerate() {
            // rank is 1-based; duplicate ids SUM contributions (locked: SEARCH-04)
            *scores.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + (idx as f64 + 1.0));
        }
    }
    let mut out: Vec<(i64, f64)> = scores.into_iter().collect();
    // sort desc by score; tie-break by id for determinism
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}
```

**Post-fusion decay blend (SEARCH-05):** multiply the fused score by the pinned-aware decay recomputed at `now` — `final = rrf_score * decay_score(now, last_accessed, half_life, is_pinned)`. Multiplicative is recommended over additive because RRF scores are tiny (max ≈ 2/61 ≈ 0.033) while decay ∈ (0,1] — an additive term at the existing 1.0 weights would drown relevance entirely. Multiplication preserves "equal fused rank → fresher wins" exactly (the locked criterion) and is the shape the milestone FEATURES research recommends ("decay multiplier on fused score"). Set each returned `view.decay_score` via the existing `apply_decay` for wire consistency.

**Degrade decision table (encode as service tests):**

| Condition | Outcome |
|-----------|---------|
| Embed OK, both legs OK | fuse → `search_mode: "hybrid"` |
| Embed OK, both legs empty | empty results, still `"hybrid"` (emptiness never means fallback — v1.0 rule extended) |
| Embed Err / empty batch | UNCHANGED v1.0 keyword path → `"keyword"`, one loud warn |
| `InvalidQuery` from keyword leg (malformed MATCH) | **propagate** → 400/`invalid_params` in ALL modes — never silently serve semantic-only |
| Internal SQLite error either leg | propagate → 500/`internal_error` |

**Wire changes to make in the same plan:** add `Hybrid` to `SearchMode` (serde lowercase → `"hybrid"`); update the `memory_search` tool description in `mcp.rs:195` which currently enumerates `'semantic'|'keyword'` (agent-facing API); update REST/MCP tests asserting mode strings; README search-mode table.

### Pattern 2: Update-in-place with same-tx vector invalidation (MCP-06)

**What:** `Store::update(id, patch, embedding, now)` — one writer-mutex acquisition, one transaction.

```rust
// sqlite.rs sketch — mirrors insert()'s tx shape (verified sqlite.rs:226-256)
// Service pre-embeds the new content (async, OUTSIDE spawn_blocking) ONLY if
// patch.content is Some — tag/scope/ttl-only patches never touch the embedder.
let mut conn = self.writer.lock().map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
let tx = conn.transaction()?;
// UPDATE in place — id is immutable for the memory's lifetime (never DELETE+INSERT:
// plain INTEGER PRIMARY KEY rowids get RECYCLED, silently re-attaching relation edges).
// The memories_au FTS trigger (0001_init.sql:42) re-mirrors content for free.
// D-09: fold the freshness bump into the same UPDATE (last_accessed = now,
// access_count = access_count + 1) — one lock acquisition, bump_access semantics.
let changed = tx.execute("UPDATE memories SET ... WHERE id = ?1", ...)?;
if changed == 0 { return Ok(None); }               // clean not-found — the forget pattern
if content_changed {
    tx.execute("DELETE FROM vec_memories WHERE memory_id = ?1", params![id])?;
    match embedding {
        Some(v) => { /* INSERT vector; embedding_status = 1 */ }
        None    => { /* embedding_status = 0 — existing sweep backfill re-embeds */ }
    }
}
tx.commit()?;
// re-read + return the post-update row (inside the same lock or via read pool)
```

**Not-found shape:** model as `Ok(None)`, NEVER `Err(MemoryError::NotFound)` — the mappers disagree about `NotFound`'s tier (MCP → `internal_error`, REST → 404; WR-02). REST `PATCH` maps `Ok(None)` → `ApiError::NotFound` (404); MCP returns a success result `{"id": N, "updated": false, "reason": "not_found"}` mirroring `memory_forget`'s shape (mcp.rs:245-249).

**TTL patch:** `Some(Some(t))` → `validate_ttl(t)` then `expires_at = now + t` (recomputed from patch time); `Some(None)` → `expires_at = NULL`. **Content:** `Some(None)` or `Some(Some(""))` → `InvalidArgument` (D-08). **mem_type is NOT patchable** — MCP-06 scope is content/tags/scope/TTL only; leaving type immutable also avoids a `base_weight` recompute path.

**Kill-test (locked by success criterion 1):** store "Postgres is the database", update to "SQLite is the database" with a `FakeEmbedder` programmed for both texts; assert a semantic query for the OLD meaning no longer returns the row and the NEW meaning does. Also: update with `FakeEmbedder::failing()` → row has `embedding_status = 0` and one `sweep()` backfills it.

### Pattern 3: Relations schema + cascade (MCP-07, migration 0003)

```sql
-- sql/0003_relations.sql (append-only; 0001/0002 are FROZEN)
CREATE TABLE memory_links (
    from_id    INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    to_id      INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL,           -- closed LinkKind enum validated at the service seam
    created_at INTEGER NOT NULL,           -- injected clock; "most-recent edges win" for D-07 overflow
    PRIMARY KEY (from_id, to_id, kind),    -- satisfies the D-02 UNIQUE requirement
    CHECK (from_id != to_id)               -- belt-and-braces; service rejects self-link first
) WITHOUT ROWID;                            -- optional; planner's call — plain table also fine
CREATE INDEX idx_links_to ON memory_links(to_id);  -- reverse-direction expansion; PK covers from_id prefix
```

- `PRAGMA foreign_keys = ON` is applied per-connection by `apply_pragmas` (verified sqlite.rs:78, writer AND pool customizer), and both delete paths run on the writer — so CASCADE fires for `forget` and TTL sweep with zero changes. **Still add belt-and-braces explicit deletes** in `forget()` and `sweep_expired()` (mirroring the existing `DELETE FROM vec_memories WHERE memory_id NOT IN (SELECT id FROM memories)` idiom at sqlite.rs:516-519) — any future connection that skips the pragma silently orphans edges otherwise (milestone Pitfall 9).
- **Duplicate link (D-02):** `INSERT OR IGNORE` — `changes() == 0` is still a success return (idempotent no-op).
- **Link to a nonexistent id:** pre-check both ids with `SELECT 1` inside the writer tx and return the clean not-found shape naming which id is missing — do NOT rely on parsing the FK-violation `SqliteFailure`, which would surface as a 500.
- **Self-link:** rejected at the service seam as `InvalidArgument` before any store call (message shape: `invalid argument: from_id and to_id must differ (got 7)`).
- **Kind validation:** `LinkKind` enum + `TryFrom<&str>` in `domain.rs` following the `MemoryType` pattern exactly, but returning the existing `MemoryError::InvalidArgument` per D-01's locked message shape: `invalid argument: kind must be one of relates_to, supersedes, caused_by (got 'blocks')`. Wire form lowercase.
- **migrations.rs quirk (verified):** `Migrations::validate()` and every migration test must call `register_vec_extension()` first — 0002 creates a vec0 table (migrations.rs:21-31 comment documents this).

### Pattern 4: New MemoryError variants + exhaustive mapper extension

Adding variants forces both mappers to classify them at compile time (the proven 02-05 safety net):

| Variant | MCP tier | REST tier | Producers |
|---------|----------|-----------|-----------|
| `Internal(String)` (NEW — WR-02 fix) | `internal_error` | 500 | All 6 existing `writer.lock()` sites (sqlite.rs:226, 306, 467, 483, 504, 526) + every new writer method |
| `InvalidArgument(String)` (existing) | `invalid_params` | 400 | LinkKind parse, self-link, empty-content patch, ttl/limit bounds |
| `NotFound` | — | — | **Zero producers after the WR-02 fix** (update/link not-found are `Ok(None)`-shaped). Recommend deleting the variant — exhaustive matches make removal compile-checked; if kept, both mappers must document it as unreachable |

A dedicated startup-only error for D-13 (e.g. `SchemaTooNew { db_version, binary_version }`) can be a `MemoryError` variant or a distinct open-time error type surfaced via `anyhow` in `main.rs` — planner's choice; it never crosses a transport (open happens before serving).

### Pattern 5: 1-hop expansion (D-03..D-07)

- `expand_links: bool` (`#[serde(default)]`) on both search and list DTOs, both transports; plumbed through `SearchArgs`/`ListArgs`.
- After results are truncated to `limit`, one read-pool query fetches neighbors for the returned ids (bounded: ≤ limit × MAX_EXPANDED_LINKS rows). Either per-id or one IN-list query grouping by anchor id — local SQLite, both fine; IN-list avoids N+1.

```sql
-- both directions in one pass; most-recent edges win the cap (D-07)
SELECT l.from_id, l.to_id, l.kind, l.created_at,
       m.id, m.mem_type, m.content, m.tags
FROM memory_links l
JOIN memories m ON m.id = CASE WHEN l.from_id = ?1 THEN l.to_id ELSE l.from_id END
WHERE l.from_id = ?1 OR l.to_id = ?1
ORDER BY l.created_at DESC
LIMIT ?2   -- MAX_EXPANDED_LINKS
```

- Summary shape (D-05): `related: [{id, kind, direction, type, snippet, tags}]`. Reverse-direction natural labels via a `LinkKind::reverse_label()` helper: `relates_to → relates_to` (symmetric), `supersedes → superseded_by`, `caused_by → caused`; keep the explicit `direction: "out"|"in"` field alongside per D-05's field list.
- **Wire compatibility:** attach `related` as `#[serde(skip_serializing_if = "Option::is_none")] pub related: Option<Vec<RelatedLink>>` — either on `MemoryView` (byte-identical envelope when absent, but touches every `MemoryView` struct-literal across the test suite) or on a wrapper. See Open Questions.
- **No bumps for neighbors** (D-06) — expansion runs after `spawn_bump(returned_ids)`; simply never pass neighbor ids to it. Kill-test: expand a result with 3 neighbors; assert neighbors' `access_count` unchanged.
- `MAX_EXPANDED_LINKS: usize = 10` recommended (D-07 suggests 10), `pub` const in `domain.rs` beside `MAX_LIMIT` (Phase 3 D-03 placement precedent), documented in README bounds table.

### Pattern 6: Migration hygiene — freeze test, backup, too-new refusal (STORE-05)

**Open-flow ordering in `SqliteStore::open` (replaces the bare `to_latest` at sqlite.rs:136-137):**

```
register_vec_extension()
open writer + prepare_connection
v = PRAGMA user_version                       -- conn.pragma_query_value
if v > LATEST_SCHEMA_VERSION (= 3):
    → Err(SchemaTooNew { db: v, binary: 3 })  -- friendly message naming both versions
      "database schema is version {v} but this binary knows version 3 —
       upgrade: brew upgrade unityinflow/tap/agent-memory (or point --db elsewhere)"
      NO writes, DB untouched (D-13). rusqlite_migration's DatabaseTooFarAhead
      remains a backstop if the probe is ever bypassed.
if 0 < v < 3 (pending migrations AND non-empty DB, D-12 trigger):
    backup via rusqlite::backup::Backup (src = writer conn or fresh read conn,
    dst = Connection::open(sibling path)); run_to_completion(pages, sleep, None)
    sibling path: "{db}.backup-pre-0003-{YYYYMMDD}" — before writing, remove any
    existing "{db}.backup-pre-0003-*" (one backup per schema version, D-11)
    backup failure → ABORT open with a message (CONTEXT leans abort; confirm in planning)
migrations.to_latest(&mut writer)
```

- `v == 0` (fresh file) → no backup, straight migration — the D-12 "non-empty" condition read as `user_version > 0`; a paranoid extra `SELECT count(*) FROM memories` check is optional.
- WAL safety: the Backup API is WAL-correct by construction (it copies via the source connection); never raw-`fs::copy` a live WAL database.
- The `{YYYYMMDD}` stamp can come from `chrono::Utc::now()` (the pinned `now` feature suffices) or epoch math — it is a filename, not domain logic; no `Local` time ever.
- **Fixture divergence test:** `git diff v0.0.1 -- sql/0001_init.sql sql/0002_embeddings.sql` is empty (verified this session), so the fixture can be generated NOW from the current 0001+0002 and committed as `tests/fixtures/v0.0.1.db` (small binary; build once via a `#[ignore]`d generator test or a script, apply only migrations 1-2 via `Migrations::to_version`). The test: open the fixture copy, migrate `to_latest`, and assert its full normalized `sqlite_master` SQL equals a fresh-from-zero database's — this converts any future edit of shipped SQL into a red CI. Remember `register_vec_extension()` in the test.
- **Too-new test:** create a temp DB, `PRAGMA user_version = 99`, open → assert the error names both versions and the brew command, and that the file's bytes/mtime are unchanged.
- **Backup tests:** upgrade the v0.0.1 fixture → assert sibling `backup-pre-0003-*` exists and itself opens as a valid v0.0.1-schema DB; re-run → exactly one backup file; fresh DB → no backup file.

### Anti-Patterns to Avoid

- **DELETE+INSERT update:** plain `INTEGER PRIMARY KEY` rowids get recycled — old relation edges silently re-attach to unrelated new memories (data corruption, not an error). `UPDATE ... WHERE id = ?` only; state id-immutability as a `domain.rs` invariant.
- **Score fusion instead of rank fusion:** the legs' scales are incomparable (bounded cosine blend vs unbounded bm25). Positions only.
- **Decay both per-leg and post-fusion:** requirement locks post-fusion; legs must be pure relevance.
- **"Keyword leg errored, just return semantic":** makes the same malformed MATCH a 400 in keyword mode and a silent 200 in hybrid — taxonomy violation.
- **Editing `sql/0001` or `sql/0002` for any reason** ("just put the FK in 0001"): user_version has no checksums; upgraded and fresh installs diverge silently.
- **`INSERT OR REPLACE` on vec_memories:** conflict clauses unreliable on virtual tables (v1.0 decision); reuse the existing DELETE+INSERT `insert_embedding` path.
- **Per-transport validation:** all new inputs validate at the `MemoryService` seam (Phase 3 D-06); transports only deserialize + parse wire strings.
- **Bumping candidate ids instead of returned ids:** corrupts decay corpus-wide and invisibly (criterion 4 is literal).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Consistent snapshot of a live WAL DB | `std::fs::copy` of the .db file | `rusqlite::backup::Backup` (or `VACUUM INTO`) | A raw copy of a WAL database misses un-checkpointed pages — the backup would be silently corrupt exactly when it matters |
| Too-new schema detection | String-matching migration errors | `PRAGMA user_version` pre-probe + `MigrationDefinitionError::DatabaseTooFarAhead` backstop | The pragma is the source of truth rusqlite_migration itself uses [CITED: docs.rs/rusqlite_migration/2.5.0] |
| Referential cleanup of edges | Manual orphan-scan jobs | FK `ON DELETE CASCADE` + explicit same-tx deletes | Engine + idiom already in the codebase (vec0 pattern); two mechanisms cover the per-connection-pragma gap |
| Merge-patch "field present vs null vs absent" | Sentinel values (e.g. `ttl: 0` = clear) | double-Option + `deserialize_with` helper | Phase 3 just made `ttl_secs: 0` an `InvalidArgument`; sentinels would contradict the seam contract (D-08 records this rejection) |
| Rank fusion math | Ad-hoc score normalization across legs | RRF `Σ 1/(60 + rank)` | 15+ years of IR literature (Cormack & Clarke 2009); the requirement locks it |

**Key insight:** every hard sub-problem in this phase (WAL-safe snapshot, virtual-table sync, patch semantics, rank fusion) has a known-correct mechanism either already in the codebase or one feature-flag away — the risk is re-deriving them subtly wrong, not missing libraries.

## Common Pitfalls

### Pitfall 1: Decay double-counting in hybrid ranking
**What goes wrong:** Reusing the existing decay-blended leg orderings AND applying the locked post-fusion decay blend counts recency twice — stale-but-relevant memories get buried, pinned types lose their advantage.
**How to avoid:** Legs rank on pure relevance (KNN distance order; new bm25-ordered keyword-candidates method). Decay applies exactly once, multiplicatively, post-fusion. Document the decision in the fusion function docstring.
**Warning signs:** Hybrid results near-identical to semantic-only; a #1 exact-phrase keyword hit never surfacing in hybrid.

### Pitfall 2: The two `k` constants collide
**What goes wrong:** `MAX_KNN_K` (oversample cap, currently `= MAX_LIMIT = 200`) vs RRF's smoothing k (60). Using 60 as fetch size caps recall; using limit as RRF k makes fusion arbitrary.
**How to avoid:** `const RRF_K: f64 = 60.0` with a doc comment; never pass either as a bare `i64` named `k`. Unit-test fusion against hand-computed sums (leg-1-rank-2 + leg-2-rank-2 = 2/62 > 1/61 = single-leg rank 1).

### Pitfall 3: Keyword leg under-fetch and bump over-fire
**What goes wrong:** `Store::search` fetches exactly `limit` (verified `LIMIT ?9`, sqlite.rs:433) — a memory ranked limit+1 in one leg but top-3 in the other never enters fusion. And bumping fusion *candidates* (up to ~800 ids) instead of returned ids permanently corrupts decay ordering.
**How to avoid:** Both legs oversample limit×4 (capped); bump only `results.iter().map(|v| v.id)` after `take(limit)` — same as both v1.0 call sites (service.rs:216, 245). Kill-test: 20-row corpus, limit=5, hybrid → exactly 5 rows have `access_count` incremented.

### Pitfall 4: `Option<Option<T>>` does NOT distinguish null from absent out of the box
**What goes wrong:** serde's `Option` deserializer maps JSON `null` to `None` at the outer level, so a plain `Option<Option<T>>` field receives `None` for BOTH "absent" and `null` — "clear my TTL" silently becomes "leave unchanged". Tests that only send present-or-absent pass; the clear path is broken in production.
**How to avoid:** The serde-issue-1042 pattern: `#[serde(default, deserialize_with = "deserialize_explicit_null")]` where the helper is `fn de<'de,D,T>(d: D) -> Result<Option<T>, D::Error> { T::deserialize(d).map(Some) }` wrapped so the field type is `Option<Option<T>>` (absent → `None` via default; `null` → `Some(None)`; value → `Some(Some(v))`). Kill-tests for all three states on ttl, tags, and scope, on BOTH transports. [ASSUMED: exact helper spelling — verify against serde docs during implementation; the null-collapse behavior itself is well-documented serde behavior]

### Pitfall 5: `NotFound` mis-tier repeats on new writer methods (WR-02)
**What goes wrong:** New `update`/`link`/`unlink` writer methods copy `self.writer.lock().map_err(|_| MemoryError::NotFound)` from the six existing sites — after any panic while holding the lock, REST `PATCH` returns 404 for a server fault, and MCP calls it `internal_error`; the transports disagree.
**How to avoid:** Land `MemoryError::Internal(String)` FIRST (whichever plan executes first), convert all six existing sites, and write new methods against it. Not-found for update/link/unlink is `Ok(None)`/status-shaped, never an error variant.

### Pitfall 6: Fixture divergence test that doesn't actually freeze anything
**What goes wrong:** Building the "v0.0.1 database" by applying the CURRENT 0001/0002 proves nothing — if someone edits 0001, the test rebuilds the fixture with the edited SQL and still passes.
**How to avoid:** Commit a **binary fixture** built once (0001/0002 verified byte-identical to the v0.0.1 tag this session — generate now, never regenerate). The test migrates a copy of the committed fixture and diffs normalized `sqlite_master` against a fresh DB.

### Pitfall 7: Expansion breaks the byte-identical envelope promise
**What goes wrong:** Adding a `related` field that serializes as `"related": null` on every result when the flag is off changes the v1.0 payload — existing exact-shape assertions and clients break (D-04 promises byte-identical).
**How to avoid:** `#[serde(skip_serializing_if = "Option::is_none")]`; regression test asserting a flag-less search/list response is byte-identical to the pre-phase golden JSON.

### Pitfall 8: Backup written after (or inside) the migration
**What goes wrong:** `to_latest` runs migrations in its own transaction; a backup taken after open has already mutated the file is worthless, and `VACUUM`-class statements can't run inside migration transactions anyway.
**How to avoid:** The probe → backup → migrate ordering in Pattern 6, all inside `SqliteStore::open` before `migrations.to_latest`. Test proves the backup file has `user_version = 2` (pre-migration schema) after an upgrade.

## Code Examples

Verified patterns from this codebase and official docs:

### Writer-transaction shape (the template for `Store::update`)
```rust
// Source: crates/agent-memory-core/src/store/sqlite.rs:305-326 (insert_embedding, v1.0)
let mut conn = self.writer.lock().map_err(|_| /* Internal after WR-02 fix */)?;
let tx = conn.transaction()?;
tx.execute("DELETE FROM vec_memories WHERE memory_id = ?1", params![memory_id])?;
tx.execute("INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)", params![id, blob])?;
tx.execute("UPDATE memories SET embedding_status = 1 WHERE id = ?1", params![memory_id])?;
tx.commit()?;
```

### Online backup (D-11)
```rust
// Source: docs.rs/rusqlite/0.39.0/rusqlite/backup — requires the `backup` feature
use rusqlite::backup::Backup;
let mut dst = Connection::open(&backup_path)?;
{
    let backup = Backup::new(&src_conn, &mut dst)?;
    backup.run_to_completion(64, std::time::Duration::from_millis(50), None)?;
}
```

### Version probe (D-13)
```rust
// PRAGMA user_version — the same integer rusqlite_migration tracks
let v: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
```

### CR-01 fix (NULL-safe dedup probe — ride-along)
```rust
// Source: .planning/phases/03-api-hardening-toolchain-spikes/03-REVIEW.md CR-01
// trait: fn exists(&self, source: Option<&str>, ...) ; SQL `IS` is null-safe equality
"SELECT 1 FROM memories WHERE source IS ?1 AND mem_type = ?2 AND content = ?3 LIMIT 1"
// service.rs:285-286 drops the `.unwrap_or("")` coercion; regression test:
// import the same source:None draft twice → skipped_duplicates == 1 on run 2
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Two search modes (`semantic`/`keyword`) | Third additive `hybrid` mode, RRF k=60 | This phase | Wire change: tool description + tests must update in the same plan |
| `tags LIKE '%…%'` substring | `json_each` exact equality | Phase 3 (shipped) | Relations queries must keep the parameterized-exact convention |
| `MemoryError::NotFound` as poisoned-lock stand-in | `Internal(String)` variant | This phase (WR-02) | REST 404-for-500 mis-tier eliminated; `NotFound` left with zero producers |
| Bare `to_latest` at open | probe → backup → migrate | This phase (STORE-05) | All future migrations inherit the backup mechanism (D-12) |

**Deprecated/outdated:** the sqlite-vec blog's single-CTE hybrid pattern (demo convenience, rejected by milestone ARCHITECTURE research); `INSERT OR REPLACE` on vec0 (v1.0 decision); raw file-copy backups of WAL databases.

## Review Findings Disposition (03-REVIEW.md — planner must schedule)

| Finding | Recommendation | Cost | Where |
|---------|---------------|------|-------|
| CR-01 (critical) import dedup NULL-source | **Fix in this phase** — `exists` signature changes ride the Store-trait extension; Phase 5 JSONL import inherits the seam | ~30 min + regression test | `store/mod.rs`, `sqlite.rs:344-362`, `service.rs:285` |
| WR-02 poisoned mutex → NotFound | **Fix in this phase** — mappers are already gaining arms; new writer methods must not copy the bug | ~30 min; compile-guided | `domain.rs`, all `writer.lock()` sites, both mappers |
| WR-01 filter-after-KNN-cut | **Partial ride-along:** decouple `const MAX_KNN_K: i64 = crate::domain::MAX_LIMIT * 4` (restores 4× oversampling at all limits; directly improves the hybrid semantic leg) + document the filtered-recall bound in the `memory_search` tool description. Full filter-aware retry / vec0 metadata pre-filtering: defer | const change ~5 min | `sqlite.rs:35` |
| WR-03 REST harness hang | Optional ride-along if `tests/rest.rs` is touched for new endpoints anyway (thread + `recv_timeout` fix is written in the review) | ~20 min | `tests/rest.rs:55-84` |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `Option<Option<T>>` + schemars derives a usable JSON Schema for the MCP tool DTOs | Standard Stack / Pattern 2 | Tool schema may render confusingly for agents; fallback: custom `JsonSchema` impl or flattened nullable fields — wire behavior unaffected |
| A2 | The exact serde `deserialize_with` helper spelling for explicit-null detection | Pitfall 4 | Compile-time discovery; the pattern itself (serde #1042) is well-established |
| A3 | `chrono` with `default-features=false, features=["now"]` supports date formatting for the backup filename (the `now`→`std`→`alloc` feature chain) | Pattern 6 | Trivial fallback: derive YYYYMMDD from epoch seconds with integer math — no new feature flags |
| A4 | `Backup::new(&src, &mut dst)` works with the writer connection as source while the writer mutex is held during open (single-threaded at open time) | Pattern 6 | Fallback: open a dedicated source connection for the backup; open() is pre-serving so no concurrency exists yet |

## Open Questions

1. **D-10 response shape vs `MemoryView` (needs planner decision — contract-level)**
   - What we know: `MemoryView` (domain.rs:104-115) has NO `embedding_status` field; D-10 says the update response includes it "same shape as memory_store" — but `memory_store` returns only an id.
   - What's unclear: whether D-10 intends (a) a dedicated update-response struct (MemoryView fields + `embedding_status`), or (b) adding `embedding_status` to `MemoryView` globally (additive JSON field on every search/list result — a v1.0 wire change D-04's byte-identical promise argues against).
   - Recommendation: **(a)** — a small `UpdatedMemory` serialize-only struct (`#[serde(flatten)]` over the view + `embedding_status`); keeps every existing envelope untouched.
2. **Where the `related` field lives**
   - What we know: D-04 promises byte-identical envelopes when the flag is absent; `MemoryView` derives `PartialEq` and is struct-literal-constructed across the test suite.
   - Recommendation: `Option<Vec<RelatedLink>>` with `skip_serializing_if` directly on `MemoryView` is simplest and wire-safe; accept the mechanical test-literal churn (compiler-guided). A wrapper type doubles the serialization surfaces on both transports for little gain.
3. **Backup failure handling** — CONTEXT leans abort-with-message (a user whose disk can't hold a backup probably shouldn't run an unattended migration either). Confirm in planning; test both branches of whatever is chosen.
4. **`WITHOUT ROWID` for memory_links** — composite-PK table would benefit marginally; plain table is equally correct. Planner discretion (D-02 only requires the UNIQUE constraint).

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| cargo / rustc | build + tests | ✓ | 1.94.1 | — |
| SQLite (bundled via libsqlite3-sys 0.37) | everything | ✓ | 3.51.3 bundled | — (no system dependency) |
| Ollama | manual hybrid verification only | ✓ (local) | 0.31.2 | `FakeEmbedder` covers all automated tests; hybrid CI never needs Ollama |
| git tag `v0.0.1` | fixture generation | ✓ | — | — |
| GitHub-hosted CI (`ubuntu-latest`, D-02 split) | phase gate | ✓ (revived in Phase 3) | — | local `cargo test --workspace` |

**Missing dependencies with no fallback:** none.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) + tokio::test; FakeEmbedder/TestClock injection via the `test-clock` feature self-dev-dependency |
| Config file | none needed (workspace Cargo.toml) |
| Quick run command | `cargo test -p agent-memory-core` |
| Full suite command | `cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MCP-06 | patch semantics (absent/null/value × ttl/tags/scope), content re-embed kill-test, embed-outage → status 0 + sweep backfill, unknown id not-found | unit (core) | `cargo test -p agent-memory-core --test update` | ❌ Wave 0 |
| MCP-06 | PATCH 404 / MCP not-found shape, 400-never-500 for bad patches | integration (in-process handler + MCP unit) | `cargo test -p agent-memory` | partially (extends handlers.rs/tools.rs suites) |
| MCP-07 | link/unlink idempotency, self-link 400, cascade on forget AND `sweep(now)`, zero orphans, expansion cap + no-bump | unit (core) | `cargo test -p agent-memory-core --test relations` | ❌ Wave 0 |
| SEARCH-04 | RRF hand-computed sums, dup-id summing, `search_mode` decision table, InvalidQuery propagation in hybrid, envelope byte-compat | unit (core) + integration | `cargo test -p agent-memory-core --test hybrid` | ❌ Wave 0 |
| SEARCH-05 | fresh-outranks-stale-at-equal-fused-rank golden fixture; bump only post-truncation ids | unit (core) | `cargo test -p agent-memory-core --test hybrid` | ❌ Wave 0 |
| STORE-05 | fixture divergence, backup created/retained-one-per-version, too-new friendly refusal, fresh-DB no-backup | unit (core, on-disk tempfiles) | `cargo test -p agent-memory-core --test migration_hygiene` | ❌ Wave 0 |
| CR-01 | source:None re-import → skipped_duplicates | unit (core) | `cargo test -p agent-memory-core --test import` | ✓ (extends tests/import.rs) |

### Sampling Rate
- **Per task commit:** `cargo test -p agent-memory-core` (< 30 s)
- **Per wave merge:** `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Phase gate:** full suite + `cargo fmt --check` green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/agent-memory-core/tests/update.rs` — covers MCP-06
- [ ] `crates/agent-memory-core/tests/relations.rs` — covers MCP-07
- [ ] `crates/agent-memory-core/tests/hybrid.rs` — covers SEARCH-04/05 (golden fusion fixture corpus)
- [ ] `crates/agent-memory-core/tests/migration_hygiene.rs` + `tests/fixtures/v0.0.1.db` — covers STORE-05 (fixture generated once from the frozen SQL, committed as binary)
- [ ] Framework install: none — cargo test is built-in

## Security Domain

### Applicable ASVS Categories (level 1, per config)

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | single-user local tool; loopback guard + `--allow-remote` warning unchanged (T-02-10) |
| V3 Session Management | no | stateless tools/routes |
| V4 Access Control | no | no multi-tenant model |
| V5 Input Validation | **yes** | Phase 3 seam extended: `LinkKind` closed enum (never free-form TEXT to SQL), double-Option patch fields validated at service, `expand_links` typed bool, ttl re-validated via shared `validate_ttl` |
| V6 Cryptography | no | none introduced |
| V10 Data Protection | **yes** | Backup file contains the FULL memory corpus — sibling file in the same directory inherits its permissions; document in README that backups live beside the DB |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via kind/patch/link inputs | Tampering | All new SQL parameterized (verified existing convention: every statement in sqlite.rs binds); kind never reaches SQL unvalidated (enum-first, MemoryType precedent) |
| Server-fault probing / error-tier confusion | Information Disclosure | `Internal(String)` fix (WR-02) restores correct 500-tier for lock poisoning; D-13 refusal message names only version integers + a public brew command — no path/internal leaks beyond the existing open-error contexts |
| DoS via expansion fan-out | Denial of Service | `MAX_EXPANDED_LINKS` hard cap × `MAX_LIMIT` bounds the worst-case join (≤ 2000 summary rows); no recursive traversal (1-hop only, no graph model by requirement) |
| Data loss via unattended migration | Tampering (integrity) | Pre-migration backup (D-11/D-12) + no `.down()` migrations (a data-destroying rollback is worse than clean refusal) |
| Backup-file disclosure | Information Disclosure | Sibling file in the user's data dir; same trust domain as the DB itself; do NOT write backups to world-readable temp dirs |
| Decay-integrity corruption via bump over-fire | Tampering (integrity) | Bump-only-returned-ids kill-test (criterion 4) |

## Sources

### Primary (HIGH confidence — verified in-session against the live codebase)
- `crates/agent-memory-core/src/{service,domain,decay}.rs`, `store/{mod,sqlite,migrations}.rs`, `sql/0001_init.sql`, `sql/0002_embeddings.sql` — all integration points, tx shapes, pragma application, envelope, mappers
- `crates/agent-memory/src/{main,mcp}.rs`, `rest/{mod,handlers}.rs` — tool descriptions, error tiers, open flow, stdout-purity constraints
- `git diff v0.0.1 -- sql/0001*.sql sql/0002*.sql` → empty (freeze baseline confirmed)
- `.planning/phases/03-api-hardening-toolchain-spikes/03-{CONTEXT,REVIEW,SECURITY}.md` — locked seam contracts, open findings, pin-triangle risk acceptance
- `.planning/research/{SUMMARY,ARCHITECTURE,PITFALLS}.md` — milestone research (HIGH confidence, verified against v1.0 source at research time)

### Secondary (MEDIUM confidence — official docs)
- [CITED: docs.rs/rusqlite/0.39.0/rusqlite/backup/index.html] — `backup` feature gate, `Backup::new`/`run_to_completion`, Online Backup API, VACUUM INTO alternative
- [CITED: docs.rs/rusqlite_migration/2.5.0] — `MigrationDefinitionError::DatabaseTooFarAhead` on too-new DB; `current_version` → `SchemaVersion::{NoneSet,Inside,Outside}`; `to_version` for fixture builds
- Cormack & Clarke SIGIR 2009 RRF k=60 convention — cross-verified in milestone research (FEATURES/Context7 LanceDB)

### Tertiary (LOW confidence — flagged in Assumptions Log)
- serde explicit-null helper spelling (A2); schemars × double-Option schema shape (A1); chrono no-default-features formatting (A3)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — zero new crates; the one feature-flag addition verified against docs.rs for the exact pinned version
- Architecture: HIGH — every seam, line number, and invariant re-verified in source this session (not inherited from milestone research)
- Pitfalls: HIGH — grounded in the shipped code + the milestone pitfalls file, plus two new phase-specific traps (serde double-Option, D-10 wire-shape conflict) discovered by cross-checking the locked decisions against the actual `MemoryView` definition
- Validation: HIGH — test infrastructure exists and the four-layer pattern (core → in-process handler → MCP unit → spawned binary) is established

**Research date:** 2026-07-12
**Valid until:** ~2026-08-12 (stable domain; the only external watch item is a fixed sqlite-vec stable release — if PR #199 ships mid-milestone, the pin bump belongs to Phase 6, not here)
