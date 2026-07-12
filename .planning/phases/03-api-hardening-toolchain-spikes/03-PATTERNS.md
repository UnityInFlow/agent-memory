# Phase 3: API Hardening & Toolchain Spikes - Pattern Map

**Mapped:** 2026-07-12
**Files analyzed:** 10 (9 modified, 1 possibly new test file)
**Analogs found:** 10 / 10 — every file this phase touches is its own analog (pure modification phase); the 02-05 error-taxonomy commits are the proven precedent

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/agent-memory-core/src/domain.rs` | model (error taxonomy + consts) | — (types) | itself — `MemoryError::InvalidQuery` variant (02-05) | exact |
| `crates/agent-memory-core/src/service.rs` | service | request-response | itself — method preambles + `map_fts_query_error` seam pattern | exact |
| `crates/agent-memory-core/src/store/sqlite.rs` | store | CRUD (SQL) | itself — the three existing `?N IS NULL OR …` NULL-disable predicates | exact |
| `crates/agent-memory/src/mcp.rs` | transport adapter (MCP) | request-response | itself — `map_mcp_error` exhaustive match (02-05) | exact |
| `crates/agent-memory/src/rest/handlers.rs` | transport adapter (REST) | request-response | itself — `map_memory_error` + in-process `mod tests` harness | exact |
| `.github/workflows/spike-cross-compile.yml` | config (CI workflow) | batch | itself — v1.0 darwin spike shape (rewrite in place) | exact |
| `.github/workflows/ci.yml` | config (CI workflow) | batch | itself — same steps, runner swap only | exact |
| `crates/agent-memory-core/tests/fallback.rs` | test | integration | itself — `keyword_fallback_honors_tag_filter` (lines 82-130) | exact |
| `crates/agent-memory-core/tests/store.rs` / `tests/semantic.rs` | test | integration | `store.rs` harness (lines 29-46) | exact |
| `crates/agent-memory-core/tests/validation.rs` (possibly new) | test | unit | `tests/fallback.rs` harness + `MemoryError` matches! assertion (lines 150-162) | role-match |

## Pattern Assignments

### `crates/agent-memory-core/src/domain.rs` (model, D-03/D-04)

**Analog:** the existing `MemoryError` enum, `domain.rs:119-141`.

**Variant pattern to copy** (`domain.rs:121-125` — InvalidType/InvalidQuery are the client tier; the new variant slots directly after InvalidQuery):
```rust
#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("invalid memory type '{0}' (expected one of DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT)")]
    InvalidType(String),

    #[error("invalid search query {0:?}: not a valid FTS5 match expression")]
    InvalidQuery(String),
    // NEW (D-04), same shape:
    // #[error("invalid argument: {0}")]
    // InvalidArgument(String),
```

**Consts pattern to copy** (`sqlite.rs:25-32` — doc comment + named `const`; the new bounds go in `domain.rs` as `pub`, per RESEARCH recommendation):
```rust
/// Default cap on returned search rows when the caller omits `limit` (T-02-04).
pub(crate) const DEFAULT_SEARCH_LIMIT: i64 = 50;

/// Hard cap on the KNN oversample size `k` (T-02-03: ...).
const MAX_KNN_K: i64 = 200;
```
New consts (D-01/D-02): `pub const MIN_LIMIT: i64 = 1; pub const MAX_LIMIT: i64 = 200; pub const MIN_TTL_SECS: i64 = 1; pub const MAX_TTL_SECS: i64 = 3_155_760_000;` — then re-point `sqlite.rs:32` at `crate::domain::MAX_LIMIT` and promote `DEFAULT_SEARCH_LIMIT` to `pub` (D-03).

**Unit-test pattern** (`domain.rs:170-177` — assert variant + payload, never full-string):
```rust
let err = MemoryType::try_from("BOGUS").unwrap_err();
match err {
    MemoryError::InvalidType(got) => assert_eq!(got, "BOGUS"),
    other => panic!("expected InvalidType, got {other:?}"),
}
```

**Validation helpers** — RESEARCH § Code Examples gives the exact `validate_limit`/`validate_ttl` bodies (range-contains + `format!("limit must be between {MIN_LIMIT} and {MAX_LIMIT} (got {l})")`, D-05 message shape). `Option::None` always passes (omitted = default/no-expiry unchanged).

---

### `crates/agent-memory-core/src/service.rs` (service, D-01/D-06)

**Analog:** its own method preambles — every public method starts by reading the clock then doing pre-store work; validation inserts at the very top.

**Insertion sites** (verified this session):
- `store()` (line 117): call `validate_ttl(new.ttl_secs)?` BEFORE the embed call at line 120 (a bad TTL must not cost an Ollama round-trip).
- `search()` (line 164): call `validate_limit(args.limit)?` at the top; then the D-01 clamp removal:
```rust
// service.rs:168 — BEFORE (verified):
let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).max(0) as usize;
// AFTER:
let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT) as usize;
```
- `list()` (line 140): `validate_limit(args.limit)?` before the spawn_blocking hop.
- `import()` (line 258): validate each draft's `ttl_secs` in a loop before the dedup hop (Pitfall 3 — import bypasses `store()` via `store.insert` at line 307).

**Error-propagation pattern to copy** (`service.rs:134-137` — `Result` + `?`, never unwrap):
```rust
tokio::task::spawn_blocking(move || store.insert(new, embedding, now))
    .await
    .map_err(MemoryError::Join)?
