---
phase: 02-semantic-search-interop-release
reviewed: 2026-07-03T08:04:43Z
depth: standard
files_reviewed: 25
files_reviewed_list:
  - .github/workflows/release.yml
  - .github/workflows/spike-cross-compile.yml
  - crates/agent-memory-core/Cargo.toml
  - crates/agent-memory-core/sql/0002_embeddings.sql
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
  critical: 1
  warning: 9
  info: 7
  total: 17
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-07-03T08:04:43Z
**Depth:** standard
**Files Reviewed:** 25
**Status:** issues_found

## Summary

Reviewed the Phase 2 semantic-search / REST / GSD-import / release surface: the
embed seam (`embed/mod.rs`, `ollama.rs`), the service degrade logic
(`service.rs`), the sqlite-vec storage layer (`sqlite.rs`, migration 0002), the
STATE.md importer, both transports (`mcp.rs`, `rest/`), the integration tests,
and the two release workflows.

The architecture holds up well against its own threat model: all SQL is
parameterized (query vectors and FTS strings are bound, never formatted), the
SEARCH-03 degrade seam genuinely lives in the service, stdout purity is tested
end-to-end, the REST bind guard works, and vec0 trigger-blindness is handled
explicitly in `forget`/`sweep_expired`. No `unwrap()` in production code; no
secrets; workflows are pinned and self-hosted-only per CLAUDE.md.

However, adversarial tracing found one **Critical** correctness defect — the
FTS5 keyword fallback silently drops the `tag` filter that the semantic path
honors and both transports document — plus a cluster of robustness defects
around unvalidated integer extremes (`limit`, `ttl_secs`), misleading error
mapping (poisoned mutex → `NotFound`, all MCP errors → `invalid_params`,
malformed FTS5 query → REST 500 despite an explicit "never 500 for bad input"
contract), and an unimplemented drift guard: the `meta` model/dimension pin
created by migration 0002 is written once and never read by any code.

## Critical Issues

### CR-01: Keyword fallback search silently ignores the `tag` filter

