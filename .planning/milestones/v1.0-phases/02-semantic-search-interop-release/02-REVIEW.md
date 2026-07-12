---
phase: 02-semantic-search-interop-release
reviewed: 2026-07-03T12:02:47Z
depth: standard
files_reviewed: 26
files_reviewed_list:
  - .github/workflows/release.yml
  - .github/workflows/spike-cross-compile.yml
  - crates/agent-memory-core/Cargo.toml
  - crates/agent-memory-core/sql/0002_embeddings.sql
  - crates/agent-memory-core/src/domain.rs
  - crates/agent-memory-core/src/embed/mod.rs
  - crates/agent-memory-core/src/embed/ollama.rs
  - crates/agent-memory-core/src/import/gsd_state.rs
  - crates/agent-memory-core/src/import/mod.rs
  - crates/agent-memory-core/src/lib.rs
  - crates/agent-memory-core/src/service.rs
  - crates/agent-memory-core/src/store/migrations.rs
  - crates/agent-memory-core/src/store/mod.rs
  - crates/agent-memory-core/src/store/sqlite.rs
  - crates/agent-memory-core/tests/fallback.rs
  - crates/agent-memory-core/tests/fixtures/STATE.md
  - crates/agent-memory-core/tests/import.rs
  - crates/agent-memory-core/tests/semantic.rs
  - crates/agent-memory/Cargo.toml
  - crates/agent-memory/src/main.rs
  - crates/agent-memory/src/mcp.rs
  - crates/agent-memory/src/rest/handlers.rs
  - crates/agent-memory/src/rest/mod.rs
  - crates/agent-memory/tests/rest.rs
  - crates/agent-memory/tests/stdio_purity.rs
  - crates/agent-memory/tests/tools.rs
findings:
  critical: 0
  warning: 6
  info: 5
  total: 11
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-07-03T12:02:47Z
**Depth:** standard
**Files Reviewed:** 26
**Status:** issues_found

## Summary

Fresh full re-review of the Phase 2 semantic-search / interop / release surface after the 02-05 gap-closure commits (39be828, e491443). The three prior findings are verifiably closed: the keyword fallback now honors the tag filter (regression test `keyword_fallback_honors_tag_filter` in `tests/fallback.rs`), MCP errors carry the two-tier `invalid_params`/`internal_error` split (`map_mcp_error` in `mcp.rs` with a unit test), and a malformed FTS5 query maps to `InvalidQuery` → REST 400 / MCP `invalid_params` at the store seam (`map_fts_query_error` in `sqlite.rs`, covered end-to-end in `tests/rest.rs` and in-process in `handlers.rs`).

Verified during this review: `cargo clippy --workspace --all-targets -- -D warnings` passes clean; every action major pinned in `release.yml`/`spike-cross-compile.yml` (`checkout@v5`, `upload-artifact@v5`, `download-artifact@v5`, `softprops/action-gh-release@v3`, `dtolnay/rust-toolchain@stable`) exists upstream. No `unwrap()` in production paths; all SQL is parameterized (no injection surface found); stdout purity for `serve` holds by construction and under test; the loopback-bind guard for the unauthenticated REST API is correct and tested.

No blockers found. Six warnings remain, clustered in two themes: (1) unvalidated client-controlled numeric inputs (`limit`, `ttl_secs`) that overflow or bypass caps at boundary values, and (2) filter/dedup predicates whose SQL semantics diverge from their documented intent (LIKE-substring tag matching, NULL-source dedup, in-batch duplicates). None is release-blocking for a single-user local tool, but all degrade correctness at edges an agent can reach.

## Narrative Findings (AI reviewer)

## Warnings

### WR-01: Tag filter is a raw LIKE substring over JSON text — false positives and user-controlled wildcards

