---
phase: 03-api-hardening-toolchain-spikes
reviewed: 2026-07-12T14:46:10Z
depth: standard
files_reviewed: 12
files_reviewed_list:
  - .github/workflows/ci.yml
  - .github/workflows/spike-cross-compile.yml
  - crates/agent-memory-core/src/domain.rs
  - crates/agent-memory-core/src/service.rs
  - crates/agent-memory-core/src/store/sqlite.rs
  - crates/agent-memory-core/tests/fallback.rs
  - crates/agent-memory-core/tests/semantic.rs
  - crates/agent-memory-core/tests/store.rs
  - crates/agent-memory-core/tests/validation.rs
  - crates/agent-memory/src/mcp.rs
  - crates/agent-memory/src/rest/handlers.rs
  - crates/agent-memory/tests/rest.rs
findings:
  critical: 1
  warning: 3
  info: 4
  total: 8
status: issues_found
---

# Phase 3: Code Review Report

**Reviewed:** 2026-07-12T14:46:10Z
**Depth:** standard
**Files Reviewed:** 12
**Status:** issues_found

## Summary

Reviewed the Phase 3 API-hardening diff (input validation at the `MemoryService` seam, exact `json_each` tag matching, the shared MCP/REST error-tier mapping, revived ubuntu-latest CI, and the 4-leg cross-compile spike) plus the tests added at each site. Cross-referenced against adjacent files not in the diff (`store/mod.rs` trait, `store/migrations.rs` + both SQL migrations, `rest/mod.rs` `ApiError`, `decay.rs`, `embed/ollama.rs`, `import/gsd_state.rs`) to verify claims rather than take doc comments at face value.

The phase's headline work is solid: `validate_limit`/`validate_ttl` are correctly shared across `store`/`list`/`search`/`import`, the boundary matrix in `tests/validation.rs` covers both invalid values and valid boundaries (1/200, 1/3_155_760_000), all SQL is parameterized, the tag filter is provably exact/case-sensitive at all three query sites, and no `unwrap()`/debug artifacts exist in production code (all hits are inside `#[cfg(test)]` modules).

However, the review found one data-integrity defect in the import idempotency probe (NULL-vs-empty-string mismatch that silently duplicates rows), a filtered-semantic-search recall gap that the phase's own tag-filter tests cannot detect (2-row corpora), and a misleading error-tier mapping for writer-mutex poisoning that surfaces an internal failure as REST 404.

## Critical Issues

### CR-01: `import()` idempotency is broken for `source: None` drafts — NULL/empty-string mismatch between the dedup probe and the insert

**File:** `crates/agent-memory-core/src/service.rs:285` and `crates/agent-memory-core/src/store/sqlite.rs:344-362`
**Issue:** The documented INTEROP-01 contract ("an unchanged re-import creates no duplicates") does not hold for drafts with `source: None`. The dedup probe coerces `None` to the empty string:

```rust
// service.rs:285
let source = draft.source.as_deref().unwrap_or("");
if store.exists(source, draft.mem_type, &draft.content)? {
```

but `Store::insert` (sqlite.rs:237) binds `new.source` as `Option<String>`, so a `None` source persists as SQL **NULL** (schema: `source TEXT -- NULL allowed`, `sql/0001_init.sql:11`). The probe then runs `WHERE source = ?1` with `''` bound (sqlite.rs:356) — and in SQLite `NULL = ''` evaluates to NULL, never true. So a `source: None` draft is **never** found by `exists()`, and every re-import inserts a fresh duplicate row.

The bundled GSD importer happens to always set `source = Some("gsd-state")` (`import/gsd_state.rs:182`), which is why the existing idempotency tests pass — but `MemoryService::import` is a public library API, `NewMemory.source` is a public `Option`, and this phase's own comment (service.rs:270-272) says future import surfaces (Phase 5 JSONL) "inherit the seam structurally." They will inherit this silent-duplication bug too. A secondary asymmetry: a `Some("")` draft would cross-dedup against `''` rows while `None` drafts never dedup against anything, so the same logical key maps to two different physical keys.

**Fix:** Make the probe NULL-aware end-to-end — pass the `Option` through and use SQLite's null-safe `IS` comparison:

```rust
// store/mod.rs — trait
fn exists(&self, source: Option<&str>, mem_type: MemoryType, content: &str)
    -> Result<bool, MemoryError>;

// sqlite.rs — `IS` is null-safe equality in SQLite and works with bound params
"SELECT 1 FROM memories \
 WHERE source IS ?1 AND mem_type = ?2 AND content = ?3 LIMIT 1"

// service.rs
if store.exists(draft.source.as_deref(), draft.mem_type, &draft.content)? {
```

