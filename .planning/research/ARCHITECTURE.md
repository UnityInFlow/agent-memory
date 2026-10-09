# Architecture Research — v1.1 Hardening & Interop Milestone

**Domain:** Single-binary Rust MCP/REST memory daemon (existing, shipped v0.0.1) — integration architecture for hybrid RRF search, memory_update/relations, shared input validation, JSONL export, Windows + musl distribution
**Researched:** 2026-07-12
**Confidence:** HIGH (all integration points verified against the actual v1.0 codebase; external facts — cargo-zigbuild target support, sqlite-vec musl status, RRF conventions — verified against current sources)

> **Milestone scope note:** This document does NOT re-derive the v1.0 architecture (transports → `MemoryService` → `Store` trait → `SqliteStore`; injected `Clock`/`Embedder`; two-tier `MemoryError` taxonomy). It answers one question: **how do the six v1.1 features integrate with what exists**, with explicit new-vs-modified component lists, migration requirements, and a dependency-aware build order.

## Existing Seams the New Features Plug Into

Verified in code (paths relative to repo root):

| Seam | Location | Why it matters for v1.1 |
|------|----------|------------------------|
| `MemoryService` — the single business-logic layer both transports call | `crates/agent-memory-core/src/service.rs` | RRF fusion, update orchestration, and validation all belong here (the SEARCH-03 degrade decision already lives here, "never in transports") |
| `Store` trait — synchronous persistence interface | `crates/agent-memory-core/src/store/mod.rs` | `update`, `link`/`unlink`/`related`, and `export_rows` are new trait methods; `knn_search` + `search` are reused unmodified by RRF |
| `SearchOutcome { search_mode, results }` — the ONE shared wire envelope | `service.rs` (`SearchMode` enum, serde lowercase) | Hybrid is an **additive enum variant** — envelope shape unchanged |
| Two-tier `MemoryError` → transport mapping | `map_mcp_error` (`mcp.rs`), `map_memory_error` (`rest/handlers.rs`) | New validation errors extend the taxonomy; exhaustive matches force both mappers to handle new variants at compile time |
| `rusqlite_migration` ordered list | `store/migrations.rs` + `sql/000N_*.sql` | Relations = migration `0003`; append-only, `user_version`-tracked |
| CLI `--from` dispatch + `MemoryService::import` | `main.rs` `run_import`, `service.rs` `import` | JSONL import is a second `--from` format through the SAME idempotent import path |
| `spike-cross-compile.yml` + `release.yml` matrix | `.github/workflows/` | Windows and musl are new/changed matrix legs, de-risked via the existing spike workflow |
| Platform isolation point | `config.rs` (the ONLY `cfg(unix)` in the codebase, with a `cfg(not(unix))` fallback already written) | Windows support was pre-planned as "a single-file change" — and it already is one |

## 1. Hybrid RRF Search (SEARCH-04)

### Recommendation: service-level combinator over the two existing store methods — NOT a SQL CTE union, NOT a third `Store` method

**Where it lives:** `MemoryService::search` grows a third branch outcome. On a successful query embed, the service runs **both** existing legs concurrently and fuses in Rust:

```rust
// service.rs — sketch (both legs are existing Store methods, unchanged)
let (knn, keyword) = tokio::join!(
    tokio::task::spawn_blocking({ let s = store.clone(); move || s.knn_search(query_vec, knn_args) }),
    tokio::task::spawn_blocking({ let s = store.clone(); move || s.search(kw_args, now, weights, cfg) }),
);
// RRF (Cormack et al. convention, k = 60):
//   score(d) = w_sem / (k + rank_knn(d)) + w_kw / (k + rank_fts(d))
// rank = 1-based position in each leg's (already decay-blended) ordering;
// a doc absent from a leg contributes 0 from that leg. Fuse, dedupe by id,
// sort desc, truncate to limit, spawn_bump, return SearchMode::Hybrid.
```

**Why not the alternatives:**