**File:** `crates/agent-memory-core/src/store/sqlite.rs:384-413` (with `crates/agent-memory-core/src/service.rs:223-227`)
**Issue:** `SearchArgs` carries a `tag` filter that both transports document
("Filter by a tag substring" — `mcp.rs:79-80`, `rest/handlers.rs:56-57`) and
that the semantic path honors (`knn_search`, sqlite.rs:258 binds `args.tag`).
The FTS5 keyword `search` never references `args.tag`: the WHERE clause has
only `MATCH`, `mem_type`, and `scope` predicates, and the params list
(sqlite.rs:402-413) does not bind the tag at all. Consequence: the same
`memory_search`/`POST /api/search` call with a `tag` filter returns a
correctly filtered set when Ollama is up and an **unfiltered** superset when
Ollama is down — the filter is silently dropped in exactly the degraded mode
SEARCH-03 promises is behavior-identical apart from ranking. Callers using
tags to partition memories (e.g. the importer's `gsd`/`deferred` tags) get
rows they explicitly excluded, with no error and no signal.
**Fix:** Add the same predicate the other queries use to the keyword search
SQL and bind `args.tag`:
```rust
// sqlite.rs, fn search — add to the WHERE clause:
"  AND (?10 IS NULL OR m.tags LIKE '%' || ?10 || '%') \
..."
// and append to params![]:
args.tag,
```
Then add a regression test: store two rows with different tags, search in
keyword mode (dead embedder) with `tag` set, assert only the tagged row
returns (mirror the existing semantic-path coverage).

## Warnings

### WR-01: Unvalidated integer extremes: `limit` and `ttl_secs` overflow (panic in debug, wrap in release)

**File:** `crates/agent-memory-core/src/store/sqlite.rs:244` and `crates/agent-memory-core/src/store/sqlite.rs:196`
**Issue:** Both `limit` and `ttl_secs` are attacker/caller-controlled i64 values
arriving unvalidated from MCP tool arguments and the loopback REST API.
1. `knn_search`: `(args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).max(1) * 4)` —
   `limit = i64::MAX` overflows the multiplication: panic in debug builds; in
   release it wraps to a negative `k`, which is bound into the vec0 `k = ?2`
   constraint and fails the whole semantic search with an error instead of
   degrading (violating the "search never fails" posture for a valid-shaped
   request).
2. `insert`: `new.ttl_secs.map(|ttl| now + ttl)` — `ttl_secs = i64::MAX`
   overflows `now + ttl`: panic in debug; wraps negative in release, making the
   row appear already expired and silently deleted by the next sweep. A
   negative `ttl_secs` likewise produces an already-expired row with no
   validation error.
**Fix:** Use saturating arithmetic and clamp at the seam:
```rust
// knn_search:
let k = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT)
    .clamp(1, MAX_KNN_K)
    .saturating_mul(4)
    .min(MAX_KNN_K);
// insert:
let expires_at = new.ttl_secs.map(|ttl| now.saturating_add(ttl.max(0)));
```
and/or reject non-positive `ttl_secs`/`limit` with `InvalidType`-style 400s in
the transports.

### WR-02: Negative `limit` returns unbounded results on the keyword path (and diverges from semantic)

**File:** `crates/agent-memory-core/src/store/sqlite.rs:374,399` (with `crates/agent-memory-core/src/service.rs:168`)
**Issue:** SQLite treats a negative `LIMIT` as "no limit". `store.search` binds
`args.limit` directly into `LIMIT ?9`, so `limit: -1` bypasses the
`DEFAULT_SEARCH_LIMIT` cap (T-02-04) and returns the entire matching corpus.
Meanwhile the semantic path computes `limit.max(0) as usize` (service.rs:168)
and returns **zero** rows for the same negative value. Same request, two modes,
opposite extremes: everything vs nothing. `list` (sqlite.rs:350) deliberately
uses `-1` for "no limit", so a negative user limit there is also unbounded —
that at least matches the omitted-limit behavior, but for search it defeats the
documented cap.
**Fix:** Normalize once in `MemoryService::search` before either path runs:
```rust
let limit = args.limit.map(|l| l.clamp(0, DEFAULT_SEARCH_LIMIT_MAX));
```
or reject `limit < 0` as invalid params in both transports.

### WR-03: Poisoned writer mutex mapped to `MemoryError::NotFound` — internal failure masquerades as 404

**File:** `crates/agent-memory-core/src/store/sqlite.rs:202,279,429,445,466,487`
**Issue:** Every writer-lane method maps a poisoned `Mutex` to
`MemoryError::NotFound`. Grep confirms these six sites are the **only**
producers of `NotFound` in the entire codebase. Via
`rest/handlers.rs:89` this becomes an HTTP 404 "not found": after any panic
while holding the writer lock, a `POST /api/memories` (insert) would return
404, a delete would return the not-found body, and the true failure (a
poisoned lock — the store is permanently degraded for writes) is invisible.
This is semantically wrong error taxonomy in production code, and it also makes
the otherwise-unused `NotFound` variant load-bearing for the wrong condition.
**Fix:** Add a dedicated variant and map it as internal:
```rust
#[error("writer lock poisoned: a prior write panicked")]
LockPoisoned,
// sqlite.rs: .map_err(|_| MemoryError::LockPoisoned)?
// handlers.rs map_memory_error: LockPoisoned => ApiError::Internal(...)
```

### WR-04: MCP maps every service error to `invalid_params` — internal DB failures misreported as client errors

**File:** `crates/agent-memory/src/mcp.rs:138,168,203,222`
**Issue:** All four tools map any `MemoryError` — including `Sqlite`, `Pool`,
`Join`, `Migration` — to `McpError::invalid_params`. A connection-pool
exhaustion or SQLite I/O error is reported to the MCP client as "your
parameters were invalid", which will send agents into pointless
argument-repair loops. The REST layer's `map_memory_error`
(handlers.rs:86-95) does the correct two-tier split and its doc comment even
claims it "mirrors mcp.rs's invalid_params / internal_error split" — but
mcp.rs has no such split (its `internal_error` is used only for JSON
serialization failures). API-01's "both transports speak one dialect" is
violated at the error tier.
**Fix:** Introduce one shared mapping in mcp.rs mirroring
`map_memory_error`:
```rust
fn map_mcp_error(e: MemoryError) -> McpError {
    match &e {
        MemoryError::InvalidType(_) => McpError::invalid_params(e.to_string(), None),
        _ => McpError::internal_error(e.to_string(), None),
    }
}
```

### WR-05: Malformed FTS5 query returns HTTP 500, contradicting the REST layer's own "never 500 for bad input" contract

**File:** `crates/agent-memory/src/rest/handlers.rs:86-95` (with `crates/agent-memory-core/src/store/sqlite.rs:417-419`)
**Issue:** A syntactically invalid FTS5 MATCH string (e.g. `"` or `AND` alone)
surfaces from rusqlite as `MemoryError::Sqlite`, which `map_memory_error`
turns into `ApiError::Internal` → **500**. The module doc in `rest/mod.rs:30`
explicitly promises "bad input can never surface as a 500", and the sqlite.rs
comment (line 417-419) claims "the service maps to a clean invalid-params
error" — no such mapping exists anywhere. Worse, the failure is
mode-dependent: with Ollama up the same query string is only embedded (never
parsed by FTS5) and succeeds; with Ollama down it 500s. The MCP path only
avoids this because WR-04 wrongly maps *everything* to invalid_params.
**Fix:** Detect the FTS5 syntax error at the service or store seam and map it
to a client error, e.g. in `MemoryService::search`'s keyword arm:
```rust
Err(MemoryError::Sqlite(e)) if e.to_string().contains("fts5: syntax error") =>
    return Err(MemoryError::InvalidQuery(args.query)),
```
(add an `InvalidQuery` variant mapped to 400/invalid_params in both
transports), and add a REST test posting `{"query": "\""}` expecting 400.

### WR-06: The `meta` model/dimension pin is write-only — the promised drift detection does not exist

**File:** `crates/agent-memory-core/sql/0002_embeddings.sql:21-26`
**Issue:** Migration 0002 creates the `meta` table and inserts
`embedding_model = 'nomic-embed-text'` / `embedding_dim = '768'`, with the
comment "detectable drift instead of silent corpus corruption (RESEARCH
Pitfall 6). A different model/dim must degrade, never mix vectors." Grep across
both crates finds **zero** reads of this table — no code compares the pin
against `ollama.rs`'s `MODEL` constant or `EMBEDDING_DIM` at startup, store, or
search time. A dimension change would incidentally fail on the vec0
`FLOAT[768]` column, but a future *model* change with the same 768 dims (e.g.
a different nomic revision, or a user pointing `--ollama-url` at a daemon
serving a differently-trained `nomic-embed-text` build) silently mixes
incompatible vectors in one corpus — the exact Pitfall 6 failure the schema
claims to prevent. The guard is dead schema.
**Fix:** In `SqliteStore::open` (or a startup check in the binary), read
`meta` and compare against the compiled constants; on mismatch either refuse
semantic writes (degrade to keyword + pending, logging loudly) or require an
explicit re-embed migration. At minimum, document that the pin is currently
informational only.