Add a regression test: import the same `source: None` draft twice and assert `skipped_duplicates == 1` on the second run.

## Warnings

### WR-01: Filtered semantic search silently loses matching rows — filters are applied AFTER the k-nearest cut, and the oversample factor collapses to 1 at `limit=200`

**File:** `crates/agent-memory-core/src/store/sqlite.rs:259-303` (with `sqlite.rs:35`, `domain.rs:122`)
**Issue:** `knn_search` selects the **global** k-nearest vectors first, then applies the type/scope/tag filters to that candidate set:

```sql
WITH knn AS (SELECT memory_id, distance FROM vec_memories
             WHERE embedding MATCH ?1 AND k = ?2)
SELECT ... FROM knn JOIN memories m ON m.id = knn.memory_id
WHERE (?3 IS NULL OR m.mem_type = ?3) AND ...
```

With `k` hard-capped at `MAX_KNN_K = MAX_LIMIT = 200`, any corpus larger than 200 rows can produce a tag/scope/type-filtered semantic search that returns **empty or partial results even though matching rows exist** — every one of the 200 globally-nearest rows may fail the filter while matching rows sit at vector rank 201+. Because the SEARCH-03 design treats an empty semantic result set as a valid answer ("emptiness never means fall back", service.rs:163-164), there is no keyword fallback to rescue this: the memories are simply unfindable. The phase's exactness tests (`tests/semantic.rs:244`, `tests/fallback.rs:133`) use 2-row corpora, so they structurally cannot catch this.

Compounding it: `k = (limit.max(1) * 4).min(MAX_KNN_K)` means the documented "fetch limit*4 and re-rank by similarity×decay" oversampling (sqlite.rs:266-267) degrades from 4× at `limit<=50` down to exactly **1×** at `limit=200` — at high limits the decay-blend re-rank operates on precisely the nearest-k set, so a fresh, high-decay row at vector rank 201 can never surface regardless of its blended score. (The `.max(1)` is also dead code now that `validate_limit` guarantees `1..=200`.)

**Fix:** Decouple the KNN oversample cap from `MAX_LIMIT` (e.g. `const MAX_KNN_K: i64 = MAX_LIMIT * 4;` — still bounded, preserving the T-02-03 intent), and when any filter is present either (a) retry with a larger `k` until `limit` post-filter rows are gathered or the corpus is exhausted, or (b) move `mem_type`/`scope`/tags into `vec_memories` metadata columns so sqlite-vec pre-filters before the KNN cut. At minimum, document the recall bound at the `memory_search` tool description so agent callers know a filtered semantic miss is possible.

### WR-02: Poisoned writer mutex is reported as `MemoryError::NotFound` — surfacing as REST **404** for an internal failure, and diverging from the MCP tier mapping

**File:** `crates/agent-memory-core/src/store/sqlite.rs:226, 306, 467, 483, 504, 526` and `crates/agent-memory/src/rest/handlers.rs:92`
**Issue:** Every writer-lane method maps lock poisoning to the semantically wrong variant:

```rust
let mut conn = self.writer.lock().map_err(|_| MemoryError::NotFound)?;
```

`NotFound` displays as "memory not found" (domain.rs:158-159), which is misleading in logs, and the two transports disagree about its tier: `mcp.rs:41` maps `NotFound` to `internal_error` (its comment even names the poisoned mutex as the only way this arises), but `rest/handlers.rs:92` maps `NotFound` → `ApiError::NotFound` → **404 "not found"**. So after any panic while holding the writer lock, every subsequent `POST /api/memories` returns 404 — a client-tier response for a server-side failure, exactly the mis-tiering this phase's two-tier mapping work (WR-04 closure) set out to eliminate. Note `NotFound` has no legitimate producer on these paths at all: forget's clean not-found is `Ok(false)`, so the REST 404 arm is reachable *only* via mutex poisoning.

**Fix:** Add a dedicated internal variant and use it at all six lock sites:

```rust
// domain.rs
#[error("internal error: {0}")]
Internal(String),

// sqlite.rs
let mut conn = self.writer.lock()
    .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
```

Map `Internal` to `ApiError::Internal` (500) in REST and `internal_error` in MCP. If a `NotFound` variant remains, it should have zero producers or be removed.

### WR-03: REST harness spawn "deadline" cannot fire while the child stays silent — the test hangs instead of failing at 10s