**File:** `crates/agent-memory-core/src/store/sqlite.rs:279` (also `:369` in `list`, `:414` in `search`)
**Issue:** All three tag predicates use `tags LIKE '%' || ?tag || '%'` against the serialized JSON array (e.g. `["gsd","deferred"]`). Two consequences:
1. **Substring false positives:** filtering by tag `gsd` also matches a row tagged `gsd-import` or `my-gsd-fork`; filtering by tag `a` matches nearly every tagged row. `tests/import.rs:70` (`count(None, Some("gsd")) == 9`) passes only because no colliding tag exists in the fixture.
2. **LIKE metacharacters are live:** a tag value of `%` or `_` (client-supplied via the MCP/REST `tag` field) acts as a wildcard — `tag: "%"` matches every row with a non-empty tags column. Not an injection (it is bound), but the filter semantics are client-manipulable in a way the docstring ("Filter by a tag substring") does not intend for `%`/`_`.
**Fix:** Anchor the match on the JSON quoting and escape LIKE metacharacters:
```sql
AND (?5 IS NULL OR m.tags LIKE '%"' || ?5 || '"%' ESCAPE '\')
```
with `%`/`_`/`\` escaped in Rust before binding — or, more robustly, use an `EXISTS (SELECT 1 FROM json_each(m.tags) WHERE json_each.value = ?5)` subquery for exact tag equality. Apply identically in `list`, `search`, and `knn_search` so all three code paths agree.

### WR-02: Negative `limit` bypasses the search cap in keyword mode and diverges from semantic mode

**File:** `crates/agent-memory-core/src/store/sqlite.rs:395,421` and `crates/agent-memory-core/src/service.rs:168`
**Issue:** `limit` is a client-supplied `Option<i64>` with no validation at either transport (`mcp.rs` `SearchArgs`/`ListArgs`, `rest/handlers.rs` DTOs). For `limit: -1`:
- **Keyword path:** `sqlite.rs:395` passes it straight into `LIMIT ?9`; SQLite treats a negative LIMIT as *no limit*, so the `DEFAULT_SEARCH_LIMIT` / T-02-04 cap is bypassed and every FTS5 match is returned (and every returned id then gets a recency bump).
- **Semantic path:** `service.rs:168` computes `args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).max(0) as usize` → truncates to **0 results**.
The same request therefore returns everything or nothing depending on whether Ollama is up — the exact class of semantic/keyword divergence the SEARCH-03 seam exists to prevent. `list` (`sqlite.rs:371`) has the same negative-limit-means-unbounded behavior.
**Fix:** Clamp once at the service seam and use the clamped value on both paths:
```rust
let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).clamp(0, DEFAULT_SEARCH_LIMIT);
```
(or reject `limit < 0` as client input → 400/`invalid_params`), then pass the sanitized value into `store.search`/`store.list` instead of the raw arg.

### WR-03: Client-controlled i64 boundary values overflow: `limit * 4` (knn k) and `now + ttl_secs` (expires_at)

**File:** `crates/agent-memory-core/src/store/sqlite.rs:265` and `crates/agent-memory-core/src/store/sqlite.rs:217`
**Issue:** Two unchecked arithmetic sites on client-supplied values:
1. `sqlite.rs:265` — `(args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).max(1) * 4).min(MAX_KNN_K)`. A search with `limit: 9223372036854775807` (valid JSON via MCP or REST) overflows: debug builds panic inside `spawn_blocking` (surfaces as `MemoryError::Join` → 500); release builds wrap to `-4`, which `.min(200)` keeps, binding a negative `k` to vec0 → SQL error → 500. Client input producing a 500 is exactly the taxonomy the WR-05/API-01 fix just eliminated for FTS5 strings.
2. `sqlite.rs:217` — `new.ttl_secs.map(|ttl| now + ttl)`. `ttl_secs` near `i64::MAX` overflows: debug panic (→ Join error → 500); release wraps `expires_at` negative, so a memory stored with a huge TTL is silently deleted by the first sweep — the opposite of the caller's intent (data-loss flavor for that row).
**Fix:** Use saturating arithmetic at both sites:
```rust
let k = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).clamp(1, MAX_KNN_K / 4).saturating_mul(4);
let expires_at = new.ttl_secs.map(|ttl| now.saturating_add(ttl.max(0)));
```
and/or reject non-positive `ttl_secs` / absurd `limit` as client errors at the transport seam.

### WR-04: Poisoned writer mutex maps to `MemoryError::NotFound` — REST reports 404 for internal failures

**File:** `crates/agent-memory-core/src/store/sqlite.rs:223` (also `:300`, `:455`, `:471`, `:492`, `:513`) and `crates/agent-memory/src/rest/handlers.rs:92`
**Issue:** Every writer-lane method maps a poisoned mutex to `NotFound`: `self.writer.lock().map_err(|_| MemoryError::NotFound)?`. `mcp.rs:28-30` explicitly documents that `NotFound`-as-error "only arises from internal store conditions (e.g. a poisoned writer mutex)" and correctly maps it to `internal_error` — but `rest/handlers.rs:92` maps `MemoryError::NotFound => ApiError::NotFound` (404). Since `forget` reports a genuinely missing row as `Ok(false)` (never `Err(NotFound)`), the REST 404 arm is reachable **only** by the poisoned-mutex path: after any panic on the writer lane, `POST /api/memories` returns `404 not found` for a create, and every subsequent write misreports an internal fault as a missing resource. The two transports also now disagree (MCP 500-class vs REST 404) for the identical error.
**Fix:** Add a dedicated variant and map it as internal on both transports:
```rust
#[error("internal error: writer lock poisoned")]
LockPoisoned,
```
`sqlite.rs`: `.map_err(|_| MemoryError::LockPoisoned)?`; `rest/handlers.rs` and `mcp.rs` map it to 500/`internal_error`. The `ApiError::NotFound` arm then becomes genuinely dead and can be removed or reserved for future id-addressed reads.

### WR-05: `exists()` never matches NULL-source rows — dedup silently broken for `source: None` drafts

**File:** `crates/agent-memory-core/src/store/sqlite.rs:349` and `crates/agent-memory-core/src/service.rs:268`
**Issue:** `insert` binds `new.source` as an `Option` — `None` persists as SQL `NULL`. But `service.rs:268` dedups with `draft.source.as_deref().unwrap_or("")` and `exists()` runs `WHERE source = ?1` with `''`; in SQL, `NULL = ''` evaluates to NULL, never true. Consequence: any `NewMemory` with `source: None` passed to the public `MemoryService::import` API is **never** detected as a duplicate — re-importing inserts a fresh copy every time. The gsd-state importer always stamps `source = Some("gsd-state")`, so the shipped CLI path is unaffected, but the invariant documented on `Store::exists` ("re-importing the same file is a no-op") does not hold for the general API, and the next importer that omits `source` inherits a silent idempotency break.
**Fix:** Make the comparison NULL-aware — bind the Option and use SQLite's `IS`:
```sql
WHERE source IS ?1 AND mem_type = ?2 AND content = ?3
```
with `exists(&self, source: Option<&str>, ...)` — or normalize by always storing `''` instead of NULL for absent sources.

### WR-06: `import` does not dedup within a batch — identical drafts in one file insert duplicate rows

**File:** `crates/agent-memory-core/src/service.rs:262-278`
**Issue:** Step (1) checks every draft against the **store** before any insert happens, so two identical bullets inside one STATE.md (same section, same text — plausible in a long-lived hand-edited file) both pass `exists()` and both insert in step (3). The store then permanently holds duplicates on the exact `(source, mem_type, content)` key the idempotency contract is defined over; a re-import correctly skips both, but the corpus is already duplicated and both copies surface in search/list. The `tests/import.rs` fixture has no repeated bullet, so this path is untested.
**Fix:** Dedup the fresh set in the same blocking hop:
```rust
let mut seen = std::collections::HashSet::new();
for draft in drafts {
    let key = (draft.source.clone(), draft.mem_type, draft.content.clone());
    if !seen.insert(key) { duplicates += 1; continue; }
    // existing exists() check ...
}
```
counting in-batch repeats as `skipped_duplicates`.

## Info

### IN-01: The `meta` model/dimension pin is written but never read — advertised drift detection is not implemented

**File:** `crates/agent-memory-core/sql/0002_embeddings.sql:21-26`
**Issue:** The migration inserts `embedding_model`/`embedding_dim` rows under the comment "detectable drift instead of silent corpus corruption... A different model/dim must degrade, never mix vectors" — but no code anywhere reads the `meta` table. The dimension is guarded per-response in `ollama.rs` and the model name is a compile-time constant, so today's binary cannot drift; the table is future-proofing presented as an active guard.
**Fix:** Either wire a startup check in `SqliteStore::open` (compare `meta` rows against `MODEL`/`EMBEDDING_DIM`, degrade to keyword mode with one loud warning on mismatch), or soften the SQL comment to say the pin is recorded for future versions to check.

### IN-02: `stdio_purity` asserts on an rmcp-internal log string and inherits the parent `RUST_LOG`

**File:** `crates/agent-memory/tests/stdio_purity.rs:69-72`
**Issue:** The stderr assertion `stderr.contains("Service initialized") || stderr.contains("Database migrated")` is doubly fragile: (a) "Database migrated" matches nothing in this codebase — the only source of "Service initialized" is rmcp 1.8.0's internal `tracing::info!("Service initialized as server")` (verified in the vendored crate), so an rmcp upgrade that rewords its log breaks the test; (b) the spawned child inherits the test runner's environment, so `RUST_LOG=warn cargo test` suppresses the info line and fails the test spuriously.
**Fix:** Emit one first-party info line in `serve()` (e.g. `tracing::info!("agent-memory serving MCP stdio")`), assert on that, and pin the child's filter with `.env("RUST_LOG", "info")` in the test's `Command`.

### IN-03: `memory_list` with no `limit` returns the entire corpus unbounded

**File:** `crates/agent-memory-core/src/store/sqlite.rs:371` and `crates/agent-memory/src/mcp.rs:164`
**Issue:** Search applies `DEFAULT_SEARCH_LIMIT = 50` when `limit` is omitted, but `list` uses `LIMIT CASE WHEN ?4 IS NULL THEN -1 ELSE ?4 END` — omission means all rows. An agent calling `memory_list` with default arguments on a mature corpus gets an arbitrarily large JSON payload injected into its context window.
**Fix:** Apply a default cap when `limit` is `None` (the same `DEFAULT_SEARCH_LIMIT`, or a larger `DEFAULT_LIST_LIMIT`), and document that omission means "capped default", not "everything".

### IN-04: Import embeds the whole fresh batch in one HTTP call — unbounded, unlike the 128-cap backfill

**File:** `crates/agent-memory-core/src/service.rs:288-299`
**Issue:** `sweep` deliberately bounds backfill to `BACKFILL_BATCH = 128` per tick (T-02-03), but `import` sends every fresh draft's content in a single `/api/embed` request. A large STATE.md (thousands of ≤8KB bullets) produces one multi-megabyte request that may exceed Ollama's limits or the 10s timeout. Failure degrades gracefully (all rows land pending; the sweep backfills at 128/tick), so this is resilience-by-fallback rather than a bug.
**Fix:** Chunk the import embed loop at `BACKFILL_BATCH` for symmetry, so a big first import gets vectors immediately instead of pending-then-backfilled over many hourly sweeps.

### IN-05: Backfill/delete race can write an orphan `vec_memories` row (self-healing next sweep)

**File:** `crates/agent-memory-core/src/service.rs:368-388` and `crates/agent-memory-core/src/store/sqlite.rs:299-320`
**Issue:** `sweep` fetches pending ids, then embeds (async, potentially seconds), then writes via `insert_embedding`. A `forget()` (or TTL expiry in a concurrent process sharing the DB file) landing between fetch and write re-inserts a vector for a now-deleted memory — vec0 has no FK enforcement. The orphan cannot resurface in results (the KNN JOIN filters it) and the next `sweep_expired`'s `NOT IN (SELECT id FROM memories)` cleanup removes it, so the window is benign but violates the "vec_memories never drifts from memories" (T-02-05) invariant for up to one sweep interval.
**Fix:** Guard the write inside the existing `insert_embedding` transaction: check `SELECT 1 FROM memories WHERE id = ?1` first and skip (without flipping `embedding_status`) when the row is gone.

---

_Reviewed: 2026-07-03T12:02:47Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
