# Phase 4: Memory Update, Relations & Hybrid Search - Pattern Map

**Mapped:** 2026-07-12
**Files analyzed:** 14 new/modified files
**Analogs found:** 13 / 14

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/agent-memory-core/sql/0003_relations.sql` (NEW) | migration | schema DDL | `sql/0001_init.sql` | exact |
| `crates/agent-memory-core/src/domain.rs` (`LinkKind`, `Internal`, consts, `RelatedLink`) | model | validation/types | `MemoryType` + `validate_ttl` in same file | exact |
| `crates/agent-memory-core/src/service.rs` (hybrid RRF, update/link/unlink/expand) | service | request-response + transform | existing `search`/`import`/`forget` in same file | exact |
| `crates/agent-memory-core/src/store/mod.rs` (trait methods) | model (trait) | CRUD | existing trait methods, esp. `insert`/`forget`/`exists` | exact |
| `crates/agent-memory-core/src/store/sqlite.rs` (`update`, `link`, `unlink`, `related`, `keyword_candidates`, backup+probe in `open`) | service (persistence) | CRUD + file-I/O | `insert`/`insert_embedding`/`forget`/`knn_search`/`search` in same file | exact |
| `crates/agent-memory-core/src/store/migrations.rs` (append 0003) | config | schema DDL | itself (lines 9-14) | exact |
| `crates/agent-memory/src/mcp.rs` (`memory_update`/`link`/`unlink` tools, `expand_links`) | controller | request-response | `memory_forget` / `memory_search` tools in same file | exact |
| `crates/agent-memory/src/rest/handlers.rs` (PATCH + link handlers) | controller | request-response | `forget_handler` / `search_handler` in same file | exact |
| `crates/agent-memory/src/rest/mod.rs` (new routes) | route/config | request-response | existing router in same file | exact |
| `crates/agent-memory-core/tests/update.rs` (NEW) | test | CRUD | `tests/ttl.rs` harness | exact |
| `crates/agent-memory-core/tests/relations.rs` (NEW) | test | CRUD | `tests/ttl.rs` harness | exact |
| `crates/agent-memory-core/tests/hybrid.rs` (NEW) | test | transform | `tests/ttl.rs` + `tests/semantic.rs` | role-match |
| `crates/agent-memory-core/tests/migration_hygiene.rs` (NEW) | test | file-I/O | `migrations.rs` test module (register_vec first) | role-match |
| `crates/agent-memory-core/tests/fixtures/v0.0.1.db` (NEW binary fixture) | test fixture | file-I/O | — (no binary fixture exists yet) | no analog |

## Pattern Assignments

### `sql/0003_relations.sql` (migration, DDL)

**Analog:** `crates/agent-memory-core/sql/0001_init.sql`

**Conventions to copy** (0001 lines 1-24): header comment stating "Run via rusqlite_migration `M::up`"; PRAGMAs are NOT in migrations (applied per-connection in sqlite.rs — `foreign_keys=ON` is already applied, sqlite.rs:78); column comments; UPPERCASE column type + aligned formatting; `INTEGER` UTC epoch timestamps; named `idx_*` indexes:

```sql
CREATE TABLE memories (
    id            INTEGER PRIMARY KEY,          -- rowid; == memories_fts.rowid
    mem_type      TEXT NOT NULL,                -- DECISION|PATTERN|...
    ...
    created_at    INTEGER NOT NULL,             -- UTC unix epoch (i64)
    ...
);
CREATE INDEX idx_memories_type    ON memories(mem_type);
```

RESEARCH Pattern 3 gives the exact table sketch (memory_links, PK(from_id,to_id,kind), FK ON DELETE CASCADE, `idx_links_to`). 0001/0002 are FROZEN — append only.

---

### `domain.rs` — `LinkKind` enum, `MemoryError::Internal`, `MAX_EXPANDED_LINKS`, `RelatedLink`

**Analog:** `MemoryType` + `MemoryError` + bounds consts in the same file.

**Enum + TryFrom pattern to copy** (domain.rs:15-69) — but `LinkKind` returns `InvalidArgument` per D-01, wire form lowercase:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]   // LinkKind: use "lowercase" instead
pub enum MemoryType { Decision, ... }

pub fn as_wire_str(self) -> &'static str { match self { ... } }

impl std::convert::TryFrom<&str> for MemoryType {
    type Error = MemoryError;
    fn try_from(s: &str) -> Result<Self, MemoryError> {
        match s {
            "DECISION" => Ok(MemoryType::Decision),
            ...
            other => Err(MemoryError::InvalidType(other.to_string())),
            // LinkKind: Err(MemoryError::InvalidArgument(format!(
            //   "kind must be one of relates_to, supersedes, caused_by (got '{other}')")))
        }
    }
}
```