| Option | Verdict | Reason |
|--------|---------|--------|
| SQL CTE union (`WITH knn AS (...), fts AS (...) SELECT ... FULL OUTER JOIN`) — the sqlite-vec blog pattern | Rejected | Duplicates every filter predicate and both ranking blends into a third SQL string; binds an embedding BLOB and an FTS5 MATCH string in one statement, entangling the `map_fts_query_error` seam with vec0 errors; and the keyword-only fallback path must still exist separately, so it removes zero code. The sqlite-vec docs' single-statement form is a demo convenience, not a layering requirement. |
| Third `Store::hybrid_search` method in `sqlite.rs` | Rejected | Same duplication cost inside Rust; the store already exposes exactly the two primitives RRF needs, both oversampled (`knn_search` fetches `limit×4` capped at 200; pass an equally inflated `limit` in the keyword-leg args clone). |
| Service-level fusion (chosen) | ✅ | Pure function over two small candidate lists → trivially unit-testable with fake stores; degrade logic composes naturally (below); zero SQL changes; concurrent legs are safe (r2d2 read pool + WAL). |

**Degrade composition (extends SEARCH-03 without touching it):**
- Embed OK → both legs → `search_mode: "hybrid"` (the new healthy mode).
- Embed Err/empty → the UNCHANGED keyword-only path → `"keyword"` (degraded mode keeps its exact meaning).
- Malformed FTS5 query in hybrid mode → the keyword leg surfaces `MemoryError::InvalidQuery` → **propagate it** (400 / `invalid_params`), do NOT silently fall back to semantic-only. The query string is client input; the same text feeds both legs, and the error taxonomy rule ("bad client input must never read as anything else") already governs this.

**Envelope evolution — additive, non-breaking:**

```rust
#[serde(rename_all = "lowercase")]
pub enum SearchMode { Hybrid, Semantic, Keyword }   // add Hybrid; keep both old variants
```

- Envelope shape `{search_mode, results}` is unchanged on both transports; existing clients that check `search_mode == "keyword"` for degraded-state detection keep working unchanged.
- `Semantic` stays in the enum: (a) old releases emitted it, (b) an optional additive request field `mode: "hybrid"|"semantic"|"keyword"` on both DTOs (MCP `SearchArgs` + REST `SearchRequest`, `#[serde(default)]`) lets agents/tests force a leg — cheap to add now, and it makes the keyword fallback deterministically testable without killing Ollama.
- The `memory_search` tool description string in `mcp.rs` currently enumerates `'semantic'|'keyword'` — it must be updated in the same change (tool descriptions are agent-facing API).

**Modified:** `service.rs` (search branch + `SearchMode` variant + fusion fn), `mcp.rs` (description, optional `mode` field), `rest/handlers.rs` (optional `mode` field). **New:** nothing outside `service.rs`. **No migration.**

## 2. `memory_update` + Relations (MCP-06)

### 2a. Update semantics — re-embed at the service, invalidate in one writer transaction

The critical invariants come from v1.0 decisions that must keep holding:

1. **FTS5 sync is FREE** — `sql/0001_init.sql` already has the `memories_au` AFTER UPDATE trigger (delete old FTS row + insert new). A plain `UPDATE memories SET content = ...` keeps the FTS mirror correct with zero new code.
2. **vec0 ignores triggers** (v1.0 RESEARCH Pitfall 3 / T-02-05) — the vector row MUST be handled explicitly in the same writer transaction, exactly as `forget`/`sweep_expired` already do.

**Flow (mirrors `MemoryService::store` exactly):**

```
memory_update(id, patch)
  service: if patch.content changed → embed new content best-effort (async, OUTSIDE spawn_blocking)
  store (ONE writer tx):
    UPDATE memories SET <patched fields>, last_accessed = now
      [if mem_type changes: recompute base_weight from is_pinned()]
    if content changed:
      DELETE FROM vec_memories WHERE memory_id = ?
      if embed succeeded: INSERT new vector, embedding_status = 1
      else:               embedding_status = 0   ← the EXISTING sweep backfill picks it up
  → Ok(Some(MemoryView)) | Ok(None) for unknown id (clean not-found, the forget pattern)
```

This reuses the pending/backfill machinery wholesale: a content update while Ollama is down degrades identically to a store while Ollama is down — no new degrade concept. Tag/scope/TTL-only patches skip embedding entirely (the embedding is derived from content alone).

**Known API-design pitfall to settle in phase discussion:** patch semantics for clearing optional fields (`scope: null` = clear vs. absent = unchanged). serde flattens both to `None` with plain `Option<T>`. Options: double-`Option<Option<T>>` deserialization, or explicit `clear_scope`/`clear_ttl` booleans. Decide once, apply to MCP and REST DTOs identically.

