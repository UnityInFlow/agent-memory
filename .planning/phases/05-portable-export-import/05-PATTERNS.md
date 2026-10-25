# Phase 5: Portable Export & Import - Pattern Map

**Mapped:** 2026-10-25
**Files analyzed:** 16 (7 new, 9 modified)
**Analogs found:** 15 / 16 (the one gap — a streaming boxed-writer export — has a partial analog only)

All line numbers below were read from the working tree on 2026-10-25 and match the `[VERIFIED]` anchors in 05-RESEARCH.md. Every excerpt is quoted verbatim from the analog; nothing here is invented code. Where RESEARCH already prescribes the new shape (Patterns 1–8, Code Examples 1–7) this document says *where to copy the surrounding conventions from*, not what the new code is.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/agent-memory-core/src/export.rs` (NEW) | model (format v1 record structs + consts, module-doc spec) | transform (serde wire ↔ domain) | `crates/agent-memory/src/rest/handlers.rs:104-117` (`UpdateBody`, `deny_unknown_fields` DTO) + `domain.rs:176-183` (`LinkOutcome` derive style) | role-match |
| `crates/agent-memory-core/src/import/jsonl.rs` (NEW) | utility (pure parser/validator, no I/O) | transform (text → typed records) | `crates/agent-memory-core/src/import/gsd_state.rs` (whole file) | exact (same role, opposite tolerance policy by design) |
| `crates/agent-memory-core/src/import/mod.rs` (MOD) | module index | — | itself (`import/mod.rs:1-16`) | exact |
| `crates/agent-memory-core/src/service.rs` (MOD: `redact_storage_error`, `export`, `import_jsonl`, `ImportReport` +4) | service | request-response + batch (two writer hops) | itself: `redact_search_error` 46-60, `search`/`search_unredacted` 299-303, `import` 394-463, `sweep` 688-744 | exact |
| `crates/agent-memory-core/src/store/mod.rs` (MOD: `find_by_key`, `insert_imported`, `export_rows`, `embedding_pin`) | model (trait) | CRUD | itself: `exists` doc 81-91, `link` doc 174-194 | exact |
| `crates/agent-memory-core/src/store/sqlite.rs` (MOD: impls) | model (store impl) | CRUD + streaming read | itself: `insert` 596-644, `exists` 751-772, `pending_embeddings` 735-749, `list` 774-803, `link` 945-990, `row_to_view` 443-470 | exact |
| `crates/agent-memory-core/src/domain.rs` (MOD: `ImportedMemory`, `LinkOutcome::Linked { created }`) | model | — | itself: `NewMemory` 240-253, `LinkOutcome` 173-183, `validate_link_endpoints` 486-497 | exact |
| `crates/agent-memory/src/main.rs` (MOD: `Command::Export`, `jsonl` arm) | controller (CLI) | file-I/O + request-response | itself: `Command::Import` 80-97, dispatch 118-126, `run_import` 225-258 | exact |
| `crates/agent-memory/src/export_file.rs` (NEW, or in `config.rs`) | utility (atomic 0600 file write) | file-I/O | `crates/agent-memory/src/config.rs:37-54` (cfg split) + `sqlite.rs:140-159, 265-291` (temp+rename discipline) | role-match |
| `crates/agent-memory-core/tests/export_import.rs` (NEW) | test (integration, in-process) | batch | `tests/relations.rs:33-102` (harness + raw helpers) + `tests/import.rs:34-99` (idempotency proof shape) | exact |
| `crates/agent-memory-core/tests/redaction.rs` (NEW) | test (integration, fault-injection) | request-response | `tests/hybrid.rs:712-736` | exact |
| `crates/agent-memory-core/src/export.rs` `#[cfg(test)]` goldens | test (unit) | transform | `gsd_state.rs:188-278` (in-module test style) | exact |
| `crates/agent-memory/src/rest/handlers.rs` test module (MOD: `/health` + store redaction tests) | test (in-process handler) | request-response | itself `handlers.rs:851-889` | exact |
| `crates/agent-memory/src/mcp.rs` test module (MOD: `memory_store` redaction test) + `LinkOutcome` match 437-444 | test / transport | request-response | itself `mcp.rs:1309-1344`, `mcp.rs:437-444` | exact |
| `crates/agent-memory/tests/cli.rs` (NEW, realism-only) | test (spawned binary) | file-I/O | `tests/tools.rs:12-44` + `tests/rest.rs:18-50` | role-match (those spawn long-running servers; CLI test runs one-shot subcommands) |
| `README.md` (MOD: "Export & import", widen redaction sentence 460-462) | docs | — | README 513-532 ("Import from GSD") | exact |

## Pattern Assignments

### `crates/agent-memory-core/src/import/jsonl.rs` (utility, transform) — the parser

**Analog:** `crates/agent-memory-core/src/import/gsd_state.rs` — the sibling importer. Copy its *shape* (module doc, pure function, `text.lines()` loop, result struct, in-module tests); invert its *policy* (strict `Result<_, MemoryError>` instead of skip-counts, per D-11).

**Module-doc pattern** (`gsd_state.rs:1-16`) — state the contract and the tolerance rule up front; the jsonl module doc should say the opposite explicitly:
```rust
//! Tolerant line-scanner parser for GSD `STATE.md` files (INTEROP-01).
//! ...
//! Tolerance rules (T-02-20/T-02-22): headings inside fenced code blocks never
//! change the current section; unknown sections are ignored; malformed bullets
//! and oversized items are skipped and counted; a garbage file yields zero
//! drafts and never panics. Pure function, no I/O — the CLI reads the file.
```

**Imports pattern** (`gsd_state.rs:18`) — core-internal `crate::domain` paths, nothing else:
```rust
use crate::domain::{MemoryType, NewMemory};
```
For jsonl add `MemoryError`, `LinkKind`, `validate_link_endpoints` (it is `pub(crate)`, `domain.rs:490`) and `crate::export::{Record, FORMAT_NAME, FORMAT_VERSION}`.