**Bounds-const pattern** (domain.rs:117-131) — place `MAX_EXPANDED_LINKS` here with a doc comment citing D-07:

```rust
/// Largest accepted `limit` on search/list (D-01/D-03). ...
pub const MAX_LIMIT: i64 = 200;
```

**Validation helper pattern** (domain.rs:179-188) — self-link check and patch-content checks follow this exact shape (pub(crate), formatted range message):

```rust
pub(crate) fn validate_ttl(ttl_secs: Option<i64>) -> Result<(), MemoryError> {
    if let Some(t) = ttl_secs {
        if !(MIN_TTL_SECS..=MAX_TTL_SECS).contains(&t) {
            return Err(MemoryError::InvalidArgument(format!(
                "ttl_secs must be between {MIN_TTL_SECS} and {MAX_TTL_SECS} (got {t})"
            )));
        }
    }
    Ok(())
}
```

**Error variant pattern** (domain.rs:135-160) — add `Internal(String)` here; every variant carries a `#[error("...")]` message:

```rust
#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    ...
}
```

`MemoryView` (domain.rs:104-115) is where the optional `related: Option<Vec<RelatedLink>>` with `#[serde(skip_serializing_if = "Option::is_none")]` goes (RESEARCH Open Question 2, recommendation accepted). Note `#[serde(rename = "type")]` on `mem_type` — `RelatedLink` should follow the same rename.

---

### `service.rs` — hybrid RRF, `update`, `link`, `unlink`, expansion

**Analog:** `MemoryService::search` / `import` / `forget` in the same file.

**Seam validation FIRST, then clock, then embed, then spawn_blocking** (service.rs:117-140, `store`):

```rust
pub async fn store(&self, new: NewMemory) -> Result<i64, MemoryError> {
    crate::domain::validate_ttl(new.ttl_secs)?;         // reject BEFORE embed
    let now = self.clock.now();
    let inputs = [new.content.clone()];
    let embedding = match self.embedder.embed(&inputs).await {
        Ok(mut vectors) if !vectors.is_empty() => Some(vectors.swap_remove(0)),
        Ok(_) => { tracing::warn!("...; storing with embedding_status = 0"); None }
        Err(e) => { tracing::warn!("embedding failed ({e}); ..."); None }
    };
    let store = self.store.clone();
    tokio::task::spawn_blocking(move || store.insert(new, embedding, now))
        .await
        .map_err(MemoryError::Join)?
}
```
`update()` copies this exactly: `validate` patch → embed new content only if `patch.content.is_some()` → one `spawn_blocking(store.update(...))`.

**Degrade seam + envelope + bump-after-truncate** (service.rs:170-252, `search`) — hybrid slots at line 182 (`Ok(mut vectors)` arm); the `other =>` keyword arm at 222-250 stays byte-identical for degradation:

```rust
match self.embedder.embed(&query_input).await {
    Ok(mut vectors) if !vectors.is_empty() => {
        // HYBRID goes here: tokio::join! both legs, rrf_fuse, ×decay, take(limit)
        ...
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        let results: Vec<MemoryView> = scored.into_iter().take(limit).map(|(v, _)| v).collect();
        self.spawn_bump(results.iter().map(|v| v.id).collect(), now);   // ONLY returned ids
        Ok(SearchOutcome { search_mode: SearchMode::Semantic /* -> Hybrid */, results })
    }
    other => {
        // UNCHANGED keyword path -> SearchMode::Keyword, one loud warn
        ...
    }
}
```