```

**Import loop shape to mirror** (`service.rs:266-274` — per-draft loop, `?` propagation): add `validate_ttl(draft.ttl_secs)?` per draft alongside the existing `store.exists` check.

---

### `crates/agent-memory-core/src/store/sqlite.rs` (store, D-07)

**Analog:** the existing NULL-disable bound-parameter predicates — same shape, new inner expression. All three `params![]` lists and parameter indices are UNCHANGED.

**Site 1 — `knn_search`, sqlite.rs:279** (outer WHERE after the `WITH knn AS (...)` join, alias `m`):
```sql
-- BEFORE:  AND (?5 IS NULL OR m.tags LIKE '%' || ?5 || '%')
-- AFTER:   AND (?5 IS NULL OR EXISTS (SELECT 1 FROM json_each(m.tags) WHERE json_each.value = ?5))
```
Bind list stays `params![query_blob, k, mem_type_filter, args.scope, args.tag]` (line 284).

**Site 2 — `list`, sqlite.rs:369** (plain WHERE, unaliased table):
```sql
-- BEFORE:  AND (?3 IS NULL OR tags LIKE '%' || ?3 || '%')
-- AFTER:   AND (?3 IS NULL OR EXISTS (SELECT 1 FROM json_each(memories.tags) WHERE json_each.value = ?3))
```
Note: the `LIMIT CASE WHEN ?4 IS NULL THEN -1 ELSE ?4 END` at line 371 stays — omitted limit = unlimited list is documented behavior (Pitfall 6).

**Site 3 — keyword `search`, sqlite.rs:414** (alongside the FTS5 MATCH, alias `m`):
```sql
-- BEFORE:  AND (?10 IS NULL OR m.tags LIKE '%' || ?10 || '%')
-- AFTER:   AND (?10 IS NULL OR EXISTS (SELECT 1 FROM json_each(m.tags) WHERE json_each.value = ?10))
```
The `map_fts_query_error` routing at lines 446-449 is untouched.

**Leave-alone clamp** — `sqlite.rs:265` defensive floor stays as-is:
```rust
let k = (args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).max(1) * 4).min(MAX_KNN_K);
```
Only re-point `MAX_KNN_K` (line 32) to the new core const.

**Tags-storage invariant** (why json_each is safe) — `sqlite.rs:215`:
```rust
let tags_json = serde_json::to_string(&new.tags).unwrap_or_else(|_| "[]".to_string());
```

---

### `crates/agent-memory/src/mcp.rs` (MCP transport, D-04)

**Analog:** `map_mcp_error`, mcp.rs:31-42 — the exhaustive two-tier match; the new variant joins the client arm:
```rust
fn map_mcp_error(e: MemoryError) -> McpError {
    match &e {
        MemoryError::InvalidType(_) | MemoryError::InvalidQuery(_) => {
            // + | MemoryError::InvalidArgument(_)
            McpError::invalid_params(e.to_string(), None)
        }
        MemoryError::Sqlite(_)
        | MemoryError::Pool(_)
        | MemoryError::Join(_)
        | MemoryError::Migration(_)
        | MemoryError::NotFound => McpError::internal_error(e.to_string(), None),
    }
}
```
Named arm, never `_` (CLAUDE.md exhaustive-match rule).

**Doc-string fixes (D-08 / Pitfall 4)** — mcp.rs:81 (`ListArgs.tag`) and mcp.rs:101 (`SearchArgs.tag`), currently:
```rust
/// Filter by a tag substring. Omit for all.
```
→ "Filter by an exact tag (case-sensitive). Omit for all."

**Mapper unit-test pattern to extend** — `map_mcp_error_splits_client_and_internal_tiers`, mcp.rs:276-295:
```rust
let invalid_params_code = McpError::invalid_params("x", None).code;
assert_eq!(
    map_mcp_error(MemoryError::InvalidQuery("\"".into())).code,
    invalid_params_code,
    "a malformed FTS5 query is client input → invalid_params"
);
```
Add the identical assertion for `MemoryError::InvalidArgument("limit must be between 1 and 200 (got 0)".into())`.

---

### `crates/agent-memory/src/rest/handlers.rs` (REST transport, D-04)

**Analog:** `map_memory_error`, handlers.rs:87-98 — extend the 400 arm:
```rust
fn map_memory_error(e: MemoryError) -> ApiError {
    match &e {
        MemoryError::InvalidType(_) | MemoryError::InvalidQuery(_) => {
            // + | MemoryError::InvalidArgument(_)
            ApiError::BadRequest(e.to_string())
        }
        MemoryError::NotFound => ApiError::NotFound,
        MemoryError::Sqlite(_) | MemoryError::Pool(_)
        | MemoryError::Join(_) | MemoryError::Migration(_) => ApiError::Internal(e.to_string()),
    }
}
```

**Doc-string fixes** — handlers.rs:55 (`SearchRequest.tag`) and handlers.rs:72 (`ListQuery.tag`): same "Filter by a tag substring" → exact-tag wording.

**In-process test harness to copy** (the coverage-bearing layer — Pitfall 5) — `test_state` + `body_json`, handlers.rs:242-272:
```rust
const DEAD_OLLAMA: &str = "http://127.0.0.1:9";