### WR-07: Tag filter over-matches: raw substring over the JSON-encoded array with unescaped LIKE metacharacters

**File:** `crates/agent-memory-core/src/store/sqlite.rs:258,348`
**Issue:** Both `knn_search` and `list` filter tags with
`tags LIKE '%' || ? || '%'` against the JSON-serialized tags array. Two
defects: (1) substring semantics — filtering by tag `gsd` also matches rows
tagged `not-gsd` or `gsd-import`, and filtering by `e` matches nearly
everything; the import test only passes because no colliding tags exist in
the fixture. (2) The user-supplied tag is not escaped for LIKE, so `%` or `_`
in a tag filter act as wildcards (`tag: "%"` matches every row that has any
tags text — a filter-bypass primitive, harmless for confidentiality on a
single-user store but wrong results nonetheless). This is parameterized (no
injection), but the predicate is semantically loose.
**Fix:** Match the exact JSON-encoded element instead of a bare substring:
```rust
// bind the tag pre-encoded: format!("\"{}\"", tag) via serde_json::to_string(&tag)
"AND (?5 IS NULL OR m.tags LIKE '%' || ?5 || '%' ESCAPE '\\')"
```
binding `serde_json::to_string(&tag)` (which yields `"gsd"` with quotes) and
escaping `%`/`_`/`\` in the tag first — or normalize tags into a separate
`memory_tags(memory_id, tag)` table for exact-match filtering.

### WR-08: Sweep backfill can write a vector for a concurrently-forgotten memory

**File:** `crates/agent-memory-core/src/store/sqlite.rs:278-299` (with `crates/agent-memory-core/src/service.rs:367-390`)
**Issue:** `sweep` fetches pending ids (read pool), awaits an embed HTTP call
(up to 10s), then writes vectors back one id at a time. If `forget(id)` runs
in that window (fully supported — cross-process MCP + REST daemons share the
DB by design), `insert_embedding` unconditionally INSERTs into `vec_memories`
and UPDATEs a now-nonexistent `memories` row: the UPDATE affects 0 rows and
the vector row becomes an orphan — `vec_memories` drifts from `memories`,
which T-02-05 declares must never happen. The orphan cannot resurface in
results (the KNN JOIN filters it) and the next `sweep_expired` NOT-IN cleanup
removes it, but until then it occupies KNN candidate slots (reducing recall
within the `k` budget) and briefly violates the stated invariant.
**Fix:** In `insert_embedding`, run the status UPDATE first and skip the
vector insert when no row changed:
```rust
let changed = tx.execute("UPDATE memories SET embedding_status = 1 WHERE id = ?1", params![memory_id])?;
if changed == 0 { return Ok(()); } // row was forgotten; nothing to embed
```

### WR-09: stdio-purity test asserts on log strings this codebase never emits (rmcp-internal text)

**File:** `crates/agent-memory/tests/stdio_purity.rs:69-72`
**Issue:** The final assertion requires stderr to contain
`"Service initialized"` or `"Database migrated"`. Grep confirms neither string
exists anywhere in this repository's source — the test currently passes only
because the rmcp crate internally logs "Service initialized as server" at
info level. Any rmcp upgrade that rewords, re-levels, or removes that internal
log line breaks the highest-priority correctness gate for a reason unrelated
to stdout purity, and a `RUST_LOG` filter excluding rmcp's target would do the
same. This is a test-reliability defect in the MCP-05 gate.
**Fix:** Emit an owned, stable log line from `serve()` (e.g.
`tracing::info!("agent-memory serve started")` after store open) and assert on
that instead — the assertion then tests this binary's contract, not a
dependency's internals.

## Info

### IN-01: Catch-all `_`/`other` match arms in the search degrade path

**File:** `crates/agent-memory-core/src/service.rs:212-222`
**Issue:** The fallback arm binds `other` and then matches `Err(e)` vs `_`,
using the catch-all the project's Rust rules say to avoid ("pattern match
exhaustively — no catch-all `_` unless truly needed"). The `_` here silently
covers `Ok(vectors)`-empty; an explicit `Ok(_)` arm would be self-documenting
and future-proof against new variants.
**Fix:** Replace `_ =>` with `Ok(_) =>` and match `Ok/Err` exhaustively at the
top level instead of the `Ok(v) if !v.is_empty()` guard + catch-all shape.

### IN-02: Deferred-table row with empty cells imports as `" — "` garbage; the emptiness check is dead code

**File:** `crates/agent-memory-core/src/import/gsd_state.rs:152-156`
**Issue:** `content = format!("{} — {}", cells[1], cells[2])` always contains
`" — "`, so `content.is_empty()` on line 153 can never be true (dead check).
A row like `| | | |` passes `cells.len() >= 3` and imports a `" — "` TODO.
**Fix:** Skip when both cells are empty: `if cells[1].is_empty() && cells[2].is_empty() { skipped += 1; continue; }` and drop the unreachable `is_empty` check.

### IN-03: Import dedup is check-then-insert with no unique index — concurrent imports can duplicate

**File:** `crates/agent-memory-core/src/service.rs:262-313`
**Issue:** `import` probes `Store::exists` in one blocking hop, awaits an embed
call, then inserts in a separate hop. Two concurrent `import` runs (two CLI
invocations, or CLI + REST-triggered flow — cross-process access is a
supported design point) can both pass the exists check and double-insert.
Single-user likelihood is low, but the idempotency key has no DB-level
enforcement.
**Fix:** Add a unique index on `(source, mem_type, content)` (or a content
hash) and use `INSERT OR IGNORE` for imports, counting changes.

### IN-04: No content-size cap on the direct store paths, unlike import's 8KB cap

**File:** `crates/agent-memory/src/mcp.rs:33-50`, `crates/agent-memory/src/rest/handlers.rs:27-45`
**Issue:** The importer enforces `MAX_IMPORT_CONTENT_BYTES = 8192` (T-02-22,
"one runaway line must not bloat the store"), but `memory_store` / `POST
/api/memories` accept unbounded `content` (REST is limited only by axum's ~2MB
default body cap; MCP stdio has no cap). Every oversized store also becomes an
embed request body. Inconsistent hardening across ingress paths.
**Fix:** Apply a shared content cap (or a much larger explicit one) in
`MemoryService::store` so all transports inherit it.

### IN-05: Zig-install and smoke-test steps are copy-pasted between the two workflows

**File:** `.github/workflows/release.yml:125-172`, `.github/workflows/spike-cross-compile.yml:53-99`
**Issue:** The pinned zig/cargo-zigbuild install script and the
host-arch-aware smoke test are duplicated verbatim. The spike exists to
de-risk the release, so any future edit applied to one and not the other
silently invalidates the gate (the spike would prove a different build than
the release runs).
**Fix:** Extract a composite action (`.github/actions/zigbuild-setup`) or a
reusable workflow both call, keeping the version pins in one place.

### IN-06: Ollama base URL not normalized; model presence check is prefix-based

**File:** `crates/agent-memory-core/src/embed/ollama.rs:55-60,128`
**Issue:** (1) `OllamaClient::new` accepts the URL verbatim; a trailing slash
in `--ollama-url` yields `http://host:11434//api/embed` (most servers
tolerate it, but it's avoidable noise). (2) `health` reports `Ready` when any
model name `starts_with("nomic-embed-text")` — a daemon with only
`nomic-embed-text-v2` (a different model) would probe Ready while `/api/embed`
requests for `nomic-embed-text` fail.
**Fix:** `base_url.trim_end_matches('/')` in the constructor; match the model
as `name == MODEL || name.starts_with("nomic-embed-text:")` (tag-suffix only).

### IN-07: Fire-and-forget recency bump swallows errors with no log

**File:** `crates/agent-memory-core/src/service.rs:329-331`
**Issue:** `spawn_bump` discards the `bump_access` result with `let _ =`. The
design intent (a bump failure must not fail search) is right, but a persistent
writer-lane failure would silently stop all recency updates — decay ranking
degrades with zero observability, contradicting the codebase's own
"loud, not silent" degradation rule (Pitfall 4).
**Fix:**
```rust
if let Err(e) = store.bump_access(&ids, now) {
    tracing::warn!("recency bump failed ({e}); ranking may staleness-drift");
}
```

---

_Reviewed: 2026-07-03T08:04:43Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