**File:** `crates/agent-memory/tests/rest.rs:55-84`
**Issue:** The startup loop checks the deadline only *between* line reads:

```rust
let deadline = Instant::now() + Duration::from_secs(10);
let port = loop {
    assert!(Instant::now() < deadline, "timed out ...");
    let n = reader.read_line(&mut line).expect("read stderr line");
```

`BufReader::read_line` on a child's piped stderr blocks indefinitely. If the spawned server stays alive but never emits the `REST listening on` line (e.g. a future logging change reroutes it to stdout — which this test discards — or a hang before bind), the test blocks forever on `read_line` and the deadline assertion is never reached. The failure mode is a wedged CI job killed by the outer job timeout with no diagnostic, instead of a clean 10-second panic naming the cause.

**Fix:** Move the blocking reads onto a thread and enforce the deadline with a channel timeout:

```rust
let (tx, rx) = std::sync::mpsc::channel::<String>();
std::thread::spawn(move || {
    let mut line = String::new();
    while reader.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
        let _ = tx.send(std::mem::take(&mut line));
    }
});
let port = loop {
    let line = rx.recv_timeout(Duration::from_secs(10))
        .expect("timed out waiting for 'REST listening on'");
    if let Some(tail) = line.split("REST listening on").nth(1) { /* parse */ }
};
```

(The same thread then doubles as the existing stderr drain.)

## Info

### IN-01: Corrupted `tags` JSON is silently masked at both ends of the store seam

**File:** `crates/agent-memory-core/src/store/sqlite.rs:177` and `:218`
**Issue:** `row_to_view` swallows a tags-column parse failure with `serde_json::from_str(&tags_json).unwrap_or_default()` — a row whose stored tags blob is corrupt silently reads back as untagged (and would then be invisible to tag filters with no signal anywhere). Symmetrically, `insert` falls back to `"[]"` if serialization fails (unreachable for `Vec<String>`, but the pattern normalizes silent data loss). Contrast: an invalid `mem_type` in the same function correctly raises `FromSqlConversionFailure`.
**Fix:** Treat unparseable tags like an invalid `mem_type` (return a conversion error), or at minimum `tracing::warn!` with the row id before defaulting.

### IN-02: CI installs cargo-llvm-cov from source every run, with no caching and inconsistent action/flag pinning

**File:** `.github/workflows/ci.yml:21, 34-43`
**Issue:** `cargo install cargo-llvm-cov --locked` compiles the tool from source on every push/PR (several minutes), there is no Rust build caching for the triple compile (build, test, instrumented coverage re-build), `ci.yml` pins `actions/checkout@v4` while the spike workflow uses `@v5`, and the ci.yml `cargo build/test/clippy` invocations omit `--locked` while the spike consistently uses it.
**Fix:** Use `taiki-e/install-action@cargo-llvm-cov` (prebuilt binary) plus `Swatinem/rust-cache`; align on `actions/checkout@v5` and add `--locked` to the ci.yml cargo invocations.

### IN-03: zig toolchain tarball is downloaded and executed without checksum verification

**File:** `.github/workflows/spike-cross-compile.yml:82-84`
**Issue:** The version is pinned but the artifact is not: `curl -fsSL https://ziglang.org/download/... | tar` runs whatever bytes the mirror serves. Low blast radius here (secretless `contents: read`, `workflow_dispatch`-only spike), but this workflow is explicitly the template for the Phase 6 *release* pipeline, where an unverified toolchain would sit in the artifact supply chain.
**Fix:** Pin the known SHA-256 for `zig 0.14.1` per host arch and verify: `echo "${ZIG_SHA256}  /tmp/zig.tar.xz" | sha256sum -c -` before extraction. Carry the pattern into the Phase 6 workflow.

### IN-04: Build-step `case` has no default arm — an unmatched matrix `tool` value silently skips the build

**File:** `.github/workflows/spike-cross-compile.yml:111-115`
**Issue:** `case "${{ matrix.tool }}" in zigbuild) ...;; xwin) ...;; mingw) ...;; esac` — under `set -e` an unmatched `case` still exits 0, so a future matrix edit with a typo'd `tool` would "pass" the build step and only fail later at the smoke test with a misleading "binary missing" error. The project's own convention (exhaustive matching, no silent catch-all) argues for a loud default.
**Fix:** Add `*) echo "unknown tool: ${{ matrix.tool }}" >&2; exit 1 ;;` (matching the pattern already used in the zig host-arch `case` at line 78).

---

_Reviewed: 2026-07-12T14:46:10Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