fn test_state(dir: &tempfile::TempDir) -> Arc<AppState> {
    let store = SqliteStore::open(&dir.path().join("mem.db")).expect("store opens");
    let embedder: Arc<dyn Embedder> = Arc::new(OllamaClient::new(DEAD_OLLAMA));
    let service = MemoryService::new(Arc::new(store), Arc::new(SystemClock),
        embedder.clone(), DecayConfig::default());
    Arc::new(AppState { service, embedder })
}

async fn body_json(resp: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.expect("body reads");
    serde_json::from_slice(&bytes).expect("body is JSON")
}
```

**400-boundary test to mirror** — `search_with_malformed_query_returns_400_never_500`, handlers.rs:400-441. The new `search_with_zero_limit_returns_400_never_500` copies this shape with `limit: Some(0)` and asserts `body["error"]` contains the `limit must be between` prefix (never full-string equality — CONTEXT specifics).

---

### `.github/workflows/spike-cross-compile.yml` (CI workflow, D-10/D-11 — rewrite in place)

**Analog:** the file itself (99 lines, v1.0 darwin spike). Keep and adapt these proven blocks; RESEARCH § Pattern 4 provides the full target YAML.

**Keep verbatim/adapt:**
- Header comment discipline (lines 1-14) — purpose + policy citations; add the `# VERDICT (Phase 3, D-11): ...` line Phase 6 reads, and cite D-10/OPS-02 for the ubuntu-latest exception.
- Pinned env block (lines 18-23): `ZIG_VERSION: '0.14.1'`, `CARGO_ZIGBUILD_VERSION: '0.23.0'` — add `CARGO_XWIN_VERSION: '0.23.0'` + the two target-suffixed `CFLAGS_*_unknown_linux_musl` shim vars (never unsuffixed `CFLAGS`).
- `strategy: fail-fast: false` (lines 33-35) — surface ALL leg verdicts in one dispatch.
- The zig official-tarball install block (lines 53-71) — host-arch `case "$(uname -m)"` + `curl … ziglang.org/download` + `GITHUB_PATH`; copy verbatim (host is x86_64 on ubuntu-latest but the block is arch-aware already).
- The host-arch-aware smoke step (lines 76-99) — `TARGET_ARCH="${TARGET%%-*}"` vs `uname -m`; Windows legs are presence-only and MUST check `agent-memory.exe` (Pitfall 9); darwin case is replaced by windows case.

**Change:**
- `runs-on: [orangepi]` (line 28) → `runs-on: ubuntu-latest`; add `permissions: contents: read` (secretless D-02 exception).
- Matrix (lines 36-43) → four `include` legs: musl x86_64/aarch64 (zigbuild), `x86_64-pc-windows-msvc` (xwin: `rustup component add llvm-tools` + `cargo install --locked --version "${CARGO_XWIN_VERSION}" cargo-xwin`), `x86_64-pc-windows-gnu` (mingw: `sudo apt-get install -y gcc-mingw-w64-x86-64`, belt-and-braces `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc`).
- Build step (line 74) → a `case "${{ matrix.tool }}"` dispatching `cargo zigbuild` / `cargo xwin build` / `cargo build`, all `--release --locked --target … -p agent-memory`.

---

### `.github/workflows/ci.yml` (CI workflow, D-12 ride-along — do FIRST)

**Analog:** the file itself (42 lines). Every step is already secretless; keep the step sequence verbatim (fmt → clippy `-D warnings` → build → test → `cargo install cargo-llvm-cov --locked` → `cargo llvm-cov --workspace --fail-under-lines 80`).