### 2b. Relations — a flat `memory_links` table, NOT tags

Tags cannot express direction, type, or referential integrity, and their storage (JSON text, currently substring-LIKE-matched) is precisely what the WR-01/02/07 hardening is fixing. Relations are typed edges:

```sql
-- sql/0003_links.sql (migration 0003; append to migrations.rs)
CREATE TABLE memory_links (
    from_id    INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    to_id      INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL,          -- validated at the service seam (LinkKind enum)
    created_at INTEGER NOT NULL,
    PRIMARY KEY (from_id, to_id, kind)
);
CREATE INDEX idx_links_to ON memory_links(to_id);
```

**Forget cascade — verified safe with existing code:** `foreign_keys = ON` is already applied per-connection by `apply_pragmas` in `sqlite.rs` (writer AND every pooled read connection), and both deletion paths (`forget`, `sweep_expired`) run on the writer connection — so `ON DELETE CASCADE` fires for both explicit forget and TTL expiry with **zero changes to those methods**. `memory_links` is a real table (not virtual), so unlike vec0 no manual cleanup is needed. Add a kill-test: forget a linked memory → its link rows are gone; the memory on the other end is untouched.

**`kind` vocabulary:** model as a `LinkKind` enum with `TryFrom<&str>` returning a new `MemoryError::InvalidLinkKind(String)` — the exact `MemoryType` pattern (D-07). Suggested initial set: `RELATES_TO`, `SUPERSEDES`, `CAUSED_BY`, `BLOCKS`. A free-form TEXT kind would forfeit the clean-400 validation story the taxonomy exists for.

**Tool/route surface (keep `MemoryView` unchanged for wire compatibility):**

| MCP tool | REST route | Store method |
|----------|-----------|--------------|
| `memory_update` | `PATCH /api/memories/{id}` | `Store::update` |
| `memory_link` | `POST /api/memories/{id}/links` | `Store::link` |
| `memory_unlink` | `DELETE /api/memories/{id}/links` (body: to_id, kind) | `Store::unlink` |
| `memory_related` | `GET /api/memories/{id}/links` | `Store::related` (JOIN back to memories, both directions) |

**New:** `sql/0003_links.sql`, `LinkKind` in `domain.rs`, 4 `Store` methods + impls, 4 service methods, 4 tool adapters, 4 REST handlers. **Modified:** `migrations.rs` (append), `domain.rs` (`MemoryError` variants — exhaustive matches force both error mappers to add arms, which is the compile-time safety net working as designed).

## 3. Shared Input Validation (WR-01/WR-02/WR-07)

### Recommendation: validate in core at the service seam, surfaced through the existing error taxonomy — never per-transport

The precedent is already set: `MemoryType::try_from` validation is currently duplicated in `mcp.rs` AND `rest/handlers.rs` (identical 6-line blocks), which is exactly the drift risk to stop compounding as update/link/export add more inputs.

**Pattern:** add `MemoryError::InvalidArgument { field: &'static str, reason: String }` and validate at the TOP of each `MemoryService` method (`store`, `search`, `list`, `update`, `link`, `import`):

- `limit`: reject `<= 0` and `> MAX_LIMIT` (e.g. 1000) with a 400 — reject, don't silently clamp; agents should learn the bound. (Today `search` does `.max(0)`/`.max(1)` ad-hoc clamping in two places and `list` binds a negative limit as "no limit" via the `CASE WHEN` — boundary behavior currently *differs by endpoint*, which is the WR finding.)
- `ttl_secs`: reject `<= 0` (a negative TTL currently lands as `expires_at` in the past → silently swept next tick — a data-loss footgun, not a server fault, so 400).
- `content`: reject empty/oversized (reuse `MAX_IMPORT_CONTENT_BYTES = 8192` from the importer as the system-wide cap — one constant, promoted to `domain.rs`).

Both mappers add ONE arm each (`InvalidArgument → invalid_params` / `→ 400`), and every transport — including future ones — inherits every rule. Transports keep only what is structurally theirs: JSON deserialization and the `MemoryType`/`LinkKind` wire-string parse (which can also move into service by passing wire strings down — optional cleanup, not required).