**`SearchMode` / `SearchOutcome` to extend** (service.rs:47-65) — add `Hybrid` variant; serde lowercase already gives `"hybrid"`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode { Semantic, Keyword }
```

**Decay recompute per-view** (service.rs:195-208) — hybrid's post-fusion blend reuses this: `decay_score(now, view.last_accessed, cfg.half_life_secs, view.mem_type.is_pinned())` and sets `view.decay_score = d`.

**Clean not-found shape** (service.rs:353-358, `forget`) — `update`/`link`/`unlink` copy: thin async wrapper, `Ok(false)`/`Ok(None)` for unknown ids, never `Err(NotFound)`:

```rust
pub async fn forget(&self, id: i64) -> Result<bool, MemoryError> {
    let store = self.store.clone();
    tokio::task::spawn_blocking(move || store.forget(id))
        .await
        .map_err(MemoryError::Join)?
}
```

**Per-draft validation loop** (service.rs:273-275, `import`) — the precedent that non-store entry points route through the same `validate_ttl` helper; update's TTL patch does the same.

RRF fusion function: use the RESEARCH Pattern 1 code verbatim as starting point (`const RRF_K: f64 = 60.0`, sum duplicate ids, `total_cmp` desc + id tie-break — the `sort_by(|a, b| b.1.total_cmp(&a.1))` idiom already at service.rs:209).

---

### `store/mod.rs` — trait methods `update`, `link`, `unlink`, `related`, `keyword_candidates`; `exists` → `Option<&str>` (CR-01)

**Analog:** existing trait methods in the same file.

**Doc + signature style to copy** (store/mod.rs:17-58): every method has a multi-line doc comment naming the requirement ID and the not-found contract; blocking signatures returning `Result<_, MemoryError>`:

```rust
/// Delete a memory by id. Returns `true` if a row was deleted, `false` if no
/// row with that id existed (clean not-found, never an error — MCP-04). ...
fn forget(&self, id: i64) -> Result<bool, MemoryError>;

fn exists(&self, source: &str, mem_type: MemoryType, content: &str) -> Result<bool, MemoryError>;
// CR-01: change to source: Option<&str>
```

`update` should return `Result<Option<Memory /* or view+status */>, MemoryError>` mirroring the `forget` bool shape; `link` returns `Result<Option<()>, _>` or a small status enum (missing-id not-found per D-02).

---

### `store/sqlite.rs` — `update`, `link`, `unlink`, `related`, `keyword_candidates`, backup + version probe

**Analog:** `insert` / `insert_embedding` / `forget` / `knn_search` / `search` in the same file.

**Writer-tx template for `update` and `link`** (sqlite.rs:305-326, `insert_embedding`) — one lock, one tx, DELETE+INSERT for vec0 (never INSERT OR REPLACE):

```rust
fn insert_embedding(&self, memory_id: i64, embedding: Vec<f32>) -> Result<(), MemoryError> {
    let mut conn = self.writer.lock().map_err(|_| MemoryError::NotFound)?;  // <- WR-02: change ALL sites to Internal("writer lock poisoned")
    let tx = conn.transaction()?;
    let blob: &[u8] = bytemuck::cast_slice(&embedding);
    tx.execute("DELETE FROM vec_memories WHERE memory_id = ?1", params![memory_id])?;
    tx.execute("INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)", params![memory_id, blob])?;
    tx.execute("UPDATE memories SET embedding_status = 1 WHERE id = ?1", params![memory_id])?;
    tx.commit()?;
    Ok(())
}
```
The six existing `writer.lock().map_err(|_| MemoryError::NotFound)` sites to convert to `Internal`: sqlite.rs:226, 306, 467, 483, 504, 526.

**Not-found via changed-rows** (sqlite.rs:466-477, `forget`) — `update` copies `if changed == 0 { return Ok(None); }`:

```rust
let changed = tx.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
tx.commit()?;
Ok(changed > 0)
```

**Belt-and-braces orphan delete idiom** (sqlite.rs:512-519, `sweep_expired`) — add the same for `memory_links` in `forget` and `sweep_expired`:

```rust
tx.execute(
    "DELETE FROM vec_memories WHERE memory_id NOT IN (SELECT id FROM memories)",
    [],
)?;
```

**`keyword_candidates` copies `search`** (sqlite.rs:395-464) — same JOIN, same NULL-disable filters, same `map_fts_query_error` routing (sqlite.rs:458-462), but strip the decay term from ORDER BY (`ORDER BY bm25(memories_fts) ASC`) and oversample `LIMIT` like `knn_search`:

```rust
// oversample pattern (sqlite.rs:268):
let k = (args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).max(1) * 4).min(MAX_KNN_K);
// filter pattern (sqlite.rs:282-285):
"WHERE (?3 IS NULL OR m.mem_type = ?3) \
   AND (?4 IS NULL OR m.scope = ?4) \
   AND (?5 IS NULL OR EXISTS (SELECT 1 FROM json_each(m.tags) WHERE json_each.value = ?5))"