**Change only:**
```yaml
# BEFORE (lines 12-18):
jobs:
  build-and-test:
    strategy:
      fail-fast: false
      matrix:
        runner: [arc-runner-unityinflow, orangepi]
    runs-on: ${{ matrix.runner }}
# AFTER:
permissions:
  contents: read
jobs:
  build-and-test:
    runs-on: ubuntu-latest   # D-10/OPS-02: public repo — self-hosted jobs never run here
```
Also update the stale policy comment (lines 9-11) to cite the D-02 exception rather than "never ubuntu-latest".

---

### Test files (D-09 regression fixture + API-02 boundary matrix)

**`tests/fallback.rs` — keyword-path tag regression.** Extend `keyword_fallback_honors_tag_filter` (fallback.rs:82-130) — the exact fixture shape to copy: two rows with distinct single tags, tag-filtered search, assert length 1 + id. The D-09 version uses tags `["rust"]` / `["rustling"]`, `tag: Some("rust")`, and adds a `tag: Some("Rust")` → empty case (D-08 case-sensitivity lock).

**Harness to copy** (fallback.rs:18-36 — dead-embedder service factory):
```rust
fn service_for(path: &std::path::Path, clock: Arc<dyn Clock>, embedder: Arc<dyn Embedder>) -> MemoryService {
    let store = SqliteStore::open(path).expect("open store");
    MemoryService::new(Arc::new(store), clock, embedder, DecayConfig::default())
}
// used with Arc::new(FakeEmbedder::failing()) + Arc::new(TestClock::new(1_000))
```

**Error-variant assertion pattern** (fallback.rs:158-161) — for the core boundary-matrix tests (`validate_limit`/`validate_ttl`, whether in a new `tests/validation.rs` or `domain.rs` unit tests):
```rust
assert!(
    matches!(err, MemoryError::InvalidQuery(_)),   // → InvalidArgument(_)
    "malformed FTS5 input must map to InvalidQuery, got: {err:?}"
);
```
Boundary matrix per Pitfall 2: valid boundaries (1, 200 / 1, 3_155_760_000) MUST succeed; 0, -1, 201/3_155_760_001, `i64::MAX`, `i64::MIN` reject; `None` passes.

**`tests/store.rs` — list-path tag regression.** Copy the `service_for` harness with succeeding `FakeEmbedder::with_vectors(HashMap::new())` (store.rs:40-46) and the store→list assertion loop shape (store.rs:48-70). **`tests/semantic.rs`** extends its existing FakeEmbedder-success harness for the knn-path site (same two-row fixture).

**`tests/rest.rs`** — realism-only spawned-binary 400 check (zero coverage — Pitfall 5); mirror its existing malformed-query HTTP test with `limit: 0`.

## Shared Patterns

### Two-tier error classification (the phase's core mechanism)
**Source:** `mcp.rs:31-42` + `handlers.rs:87-98` (02-05 precedent)
**Apply to:** both mappers; the exhaustive match makes the compiler force the new arm — add the variant to `domain.rs` first, then follow the two compile errors.

### Result + `?`, no unwrap, named match arms
**Source:** `service.rs` throughout (e.g. lines 134-137); CLAUDE.md rules.
**Apply to:** validation helpers, import loop, all test code (`expect` allowed in tests only).

### Doc comments cite decisions
**Source:** every touched file (e.g. `sqlite.rs:263`, `mcp.rs:21-26`).
**Apply to:** new consts, helpers, mapper arms, and workflow comments — cite D-01..D-12 / API-02 / API-03 inline, matching house style.

### Stable-prefix error assertions
**Source:** CONTEXT specifics + handlers.rs:307-310 (`contains`, never `==`).
**Apply to:** every test asserting the new messages — `contains("limit must be between")` / `contains("ttl_secs must be between")`.

### Pinned-version, host-arch-aware CI steps
**Source:** `spike-cross-compile.yml:18-23, 53-71, 76-99`.
**Apply to:** both workflow files — never `latest`, always `--locked --version`, arch decided at runtime.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| — | — | — | none; cargo-xwin/mingw workflow legs are new territory but RESEARCH § Pattern 4 supplies verified YAML + Assumptions A1-A3 are self-verifying in the dispatch |

## Metadata

**Analog search scope:** `crates/agent-memory-core/src/`, `crates/agent-memory/src/`, `crates/*/tests/`, `.github/workflows/`
**Files scanned:** 9 read in full or targeted ranges (domain.rs, service.rs, sqlite.rs §§, mcp.rs, handlers.rs, fallback.rs, store.rs §, both workflows)
**Pattern extraction date:** 2026-07-12