**Exact tag-match (WR-07) is a store change, not validation:** replace the three copies of `tags LIKE '%' || ? || '%'` (in `knn_search`, `list`, `search` — currently a substring match over JSON text, so `?tag=rust` matches `"rustls"`) with:

```sql
EXISTS (SELECT 1 FROM json_each(m.tags) WHERE json_each.value = ?N)
```

`json_each` is built into the bundled SQLite (JSON functions are core since 3.38; rusqlite 0.39 bundles far newer). All three call sites must change in one commit with a shared regression test (tag `rust` must NOT match `rustls`). This is a **behavioral change** — release notes must say so.

**Modified:** `domain.rs`, `service.rs`, both error mappers, 3 SQL strings in `sqlite.rs`. **New:** a `validate` helper module (or inline fns) in core + boundary-value test suite. **No migration.**

## 4. Export / Import (DIST-04)

### Recommendation: streaming JSONL through a new `export` module beside the existing importer; embeddings do NOT export

**Format:** JSONL — line 1 a header record (`{"format":"agent-memory-export","version":1,"exported_at":...}`), then one self-describing record per line. Two record types: `{"record":"memory", ...full row...}` and `{"record":"link", from, to, kind}` (which is why export lands AFTER relations in the build order — the portable file should carry the whole graph). JSONL streams row-by-row on both sides (no full-corpus buffering), appends cleanly, and diffs/greps well — the right shape for a local-first tool.

**Do embeddings export? No.**
- They are **derived data**: the existing `embedding_status = 0` + sweep-backfill machinery re-creates them on the import side automatically — the "install Ollama later" story already shipped in v1.0 and this reuses it verbatim.
- They are **model/dim-pinned** (the `meta` table pin exists precisely so foreign vectors never mix — v1.0 Pitfall 6); importing vectors embedded elsewhere would need model/dim verification for ~zero benefit when re-embedding is local and free.
- 768 × f32 ≈ 3 KB per row dwarfs the content it annotates.
- A `--include-embeddings` flag can be added later without a format break (additive field + header capability flag). Defer.

**Export side (new):** `Store::export_rows` — a streaming read (full `Memory` shape including `source`, `base_weight`, `access_count`, `created_at`, `last_accessed`, `expires_at`, NOT the trimmed `MemoryView`) with optional scope/type filters; CLI `agent-memory export [--output PATH] [--scope S] [--type T]` (stdout default — sanctioned for CLI subcommands per the import precedent; MCP-05 stdout purity applies only to `serve`).

**Import side (modified, not new):** a `jsonl.rs` parser beside `import/gsd_state.rs`, dispatched by the existing `--from` match in `main.rs` (`gsd-state` | `jsonl`). It feeds the SAME `MemoryService::import`, so `(source, mem_type, content)` dedup gives round-trip idempotency for free.

**One real design decision — timestamp preservation:** `MemoryService::import` stamps `created_at = last_accessed = now`, which is correct for STATE.md ingestion but wrong for backup/restore (all decay history resets). For a faithful round-trip, extend `NewMemory` with `created_at: Option<i64>` / `last_accessed: Option<i64>` overrides (defaulting to `now` in `Store::insert`) and have only the JSONL importer populate them. Small, additive, but it touches `insert` — schedule it inside the export phase, not as an afterthought.

**New:** `core/src/export.rs` (or `interop/` regrouping), `import/jsonl.rs`, `Store::export_rows`, `Export` CLI subcommand. **Modified:** `main.rs` dispatch, `NewMemory`/`Store::insert` (timestamp overrides), `service::import` (pass-through). **No migration.**

## 5. Windows (DIST-03) + musl

### 5a. Codebase audit — agent-memory is NOT mcp-hub; it is already ~Windows-clean

Verified by grep across both crates: the **only** platform-conditional code is `config.rs::create_data_dir` (`cfg(unix)` mode-0700 directory, with the `cfg(not(unix))` fallback **already implemented**). There is:
- **No signal handling** (shutdown is stdio-EOF / process kill — portable).
- **No file locking** beyond SQLite's own (bundled SQLite handles Windows locking internally).
- **No unix-only dependencies**: `dirs` resolves `%APPDATA%` on Windows; `rmcp` stdio, `axum`, `tokio`, `clap`, `tracing` are all Windows-native; `reqwest` with `default-features = false, features=["json"]` needs no TLS (localhost Ollama) so no schannel/openssl surface; `chrono` is already trimmed to `now`.