// error routing (sqlite.rs:458-462):
let rows = rows.map_err(|e| map_fts_query_error(&args.query, e))?;
```
WR-01 ride-along: `const MAX_KNN_K: i64 = crate::domain::MAX_LIMIT` at sqlite.rs:35 → `MAX_LIMIT * 4`.

**Read-pool query pattern for `related`** (sqlite.rs:364-393, `list`) — `self.reads.get()`, prepared statement, `query_map` with `row_to_view`-style mapper, collect loop. Reuse `row_to_view` (sqlite.rs:164-188) mapping conventions (tags JSON via `serde_json::from_str(...).unwrap_or_default()`).

**Parameterized IN-list** (sqlite.rs:486-499, `bump_access`) — template if `related` uses one IN-list query over returned ids:

```rust
let placeholders: String = (0..ids.len()).map(|i| format!("?{}", i + 2)).collect::<Vec<_>>().join(", ");
let sql = format!("UPDATE ... WHERE id IN ({placeholders})");
let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 1);
```

**Open-flow to extend** (sqlite.rs:127-160) — probe + backup insert between `prepare_connection` and `to_latest`:

```rust
register_vec_extension()?;                       // MUST precede Connection::open (line 131)
let mut writer = Connection::open(path)?;
prepare_connection(&writer)?;
// NEW: PRAGMA user_version probe -> SchemaTooNew refusal (D-13)
// NEW: if 0 < v < LATEST && pending -> rusqlite::backup::Backup sibling file (D-11)
let migrations: Migrations<'static> = migrations();
migrations.to_latest(&mut writer)?;              // currently bare, line 136-137
```

**`exists` CR-01 fix** (sqlite.rs:344-362): change `source = ?1` to NULL-safe `source IS ?1` and take `Option<&str>`; drop the `.unwrap_or("")` at service.rs:285.

---

### `store/migrations.rs` — append 0003

**Analog:** itself (lines 9-14 + test module):

```rust
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../../sql/0001_init.sql")),
        M::up(include_str!("../../sql/0002_embeddings.sql")),
        // append: M::up(include_str!("../../sql/0003_relations.sql")),
    ])
}
```
Test quirk (migrations.rs:20-31): every migration test must call `register_vec_extension()` first — copy this into `migration_hygiene.rs`. Add a `pub const LATEST_SCHEMA_VERSION: i64 = 3` here for the D-13 probe.

---

### `mcp.rs` — `memory_update` / `memory_link` / `memory_unlink` tools, `expand_links`

**Analog:** `memory_forget` (simplest) and `memory_search` (envelope) tools in the same file.

**DTO pattern** (mcp.rs:56-118): `#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]`, doc comments become tool schema descriptions, `#[serde(default)]` on every optional. `UpdateArgs` patch fields additionally need the double-Option `deserialize_with` helper (RESEARCH Pitfall 4) — this is the one place plain `#[serde(default)]` is NOT enough:

