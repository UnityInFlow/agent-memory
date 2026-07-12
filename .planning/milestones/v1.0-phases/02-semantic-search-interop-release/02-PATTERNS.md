# Phase 2: Semantic Search, Interop & Release - Pattern Map

**Mapped:** 2026-07-02
**Files analyzed:** 20 new/modified files
**Analogs found:** 16 / 20 (4 no-analog files use RESEARCH.md code examples)

All paths relative to repo root `10-agent-memory/` unless prefixed. Ecosystem analogs live in sibling repos (absolute paths given).

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/agent-memory-core/src/embed/mod.rs` (NEW) | trait seam + test double | request-response (async) | `crates/agent-memory-core/src/store/mod.rs` (trait) + `src/clock.rs` (test double) | role-match |
| `crates/agent-memory-core/src/embed/ollama.rs` (NEW) | HTTP client adapter | request-response | none (no HTTP client in codebase) | no analog — RESEARCH.md "Ollama embed call" |
| `crates/agent-memory-core/src/import/mod.rs` + `gsd_state.rs` (NEW) | parser/transform | batch transform | `crates/agent-memory-core/src/domain.rs` (typed parsing + thiserror) | role-match |
| `crates/agent-memory-core/src/store/sqlite.rs` (EXTEND) | store impl | CRUD + KNN | itself | exact |
| `crates/agent-memory-core/src/store/mod.rs` (EXTEND trait) | trait | CRUD | itself | exact |
| `crates/agent-memory-core/src/store/migrations.rs` (EXTEND) | config/migration | — | itself | exact |
| `crates/agent-memory-core/sql/0002_embeddings.sql` (NEW) | migration | — | `crates/agent-memory-core/sql/0001_init.sql` | exact |
| `crates/agent-memory-core/src/service.rs` (EXTEND) | service | request-response | itself | exact |
| `crates/agent-memory-core/src/domain.rs` (EXTEND errors) | model/errors | — | itself | exact |
| `crates/agent-memory/src/rest/mod.rs` + `handlers.rs` (NEW) | transport adapter (axum) | request-response | `crates/agent-memory/src/mcp.rs` (thin-adapter rule) | role-match (transport differs) |
| `crates/agent-memory/src/main.rs` (EXTEND: ServeRest + Import subcommands) | CLI entry | — | itself (Serve subcommand + `serve()` fn) | exact |
| `crates/agent-memory/src/mcp.rs` (minor: search_mode) | transport adapter | request-response | itself | exact |
| `crates/agent-memory/src/config.rs` (EXTEND: Ollama URL/REST addr env) | config | — | itself (`resolve_db_path` precedence) | exact |
| `Cargo.toml` (workspace deps: sqlite-vec, reqwest, axum, bytemuck) | config | — | itself (commented pins) | exact |
| `crates/agent-memory-core/tests/semantic.rs` (NEW) | test | integration | `crates/agent-memory-core/tests/store.rs` | exact |
| `crates/agent-memory-core/tests/fallback.rs` (NEW) | test | integration | `tests/store.rs` + `src/clock.rs` TestClock double pattern | exact |
| `crates/agent-memory-core/tests/import.rs` + `tests/fixtures/STATE.md` (NEW) | test + fixture | file I/O | `tests/store.rs` | role-match |
| `crates/agent-memory/tests/rest.rs` (NEW) | test | request-response | `crates/agent-memory/tests/tools.rs` / `tests/stdio_purity.rs` (binary spawn via `CARGO_BIN_EXE`) | role-match |
| `.github/workflows/release.yml` + `spike-cross-compile.yml` (NEW) | CI workflow | batch | `/Users/jirihermann/Documents/workspace-1-ideas/unity-in-flow-ai/03-injection-scanner/.github/workflows/release.yml` | exact (proven twice in-ecosystem) |
| `homebrew-tap/Formula/agent-memory.rb` (NEW repo) | package config | — | none in-ecosystem | no analog — RESEARCH.md formula skeleton |

## Pattern Assignments

### `crates/agent-memory-core/src/embed/mod.rs` (trait seam, async)

**Analog:** `crates/agent-memory-core/src/store/mod.rs` (lines 16-61) — how this codebase defines a swappable seam: a `Send + Sync` trait with rich doc comments citing requirement IDs, production impl in a submodule.

```rust
/// Synchronous persistence interface, implemented by [`sqlite::SqliteStore`].
pub trait Store: Send + Sync {
    /// Insert a new memory, returning its assigned id. `now` is the injected
    /// timestamp used for `created_at`/`last_accessed`.
    fn insert(&self, new: NewMemory, now: i64) -> Result<i64, MemoryError>;
    ...
```

**Test-double pattern:** copy from `crates/agent-memory-core/src/clock.rs` — `SystemClock` (prod) + `TestClock` (deterministic, feature-gated `test-clock` via self dev-dependency). `FakeEmbedder` follows the same shape: same module, deterministic per-input seeded vectors, injected as `Arc<dyn Embedder>` the way `Arc<dyn Clock>` is injected in `MemoryService::new` (`service.rs:49-55`).

**Difference from analog:** `Embedder` methods are `async` (reqwest); `Store` is blocking. Use async-fn-in-trait (Rust 1.75+, no `async_trait` dep — RESEARCH Pattern 2 preference).

**Error type:** define `EmbedError` with `thiserror` following `MemoryError` in `domain.rs:119-138` (`#[derive(Debug, Error)]`, `#[error("...")]`, `#[from]` conversions).

---

### `crates/agent-memory-core/src/embed/ollama.rs` (HTTP client — no analog)

No HTTP code exists in the codebase (core is offline-by-construction, `tests/store.rs:6-8`). Use RESEARCH.md "Ollama embed call" example verbatim (POST `/api/embed`, `EmbedRequest { model, input }`, `error_for_status()`, 10s timeout, health via GET `/api/tags`). Conventions to carry over from the codebase:
- Module-level `//!` doc comment stating the invariant it guards (every Phase-1 file has one).
- Base URL from env `AGENT_MEMORY_OLLAMA_URL` — follow the env precedence style of `config.rs:18-35` (flag → env → default `http://localhost:11434`).
- All warnings via `tracing::warn!` (stderr only — MCP-05); see `main.rs:117-124` for the log-and-continue style.

---

### `crates/agent-memory-core/sql/0002_embeddings.sql` (migration)

**Analog:** `crates/agent-memory-core/sql/0001_init.sql` — header-comment style (lines 1-4) explaining what runs it and what deliberately is NOT here:

```sql
-- 0001_init.sql — initial schema for agent-memory (Phase 1).
-- Run via rusqlite_migration `M::up`. Connection-level PRAGMAs (WAL, synchronous,
-- foreign_keys, buy_timeout) are applied per-connection in sqlite.rs, NOT here.
```

Content per RESEARCH Pattern 3 (embedding_status column, `vec_memories` vec0 table with `distance_metric=cosine`, `meta` table). **CRITICAL divergence from the analog:** 0001 keeps `memories_fts` in sync via triggers (lines 34-45) — do NOT copy that for vec0; vec0 virtual tables ignore triggers (RESEARCH Pitfall 3). Deletes must be explicit in Rust (`forget`, `sweep_expired`).

**Registration:** `crates/agent-memory-core/src/store/migrations.rs:9-11` — append one line:

```rust
Migrations::new(vec![M::up(include_str!("../../sql/0001_init.sql"))])
// becomes: vec![M::up(...0001...), M::up(include_str!("../../sql/0002_embeddings.sql"))]
```

Keep the existing `migrations_validate` test (lines 17-23) — it validates the new migration for free. Note: `M::up` migration SQL cannot create the vec0 table before the extension is registered — sqlite-vec `register_auto_extension` (RESEARCH Pattern 1) must run BEFORE `SqliteStore::open`'s `Connection::open` at `sqlite.rs:83`.

---

### `crates/agent-memory-core/src/store/sqlite.rs` (EXTEND: insert_embedding, knn_search, pending_embeddings, vec deletes)

**Analog:** itself. Patterns to replicate exactly:

**Extension/function setup before use** (lines 44-61): `register_functions`/`prepare_connection` is where the `exp` scalar function is registered — sqlite-vec auto-extension registration belongs in `SqliteStore::open` (line 82) before `Connection::open`, guarded by `std::sync::Once` (it is process-global). The smoke-check pattern at lines 89-92 (`SELECT count(*) FROM memories_fts`) should be mirrored: `SELECT count(*) FROM vec_memories` after migration to fail fast if vec0 is missing.

**Writer-lane write** (insert, lines 140-157): `self.writer.lock()`, fully parameterized `params![...]`. The vec insert (RESEARCH "Inserting a vector") goes in the same writer transaction as the memories insert; blob via `bytemuck::cast_slice`.

**Read-pool parameterized query with JOIN + inline ranking** (search, lines 195-248): this is the direct template for `knn_search` — read pool `self.reads.get()`, prepared statement with a CTE/JOIN, every value bound (`args.query`, weights, `now`, limit as params), `query_map(_, row_to_view)`, collect with `row?`. Reuse `row_to_view` (lines 108-132) — the KNN SELECT must emit the same column names, plus `distance` handled separately. The comment discipline (sign rule, threat IDs like T-02-01) should continue.

**Explicit sync deletes** (forget lines 251-256, sweep_expired lines 282-292): both currently rely on the FTS trigger comment — extend each with an explicit `DELETE FROM vec_memories WHERE memory_id = ?1` (Pitfall 3); update the comments (they currently say triggers handle sync — true for FTS only).

---

### `crates/agent-memory-core/src/service.rs` (EXTEND: semantic path + fallback + import fn)

**Analog:** itself. The `search` method (lines 87-115) is the template for the semantic path:

```rust
let mut views = tokio::task::spawn_blocking(move || store.search(args, now, weights, cfg))
    .await
    .map_err(MemoryError::Join)??;
// recompute decay on read; then fire-and-forget bump_access (lines 106-112)
```

Rules to preserve: embed call happens async OUTSIDE `spawn_blocking`; every rusqlite call INSIDE it (Pitfall 5). The fallback decision (`Embedder::embed` err → existing FTS5 path) lives here, never in transports; result gains a `search_mode` field. Constructor extension follows `new` (lines 49-55) — add `embedder: Arc<dyn Embedder>` alongside `Arc<dyn Clock>`.

The sweep-backfill (optional, Open Question 1) hooks into `sweep` (lines 133-139) the same way `DecayEngine::sweep` does.

---

### `crates/agent-memory-core/src/import/{mod.rs,gsd_state.rs}` (parser)

**Analog (typed-model + error style):** `crates/agent-memory-core/src/domain.rs` — validate-at-boundary via `TryFrom` (lines 55-69), `thiserror` enum (119-138), inline `#[cfg(test)] mod tests` with 3+ pass / 3+ fail cases (140-196). Parser output is `Vec<NewMemory>` (domain.rs:77-84) with `source = Some("gsd-state")`, tag `gsd`. Tolerant scanning (skip + count malformed, never hard-error) mirrors the "clean error, never panic" doctrine of `MemoryType::try_from`. Section→type mapping table is in RESEARCH Pattern 5. Idempotency check is a store-level parameterized SELECT — same bound-params discipline as `sqlite.rs:164-174` (NULL-disables-predicate style).

---

### `crates/agent-memory/src/rest/{mod.rs,handlers.rs}` (axum adapter)

**Analog:** `crates/agent-memory/src/mcp.rs` — the thin-adapter contract is the load-bearing pattern. Module doc (lines 1-5):

```rust
//! Per the architecture rule (RESEARCH Anti-Pattern 1), tool methods contain NO
//! SQL, NO decay math, and NO clock access. Each method only: deserializes its
//! `Parameters<T>`, calls the service, and maps the `Result` to a `CallToolResult`.
```

Copy per-handler shape from `memory_store` (lines 111-136): (1) validate wire type up front — `MemoryType::try_from(args.r#type.as_str()).map_err(...)`; (2) build the service arg struct (`NewMemory`, `ServiceListArgs`, `ServiceSearchArgs` — reuse these exact structs); (3) one service call; (4) map error. Request DTOs copy `StoreArgs`/`ListArgs`/`SearchArgs` (lines 27-89) — same fields, `#[serde(default)]` on optionals, `r#type: String` wire form (drop `schemars` for REST; serde only). Shared state copies `AppState` (lines 21-23) via axum `State<Arc<AppState>>` instead of `self.state`. Forget's clean not-found (lines 217-226: `Ok(false)` → JSON `{deleted:false, reason:"not_found"}`) maps to REST 404. Router wiring + `{id}` path syntax + loopback bind rule: RESEARCH Pattern 4 snippet.

Define an `ApiError` implementing `IntoResponse` that mirrors the two-tier mapping in mcp.rs: `invalid_params` (→400/422) vs `internal_error` (→500).

---

### `crates/agent-memory/src/main.rs` (EXTEND: ServeRest + Import subcommands)

**Analog:** itself. New subcommands copy the existing shape exactly:

- `Command` enum (lines 46-50) gains `ServeRest { #[arg(long, default_value = "127.0.0.1:7437")] addr: ... , #[arg(long)] allow_remote: bool }` and `Import { #[arg(long)] from: ..., path: PathBuf, #[arg(long)] scope: Option<String> }`; dispatch in `match cli.command` (lines 70-72).
- Wiring function copies `serve` (lines 76-103): `resolve_db_path` → `SqliteStore::open` with `.with_context(...)` → `MemoryService::new` → transport. `serve_rest` reuses `spawn_sweep_task` (lines 112-127) if the REST daemon should also sweep.
- Global `--db` flag pattern with `env = "AGENT_MEMORY_DB"` (lines 38-40) is the template for `AGENT_MEMORY_OLLAMA_URL`.
- stderr-only tracing + panic hook (lines 56-66) stays first in `main` — Import may print its report to stdout (CLI subcommand, MCP-05 applies only to `serve`).

---

### `crates/agent-memory/src/config.rs` (EXTEND)

**Analog:** itself — `resolve_db_path` precedence chain (flag → env → default, lines 18-35) is the template for resolving the Ollama base URL and REST bind address. Keep the "ONE place that touches platform specifics" doctrine (lines 1-5). Loopback-refusal check for `--addr` lives here or in `serve_rest`, with a unit test in the inline `#[cfg(test)]` module (lines 56-79 style).

---

### `Cargo.toml` (workspace deps)

**Analog:** itself — every pin carries a rationale comment (see the rusqlite 0.39 block). New entries must do the same:

```toml
# sqlite-vec 0.1.9: only build-dep is `cc` — no libsqlite3-sys conflict with the
# rusqlite 0.39 pin (rusqlite is only a dev-dep upstream). 0.1.10 is alpha-only.
sqlite-vec = "0.1.9"
# Localhost-only HTTP → no TLS backend at all (reduces darwin cross-compile surface).
reqwest = { version = "0.12", default-features = false, features = ["json"] }
axum = "0.8"
bytemuck = "1"
```

reqwest/sqlite-vec/bytemuck go into `agent-memory-core`'s deps; axum into `agent-memory`. Do NOT bump rusqlite/rusqlite_migration (Anti-Pattern).

---

### Tests: `semantic.rs`, `fallback.rs`, `import.rs` (core) and `rest.rs` (binary)

**Analog for core integration tests:** `crates/agent-memory-core/tests/store.rs` — copy:
- Real on-disk temp DB, never `:memory:` (lines 1-8 rationale, lines 46-48 `tempfile::tempdir` + join).
- Helper builders `new_memory(...)` (lines 28-37) and `service_for(path, clock)` (lines 39-42) — extend `service_for` to also take `Arc<dyn Embedder>` (FakeEmbedder).
- `TestClock::new(n)` + `clock.advance(n)` determinism (lines 48-56).
- `#[tokio::test]` + behavior-named tests (`six_typed_rows_survive_store_drop_reopen`).

`fallback.rs`: FakeEmbedder configured to fail → assert keyword results + `search_mode == "keyword"` (Pitfall 2 kill-test). `import.rs`: fixture at `crates/agent-memory-core/tests/fixtures/STATE.md`; assert typed counts, re-run imports 0.

**Analog for `crates/agent-memory/tests/rest.rs`:** two options, both in-repo — spawn the real binary like `tests/stdio_purity.rs` (lines 11-14 `env!("CARGO_BIN_EXE_agent-memory")`, lines 21-39 spawn/pipe/wait), or in-process axum `oneshot` (RESEARCH test map). If spawning, copy the binary-path + tempdir-db pattern from stdio_purity.rs exactly.

**Extend `tests/stdio_purity.rs`:** add `AGENT_MEMORY_OLLAMA_URL` pointing at a dead port via `.env(...)` on the existing `Command` (line 21) so the degrade warning fires during the purity check (Pitfall 4).

---

### `.github/workflows/release.yml` + `spike-cross-compile.yml`

**Analog (exact, proven twice):** `/Users/jirihermann/Documents/workspace-1-ideas/unity-in-flow-ai/03-injection-scanner/.github/workflows/release.yml` — copy the whole 3-job structure (test gate → build-binaries matrix → release), adapting:

- Env pins (lines 35-40): `ZIG_VERSION: '0.14.1'`, `CARGO_ZIGBUILD_VERSION: '0.23.0'` — never `latest`.
- Zig install from official tarball, host-arch-matched, arch-first naming (lines 128-147) — copy verbatim (runners lack pip3).
- Build step: `cargo zigbuild --release --locked --target ${{ matrix.target }}` (line 150).
- Host-arch-aware smoke test (lines 163-186): darwin = presence-only; Linux exec only when `TARGET_ARCH == uname -m` (arm64→aarch64 normalization) — copy verbatim.
- Matrix of 6 triples with `experimental: true` + `continue-on-error` on the two darwin legs (lines 91-118) — for agent-memory the darwin legs are REQUIRED by DIST-01, so keep `continue-on-error` only for the spike workflow; the release workflow should hard-require darwin (or invoke Plan B first).
- Release job: required-asset validation loop, `SHA256SUMS.txt` generation, `softprops/action-gh-release@v3` (lines 202-302).
- **Divergence:** agent-memory has no raw-binary consumer contract — package `tar.gz` per triple (the Homebrew formula expects `agent-memory-<triple>.tar.gz`), not raw binaries.
- `runs-on: [orangepi]` throughout (fleet-offline reality; analog lines 55, 94, 205).

`spike-cross-compile.yml` = the same build-binaries job with `on: workflow_dispatch`, all legs `continue-on-error: false`, no release job (Wave-1 gate for Pitfall 1).

CI header-comment convention: see this repo's `.github/workflows/ci.yml` lines 9-11 (self-hosted-only rationale comment).

---

### `homebrew-tap/Formula/agent-memory.rb` (no in-ecosystem analog)

Use the RESEARCH.md "Homebrew tap formula skeleton" verbatim (class `AgentMemory`, `on_macos`/`on_linux` × `Hardware::CPU.arm?` url+sha256 blocks, `bin.install "agent-memory"`, `--version` test block). New public repo `UnityInFlow/homebrew-tap`; sha256 values filled manually post-release for v0.0.1.

## Shared Patterns

### Error handling: thiserror in lib, anyhow at binary edges
**Source:** `crates/agent-memory-core/src/domain.rs:119-138` (`MemoryError` with `#[from]` conversions) and `crates/agent-memory/src/main.rs:76-79` (`anyhow::Context` with `.with_context(|| format!(...))`).
**Apply to:** `EmbedError`, `ImportError` (lib, thiserror; likely new `MemoryError` variants via `#[from]`); `serve_rest`/`import` wiring (binary, anyhow).

### spawn_blocking discipline
**Source:** `crates/agent-memory-core/src/service.rs:62-68` — every rusqlite call wrapped:
```rust
tokio::task::spawn_blocking(move || store.insert(new, now))
    .await
    .map_err(MemoryError::Join)?
```
**Apply to:** knn_search, vec insert, import dedup/insert, pending-embeddings backfill. Async HTTP (embed) stays outside.

### Parameterized SQL, NULL-disables-filter
**Source:** `crates/agent-memory-core/src/store/sqlite.rs:164-174` (`WHERE (?1 IS NULL OR mem_type = ?1) ...`) and the bound-IN-list builder (lines 265-278).
**Apply to:** KNN query (blob + k bound), import dedup SELECT, REST-reachable queries. Never format user input into SQL (T-01-01 comments carry the threat IDs).

### stderr-only logging (MCP-05)
**Source:** `crates/agent-memory/src/main.rs:56-66` (tracing to `std::io::stderr`, ANSI off, panic hook) — guarded by `crates/agent-memory/tests/stdio_purity.rs`.
**Apply to:** Ollama degrade warnings, REST startup logs, sweep-backfill logs. Import report stdout is the sanctioned exception (CLI subcommand).

### Injected clock/dep seams for determinism
**Source:** `crates/agent-memory-core/src/clock.rs` (`Arc<dyn Clock>`, `TestClock`) + injection in `service.rs:49-55`.
**Apply to:** `Arc<dyn Embedder>` with `FakeEmbedder`, injected via `MemoryService::new`.

### Doc-comment discipline
**Source:** every Phase-1 file — module `//!` header stating the invariant + requirement IDs (e.g. `sqlite.rs:1-10`, `mcp.rs:1-5`); load-bearing inline comments cite RESEARCH patterns/pitfalls and decision IDs.
**Apply to:** all new files.

## No Analog Found

| File | Role | Data Flow | Reason | Fallback |
|------|------|-----------|--------|----------|
| `crates/agent-memory-core/src/embed/ollama.rs` | HTTP client | request-response | No HTTP/reqwest code anywhere in the codebase (offline-by-construction core) | RESEARCH.md "Ollama embed call" example + config.rs env-precedence style |
| sqlite-vec registration block (in `sqlite.rs::open`) | FFI init | — | No `unsafe`/extension registration exists yet | RESEARCH Pattern 1 (`register_auto_extension` + `// SAFETY:` comment); do NOT copy the stale sqlite-vec docs snippet |
| `homebrew-tap/Formula/agent-memory.rb` | package config | — | No Homebrew formula in any UnityInFlow repo | RESEARCH.md formula skeleton |
| axum Router/IntoResponse specifics | transport plumbing | request-response | No axum in this repo (mcp-hub is Rust+axum but a different service style) | RESEARCH Pattern 4 snippet; adapter *shape* from mcp.rs |

## Metadata

**Analog search scope:** `crates/agent-memory-core/src/**`, `crates/agent-memory/src/**`, `crates/**/tests/**`, `crates/agent-memory-core/sql/`, `.github/workflows/`, workspace `Cargo.toml`; ecosystem siblings `03-injection-scanner/.github/workflows/release.yml` (read) and `07-mcp-hub/.github/workflows/release.yml` (located, same template lineage — injection-scanner copy read as the newer refinement)
**Files scanned:** 17 in-repo source/test/config files + 1 ecosystem release workflow
**Pattern extraction date:** 2026-07-02