The mcp-hub precedent (unguarded `cfg(unix)`-gated deps forcing a CLI refactor and a v2 deferral) **does not apply here** — that risk was designed out in v1.0 ("keep platform paths in one file"). The DIST-03 work is therefore ~90% release-pipeline, ~10% code:
- Confirm `resolve_db_path` behavior on Windows (path join, `%APPDATA%\agent-memory\memory.db`) — the 0700 permission guarantee (T-01-03) silently doesn't hold on Windows; document that in the README security note rather than reimplementing ACLs for v1.1.
- One possible papercut: none found in `default_scope_for` (pure `Path` APIs, portable).

### 5b. Release workflow — cargo-zigbuild CANNOT build the Windows leg (verified)

The cargo-zigbuild README states: *"Currently only Linux and macOS targets are supported."* So the Windows leg is a **new, differently-tooled matrix entry**, not a seventh zigbuild target:

```yaml
# release.yml — new matrix leg (orangepi ARM64 host)
- target: x86_64-pc-windows-gnu
  continue-on-error: true        # best-effort in v1.1, promote later
# steps for this leg only:
#   apt-get install gcc-mingw-w64-x86-64   (Debian/Ubuntu ships it for arm64 hosts)
#   rustup target add x86_64-pc-windows-gnu
#   cargo build --release --locked --target x86_64-pc-windows-gnu -p agent-memory
#   package as agent-memory-x86_64-pc-windows-gnu.zip (agent-memory.exe at root)
```