```rust
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ForgetArgs {
    /// The numeric id of the memory to delete.
    pub id: i64,
}
```

**Tool arm pattern** (mcp.rs:229-253, `memory_forget`) — deserialize → one service call → `map_mcp_error` → JSON status; the not-found status body `memory_update` must mirror:

```rust
#[tool(description = "Delete a memory by its numeric id. Returns a deleted result, or a clean not-found result if no such id exists.")]
async fn memory_forget(&self, Parameters(args): Parameters<ForgetArgs>) -> Result<CallToolResult, McpError> {
    let deleted = self.state.service.forget(args.id).await.map_err(map_mcp_error)?;
    let status = if deleted {
        serde_json::json!({ "id": args.id, "deleted": true })
    } else {
        serde_json::json!({ "id": args.id, "deleted": false, "reason": "not_found" })
    };
    let json = serde_json::to_string(&status).map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}
```

**Wire-type parse up front** (mcp.rs:145-146) — `LinkArgs.kind` parses the same way:

```rust
let mem_type = MemoryType::try_from(args.r#type.as_str())
    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
```

**Exhaustive mapper to extend** (mcp.rs:32-43) — add `Internal(_)` to the internal tier; NO catch-all `_`:

```rust
fn map_mcp_error(e: MemoryError) -> McpError {
    match &e {
        MemoryError::InvalidType(_) | MemoryError::InvalidQuery(_) | MemoryError::InvalidArgument(_) =>
            McpError::invalid_params(e.to_string(), None),
        MemoryError::Sqlite(_) | MemoryError::Pool(_) | MemoryError::Join(_)
        | MemoryError::Migration(_) | MemoryError::NotFound =>
            McpError::internal_error(e.to_string(), None),
    }
}
```

Wire change: the `memory_search` tool description at mcp.rs:195 enumerates `'semantic'|'keyword'` — must gain `'hybrid'` in the same plan.

---

### `rest/handlers.rs` — PATCH + link handlers

**Analog:** `forget_handler` and `search_handler` in the same file.

**Handler shape + Path extractor + dual-status not-found** (handlers.rs:180-195) — `PATCH /api/memories/{id}` copies this, mapping `Ok(None)` → 404:

```rust
pub async fn forget_handler(State(state): State<Arc<AppState>>, Path(id): Path<i64>) -> Result<Response, ApiError> {
    let deleted = state.service.forget(id).await.map_err(map_memory_error)?;
    if deleted {
        Ok(Json(serde_json::json!({ "id": id, "deleted": true })).into_response())
    } else {
        Ok((StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "id": id, "deleted": false, "reason": "not_found" }))).into_response())
    }
}
```

**REST mapper to extend** (handlers.rs:87-98) — add `Internal(_)` → `ApiError::Internal`; REST DTOs are serde-only, NO schemars (file header rule, handlers.rs:1-8; DTOs mirror MCP field-for-field, lines 27-81).

**In-process handler test pattern** (handlers.rs:229-311) — new handler tests copy `test_state` (tempdir SqliteStore + `DEAD_OLLAMA = "http://127.0.0.1:9"` dead-loopback embedder + `AppState`), `body_json`, and status/body assertions. These in-process tests are what carry coverage (spawned binaries flush no profile data).

`rest/mod.rs`: add routes next to the existing ones; axum 0.8 `{id}` path syntax (existing `DELETE /api/memories/{id}` route is the template).

---

### Test files `update.rs` / `relations.rs` / `hybrid.rs` / `migration_hygiene.rs`

**Analog:** `crates/agent-memory-core/tests/ttl.rs` (harness), `tests/semantic.rs` (FakeEmbedder programming).

**Deterministic harness to copy** (ttl.rs:19-39):