**Result-struct pattern** (`gsd_state.rs:33-41`) — a public `#[derive(Debug)]` struct with doc'd pub fields, consumed by the service:
```rust
/// Result of parsing one STATE.md: the typed drafts plus the count of lines
/// that were recognized as items but skipped (malformed or oversized).
#[derive(Debug)]
pub struct ParsedStateFile {
    /// Drafts ready for [`crate::service::MemoryService::import`].
    pub drafts: Vec<NewMemory>,
    /// Malformed or oversized items skipped by the tolerance rules.
    pub skipped: usize,
}
```
jsonl equivalent: `pub struct JsonlFile { pub header: ..., pub memories: Vec<...>, pub links: Vec<...> }` (RESEARCH Pattern 3). No `skipped` counter — strictness means parse errors abort.

**Core line-loop pattern** (`gsd_state.rs:61-69, 161-164`) — single pass over `text.lines()`; jsonl adds `.enumerate()` for the 1-based counter (RESEARCH Pitfall 1):
```rust
pub fn parse_gsd_state(text: &str, scope: Option<String>) -> ParsedStateFile {
    let mut drafts = Vec::new();
    let mut skipped = 0usize;
    ...
    for line in text.lines() {
        let trimmed = line.trim();
```

**Validation-at-the-seam pattern** — the wire strings must go through the existing `try_from`s, whose messages are the established client-tier text (`domain.rs:158-171`):
```rust
impl std::convert::TryFrom<&str> for LinkKind {
    type Error = MemoryError;

    fn try_from(s: &str) -> Result<Self, MemoryError> {
        match s {
            "relates_to" => Ok(LinkKind::RelatesTo),
            "supersedes" => Ok(LinkKind::Supersedes),
            "caused_by" => Ok(LinkKind::CausedBy),
            other => Err(MemoryError::InvalidArgument(format!(
                "kind must be one of relates_to, supersedes, caused_by (got '{other}')"
            ))),
        }
    }
}
```
and the self-link + empty-content wording to reuse (`domain.rs:490-497`, `domain.rs:475-478`):
```rust
pub(crate) fn validate_link_endpoints(from_id: i64, to_id: i64) -> Result<(), MemoryError> {
    if from_id == to_id {
        return Err(MemoryError::InvalidArgument(format!(
            "from_id and to_id must differ (got {from_id})"
        )));
    }
    Ok(())
}
```
```rust
            return Err(MemoryError::InvalidArgument(
                "content must be a non-empty string".to_string(),
            ));
```
Wrap each as `MemoryError::InvalidArgument(format!("line {line_no}: {inner}"))` — `InvalidArgument`'s Display is `"invalid argument: {0}"` (`domain.rs:389-390`), giving D-11's `invalid argument: line 42: …`.