- **windows-gnu via MinGW-w64, not windows-msvc via cargo-xwin**, for v1.1: mingw is one apt package on the existing runner, fully self-contained (no MSVC CRT download/EULA), and rusqlite `bundled` + sqlite-vec's `cc` build compile with `x86_64-w64-mingw32-gcc` (the `cc` crate picks it up per-target). cargo-xwin/msvc is the "more native for end-users" upgrade path if gnu binaries hit issues — defer.
- **Smoke test: presence-only** (a PE binary can't execute on the Linux host — same rule the darwin legs already follow). There is no Windows runner in the fleet, so the binary ships **untested-at-runtime**; mitigate with (a) the spike workflow proving the build early, (b) `continue-on-error: true` for the leg, (c) an explicit "community-validated" label on the release notes for the first Windows asset. Optional stretch: a `wine64 agent-memory.exe --version` smoke if wine is installable on the runner — do not gate on it.
- Windows asset is a **zip** (Windows convention; the Homebrew tarball contract is untouched — Homebrew never sees the Windows asset). SHA256SUMS glob in the release job must widen from `agent-memory-*.tar.gz` to include the zip.

### 5c. musl — CFLAGS shim now, dependency bump when released

Verified: sqlite-vec PR #199 (removes the BSD `u_int*_t` typedef fallback) is **still open/unreleased**; the pinned crate 0.1.9 ships the broken typedefs (0.1.10 is alpha-only, per the existing Cargo.toml note), though a June 2026 comment reports latest main no longer needs the patch. Ordered plan:

1. **CFLAGS shim in the two musl legs** (zero dependency change): the `cc` crate honors per-target flags — `CFLAGS_x86_64_unknown_linux_musl="-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int32_t=uint32_t -Du_int64_t=uint64_t"` (and the aarch64 twin). Pure workflow-env change.
2. If a fixed stable sqlite-vec release exists at build time, bump the pin instead and delete the shim.
3. On green musl builds: flip both musl legs from `continue-on-error: true` to required, and add them to the release job's REQUIRED asset list.

## Data Flow Changes (delta only)

```
memory_search (hybrid, healthy path — NEW fan-out):
  transport → service.search → embed(query) ──ok──┐
                                                   ├─ tokio::join! ─ spawn_blocking(knn_search)  ─┐
                                                   │                 spawn_blocking(fts search)   ─┤
                                                   └──err──> UNCHANGED keyword-only path           │
                                        RRF fuse (k=60) + dedupe + truncate ←─────────────────────┘
                                        → spawn_bump → SearchOutcome{ "hybrid", results }

memory_update (NEW, mirrors store):
  transport → service.update → embed(new content, best-effort, async)
            → spawn_blocking → ONE writer tx: UPDATE memories (FTS trigger fires)
                                              + explicit vec_memories DELETE/INSERT or status=0
            → sweep backfill covers the pending case (existing machinery)

forget/TTL sweep (UNCHANGED code, new effect):
  DELETE memories row → memory_links cascades via FK (foreign_keys=ON already applied)
                      → vec_memories still deleted explicitly (vec0 ignores FK/triggers too)

export (NEW):
  CLI export → service → Store::export_rows (streaming, read pool)
             → JSONL: header + memory records + link records (NO embedding blobs)
  import --from jsonl → jsonl parser → SAME service.import (dedup) → pending embeddings → sweep backfills
```

## Suggested Build Order

Dependencies, not preferences:

| # | Work | Depends on | Why this position |
|---|------|-----------|-------------------|
| 1 | **API hardening + shared validation seam** (`InvalidArgument` variant, limit/ttl bounds, exact tag match via `json_each`) | — | Foundation: update/link/export all route their new inputs through this seam; adding the error variant early means every later mapper arm is written once. Small, high-certainty, ships confidence. |
| 2 | **memory_update + relations** (migration 0003, `LinkKind`, store/service/tools/routes) | 1 (validation of patch fields + kind) | Must precede export so the portable file format includes links from day one — a format v1 without links forces a format v2 immediately after. |
| 3 | **Hybrid RRF search** (service combinator, `SearchMode::Hybrid`, optional `mode` param) | — (parallel with 2) | Touches only `service.rs` + DTO descriptions; no schema, no store changes. Independent of 1 and 2 — can run as a parallel track. |
| 4 | **Export/import JSONL** (export module, jsonl importer, timestamp-preserving `NewMemory` extension) | 2 (links in format), 1 (filter validation) | Last core feature: exports the final v1.1 schema. |
| 5 | **musl CFLAGS shim** (workflow env only) | — | Anytime; verify via the existing `spike-cross-compile.yml` early so the release flip (best-effort → required) is proven before tag day. |
| 6 | **Windows leg** (mingw matrix entry, zip packaging, SHA256SUMS glob, README note on Windows dir permissions) | all code final (5 too) | Spike the mingw build in `spike-cross-compile.yml` in week 1 (de-risk), but the release.yml change lands in the final release phase since the shipped binary must contain everything. |

Phase-shape suggestion for the roadmap: **Phase 1** = row 1 (hardening), **Phase 2** = rows 2+3 as parallel tracks (update/relations and hybrid search don't share files beyond `service.rs`/`domain.rs` — sequence the `domain.rs` error-variant additions in Phase 1 to avoid conflicts), **Phase 3** = row 4 (export/interop), **Phase 4** = rows 5+6 + release (with the spike workflow runs pulled forward into Phase 1 as a de-risk task).

## Anti-Patterns (v1.1-specific)

### Anti-Pattern 1: Fusing in SQL because the sqlite-vec blog does
**What people do:** copy the documented single-statement CTE hybrid query into a new store method.
**Why it's wrong here:** this codebase deliberately keeps the degrade decision at the service seam and the FTS5-error classification at the store seam; a combined statement entangles both, duplicates three filter predicates, and still leaves the keyword-only fallback as separate code.
**Instead:** fuse in Rust over the two existing, individually-tested store methods.

### Anti-Pattern 2: Updating content without touching `vec_memories`
**What people do:** trust triggers — the FTS mirror updates itself, so it "looks done."
**Why it's wrong:** vec0 virtual tables ignore triggers (proven in v1.0; `forget` and `sweep_expired` both carry explicit vector deletes for this reason). A content update that leaves the old vector behind makes the memory findable by its *previous* meaning — a semantic-staleness bug that no keyword test catches.
**Instead:** DELETE + (INSERT | `embedding_status = 0`) in the same writer transaction, plus a kill-test: update content, search for the OLD meaning semantically, assert absence.

### Anti-Pattern 3: Per-transport validation "just for this one field"
**What people do:** bounds-check `limit` in the REST handler because that's where the bug report came from.
**Why it's wrong:** MCP inherits nothing; the surfaces drift (they already differ on negative limits today).
**Instead:** validate once in `MemoryService`, surface through `MemoryError`, let the exhaustive-match mappers force both transports to comply.

### Anti-Pattern 4: Exporting embeddings for "completeness"
**What people do:** dump `vec_memories` blobs into the export file.
**Why it's wrong:** 3 KB/row of model-pinned derived data; importing them requires model/dim verification the meta-pin exists to prevent, and the backfill sweep regenerates them for free.
**Instead:** export content only; let `embedding_status = 0` + the existing sweep rebuild vectors on the import side.

### Anti-Pattern 5: Treating Windows as "add a zigbuild target"
**What people do:** append `x86_64-pc-windows-gnu` to the existing matrix and expect `cargo zigbuild` to handle it.
**Why it's wrong:** cargo-zigbuild supports Linux and macOS targets only (verified against the README, 2026-07).
**Instead:** a separate mingw-w64 leg (`gcc-mingw-w64-x86-64` on the ARM64 runner, plain `cargo build`), zip packaging, presence-only smoke.

## Integration Points Summary (for the roadmapper)

| Feature | New components | Modified components | Migration | Wire compat |
|---------|---------------|--------------------|-----------|-------------|
| Hybrid RRF | RRF fusion fn (service) | `service.rs`, `SearchMode`, tool description, DTOs (optional `mode`) | none | additive (`"hybrid"` value; envelope shape unchanged) |
| memory_update | `Store::update`, service method, tool, PATCH route | `domain.rs` (patch type), both error mappers | none | new surface only |
| Relations | `0003_links.sql`, `LinkKind`, 3 store methods, 3 tools, 3 routes | `migrations.rs`, `domain.rs` | **0003** (FK cascade; `foreign_keys=ON` already live) | new surface only; `MemoryView` unchanged |
| Validation | validate helpers, `InvalidArgument` variant | `service.rs`, both mappers, 3 SQL tag predicates | none | **behavioral**: out-of-range inputs now 400; tag filter becomes exact-match — release-notes item |
| Export/import | `export.rs`, `jsonl.rs`, `Store::export_rows`, CLI subcommand | `main.rs`, `NewMemory`/`insert` (timestamp overrides) | none | new format, versioned header |
| Windows | mingw matrix leg, zip packaging | `release.yml`, `spike-cross-compile.yml`, SHA256SUMS glob, README | none | new asset |
| musl | CFLAGS env shim | `release.yml` musl legs (later: required) | none | asset promotion |

## Sources

**Codebase (HIGH confidence — read directly, 2026-07-12):** `crates/agent-memory-core/src/{service.rs, domain.rs, store/{mod.rs, sqlite.rs, migrations.rs}, import/gsd_state.rs}`, `crates/agent-memory/src/{main.rs, config.rs, mcp.rs, rest/{mod.rs, handlers.rs}}`, `crates/agent-memory-core/sql/{0001_init.sql, 0002_embeddings.sql}`, `.github/workflows/release.yml`, `Cargo.toml`, `.planning/PROJECT.md`.

**External (verified 2026-07-12):**
- [sqlite-vec hybrid search with FTS5 + RRF — Alex Garcia (sqlite-vec author)](https://alexgarcia.xyz/blog/2024/sqlite-vec-hybrid-search/index.html) — RRF formula `1/(k+rank)` with k=60, CTE reference pattern (HIGH: author's own docs)
- [Hybrid full-text + vector search with SQLite — Simon Willison](https://simonwillison.net/2024/Oct/4/hybrid-full-text-search-and-vector-search-with-sqlite/) (MEDIUM: corroborating writeup)
- [cargo-zigbuild README — supported targets](https://github.com/rust-cross/cargo-zigbuild) — "Currently only Linux and macOS targets are supported" (HIGH: official README, fetched directly)
- [sqlite-vec PR #199 — musl `u_int*_t` fix](https://github.com/asg017/sqlite-vec/pull/199) — still open/unreleased as of 2026-07; latest main reportedly no longer needs it (HIGH for status; MEDIUM for the "fixed on main" comment)
- [rusqlite cross-compile to windows-gnu discussions](https://github.com/rusqlite/rusqlite/issues/1201) — `bundled` + mingw-w64 is the working path (MEDIUM: community-reported, multiple corroborating sources)

---
*Architecture research for: agent-memory v1.1 Hardening & Interop milestone*
*Researched: 2026-07-12*