```rust
fn service_on_temp_db(clock: Arc<dyn Clock>) -> (MemoryService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("memory.db")).expect("open store");
    let embedder = Arc::new(FakeEmbedder::with_vectors(std::collections::HashMap::new()));
    let service = MemoryService::new(Arc::new(store), clock, embedder, DecayConfig::default());
    (service, dir)
}
```
`TestClock::new(1_000)` + `clock.advance(...)` (ttl.rs:43-64) drives decay/freshness assertions — hybrid's fresh-outranks-stale golden test uses exactly this. `FakeEmbedder::with_vectors(HashMap)` programs per-text vectors for the update re-embed kill-test; a failing variant exercises `embedding_status = 0` (see `tests/semantic.rs` / `tests/fallback.rs` for both FakeEmbedder modes).

`migration_hygiene.rs` differs: it uses raw `Connection`/`SqliteStore::open` on tempfiles (not the service), and MUST call `register_vec_extension()` first (migrations.rs:25 precedent). Test-file doc-header convention: `//! REQ-ID: invariant prose` (ttl.rs:1-8).

## Shared Patterns

### Seam validation (apply to every new input: kind, patch fields, expand_links, self-link)
**Source:** `domain.rs:165-188` (`validate_limit`/`validate_ttl`) + call sites `service.rs:120,146,174,273`
All validation lives in `MemoryService`/`domain.rs`, produces `InvalidArgument` with the greppable `"X must be ... (got Y)"` message shape, and runs BEFORE any embed call or store hop. Transports only parse wire strings via `TryFrom`.

### Two-tier exhaustive error mapping
**Source:** `mcp.rs:32-43` + `handlers.rs:87-98`
New `MemoryError::Internal(String)` lands FIRST (whichever plan executes first); both mappers gain the arm at compile time (no `_` catch-alls). Client tier = `InvalidType|InvalidQuery|InvalidArgument`; everything else internal/500. Not-found for update/link/unlink is `Ok(None)`-shaped, never an error variant (WR-02 / Pitfall 5).

### Writer-mutex single-tx write
**Source:** `sqlite.rs:226-256` (insert), `sqlite.rs:305-326` (insert_embedding)
`self.writer.lock()` → `conn.transaction()` → all statements → `tx.commit()`. vec0 rows always explicit DELETE+INSERT (triggers cover FTS only). All SQL parameterized via `params![]`.

### spawn_blocking + injected clock + embed-outside-blocking
**Source:** `service.rs:117-140`
`let now = self.clock.now()` once per operation; embed async before the hop; `tokio::task::spawn_blocking(move || store.method(...)).await.map_err(MemoryError::Join)?`.

### Not-found as data, not error
**Source:** `service.rs:353-358`, `sqlite.rs:474-476`, `mcp.rs:245-249`, `handlers.rs:186-194`
`changed == 0` → `Ok(false)`/`Ok(None)` → MCP success-status JSON with `"reason": "not_found"` / REST 404 with the same body.

### Bump only post-truncation returned ids
**Source:** `service.rs:216, 245, 341-349` (`spawn_bump`)
Fire-and-forget after `take(limit)`; expanded neighbors are NEVER passed in (D-06).

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `tests/fixtures/v0.0.1.db` | test fixture | file-I/O | No committed binary DB fixture exists; generate ONCE from the frozen 0001+0002 via `Migrations::to_version` per RESEARCH Pattern 6 (never regenerate — Pitfall 6) |

Partial-analog notes: the backup/version-probe code in `open()` has no in-repo precedent — use the RESEARCH §Code Examples verbatim (`rusqlite::backup::Backup::run_to_completion`, `conn.pragma_query_value(None, "user_version", ...)`); requires the `backup` feature flag on the already-pinned rusqlite 0.39 (only manifest change). The double-Option `deserialize_with` helper is also new — RESEARCH Pitfall 4 pattern, kill-tests for absent/null/value on both transports.

## Metadata

**Analog search scope:** `crates/agent-memory-core/src/**`, `crates/agent-memory-core/sql/**`, `crates/agent-memory-core/tests/**`, `crates/agent-memory/src/**`
**Files scanned:** 15 read in full or targeted (all core src, both transports, 0001 SQL, migrations, ttl.rs harness)
**Pattern extraction date:** 2026-07-12