**In-module test pattern** (`gsd_state.rs:188-200`) — `#[cfg(test)] mod tests { use super::*; fn parse(...) }` with one behaviour per `#[test]`, descriptive snake_case names:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> ParsedStateFile {
        parse_gsd_state(text, Some("test-scope".to_string()))
    }

    #[test]
    fn decisions_bullets_map_to_decision_drafts_with_provenance() {
        let parsed = parse("### Decisions\n\n- first decision\n- second decision\n");
        assert_eq!(parsed.drafts.len(), 2);
```

---

### `crates/agent-memory-core/src/import/mod.rs` (module index)

**Analog:** itself (`import/mod.rs:1-16`). Add `pub mod jsonl;` after `pub mod gsd_state;` and amend the module doc: the first invariant currently reads "**Tolerant**" for *every* importer — it must become "gsd-state is tolerant; jsonl is strict (D-11)" or the doc contradicts the new module:
```rust
//! Invariants every importer guards:
//!
//! - **Tolerant**: messy input degrades to skip-counts — ...
//! - **Idempotent**: importers produce [`crate::domain::NewMemory`] drafts that
//!   [`crate::service::MemoryService::import`] dedups against the store on the
//!   exact `(source, mem_type, content)` key, so re-running an import never
//!   duplicates.
//! - **Data-only**: imported content is stored strictly as data. ...

pub mod gsd_state;
```

---

### `crates/agent-memory-core/src/export.rs` (model, transform) — format v1 definition

**Analog (DTO strictness):** `crates/agent-memory/src/rest/handlers.rs:104-110` — the 04-07 `deny_unknown_fields` precedent, including the doc sentence explaining *why* unknown keys are rejected:
```rust
/// Unknown keys are rejected (`deny_unknown_fields`, REVIEW WR-02), exactly as
/// on the MCP DTO: the immutable `type`/`source` and typos such as `ttl_sec`
/// make the whole PATCH a 400 naming the field, and nothing is written. A
/// silently dropped merge-patch member would be a silent partial write.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateBody {
```

**Analog (enum derive + lowercase wire tags):** `domain.rs:188-193`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkDirection {
    Out,
    In,
}
```
The `Record` enum shape itself is RESEARCH Pattern 1 (`#[serde(tag = "record", rename_all = "lowercase", deny_unknown_fields)]`, `String` for `type`/`kind`, `#[serde(rename = "type")] mem_type`). Keep `pub const FORMAT_NAME` / `FORMAT_VERSION` beside the enum the way `gsd_state.rs:22` keeps `pub const MAX_IMPORT_CONTENT_BYTES` beside its parser.

**Registration:** add `pub mod export;` to `crates/agent-memory-core/src/lib.rs:7-13` (alphabetical: after `embed`, before `import`).

**Golden test:** RESEARCH Code Example 1, in a `#[cfg(test)] mod tests` following `gsd_state.rs:188-194`.

---

### `crates/agent-memory-core/src/service.rs` (service) — `redact_storage_error`, `export`, `import_jsonl`, `ImportReport`

**Analog:** itself.

**D-14 redaction helper to generalise** (`service.rs:33-60`) — keep the constant, add an `op: &str` parameter, keep the exhaustive match with no catch-all:
```rust
pub const SEARCH_STORAGE_FAILURE: &str =
    "search failed due to a storage error (details are in the server log)";

/// Tier every search-path error for the wire — the ONE place this happens.
/// ...
/// No catch-all arm: a new [`MemoryError`] variant must be tiered here at
/// compile time.
fn redact_search_error(e: MemoryError) -> MemoryError {
    match e {
        MemoryError::InvalidType(_)
        | MemoryError::InvalidQuery(_)
        | MemoryError::InvalidArgument(_) => e,
        MemoryError::Internal(_) | MemoryError::SchemaTooNew { .. } => e,
        MemoryError::Sqlite(_)
        | MemoryError::Pool(_)
        | MemoryError::Join(_)
        | MemoryError::Migration(_) => {
            tracing::error!(error = %e, "search failed with a storage error");
            MemoryError::Internal(SEARCH_STORAGE_FAILURE.to_string())
        }
    }
}
```
New body: `MemoryError::Internal(format!("{op} failed due to a storage error (details are in the server log)"))` — byte-identical to the constant when `op == "search"` (RESEARCH Pattern 8). Add a unit test pinning that equality.

**Public/unredacted split to apply to every method** (`service.rs:299-303`) — the inner-fn shape, because the bodies have multiple `?` exits:
```rust
    pub async fn search(&self, args: SearchArgs) -> Result<SearchOutcome, MemoryError> {
        self.search_unredacted(args)
            .await
            .map_err(redact_search_error)
    }
```
Apply to `store` 209-232, `list` 244-265, `import` 394-463, `forget` 573-578, `link` 592-604, `unlink` 610-620, `update` 634-666, `sweep` 688-744, and the two new methods.

**Current `spawn_blocking` hop idiom** (`service.rs:228-231`, every method uses it; `?` after `map_err(Join)` is doubled when the closure returns `Result`):
```rust
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.insert(new, embedding, now))
            .await
            .map_err(MemoryError::Join)?
```

**`import()` — the sibling `import_jsonl` copies this structure** (`service.rs:394-463`): one clock read, hop 1 dedup on the read pool, hop 3 inserts on the writer lane, counts assembled in one `ImportReport`:
```rust
    pub async fn import(&self, drafts: Vec<NewMemory>) -> Result<ImportReport, MemoryError> {
        for draft in &drafts {
            crate::domain::validate_ttl(draft.ttl_secs)?;
        }
        let now = self.clock.now();

        // (1) Partition new vs duplicate in one blocking hop (read pool).
        let store = self.store.clone();
        let (fresh, skipped_duplicates) =
            tokio::task::spawn_blocking(move || -> Result<(Vec<NewMemory>, usize), MemoryError> {
                let mut fresh = Vec::new();
                let mut duplicates = 0usize;
                for draft in drafts {
                    // Pass the source through as-is: a `None` must probe for
                    // NULL, exactly what the insert stores (CR-01).
                    if store.exists(draft.source.as_deref(), draft.mem_type, &draft.content)? {
                        duplicates += 1;
                    } else {
                        fresh.push(draft);
                    }
                }
                Ok((fresh, duplicates))
            })
            .await
            .map_err(MemoryError::Join)??;
        ...
        // (3) Insert every new draft through the standard store path.
        let store = self.store.clone();
        let imported = tokio::task::spawn_blocking(move || -> Result<usize, MemoryError> {
            let mut imported = 0usize;
            for (i, draft) in fresh.into_iter().enumerate() {
                let embedding = embeddings.as_ref().map(|v| v[i].clone());
                store.insert(draft, embedding, now)?;
                imported += 1;
            }
            Ok(imported)
        })
        .await
        .map_err(MemoryError::Join)??;

        Ok(ImportReport {
            imported,
            skipped_duplicates,
        })
    }
```
Differences for `import_jsonl` (D-08/D-13): **no step (2) embed call at all**; dedup and insert happen per record in the same pass so the `key → id` map is built in order (`find_by_key` → `Some(id)` maps, else `insert_imported`); a third hop walks links through `store.link(a, b, kind, record.created_at)`. Line 413 (`store.exists(...)`) becomes `.find_by_key(...)?.is_some()` here.

**`ImportReport` to extend additively** (`service.rs:159-168`) — `Copy + serde::Serialize`, doc comment per field; the two constructors at 425-428 and 459-462 are the only literals (RESEARCH Pitfall 11), so adding four `usize` fields with explicit `0` there breaks no test:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ImportReport {
    /// Drafts inserted as new memories.
    pub imported: usize,
    /// Drafts skipped because an identical `(source, mem_type, content)` row
    /// already exists — the idempotency guarantee.
    pub skipped_duplicates: usize,
}
```

**`sweep` backfill — the D-13 mechanism the import relies on, untouched** (`service.rs:698-701, 747-749`):
```rust
        let store = self.store.clone();
        let pending = tokio::task::spawn_blocking(move || store.pending_embeddings(BACKFILL_BATCH))
            .await
            .map_err(MemoryError::Join)??;
```
```rust
const BACKFILL_BATCH: usize = 128;
```

**Clock-skew warning + crate log idiom** (`service.rs:224`, `218-220`): `tracing::warn!("embedding failed ({e}); storing with embedding_status = 0");` — use the same `tracing::warn!` for the `exported_at > now + 3600` case (RESEARCH Open Question 2 recommends emitting it in core).

---

### `crates/agent-memory-core/src/store/mod.rs` (trait) — `find_by_key`, `insert_imported`, `export_rows`, `embedding_pin`

**Analog:** itself. Trait-method doc style: explain the invariant, name the review finding, state the NULL/edge semantics.

**`exists` doc to carry over to `find_by_key`** (`store/mod.rs:81-91`) — keep the CR-01 paragraph verbatim, change the return type sentence:
```rust
    /// The INTEROP-01 idempotency probe: does a row with exactly this
    /// `(source, mem_type, content)` key already exist? `MemoryService::import`
    /// checks it before every insert so re-importing the same file is a no-op.
    /// `source` is `Option` and compared NULL-safely: a `None` source matches a
    /// stored NULL source, never an empty string (CR-01).
    fn exists(
        &self,
        source: Option<&str>,
        mem_type: MemoryType,
        content: &str,
    ) -> Result<bool, MemoryError>;
```

**`insert` doc + signature to mirror for `insert_imported`** (`store/mod.rs:21-32`):
```rust
    /// Insert a new memory, returning its assigned id. `now` is the injected
    /// timestamp used for `created_at`/`last_accessed`. When `embedding` is
    /// `Some`, the memories INSERT, the `vec_memories` vector INSERT, and
    /// `embedding_status = 1` all happen in ONE writer transaction; when `None`,
    /// the row lands with `embedding_status = 0` (pending — backfilled by the
    /// sweep once the embedder recovers, SEARCH-03).
    fn insert(
        &self,
        new: NewMemory,
        embedding: Option<Vec<f32>>,
        now: i64,
    ) -> Result<i64, MemoryError>;
```
`insert_imported(&self, m: ImportedMemory, now: i64) -> Result<i64, MemoryError>` takes no embedding parameter — document "ALWAYS lands `embedding_status = 0` (D-13)".

**`link` doc — the idempotent `INSERT OR IGNORE` contract the links pass relies on** (`store/mod.rs:174-194`); its sentence "A duplicate of the stored row is an idempotent no-op that still returns `LinkOutcome::Linked`" must be reworded for `Linked { created: false }`:
```rust
    /// The row is stored under [`LinkKind::canonical_endpoints`]: `relates_to`
    /// as (smaller id, larger id), so linking b→a after a→b is the same
    /// symmetric relation (REVIEW WR-03); `supersedes` / `caused_by` as given
    /// (D-03 directed identity). A duplicate of the stored row is an idempotent
    /// no-op that still returns [`LinkOutcome::Linked`]. ...
    fn link(
        &self,
        from_id: i64,
        to_id: i64,
        kind: LinkKind,
        now: i64,
    ) -> Result<LinkOutcome, MemoryError>;
```

**Streaming read-pool method precedent** (`store/mod.rs:75-79`, `pending_embeddings`): returns a `Vec` today; `export_rows` should instead take a visitor closure (RESEARCH Pattern 4) — document "one pooled connection, one read transaction, memories ascending id then links" in the trait doc as Pitfall 5 asks.

**Expiry predicate to document the boundary against** (`store/mod.rs:219`): the sweep deletes `expires_at < now` (strict); import skips `expires_at <= now` (D-10). State both in `insert_imported`'s doc.

---

### `crates/agent-memory-core/src/store/sqlite.rs` (store impl) — `find_by_key`, `insert_imported`, `export_rows`, `embedding_pin`, `link { created }`

**Analog:** itself.

**Imports convention (already present at top of file):** `params!`, `OptionalExtension` (`.optional()` is used at 770 and 966), `Connection`, `Backup`. Nothing new needed.

**Writer-lane transaction shape** (`sqlite.rs:607-614, 642-643`) — every write starts exactly like this; `insert_imported` copies it:
```rust
        // ONE writer transaction covers the memories row, the vector row, and
        // the status flag, so `vec_memories` can never hold a vector for a row
        // that failed to insert (T-02-05).
        let mut conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        let tx = conn.transaction()?;
        ...
        tx.commit()?;
        Ok(id)
```

**The INSERT to share via a private `insert_row` helper** (`sqlite.rs:602-605, 615-632`) — note `?7` is bound once for both `created_at` and `last_accessed`; `insert_imported` binds them separately and binds `access_count` instead of literal `0`:
```rust
        let tags_json = serde_json::to_string(&new.tags).unwrap_or_else(|_| "[]".to_string());
        let base_weight = if new.mem_type.is_pinned() { 2.0 } else { 1.0 };
        let expires_at = new.ttl_secs.map(|ttl| now + ttl);
        let embedding_status: i64 = i64::from(embedding.is_some());
        ...
        tx.execute(
            "INSERT INTO memories \
             (mem_type, content, tags, source, scope, base_weight, decay_score, \
              access_count, created_at, last_accessed, expires_at, embedding_status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1.0, 0, ?7, ?7, ?8, ?9)",
            params![
                new.mem_type.as_wire_str(),
                new.content,
                tags_json,
                new.source,
                new.scope,
                base_weight,
                now,
                expires_at,
                embedding_status,
            ],
        )?;
        let id = tx.last_insert_rowid();
```

**`exists` → `find_by_key`** (`sqlite.rs:751-772`) — keep the comment and `IS ?1`; change `SELECT 1` to `SELECT id`, the row mapper to `|row| row.get::<_, i64>(0)`, and return `Ok(found)`:
```rust
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        // The INTEROP-01 dedup probe: all three key values bound, never
        // formatted into the SQL (T-02-20). `source IS ?1` is SQLite's
        // NULL-safe equality: a `None` source binds NULL and matches the NULL
        // the insert stored (`source = ?1` would be NULL = NULL → never true,
        // which made every source-less re-import a duplicate — CR-01).
        let found = conn
            .query_row(
                "SELECT 1 FROM memories \
                 WHERE source IS ?1 AND mem_type = ?2 AND content = ?3 LIMIT 1",
                params![source, mem_type.as_wire_str(), content],
                |_| Ok(()),
            )
            .optional()?;
        Ok(found.is_some())
```

**Read-pool `query_map` streaming shape for `export_rows`** (`sqlite.rs:735-749`) — same `prepare` + `query_map` + `for row in rows`, but call the visitor instead of `out.push`:
```rust
    fn pending_embeddings(&self, limit: usize) -> Result<Vec<(i64, String)>, MemoryError> {
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        let mut stmt = conn.prepare(
            "SELECT id, content FROM memories \
             WHERE embedding_status = 0 ORDER BY id LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
```
The memories SELECT column list to start from is `list`'s (`sqlite.rs:782-784`); export needs `id, mem_type, content, tags, source, scope, created_at, last_accessed, access_count, expires_at` (no `base_weight`/`decay_score`, D-04). Row mapping convention: `row_to_view` (`sqlite.rs:443-470`) — named columns via `row.get("col")`, tags via `serde_json::from_str(&tags_json).unwrap_or_default()`, and an invalid stored `mem_type` reported as `rusqlite::Error::FromSqlConversionFailure` (internal tier, never client). Export can write `mem_type` as the stored string directly since the record field is `String`.

**`link` — add `created`** (`sqlite.rs:982-989`): read `tx.changes()` *before* `commit`:
```rust
        let (stored_from, stored_to) = kind.canonical_endpoints(from_id, to_id);
        tx.execute(
            "INSERT OR IGNORE INTO memory_links (from_id, to_id, kind, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![stored_from, stored_to, kind.as_wire_str(), now],
        )?;
        tx.commit()?;
        Ok(LinkOutcome::Linked)
```
The links SELECT for export mirrors this column list: `SELECT from_id, to_id, kind, created_at FROM memory_links ORDER BY from_id, to_id, kind`.

**`meta` pin read for `embedding_pin`:** RESEARCH Code Example 5 (`conn.query_row("SELECT value FROM meta WHERE key = 'embedding_model'", [], |r| r.get(0))?`), on the read pool like `pending_embeddings`.

---

### `crates/agent-memory-core/src/domain.rs` (model) — `ImportedMemory`, `LinkOutcome::Linked { created }`

**Analog:** itself.

**`NewMemory` — the struct `ImportedMemory` sits beside and mirrors** (`domain.rs:240-253`); do NOT extend it (15 construction sites, RESEARCH Pattern 5):
```rust
/// Input shape for storing a new memory (D-05).
///
/// Only `content` and `mem_type` are conceptually required at the service boundary;
/// the remaining fields carry sensible defaults supplied by the caller
/// (`tags = []`, `source`/`scope` = `None`, `ttl_secs` = `None`).
#[derive(Debug, Clone)]
pub struct NewMemory {
    pub content: String,
    pub mem_type: MemoryType,
    pub tags: Vec<String>,
    pub source: Option<String>,
    pub scope: Option<String>,
    pub ttl_secs: Option<i64>,
}
```
`ImportedMemory` replaces `ttl_secs` with absolute `created_at: i64, access_count: i64, expires_at: Option<i64>` (RESEARCH Pattern 5 shape).

**`LinkOutcome` to change** (`domain.rs:173-183`) — `Linked` becomes `Linked { created: bool }`; keep `Copy`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOutcome {
    /// The edge exists now — freshly created, or already present (a duplicate
    /// link is an idempotent no-op success).
    Linked,
    /// No memory with `missing_id` exists; nothing was written.
    NotFound { missing_id: i64 },
}
```
Sites to update (grep-verified in RESEARCH 6b): `sqlite.rs:989`, `mcp.rs:438`, `handlers.rs:370`, `tests/relations.rs:139,149,791,794`. Transport matches become `LinkOutcome::Linked { .. } =>` — the wire JSON at `mcp.rs:438-443` is unchanged:
```rust
        let status = match outcome {
            LinkOutcome::Linked => serde_json::json!({
                "from_id": args.from_id,
                "to_id": args.to_id,
                "kind": kind,
                "linked": true
            }),
```

---

### `crates/agent-memory/src/main.rs` (CLI controller) — `Command::Export`, `jsonl` arm

**Analog:** itself.

**Subcommand declaration pattern** (`main.rs:80-97`) — clap derive, doc comments become `--help` text, `#[arg(long)]` for flags, positional `PathBuf`:
```rust
    /// Import memories from an external file (INTEROP-01). Re-running the same
    /// import is a no-op: duplicates are skipped on the exact
    /// (source, type, content) key.
    Import {
        /// Source format. Supported: gsd-state (a GSD .planning/STATE.md file).
        #[arg(long)]
        from: String,

        /// Path to the file to import.
        path: PathBuf,
        ...
    },
```
Add `Export { #[arg(long)] output: Option<PathBuf> }` and update the `from` doc to "Supported: gsd-state, jsonl".

**Dispatch pattern** (`main.rs:118-126`):
```rust
    match cli.command {
        Command::Serve => serve(cli.db, cli.ollama_url).await,
        Command::ServeRest { addr, allow_remote } => {
            serve_rest(cli.db, cli.ollama_url, addr, allow_remote).await
        }
        Command::Import { from, path, scope } => {
            run_import(cli.db, cli.ollama_url, from, path, scope).await
        }
    }
```

**One-shot subcommand body — `run_export` copies this wiring** (`main.rs:225-258`): `anyhow::Context` on every edge, `read_to_string`, open store, build service, print report to **stdout** (sanctioned for CLI subcommands per the doc at 221-224):
```rust
async fn run_import(
    db_flag: Option<PathBuf>,
    ollama_url: String,
    from: String,
    path: PathBuf,
    scope: Option<String>,
) -> anyhow::Result<()> {
    if from != "gsd-state" {
        anyhow::bail!("unsupported --from value {from:?} (supported formats: gsd-state)");
    }

    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let scope = scope.unwrap_or_else(|| default_scope_for(&path));
    let parsed = agent_memory_core::import::gsd_state::parse_gsd_state(&text, Some(scope));

    let db_path = resolve_db_path(db_flag).context("resolving database path")?;
    let store = SqliteStore::open(&db_path)
        .with_context(|| format!("opening database at {}", db_path.display()))?;
    let embedder: Arc<dyn Embedder> = Arc::new(OllamaClient::new(ollama_url));
    let service = MemoryService::new(
        Arc::new(store),
        Arc::new(SystemClock),
        embedder,
        DecayConfig::default(),
    );

    let report = service.import(parsed.drafts).await.context("importing")?;
    println!(
        "imported: {}, skipped duplicates: {}, malformed lines skipped: {}",
        report.imported, report.skipped_duplicates, parsed.skipped
    );
    Ok(())
}
```
The `if from != "gsd-state" { bail }` at 232-234 becomes a `match from.as_str() { "gsd-state" => ..., "jsonl" => ..., other => bail!(... "supported formats: gsd-state, jsonl") }`. **For `export` the report line must go to `eprintln!`** — stdout may be the file (RESEARCH Code Example 4). Keep the binary's `env!("CARGO_PKG_VERSION")` as the header version source (Pitfall 9).

---

### `crates/agent-memory/src/export_file.rs` (utility, file-I/O) — atomic 0600 write

**Analog A (platform split):** `crates/agent-memory/src/config.rs:37-54` — the one-seam `cfg(unix)` / `cfg(not(unix))` pair with the Windows-deferral comment; copy the structure for `create_private`:
```rust
/// Create `dir` (and parents) if missing, restricting it to the owner on Unix.
#[cfg(unix)]
fn create_data_dir(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if dir.exists() {
        return Ok(());
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// Non-Unix fallback (Windows support is deferred to v2 — DIST-03).
#[cfg(not(unix))]
fn create_data_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}
```
Also copy `config.rs:1-5`'s module-doc sentence pattern ("This is the ONE place that touches platform paths … Windows support is a single-file change").

**Analog B (temp naming + rename + cleanup discipline):** `crates/agent-memory-core/src/store/sqlite.rs:136-159, 265-291` — pid-qualified temp in the *same directory*, best-effort removal that never masks the original error, rename as the last step:
```rust
/// The in-flight temp file for this process's backup of `file_name`:
/// `{db}.backup-tmp-{NNNN}-{pid}`. ...
fn backup_temp_path(dir: &Path, file_name: &str) -> PathBuf {
    dir.join(format!(
        "{file_name}.backup-tmp-{LATEST_SCHEMA_VERSION:04}-{}",
        std::process::id()
    ))
}

/// Best-effort removal of a backup temp file and any SQLite sidecar it left.
/// Never fails: a leftover temp is harmless (T-04-32) and must not mask the
/// error that led here.
fn discard_backup_temp(temp: &Path) {
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let path = PathBuf::from(format!("{}{suffix}", temp.display()));
        if let Err(e) = std::fs::remove_file(&path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(temp = %path.display(), error = %e, "could not remove backup temp file");
            }
        }
    }
}
```
```rust
    let copied_version = match copy_into_temp(src, &temp_path) {
        Ok(version) => version,
        Err(what) => {
            discard_backup_temp(&temp_path);
            return Err(fail(what));
        }
    };
    ...
    if let Err(e) = std::fs::rename(&temp_path, &final_path) {
        discard_backup_temp(&temp_path);
        return Err(fail(format!(
            "cannot move {} into place as {}: {e}",
            temp_path.display(),
            final_path.display()
        )));
    }
```
These are private fns in core and specific to SQLite sidecars — **reuse the shape, not the code** (RESEARCH Pattern 7 verdict). The export helper needs only `""` (no sidecars) and `OpenOptions::new().write(true).create_new(true).mode(0o600)`.

**Unit-test style for the helper** (`config.rs:56-67`) — plain `#[test]` with `tempfile::tempdir()`; add a `#[cfg(unix)]` test asserting `PermissionsExt::mode() & 0o777 == 0o600` (Pitfall 12: this is where the coverage for the file helper comes from, not the spawned CLI test):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_takes_precedence_over_env_and_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let explicit = dir.path().join("explicit").join("memory.db");
        let resolved = resolve_db_path(Some(explicit.clone())).expect("resolve");
        assert_eq!(resolved, explicit);
        assert!(explicit.parent().expect("parent").exists());
    }
```

---

### `crates/agent-memory-core/tests/export_import.rs` (integration test)

**Analog A (harness + raw-connection helpers):** `crates/agent-memory-core/tests/relations.rs:20-102` — copy verbatim (`Harness`, `harness()`, `harness_with()`, `new_memory`, `store`, `raw_conn`, `edge_count`, `edges_touching`):
```rust
use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{ ... MemoryError, MemoryType, MemoryView, NewMemory, ... };
use agent_memory_core::embed::{Embedder, FakeEmbedder};
use agent_memory_core::service::{ListArgs, MemoryService, SearchArgs, SearchMode};
use agent_memory_core::store::sqlite::{register_vec_extension, SqliteStore};

struct Harness {
    service: MemoryService,
    clock: Arc<TestClock>,
    db_path: PathBuf,
    _dir: tempfile::TempDir,
}

fn harness() -> Harness {
    harness_with(Arc::new(FakeEmbedder::failing()))
}

fn harness_with(embedder: Arc<dyn Embedder>) -> Harness {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let store = SqliteStore::open(&db_path).expect("open store");
    let clock = Arc::new(TestClock::new(1_000));
    let service = MemoryService::new(
        Arc::new(store),
        clock.clone() as Arc<dyn Clock>,
        embedder,
        DecayConfig::default(),
    );
    Harness { service, clock, db_path, _dir: dir }
}

fn raw_conn(db_path: &Path) -> rusqlite::Connection {
    register_vec_extension().expect("vec0 registers");
    rusqlite::Connection::open(db_path).expect("open raw connection")
}

/// Edge rows touching `id` in either direction.
fn edges_touching(db_path: &Path, id: i64) -> i64 {
    raw_conn(db_path)
        .query_row(
            "SELECT count(*) FROM memory_links WHERE from_id = ?1 OR to_id = ?1",
            [id],
            |row| row.get(0),
        )
        .expect("count edges touching id")
}
```
Two harnesses (source + target DB) are needed for the round-trip — `harness()` is already a factory, call it twice. `TestClock` drives `exported_at`, the expiry fixture and the D-10 `last_accessed` refresh.

**Analog B (idempotent-import proof shape):** `tests/import.rs:34-99` — first import asserts counts, list-by-type counts, keyword search with a *failing* embedder proves fresh-machine recall, second import asserts `imported == 0` and `skipped_duplicates == first.imported`:
```rust
    // First import: everything lands, nothing deduped.
    let first = service.import(parsed.drafts).await.expect("first import");
    assert_eq!(first.imported, 9);
    assert_eq!(first.skipped_duplicates, 0);
    ...
    assert_eq!(outcome.search_mode, SearchMode::Keyword);
    ...
    // Idempotency (phase success criterion 4): a re-parse + re-import of the
    // SAME file imports nothing and reports every draft as a duplicate.
    let reparsed = parse_gsd_state(FIXTURE, Some("fixture".to_string()));
    let second = service.import(reparsed.drafts).await.expect("second import");
    assert_eq!(second.imported, 0, "re-run must import nothing");
    assert_eq!(second.skipped_duplicates, first.imported);
```
Extend `tests/import.rs` itself with one assertion that the gsd path reports zeros for the four new fields (RESEARCH test map row).

**Export-to-string helper for tests:** pass `Box::new(Vec::<u8>::new())` is not retrievable after the move — use a `Box<dyn Write + Send>` over an `Arc<Mutex<Vec<u8>>>` wrapper, or export to a `tempfile` path and `read_to_string` it (simplest; `tempfile` is a dev-dep of core, `Cargo.toml:32`).

---

### `crates/agent-memory-core/tests/redaction.rs` (fault-injection test)

**Analog:** `crates/agent-memory-core/tests/hybrid.rs:712-736` — the T-04-19 recipe: rename a table through a raw side connection, call the service, assert the exact `Internal` text and no `sqlite` substring:
```rust
#[tokio::test]
async fn a_broken_fts_index_in_keyword_mode_is_the_generic_internal_error() {
    let h = harness(Arc::new(FakeEmbedder::failing()));
    store(&h, "ledger reconciliation runbook", MemoryType::Pattern).await;
    raw_conn(&h.db_path)
        .execute_batch("ALTER TABLE memories_fts RENAME TO memories_fts_offline;")
        .expect("rename fts table");

    let err = h
        .service
        .search(query("ledger", None))
        .await
        .expect_err("a broken FTS index must fail keyword search");
    assert!(
        matches!(err, MemoryError::Internal(ref m) if m == SEARCH_STORAGE_FAILURE),
        "keyword mode must surface the generic internal error, got {err:?}"
    );
    assert!(
        !err.to_string().to_lowercase().contains("sqlite"),
        "no SQLite text may reach the client: {err}"
    );
}
```
For write paths rename `memories` (→ `store`, `list`, `update`, `forget`, `import`, `export`) or `memory_links` (→ `link`, `unlink`). **Use RENAME, never DROP** — `.claude/hooks/pre-bash-rust.sh` blocks shell text matching `drop table`. Note `hybrid.rs:729,760` pin `SEARCH_STORAGE_FAILURE` by equality — they must stay green after D-14.

---

### `crates/agent-memory/src/rest/handlers.rs` + `crates/agent-memory/src/mcp.rs` test modules (in-process redaction tests)

**Analog (REST):** `handlers.rs:851-889` — `test_state(&dir)` / `live_state(&dir)`, store one row, rename via raw connection on `dir.path().join("mem.db")`, call the handler fn directly, convert `Err` with `.into_response()`, assert 500 + body text:
```rust
    #[tokio::test]
    async fn search_storage_failure_is_500_without_sqlite_text() {
        for live in [true, false] {
            let dir = tempfile::tempdir().expect("tempdir");
            let state = if live { live_state(&dir) } else { test_state(&dir) };
            stored_id(&state, "ledger reconciliation runbook").await;
            agent_memory_core::store::sqlite::register_vec_extension().expect("register vec0");
            rusqlite::Connection::open(dir.path().join("mem.db"))
                .expect("open db")
                .execute_batch("ALTER TABLE memories_fts RENAME TO memories_fts_offline;")
                .expect("rename fts table");

            let resp = match search_handler(State(state), Json(search_request("ledger"))).await {
                Ok(_) => panic!("live={live}: a broken FTS index must fail the search"),
                Err(err) => err.into_response(),
            };
            assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR, "live={live}");
            let body = body_json(resp).await;
            let error = body["error"].as_str().expect("500 body carries an error");
            assert!(error.contains("search failed due to a storage error"), "live={live}: {body}");
            assert!(!error.to_lowercase().contains("sqlite"), "live={live}: no SQLite text may reach the client: {body}");
        }
    }
```
New tests: `health_handler(State(state))` after `ALTER TABLE memories RENAME TO memories_offline` → 500 with `"list failed due to a storage error"`; `store_handler` → `"store failed …"`. `/health`'s store ping is `service.list(limit 1)` (`handlers.rs:437-445`), which is why the op string is `list`.

**Analog (MCP):** `mcp.rs:1309-1344` — same recipe through `test_server(&dir)` / `live_server(&dir)`, asserting `err.code == McpError::internal_error("x", None).code` and `err.message` content.

**Mappers stay untouched** (`handlers.rs:167-182`, `mcp.rs:35-50`) — both match every `MemoryError` variant explicitly with no catch-all; D-14 adds no variant so they keep compiling.

---

### `crates/agent-memory/tests/cli.rs` (spawned-binary realism test)

**Analog:** `crates/agent-memory/tests/tools.rs:12-36` and `tests/rest.rs:18-50` — `env!("CARGO_BIN_EXE_agent-memory")`, `--db` on a tempdir, dead Ollama URL on the discard port:
```rust
fn binary_path() -> String {
    env!("CARGO_BIN_EXE_agent-memory").to_string()
}
...
        let mut child = Command::new(binary_path())
            .args(["serve", "--db"])
            .arg(db_path)
            // Deterministically dead Ollama URL: the binary is in keyword mode
            // on EVERY machine regardless of a locally running daemon
            // (SEARCH-03 — port 9 is the discard port, connection refused).
            .env("AGENT_MEMORY_OLLAMA_URL", "http://127.0.0.1:9")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn agent-memory serve");
```
Difference: `export`/`import` are one-shot, so use `Command::output()` (no `Child` lifecycle, no `Drop` reaper) and assert on `status.success()`, `stdout` (the JSONL for `export`, the counts line for `import`) and `stderr` (the export report line). Also assert `--from bogus` exits non-zero. These tests contribute zero coverage (`handlers.rs:456-460` comment) — keep all logic assertions in core tests.

---

### `README.md` (docs)

**Analog:** README 513-532 ("Import from GSD") — short bash block, a table, one paragraph on provenance and idempotency:
```markdown
## Import from GSD

Bootstrap your memory from an existing [GSD](...) project's state:

```bash
agent-memory import --from gsd-state .planning/STATE.md
```

| STATE.md section | Imported as |
|------------------|-------------|
| `### Decisions` bullets | `DECISION` |
...
Every imported memory carries `source = "gsd-state"`, the `gsd` tag, and a
scope (...). Re-running the same import is a **no-op**: unchanged items are
deduplicated on the exact (source, type, content) key and reported as skipped.
```
New "Export & import" section follows this shape with: the two commands, the format v1 header/memory/link spec, a D-10 timestamp-policy table, the pending-embedding note (cross-reference "Lifecycle: TTL & decay" at 534), the strict-vs-tolerant importer contrast, and the optional `VACUUM INTO` backup tip. **Widen README 460-462** from "A storage failure during search reports a generic internal error … `search failed due to a storage error`" to every service response (`<operation> failed due to a storage error`).

## Shared Patterns

### Error tiering at the service seam (D-14 / Phase 3 D-06)
**Source:** `crates/agent-memory-core/src/service.rs:46-60` (helper), `:299-303` (application), `crates/agent-memory-core/src/domain.rs:375-426` (the nine `MemoryError` variants).
**Apply to:** every public `MemoryService` method, both new methods, and the new parser (which only ever returns `InvalidArgument`).
Rule: client tier = `InvalidType | InvalidQuery | InvalidArgument` (pass through); crate text = `Internal | SchemaTooNew` (pass through); storage = `Sqlite | Pool | Join | Migration` (log at `error`, replace). Exhaustive match, no `_` arm.

### `spawn_blocking` hop + `map_err(MemoryError::Join)`
**Source:** `service.rs:228-231` (single `?`), `service.rs:406-422` (closure returning `Result`, double `??`).
**Apply to:** `export`, `import_jsonl` (two or three hops), any new store call from the service. Embed calls (none in this phase's import) stay *outside* `spawn_blocking`.

### Writer-lane transaction
**Source:** `sqlite.rs:610-614, 642` (`self.writer.lock().map_err(|_| Internal("writer lock poisoned"))`, `conn.transaction()?`, `tx.commit()?`).
**Apply to:** `insert_imported`; `link` already has it. Read-only work (`find_by_key`, `export_rows`, `embedding_pin`) uses `self.reads.get().map_err(MemoryError::Pool)?` (`sqlite.rs:736, 757, 775`).

### Parameterised SQL only
**Source:** every `params![...]` in `sqlite.rs` (e.g. 620-630, 767, 986); comments cite T-02-01/T-02-20.
**Apply to:** all new statements. Wire strings reach SQL only as `as_wire_str()` of a parsed enum (`sqlite.rs:621, 986`).

### Fault injection = RENAME, never DROP
**Source:** `tests/hybrid.rs:719-721`, `handlers.rs:863-867`, `mcp.rs:1322-1326`.
**Apply to:** `tests/redaction.rs`, new handler/MCP tests. Rename `memories` for store/list/update/forget/import/export/`/health`; rename `memory_links` for link/unlink.

### `cfg(unix)` / `cfg(not(unix))` one-seam split
**Source:** `crates/agent-memory/src/config.rs:37-54`.
**Apply to:** `export_file.rs` `create_private` (0600). Comment the non-unix arm with the DIST-03 / Phase 6 deferral like line 50.

### Temp-in-same-dir + rename + best-effort cleanup
**Source:** `sqlite.rs:140-145` (pid-suffixed name), `:150-159` (never-failing discard), `:265-291` (discard on every error path, rename last).
**Apply to:** `--output` handling in `main.rs` / `export_file.rs`.

### Test harness conventions
**Source:** `tests/relations.rs:33-61` (`Harness` with `TestClock::new(1_000)`, `FakeEmbedder::failing()` default), `tests/relations.rs:81-102` (raw-connection counters), `tests/import.rs:21-32` (`service_for(path)`).
**Apply to:** `tests/export_import.rs`, `tests/redaction.rs`. Dev-deps already present: `tempfile`, `tokio`, self-dep with `test-clock` (`crates/agent-memory-core/Cargo.toml:31-37`); binary crate also has `serde_json` + `rusqlite` as dev-deps (`crates/agent-memory/Cargo.toml:27-33`).

### Doc-comment conventions
**Source:** every trait method in `store/mod.rs` (invariant + review-finding tag, e.g. `(CR-01)`, `(D-02 / T-04-13)`); module docs in `gsd_state.rs:1-16`, `config.rs:1-5`.
**Apply to:** all new `pub` items (ecosystem rule: rustdoc on every public item). Cite the Phase 5 decision ids (D-03..D-14) the same way.

## No Analog Found

| File | Role | Data Flow | Reason / What to use instead |
|------|------|-----------|------------------------------|
| `MemoryService::export` + `Store::export_rows` (streaming boxed `Write` through `spawn_blocking`, visitor over one read snapshot) | service + store | streaming | Nothing in the repo streams rows to a writer; every read returns a `Vec` (`pending_embeddings` 735-749, `list` 774-803). Copy the read-pool `prepare`/`query_map` loop from `pending_embeddings` and the `spawn_blocking` idiom from `service.rs:228-231`; the `Box<dyn Write + Send>` sink, `serde_json::to_writer` + `b"\n"`, explicit `flush()`, and the single read transaction are RESEARCH Pattern 4 / Pitfall 8 (ASSUMED A1 on `unchecked_transaction`). |

## Metadata

**Analog search scope:** `crates/agent-memory-core/src/{service.rs, domain.rs, lib.rs, import/, store/}`, `crates/agent-memory-core/tests/{import.rs, relations.rs, hybrid.rs}`, `crates/agent-memory/src/{main.rs, config.rs, mcp.rs, rest/handlers.rs}`, `crates/agent-memory/tests/{tools.rs, rest.rs}`, `README.md`, both `Cargo.toml` dev-dep sections.
**Files scanned:** 17 (6 read whole; `service.rs`, `sqlite.rs`, `domain.rs`, `hybrid.rs`, `handlers.rs`, `mcp.rs`, `relations.rs`, `tools.rs`, `rest.rs`, `main.rs`, `README.md` read in targeted non-overlapping ranges).
**Pattern extraction date:** 2026-10-25
**Verified-against:** 05-RESEARCH.md line anchors; no divergence found between RESEARCH's `[VERIFIED]` quotes and the working tree.
