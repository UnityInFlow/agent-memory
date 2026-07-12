# Phase 3: API Hardening & Toolchain Spikes - Research

**Researched:** 2026-07-12
**Domain:** Rust input validation at a shared service seam + SQLite json_each exact tag matching + Windows/musl cross-compile spikes on GitHub-hosted CI
**Confidence:** HIGH (every code claim verified by reading the actual source; json_each SQL shape empirically verified with sqlite3; toolchain claims verified against upstream READMEs + crates registry)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Validation bounds (API-02)
- **D-01:** `limit` valid range is **1..=200** — floor 1 (zero/negative rejected), ceiling aligned with the existing `MAX_KNN_K = 200` hard cap so a valid `limit` can never exceed what the KNN leg can honor. Omitted `limit` keeps defaulting to `DEFAULT_SEARCH_LIMIT = 50` (unchanged behavior). The current silent `.max(0)` clamp in `service.rs:168` is REMOVED — out-of-range now rejects, per the no-silent-clamp requirement.
- **D-02:** `ttl_secs` valid range is **1..=3_155_760_000** (~100 years) — rejects zero, negative (which currently creates an already-expired row via `now + ttl`), and extremes that could overflow `expires_at` arithmetic. Omitted `ttl_secs` = no expiry (unchanged).
- **D-03:** Bounds live as **named `pub` consts in `agent-memory-core`** next to `DEFAULT_SEARCH_LIMIT`/`MAX_KNN_K`, and are documented in the README API tables. Both transports and all future surfaces (update/link/import in Phases 4-5) validate against the same consts.

#### Error taxonomy & message shape (API-02)
- **D-04:** New variant **`MemoryError::InvalidArgument(String)`** in `domain.rs`, client tier — joins `InvalidType | InvalidQuery` in `map_memory_error` → `ApiError::BadRequest` (REST 400) and `map_mcp_error` → `invalid_params` (MCP). Both matches are exhaustive, so the compiler forces every mapper to classify the new variant — same pattern proven by 02-05.
- **D-05:** Message shape is **field + allowed range + offending value**: e.g. `invalid argument: limit must be between 1 and 200 (got 0)`. Deterministic, greppable, and safe to surface verbatim on both transports (no internal detail leaks — value echoes only what the client sent).
- **D-06:** Validation happens at the **`MemoryService` seam** (before any store call), NOT per-transport. Serde-level `deny_unknown_fields` may be added on REST DTOs if cheap, but the contract lives in core — transports stay thin adapters.

#### Exact tag matching (API-03)
- **D-07:** Replace all **three** `tags LIKE '%' || ? || '%'` predicates (`sqlite.rs:279` knn_search, `:369` list, `:414` keyword search) with `EXISTS (SELECT 1 FROM json_each(m.tags) WHERE json_each.value = ?)` — tags are already stored as a JSON array string, so `json_each` equality is exact per-element matching with a bound parameter (no injection surface change).
- **D-08:** Matching is **case-sensitive, whole-tag equality** (`rust` ≠ `Rust` ≠ `rustling`). Case-folding is NOT added — it would be a second silent behavior change; documented in the README and v0.1.0 release notes alongside the substring→exact change itself.
- **D-09:** A regression test locks each of the three sites (a `rustling`-tagged row must no longer match `tag=rust` in semantic, keyword-fallback, and list paths).

#### Toolchain spikes & CI reality (Phase 6 de-risk)
- **D-10:** **Spikes run on GitHub-hosted `ubuntu-latest`** in a secretless, `contents: read`, manually-triggered (`workflow_dispatch`) spike workflow. Rationale — this is now the only runnable option: the repo went public with v0.0.1 and the org runner group's `allows_public_repositories: false` (enforced 2026-07-09, ecosystem OPS-02) means NO self-hosted job (orangepi included) will ever pick up jobs from this repo again. This is the ecosystem's sanctioned D-02 exception (public/secretless CI on GitHub-hosted).
- **D-11:** Spike matrix and verdict: (a) **musl leg** — zigbuild `x86_64/aarch64-unknown-linux-musl` with target-suffixed `CFLAGS_<triple>="-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int32_t=uint32_t -Du_int64_t=uint64_t"`; (b) **Windows leg** — `cargo-xwin` → `x86_64-pc-windows-msvc` first, `mingw-w64` → `x86_64-pc-windows-gnu` as the fallback leg in the same matrix. The verdict (which legs build green, exact flags) is **recorded in the phase SUMMARY and as a comment in the spike workflow file** — Phase 6 consumes it directly.
- **D-12:** **Flag for Phase 6 (decision deferred, groundwork noted):** the existing `ci.yml`/`release.yml` target self-hosted runners and therefore currently never run on this public repo — CI has been silently dead since publication. Phase 3 spikes will prove GitHub-hosted builds work; Phase 6 decides whether to move `release.yml` to GitHub-hosted (viable — release needs only `GITHUB_TOKEN`) and/or restore a workflow-restricted runner-group exception. Migrating `ci.yml` to a D-02 split (hosted, secretless) SHOULD ride along in this phase if trivially cheap, since a dead CI gate undermines every later phase.

### Claude's Discretion
- Whether `deny_unknown_fields` lands on REST DTOs this phase (nice-to-have, not a criterion).
- Exact const names and module placement for the bounds.
- Whether the spike workflow reuses `spike-cross-compile.yml` or adds a new file.

### Deferred Ideas (OUT OF SCOPE)
- Request-level `mode` override for search (`keyword|semantic|hybrid`) — already captured as SEARCH-06 (future).
- Case-insensitive tag matching / tag normalization on write — revisit only if exact-match causes real friction (would be its own behavioral change).
- Full `ci.yml` D-02 split rework if it turns out non-trivial — Phase 6 owns the release-side runner decision (D-12).
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| API-02 | Out-of-bounds `limit` and `ttl_secs` (zero, negative, absurd extremes) rejected at the shared core seam → REST 400 / MCP `invalid_params` — never a 500, never a silent clamp — across all surfaces | Complete call-path enumeration (§ Validation Seam), exact clamp sites to remove/leave, `InvalidArgument` variant + both mapper arms, boundary-value matrix, ttl overflow analysis, 02-05 four-layer test template |
| API-03 | Tag filtering matches exact tags (json_each equality) instead of LIKE substring over-match — `tag=rust` no longer matches `rustling` | All three predicate sites with exact replacement SQL, empirically verified in CTE-join and plain-WHERE contexts (sqlite3 run), NULL-disable pattern preserved, doc-string update list, regression test shapes per site |
| (Phase 6 de-risk) | Windows (cargo-xwin msvc primary, mingw-w64 gnu fallback) + musl (CFLAGS shims) spikes on GitHub-hosted ubuntu-latest with recorded verdict | Spike workflow YAML shape (§ Spike Workflow), exact tool versions/install commands, license/prereq facts, host-arch-aware smoke rules, `gh workflow run` dispatch/verdict-recording flow |
</phase_requirements>

## Summary

This phase is three small, well-bounded work packages against a codebase whose seams were built for exactly this: (1) add `MemoryError::InvalidArgument(String)` and bounds validation at the top of the `MemoryService` methods, removing the silent `.max(0)` clamp at `service.rs:168`; (2) swap three `tags LIKE '%'||?||'%'` predicates in `sqlite.rs` for a correlated `json_each` EXISTS equality check (SQL shape empirically verified this session — the LIKE over-match reproduces and the EXISTS form fixes it in both the KNN-CTE and plain-WHERE contexts); (3) stand up a secretless `workflow_dispatch` spike on GitHub-hosted `ubuntu-latest` with four matrix legs (musl ×2 via zigbuild+CFLAGS shim, windows-msvc via cargo-xwin, windows-gnu via mingw-w64) and record the verdict.

The error-taxonomy extension is compile-time-enforced: both `map_memory_error` (handlers.rs:87) and `map_mcp_error` (mcp.rs:31) match exhaustively on `&MemoryError`, so adding the variant breaks the build until both mappers gain the arm — the 02-05 pattern working as designed. The four-layer test template from 02-05 (core unit → in-process handler → MCP unit → spawned-binary HTTP) maps 1:1 onto API-02's proof. Everything runs with a dead-loopback embedder — no Ollama, no network, deterministic.

The one genuinely new territory is the spike workflow: this repo's CI has been dead since going public (all workflows target self-hosted runners that can no longer pick up jobs), so the spike is also the first proof that GitHub-hosted builds work at all — which is why the cheap `ci.yml` → `ubuntu-latest` migration should ride along (D-12).

**Primary recommendation:** Split into two plans — (A) API-02 + API-03 code hardening (pure Rust/SQL, fully testable locally, no network), (B) spike workflow + ci.yml ride-along (YAML + `gh workflow run` dispatch + verdict recording). Plan A has zero external dependencies; Plan B needs only `gh` (verified authenticated locally).

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| limit/ttl_secs bounds validation | Core service (`MemoryService`, agent-memory-core) | — | D-06: one seam, both transports inherit; transports stay thin adapters |
| Error → wire-status mapping | Transport adapters (mcp.rs, rest/handlers.rs) | — | Each transport owns only its wire dialect; classification lives in `MemoryError` tiers |
| Exact tag matching | Store (`SqliteStore`, sqlite.rs SQL) | — | Matching semantics are a persistence-layer predicate; service/transports unchanged |
| Bounds constants | Core domain (agent-memory-core, `pub`) | README API tables | D-03: single source of truth for all current + future surfaces |
| Cross-compile spikes | CI (GitHub-hosted `ubuntu-latest` workflow) | Phase SUMMARY (verdict record) | D-10/D-11: no code change; build-tooling feasibility proof only |
| CI gate (ride-along) | CI (`ci.yml` → `ubuntu-latest`) | — | D-12: secretless fmt/clippy/build/test/coverage needs no self-hosted runner |

## Standard Stack

### Core (unchanged — zero new runtime dependencies)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `rusqlite` | 0.39 (`bundled`, `functions`) | Embedded SQLite 3.51.3 | Pin triangle with libsqlite3-sys 0.37 / r2d2_sqlite 0.34 — MUST NOT move (STATE.md 01-01) `[VERIFIED: Cargo.toml + STACK.md]` |
| `thiserror` | (already pinned) | `MemoryError::InvalidArgument` variant | Ecosystem rule: thiserror in libraries `[VERIFIED: domain.rs]` |
| SQLite `json_each` | built-in (core since 3.38; bundled = 3.51.3) | Exact per-element tag matching | JSON functions are core SQLite; no feature flag, no pin change `[VERIFIED: empirical sqlite3 run this session + STACK.md docs.rs citation]` |

**No validation crate** (`validator`/`garde`) — explicitly rejected in REQUIREMENTS.md Out of Scope and milestone STACK.md: three bounds checks at one seam do not justify a derive-macro dependency tree.

### Build tools (spike only — installed on the runner, never dependencies)

| Tool | Version | Purpose | When to Use |
|------|---------|---------|-------------|
| `cargo-xwin` | 0.23.0 | `x86_64-pc-windows-msvc` cross from Linux (clang-cl against xwin-fetched MSVC CRT/SDK) | Windows primary leg `[VERIFIED: crates.io via package-legitimacy seam — 59,652 dl/wk, rust-cross org, published since 2022]` |
| `cargo-zigbuild` | 0.23.0 (zig 0.14.1, both pinned) | musl legs (existing pipeline pattern) | musl x86_64/aarch64 `[VERIFIED: crates.io via package-legitimacy seam — 99,951 dl/wk; version pins from existing spike-cross-compile.yml]` |
| `gcc-mingw-w64-x86-64` | Ubuntu apt current | `x86_64-pc-windows-gnu` fallback leg | Windows fallback `[ASSUMED: standard Ubuntu package — apt name well-established; verify with `apt-get install` in the spike itself]` |
| `rustup component add llvm-tools` | stable | cargo-xwin prerequisite | Windows msvc leg `[CITED: github.com/rust-cross/cargo-xwin README]` |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| cargo-xwin (msvc) | mingw-w64 (gnu) only | gnu is self-contained (no MS CRT download) but non-tier-1 ABI for Windows users; D-11 mandates BOTH legs in the matrix so the verdict answers Phase 6's choice |
| Rewriting spike-cross-compile.yml in place | New spike file | Existing file can never run again (self-hosted, public repo). Recommend: rewrite it in place — one file, one dispatch = full verdict, and D-11's verdict comment lands in the same file Phase 6 reads. Git history preserves the v1.0 shape. (Claude's discretion per CONTEXT.) |
| `taiki-e/install-action` for cargo-llvm-cov in ci.yml | `cargo install cargo-llvm-cov --locked` | The install-action fetches prebuilt binaries (~seconds vs ~5 min compile on hosted runners). Keep `cargo install --locked` if minimizing third-party actions in the secretless workflow; acceptable either way. |

**Installation (spike workflow steps, not Cargo.toml):**
```bash
# msvc leg
rustup target add x86_64-pc-windows-msvc && rustup component add llvm-tools
cargo install --locked --version 0.23.0 cargo-xwin
# gnu leg
rustup target add x86_64-pc-windows-gnu
sudo apt-get update && sudo apt-get install -y gcc-mingw-w64-x86-64
# musl legs (zig tarball pattern copied from existing spike file, host arch x86_64 on ubuntu-latest)
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
cargo install --locked --version 0.23.0 cargo-zigbuild
```

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| cargo-xwin | crates.io | since 2022-03 | 59,652/wk | github.com/rust-cross/cargo-xwin | OK | Approved (build tool only) |
| cargo-zigbuild | crates.io | since 2022-02 | 99,951/wk | github.com/rust-cross/cargo-zigbuild | OK | Approved (already in use, v1.0 pipeline) |

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none
No new runtime dependencies this phase — Cargo.toml is untouched.

## Architecture Patterns

### System Architecture Diagram (validation + error flow, this phase's delta)

```
MCP client                REST client
    │                          │
    ▼                          ▼
mcp.rs tools              rest/handlers.rs          (transports: deserialize, parse
 StoreArgs.ttl_secs        StoreRequest.ttl_secs     MemoryType wire string, ONE
 SearchArgs.limit          SearchRequest.limit       service call, map error — no
 ListArgs.limit            ListQuery.limit           validation logic added here)
    │                          │
    └───────────┬──────────────┘
                ▼
      MemoryService (service.rs)  ◄── NEW: validate_limit / validate_ttl at the TOP
        store()   ── validate ttl_secs ──► Err(InvalidArgument) ─┐   of each method,
        search()  ── validate limit  [REMOVE .max(0) at :168]    │   BEFORE any embed
        list()    ── validate limit                              │   or store call
        import()  ── validate each draft's ttl_secs              │
                │                                                │
                ▼                                                ▼
      Store trait → SqliteStore (sqlite.rs)          map_memory_error (handlers.rs)
        knn_search :279 ── json_each EXISTS (?5)       InvalidArgument → 400 BadRequest
        list       :369 ── json_each EXISTS (?3)     map_mcp_error (mcp.rs)
        search     :414 ── json_each EXISTS (?10)      InvalidArgument → invalid_params
        insert     :217 ── now + ttl (protected by D-02 ceiling upstream)
```

### Pattern 1: The Validation Seam (API-02) — exact call-path inventory

**Every surface that accepts `limit` or `ttl_secs` today (verified by grep + read):**

| Field | Wire surface | DTO | Flows to | Current behavior at boundary |
|-------|-------------|-----|----------|------------------------------|
| `ttl_secs` | MCP `memory_store` | `mcp.rs::StoreArgs.ttl_secs: Option<i64>` (line 72) | `NewMemory.ttl_secs` → `service.store()` → `sqlite.rs:217 expires_at = now + ttl` | Unvalidated: negative → already-expired row (swept next tick); `i64::MAX` → `now + ttl` overflow (panic in debug, wrap in release) |
| `ttl_secs` | REST `POST /api/memories` | `handlers.rs::StoreRequest.ttl_secs` (line 44) | same path | same |
| `ttl_secs` | CLI `import --from gsd-state` | `import/gsd_state.rs:184` — always `ttl_secs: None` | `service.import()` → `store.insert()` | Cannot currently be bad, but `import()` bypasses `store()` — the shared helper must be called there too so Phase 4-5 surfaces (jsonl import, update) inherit it (D-03) |
| `limit` | MCP `memory_search` | `mcp.rs::SearchArgs.limit` (line 109) | `service.search()` → `service.rs:168 .max(0)` silent clamp → `sqlite.rs:265` knn `k=(limit.max(1)*4).min(200)` / `sqlite.rs:421 LIMIT ?9` | `.max(0)`: `limit=-5` becomes 0 → empty results, silently; store search binds negative limit → SQLite `LIMIT -5` = **unlimited** |
| `limit` | MCP `memory_list` | `mcp.rs::ListArgs.limit` (line 89) | `service.list()` → `sqlite.rs:371 LIMIT CASE WHEN ?4 IS NULL THEN -1 ELSE ?4 END` | negative limit binds as-is → `LIMIT -5` = **unlimited** (differs from search — the WR finding) |
| `limit` | REST `POST /api/search` | `handlers.rs::SearchRequest.limit` (line 63) | same as MCP search | same |
| `limit` | REST `GET /api/memories?limit=` | `handlers.rs::ListQuery.limit` (line 80) | same as MCP list | same |
| `limit` | internal: `health_handler` | `handlers.rs:205 limit: Some(1)` | `service.list()` | Valid under 1..=200 — no change needed, but it WILL pass through the new validation (good: internal callers obey the same contract) |

**Insertion points (all in `service.rs`, before any embed/store call):**
- `store()` — validate `new.ttl_secs` first (before the embed call, so a bad TTL never costs an Ollama round-trip).
- `search()` — validate `args.limit`; then replace line 168 with `let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT) as usize;` (clamp removed per D-01).
- `list()` — validate `args.limit`.
- `import()` — validate each draft's `ttl_secs` (loop over drafts before the dedup hop; today always `None`, tomorrow the jsonl importer).

**Downstream clamp sites — leave the store untouched:**
- `sqlite.rs:265` knn `k = (limit.unwrap_or(50).max(1) * 4).min(MAX_KNN_K)` — the `.max(1)` becomes unreachable defensive code once the service validates; leave it (the `Store` trait is a public core API and the defensive floor costs nothing). Only `service.rs:168` is removed per D-01.
- `sqlite.rs:371` `LIMIT CASE WHEN ?4 IS NULL THEN -1 ELSE ?4 END` — the NULL→-1 (no limit) branch stays: omitted limit = unlimited list is existing documented behavior; validation only guarantees a *present* limit is 1..=200.

**Const placement (recommendation — Claude's discretion per CONTEXT):** the existing consts live in `sqlite.rs` (`DEFAULT_SEARCH_LIMIT` is `pub(crate)` at line 28; `MAX_KNN_K` is private at line 32). D-03 wants `pub` consts in core "next to" them. Recommended: define in `domain.rs` (beside `MemoryError`, the natural home for the contract):

```rust
// domain.rs — the validation contract (D-01/D-02/D-03)
pub const MIN_LIMIT: i64 = 1;
pub const MAX_LIMIT: i64 = 200;              // == MAX_KNN_K: a valid limit never exceeds the KNN cap
pub const MIN_TTL_SECS: i64 = 1;
pub const MAX_TTL_SECS: i64 = 3_155_760_000; // ~100 years; now + MAX_TTL_SECS ≈ 4.9e9 « i64::MAX — no overflow
```

and change `sqlite.rs:32` to `const MAX_KNN_K: i64 = crate::domain::MAX_LIMIT;` (single source of truth, keeps the D-01 alignment mechanical). Promote `DEFAULT_SEARCH_LIMIT` to `pub` (it must appear in the README API tables per D-03; it's already re-exported conceptually via service).

### Pattern 2: Error taxonomy extension (D-04/D-05) — the 02-05 pattern, one variant

```rust
// domain.rs — after InvalidQuery, client tier
#[error("invalid argument: {0}")]
InvalidArgument(String),
```

Message payload carries field + range + offending value (D-05): `format!("limit must be between {MIN_LIMIT} and {MAX_LIMIT} (got {l})")` → full Display: `invalid argument: limit must be between 1 and 200 (got 0)`. Same shape for ttl: `invalid argument: ttl_secs must be between 1 and 3155760000 (got -1)`.

Both mappers match exhaustively on `&e` — adding the variant is a compile error until both gain the arm (the enforcement working as designed):

```rust
// handlers.rs map_memory_error — extend the client arm:
MemoryError::InvalidType(_) | MemoryError::InvalidQuery(_) | MemoryError::InvalidArgument(_) => {
    ApiError::BadRequest(e.to_string())
}
// mcp.rs map_mcp_error — extend the invalid_params arm identically.
```

Test-stability rule (CONTEXT specifics): tests assert on the `limit must be between` / `ttl_secs must be between` prefix, never full-string equality.

### Pattern 3: json_each exact tag match (API-03) — exact SQL per site, empirically verified

Tags storage verified: `0001_init.sql:10` — `tags TEXT NOT NULL DEFAULT '[]' -- JSON array (serde_json)`; always written via `serde_json::to_string` (`sqlite.rs:215`), so every row holds a valid JSON array. `[VERIFIED: source read]`

The replacement predicate was **empirically verified this session** with sqlite3 against a mirror of the knn CTE-join shape and the plain-WHERE shape: exact match returns only the `rust`-tagged row (the LIKE form returned both `rust` and `rustling`); the `?N IS NULL OR EXISTS(...)` NULL-disable pattern still works; an empty `[]` tags array matches nothing without erroring; matching is case-sensitive (`Rust` ≠ `rust`). `[VERIFIED: sqlite3 empirical run 2026-07-12]`

| Site | Current (verified) | Replacement |
|------|--------------------|-------------|
| `sqlite.rs:279` (knn_search — outer WHERE after the `WITH knn AS (...)` CTE join) | `AND (?5 IS NULL OR m.tags LIKE '%' \|\| ?5 \|\| '%')` | `AND (?5 IS NULL OR EXISTS (SELECT 1 FROM json_each(m.tags) WHERE json_each.value = ?5))` |
| `sqlite.rs:369` (list — plain WHERE, unaliased table) | `AND (?3 IS NULL OR tags LIKE '%' \|\| ?3 \|\| '%')` | `AND (?3 IS NULL OR EXISTS (SELECT 1 FROM json_each(memories.tags) WHERE json_each.value = ?3))` |
| `sqlite.rs:414` (keyword search — WHERE alongside the FTS5 MATCH) | `AND (?10 IS NULL OR m.tags LIKE '%' \|\| ?10 \|\| '%')` | `AND (?10 IS NULL OR EXISTS (SELECT 1 FROM json_each(m.tags) WHERE json_each.value = ?10))` |

Notes:
- The predicate is NOT inside the CTE — it's in the outer SELECT that joins `knn` to `memories m`, so `json_each(m.tags)` is a standard correlated table-valued function. Verified working.
- Parameter indices and `params![]` bind lists are **unchanged** — same bound parameter, same injection posture (T-02-01 preserved).
- `json_each.value` as qualifier works because each predicate contains exactly one `json_each`; an alias (`json_each(m.tags) AS jt ... jt.value`) is equivalent if preferred for style.
- In the keyword-search site the FTS5 error mapping (`map_fts_query_error` routing at `sqlite.rs:446-449`) is untouched — a json_each predicate cannot produce an FTS5 parse error on valid stored rows.
- All three sites change **in one commit** with the shared regression fixture (D-09).

**Doc strings that must change in the same plan (agent-facing API, D-08):**
- `mcp.rs:82` (`ListArgs.tag`) and `mcp.rs:103` (`SearchArgs.tag`): "Filter by a tag substring" → "Filter by an exact tag (case-sensitive)".
- `handlers.rs:56` (`SearchRequest.tag`) and `handlers.rs:73` (`ListQuery.tag`): same wording fix.
- README API tables: tag filter semantics + the new bounds consts (D-03) + release-notes note for the behavioral change (feeds DIST-06 in Phase 6).

### Pattern 4: Spike workflow (D-10/D-11) — hosted, secretless, four legs

The existing `.github/workflows/spike-cross-compile.yml` is `workflow_dispatch`, `runs-on: [orangepi]` — permanently dead on this now-public repo. **Recommendation: rewrite it in place** (discretion): keep the dispatch-only trigger and the pinned-versions/host-arch-aware-smoke discipline, change the runner and matrix. Shape:

```yaml
name: Spike — Windows + musl cross-compile (Phase 3, D-10/D-11)
# VERDICT (Phase 3, D-11): <recorded after the dispatch runs — Phase 6 reads this comment>
on:
  workflow_dispatch:
permissions:
  contents: read          # secretless, read-only (ecosystem D-02 exception)
env:
  CARGO_TERM_COLOR: always
  ZIG_VERSION: '0.14.1'                 # pinned — zig↔zigbuild compat is version-coupled
  CARGO_ZIGBUILD_VERSION: '0.23.0'
  CARGO_XWIN_VERSION: '0.23.0'
  # musl typedef shim (DIST-05 groundwork) — target-suffixed so gnu/msvc legs are untouched
  CFLAGS_x86_64_unknown_linux_musl: "-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int32_t=uint32_t -Du_int64_t=uint64_t"
  CFLAGS_aarch64_unknown_linux_musl: "-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int32_t=uint32_t -Du_int64_t=uint64_t"
jobs:
  spike:
    runs-on: ubuntu-latest              # D-10: the only runnable option on this public repo
    strategy:
      fail-fast: false                  # surface ALL leg verdicts in one dispatch
      matrix:
        include:
          - { target: x86_64-unknown-linux-musl,  tool: zigbuild }
          - { target: aarch64-unknown-linux-musl, tool: zigbuild }
          - { target: x86_64-pc-windows-msvc,     tool: xwin }
          - { target: x86_64-pc-windows-gnu,      tool: mingw }
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: "${{ matrix.target }}" }
      - name: Install zig + cargo-zigbuild (musl legs)
        if: matrix.tool == 'zigbuild'
        run: |
          # official tarball, pinned, host arch = x86_64 on ubuntu-latest
          # (copy the existing spike file's case/uname block verbatim — it is host-arch-aware already)
          ...
          cargo install --locked --version "${CARGO_ZIGBUILD_VERSION}" cargo-zigbuild
      - name: Install cargo-xwin (msvc leg)
        if: matrix.tool == 'xwin'
        run: |
          rustup component add llvm-tools     # cargo-xwin prerequisite (upstream README)
          cargo install --locked --version "${CARGO_XWIN_VERSION}" cargo-xwin
      - name: Install mingw-w64 (gnu leg)
        if: matrix.tool == 'mingw'
        run: sudo apt-get update && sudo apt-get install -y gcc-mingw-w64-x86-64
      - name: Build
        run: |
          case "${{ matrix.tool }}" in
            zigbuild) cargo zigbuild --release --locked --target ${{ matrix.target }} -p agent-memory ;;
            xwin)     cargo xwin build --release --locked --target ${{ matrix.target }} -p agent-memory ;;
            mingw)    cargo build --release --locked --target ${{ matrix.target }} -p agent-memory ;;
          esac
      - name: Smoke (host-arch-aware)
        run: |
          # Windows: presence-only (agent-memory.exe); aarch64-musl: presence-only.
          # x86_64-musl: STATIC binary on an x86_64 host — actually run `--version`
          # (better signal than orangepi ever gave for this leg).
          ...
```

Key facts baked into this shape:
- **cargo-xwin prerequisites on ubuntu-latest:** `clang` is preinstalled on GitHub-hosted Ubuntu images `[ASSUMED: runner-images documentation — verify in the run itself]`; `rustup component add llvm-tools` is required `[CITED: cargo-xwin README]`. The Microsoft CRT/SDK license is accepted **implicitly by use** — no env var or flag needed ("By using this software you are consented to accept the license") `[CITED: cargo-xwin README]`. Ninja is only needed for CMake-based C deps — this workspace uses only the `cc` crate (bundled `sqlite3.c` + `sqlite-vec.c`), which clang-cl handles directly.
- **mingw leg needs no `.cargo/config`:** the repo has no `.cargo` dir (verified), and Rust's `x86_64-pc-windows-gnu` target defaults its linker to `x86_64-w64-mingw32-gcc`, which the apt package provides; the `cc` crate auto-detects `x86_64-w64-mingw32-gcc` for the C code `[ASSUMED: standard Rust target spec behavior — the spike run is itself the verification]`. Belt-and-braces alternative: set `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc` in the leg's env (self-documenting, zero risk).
- **cc-crate CFLAGS precedence:** the `cc` crate honors `CFLAGS_<target-with-underscores>` (and the hyphenated form, then `TARGET_CFLAGS`, then `CFLAGS`); the underscore form is what D-11 specifies and what milestone STACK.md verified — target-suffixed so darwin/gnu builds never see the `-Du_int*_t` macros `[VERIFIED: STACK.md 2026-07-12 + cc-rs documented convention]`.
- **Windows binary name:** `target/<triple>/release/agent-memory.exe` — the smoke step's `test -f` must add the `.exe` suffix for the two Windows legs.

**Dispatch + verdict flow (executor task):** `gh` 2.55.0 is installed and authenticated for github.com (verified); remote is `git@github.com:UnityInFlow/agent-memory.git`. Sequence: push the workflow to `main` → `gh workflow run spike-cross-compile.yml` → `gh run watch $(gh run list --workflow=spike-cross-compile.yml --limit 1 --json databaseId -q '.[0].databaseId')` → read per-leg conclusions via `gh run view --json jobs` → record the verdict (which legs green, exact flags) in the phase SUMMARY **and** as the comment at the top of the workflow file (D-11). Budget for one iterate-and-rerun cycle — the workflow is dispatchable independently of CI precisely so re-runs are cheap (CONTEXT specifics).

**D-12 ride-along (`ci.yml`):** trivially cheap and recommended. Verified current ci.yml: fmt → clippy `-D warnings` → build → test → `cargo install cargo-llvm-cov --locked` → `cargo llvm-cov --workspace --fail-under-lines 80`, matrix over two self-hosted labels. Migration = replace the matrix/`runs-on` with `ubuntu-latest`, add `permissions: contents: read`, keep every step (none needs secrets). This un-deadens the CI gate for all of Phase 3's own commits — do it FIRST in the phase so the hardening commits get CI coverage.

### Anti-Patterns to Avoid
- **Per-transport validation** ("bounds-check limit in the REST handler"): MCP inherits nothing; the surfaces already drift today (search clamps, list binds negative as unlimited). Validate once in `MemoryService` (D-06; milestone ARCHITECTURE.md Anti-Pattern 3).
- **Silent clamping instead of rejecting** (`.max(0)`, `.min(200)`): the requirement is explicit — reject with the range in the message so agents learn the bound.
- **Full-string equality assertions on error messages**: assert the `limit must be between` prefix; the exact value echo may evolve.
- **Adding the windows triple to a zigbuild matrix**: cargo-zigbuild supports only Linux/macOS targets — the Windows legs need cargo-xwin/mingw (milestone PITFALLS.md Pitfall 13).
- **Unsuffixed global `CFLAGS`** for the musl shim: bleeds `-Du_int*_t` into every leg; use the target-suffixed form only.
- **Validating in the Store**: the store trait's small defensive clamps stay as-is; putting rejection logic there duplicates the seam and entangles store errors with client errors.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Tag exact-match | Rust-side post-filtering of fetched rows | SQL `json_each` EXISTS predicate | Post-filtering breaks LIMIT semantics (under-fetch) and the KNN oversample math; json_each is exact, indexed-adjacent, bound-parameterized |
| Windows msvc cross | Custom clang-cl + xwin scripting | cargo-xwin 0.23.0 | It IS the clang-cl+xwin scripting, maintained by the same org as cargo-zigbuild |
| DTO validation framework | validator/garde derive stack | 2 small helper fns + consts | Explicitly out of scope (REQUIREMENTS.md); 3 checks at one seam |
| musl typedef fix | Forking/vendoring sqlite-vec.c | `CFLAGS_<triple>` env shim | Zero code change, trivially removable when sqlite-vec PR #199 ships in a pinned release |

**Key insight:** every hard problem in this phase already has a proven in-repo or upstream mechanism; the work is wiring, not invention.

## Common Pitfalls

### Pitfall 1: `ttl_secs = i64::MAX` overflows `now + ttl` at `sqlite.rs:217`
**What goes wrong:** `new.ttl_secs.map(|ttl| now + ttl)` — a huge TTL overflows: panic in debug builds, silent wrap to a negative `expires_at` in release (row swept immediately).
**Why it happens:** the arithmetic is unguarded; only the D-02 ceiling protects it.
**How to avoid:** the service-seam validation makes overflow unreachable (`now + 3_155_760_000 ≈ 4.9e9 « i64::MAX`). Include `i64::MAX` and `i64::MIN` in the boundary test matrix to lock it.
**Warning signs:** any code path building `NewMemory` that bypasses `validate_ttl` (watch Phase 4's `memory_update` and Phase 5's jsonl import).

### Pitfall 2: The boundary matrix must include the VALID boundaries, not just the invalid ones
**What goes wrong:** tests cover 0/-1/201 but not 1 and 200 — a later off-by-one (`<` vs `<=`) regresses valid input to a 400 unnoticed.
**How to avoid:** success criterion 1 explicitly requires "valid boundaries succeed". Matrix per field: limit ∈ {absent→50-default, 1 OK, 200 OK, 0 reject, -1 reject, 201 reject, i64::MAX reject}; ttl_secs ∈ {absent→no expiry, 1 OK, 3_155_760_000 OK, 0 reject, -1 reject, 3_155_760_001 reject, i64::MAX reject}.

### Pitfall 3: `import()` bypasses `store()` — validating only `store()` leaves a hole
**What goes wrong:** `MemoryService::import` builds rows via `store.insert` directly (verified, service.rs:258-319). Today's gsd importer hard-codes `ttl_secs: None`, so nothing bad can arrive — but Phase 5's jsonl importer feeds the same path with user-controlled files.
**How to avoid:** call the shared ttl helper per draft inside `import()` now (one loop line); D-03's "all future surfaces validate against the same consts" is then structural, not aspirational.

### Pitfall 4: MCP/REST doc strings still advertise substring matching after the D-07 fix
**What goes wrong:** agents read tool descriptions, not release notes. `mcp.rs:82/:103` and `handlers.rs:56/:73` say "Filter by a tag substring" — stale docs actively mislead agents into expecting over-match.
**How to avoid:** update all four doc comments + README in the same plan as the SQL change (they are the agent-facing API surface of the behavioral change, D-08).

### Pitfall 5: Coverage gate — spawned-binary tests contribute ZERO coverage
**What goes wrong:** proving the 400s only in `tests/rest.rs` (spawned binary, SIGKILL'd — never flushes LLVM profile data) leaves the new validation branches uncovered; `cargo llvm-cov --fail-under-lines 80` fails.
**How to avoid:** the 02-05 rule — coverage-bearing tests are the in-process ones (core unit tests + `handlers.rs` `mod tests` + `mcp.rs` `mod tests`); `tests/rest.rs` additions are realism-only.

### Pitfall 6: List's `LIMIT CASE WHEN ?4 IS NULL THEN -1` — don't confuse "omitted" with "invalid"
**What goes wrong:** omitted limit in list = unlimited (existing behavior, `sqlite.rs:371`); a hasty "all lists get DEFAULT_SEARCH_LIMIT" change would be a second silent behavioral change nobody decided.
**How to avoid:** validation constrains only Some(l); None semantics stay exactly as shipped (D-01 keeps defaults unchanged).

### Pitfall 7: The spike workflow file currently in the repo can never run — and neither can CI
**What goes wrong:** planning assumes "dispatch the existing spike workflow" — but `runs-on: [orangepi]` queues forever on this public repo (`allows_public_repositories: false`, OPS-02). Same for ci.yml: every Phase 3 commit lands with a dead CI gate until the ride-along.
**How to avoid:** the runner swap IS the first task of the spike plan; do the ci.yml `ubuntu-latest` migration early in the phase so the hardening commits get real CI runs (D-12 SHOULD).

### Pitfall 8: cargo-xwin first run downloads the MSVC CRT/SDK (~hundreds of MB)
**What goes wrong:** the xwin fetch happens inside the build step on every fresh runner; a slow/flaky download reads as a build failure.
**How to avoid:** accept it for a spike (one dispatch, four legs); if it flakes, add `XWIN_CACHE_DIR` + `actions/cache`. Do not gate the verdict on infra flakes — re-dispatch (that is why it's `workflow_dispatch`).

### Pitfall 9: Windows smoke must look for `agent-memory.exe`, not `agent-memory`
**What goes wrong:** copying the existing smoke step's `BIN="target/<triple>/release/agent-memory"` verbatim fails presence-check on the Windows legs even when the build succeeded — a false-negative verdict.
**How to avoid:** suffix-aware presence check; both Windows legs are presence-only (PE binaries don't run on Linux; no wine dependency for the verdict).

### Pitfall 10: json_each on a row whose `tags` is not valid JSON errors the whole query
**What goes wrong:** `json_each` raises `malformed JSON` if a row's tags column were corrupt — unlike LIKE, which just string-matches.
**Why it's acceptable:** tags are always written via `serde_json::to_string` with a `'[]'` schema default (verified); no code path writes non-JSON. A corrupt row would be external DB tampering — an internal-tier error is the honest outcome.
**How to avoid:** nothing to do in code; keep the empty-tags-array regression case (verified matching nothing without error) in the fixture.

## Code Examples

### Validation helpers (core, shared by all service methods)
```rust
// agent-memory-core — domain.rs (or a small validate module; discretion)
// Source: D-01/D-02/D-05 + existing MemoryError shape (domain.rs:119)
fn validate_limit(limit: Option<i64>) -> Result<(), MemoryError> {
    if let Some(l) = limit {
        if !(MIN_LIMIT..=MAX_LIMIT).contains(&l) {
            return Err(MemoryError::InvalidArgument(format!(
                "limit must be between {MIN_LIMIT} and {MAX_LIMIT} (got {l})"
            )));
        }
    }
    Ok(())
}

fn validate_ttl(ttl_secs: Option<i64>) -> Result<(), MemoryError> {
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

### service.rs:168 — the clamp removal (D-01)
```rust
// BEFORE (verified current):
let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).max(0) as usize;
// AFTER (validate_limit(args.limit)? already ran at the top of search()):
let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT) as usize;
```

### Regression fixture for the three tag sites (D-09)
```rust
// Store two rows: tags ["rust"] and tags ["rustling"], then per path assert
// tag=Some("rust") returns ONLY the ["rust"] row:
//  1. semantic path  — FakeEmbedder success harness (tests/semantic.rs pattern) → knn_search site
//  2. keyword path   — dead-embedder harness (tests/fallback.rs pattern; extends
//                      02-05's keyword_fallback_honors_tag_filter) → search site
//  3. list path      — plain store/list (tests/store.rs pattern) → list site
// Plus: tag=Some("Rust") matches nothing (case-sensitivity lock, D-08).
```

### In-process handler boundary test (the 02-05 layer-2 shape)
```rust
#[tokio::test]
async fn search_with_zero_limit_returns_400_never_500() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(&dir); // existing dead-Ollama harness in handlers.rs tests
    let err = search_handler(State(state), Json(SearchRequest {
        query: "anything".into(), r#type: None, tag: None, scope: None, limit: Some(0),
    })).await.expect_err("limit=0 is rejected");
    let resp = err.into_response();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains("limit must be between"));
}
```

### MCP mapper unit test extension (layer 3)
```rust
// mcp.rs mod tests — extend map_mcp_error_splits_client_and_internal_tiers:
assert_eq!(
    map_mcp_error(MemoryError::InvalidArgument("limit must be between 1 and 200 (got 0)".into())).code,
    invalid_params_code,
    "an out-of-bounds argument is client input → invalid_params"
);
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `tags LIKE '%'||?||'%'` substring over JSON text | `json_each` equality EXISTS | This phase (API-03) | Behavioral: `rust` no longer matches `rustling` — release-notes item for v0.1.0 (DIST-06) |
| Silent `.max(0)`/`.max(1)` clamps + `LIMIT -1` negative-as-unlimited | Reject at the seam with range-carrying 400/invalid_params | This phase (API-02) | Behavioral: previously-"accepted" garbage now rejects — agents learn the bound |
| Self-hosted-only workflows (orangepi/zigbuild, OPS-01) | GitHub-hosted secretless spike + CI for this public repo | This phase (D-10/D-12) | First UnityInFlow repo running the sanctioned D-02 hosted exception end-to-end; Windows via cargo-xwin is new territory for the whole ecosystem (mcp-hub deferred it) |

**Deprecated/outdated:** the in-repo `spike-cross-compile.yml` runner targeting and its 6-triple darwin matrix (v1.0 purpose fulfilled; darwin legs shipped in release.yml) — superseded by the Phase 3 hosted rewrite.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `gcc-mingw-w64-x86-64` is the correct Ubuntu apt package name for the x86_64 mingw cross-gcc | Standard Stack / spike YAML | Spike gnu leg fails at apt step — fix is a one-line package-name correction and re-dispatch (cheap; the workflow is dispatch-iterable by design) |
| A2 | ubuntu-latest preinstalls clang/lld sufficient for cargo-xwin's clang-cl backend | Spike workflow | msvc leg fails at build — add `sudo apt-get install -y clang lld` to the xwin leg and re-dispatch |
| A3 | Rust's `x86_64-pc-windows-gnu` target default linker resolves to `x86_64-w64-mingw32-gcc` with the apt package installed (no `.cargo/config` needed; repo has none) | Spike workflow | gnu leg link failure — set `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc` in the leg env (belt-and-braces recommended anyway) |
| A4 | `sqlite-vec.c` + bundled `sqlite3.c` compile under clang-cl (msvc) and mingw-gcc (gnu) for this workspace | Spike workflow | This is NOT an assumption to mitigate — it is the exact question the spike exists to answer (D-11); fallback ladder gnu → document-and-defer is pre-approved (STATE.md) |

All A1-A3 are self-verifying inside the spike run itself; none blocks planning. Everything else in this document is `[VERIFIED]` (code read / empirical sqlite3 run / registry seam) or `[CITED]` (upstream README).

## Open Questions (RESOLVED)

1. **Does the msvc leg build green?** — RESOLVED by design: answering this IS the spike's deliverable. Plan 03-02 T3 records the verdict and handles every outcome via the pre-approved ladder {msvc green → adopt msvc; gnu-only green → adopt gnu; both red → document-and-defer}. (What we knew: cc/clang-cl is cargo-xwin's core path, rusqlite recommends `bundled` on Windows, sqlite-vec ships official Windows artifacts — but this workspace had never been compiled for Windows.)
2. **`deny_unknown_fields` on REST DTOs (discretion)** — RESOLVED: NOT added (plan 03-01 objective, discretion exercised). It would be a third behavioral change (currently-ignored unknown fields start rejecting); skipping keeps the release-notes delta to exactly the two decided changes.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain (cargo/rustc) | build + all tests | ✓ | 1.94.1 | — |
| sqlite3 CLI | (used this session to verify SQL only) | ✓ | system | not needed by plans |
| gh CLI (authenticated) | spike dispatch + verdict readback | ✓ | 2.55.0, logged in (hermanngeorge15), remote UnityInFlow/agent-memory | manual dispatch via GitHub UI |
| Ollama | NOT needed | n/a | — | all tests use dead-loopback embedder / FakeEmbedder |
| GitHub-hosted ubuntu-latest | spike + ci.yml ride-along | ✓ (public repo) | — | none needed — this IS the fallback for the dead self-hosted fleet |
| cargo-llvm-cov | coverage gate (local/CI) | install step in ci.yml | — | `cargo install cargo-llvm-cov --locked` |

**Missing dependencies with no fallback:** none.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness (`cargo test`), tokio::test for async; no external framework |
| Config file | none needed — workspace `Cargo.toml` (verified: members = core + binary crates) |
| Quick run command | `cargo test -p agent-memory-core` (core) / `cargo test -p agent-memory` (transports) |
| Full suite command | `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| API-02 | limit/ttl bounds reject via `InvalidArgument` at the seam; boundaries succeed | unit (core, layer 1) | `cargo test -p agent-memory-core --test fallback` (or a new `tests/validation.rs`) | ❌ new tests, existing harness patterns (`tests/fallback.rs` dead-embedder) |
| API-02 | REST 400 with `limit must be between` body, never 500 | in-process handler (layer 2, coverage-bearing) | `cargo test -p agent-memory --lib rest::handlers::tests` | ✅ `handlers.rs mod tests` — extend |
| API-02 | MCP `invalid_params` for `InvalidArgument` | MCP unit (layer 3) | `cargo test -p agent-memory --lib mcp::tests` | ✅ `mcp.rs mod tests` — extend |
| API-02 | Real-HTTP 400 on the spawned binary (realism only) | e2e (layer 4) | `cargo test -p agent-memory --test rest` | ✅ `tests/rest.rs` — extend |
| API-03 | `tag=rust` excludes `rustling` on the semantic (knn) path | integration (FakeEmbedder success) | `cargo test -p agent-memory-core --test semantic` | ✅ `tests/semantic.rs` — extend |
| API-03 | same on keyword-fallback path | integration (dead embedder) | `cargo test -p agent-memory-core --test fallback` | ✅ `tests/fallback.rs` — extends 02-05's `keyword_fallback_honors_tag_filter` |
| API-03 | same on list path + case-sensitivity (`Rust` ≠ `rust`) | integration | `cargo test -p agent-memory-core --test store` | ✅ `tests/store.rs` — extend |
| Spike | 4 matrix legs build; verdict recorded | CI dispatch (manual trigger, automated legs) | `gh workflow run spike-cross-compile.yml` then `gh run watch <id>`; per-leg conclusions via `gh run view --json jobs` | ❌ workflow rewrite (Wave 0 of the spike plan) |
| Spike | verdict persisted for Phase 6 | manual-only (justified: prose in SUMMARY + workflow comment) | grep `VERDICT` in the workflow file | — |

### Sampling Rate
- **Per task commit:** `cargo test -p <touched-crate>` + `cargo clippy --workspace --all-targets -- -D warnings` (CLAUDE.md gate) + `cargo fmt --check`
- **Per wave merge:** `cargo test --workspace` (66+ tests currently green)
- **Phase gate:** full suite + `cargo llvm-cov --workspace --fail-under-lines 80` (the >80% CLAUDE.md rule; remember layer-4 spawned-binary tests contribute zero coverage — the in-process layers carry the gate) before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] Spike workflow rewrite (`.github/workflows/spike-cross-compile.yml` → hosted 4-leg matrix) — the spike cannot run at all until this lands
- [ ] `ci.yml` → `ubuntu-latest` ride-along (D-12 SHOULD) — do first so every subsequent phase commit gets a live CI gate
- No test-framework gaps: all four test layers exist with proven harnesses (dead-loopback embedder, FakeEmbedder, tempdir stores, spawned-binary REST); new test fns slot into existing files/patterns

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | Unauthenticated-by-design local tool; loopback-guarded (`ensure_bind_allowed`, unchanged) |
| V3 Session Management | no | Stateless request/response |
| V4 Access Control | no | Single-user local; unchanged this phase |
| V5 Input Validation | **yes — this IS the phase** | Manual bounds checks at the service seam + typed `MemoryError::InvalidArgument`; no validation crate (locked decision) |
| V6 Cryptography | no | None touched |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via tag filter | Tampering | json_each predicate keeps the tag as a bound parameter (same posture as the LIKE it replaces — T-02-01 preserved; verified all three `params![]` lists unchanged) |
| Internal-detail disclosure via error bodies | Information Disclosure | D-05 message echoes only client-sent values + the public range; internal tier stays generic (02-05 contract) |
| Server-fault probing via crafted inputs (500s on bad input) | Information Disclosure / DoS | The API-02 rejection path: bad input can never reach `now + ttl` overflow or negative-LIMIT SQL |
| CI secret exfiltration via public-repo workflows | Elevation of Privilege | D-10: spike + ci.yml are secretless, `permissions: contents: read`, on GitHub-hosted runners only (OPS-02 keeps self-hosted unreachable from this repo) |
| Malicious build tooling | Tampering | cargo-xwin / cargo-zigbuild verified OK via legitimacy seam (rust-cross org, 59k/99k weekly downloads); versions pinned, `--locked` installs |

## Project Constraints (from CLAUDE.md)

- Rust stable, edition 2021; `thiserror` in the core library (the new variant follows), `anyhow` only at binary edges.
- **No `unwrap()` in production code** — validation helpers use `Result` + `?` throughout; tests may `expect` (existing convention).
- **Exhaustive pattern matches, no catch-all `_`** — the two error mappers already comply; keep the new arms explicit (`InvalidArgument(_)` named, not `_`).
- `cargo fmt` before every commit; `cargo clippy -- -D warnings` must pass (note 02-05 lesson: `items_after_test_module` — keep test mods at end of file).
- Test coverage >80% on core logic (in-process tests carry it; ci.yml enforces `--fail-under-lines 80`).
- No secrets committed; the spike workflow is deliberately secretless.
- CI runner policy: the inner CLAUDE.md's "never ubuntu-latest" is **superseded for this repo** by the wrapper CLAUDE.md OPS-01/OPS-02 + phase CONTEXT D-10: public repo ⇒ self-hosted jobs never run ⇒ GitHub-hosted is the sanctioned (and only) option. Cite D-10/OPS-02 in the workflow comments so the contradiction is documented, not silent.
- Zero cloud dependency preserved: no test in this phase needs Ollama or network.

## Sources

### Primary (HIGH confidence)
- Codebase read this session (2026-07-12): `crates/agent-memory-core/src/{service.rs, domain.rs, store/sqlite.rs}`, `crates/agent-memory/src/{mcp.rs, rest/mod.rs, rest/handlers.rs}`, `crates/agent-memory-core/sql/0001_init.sql`, `.github/workflows/{ci.yml, spike-cross-compile.yml}`, root `Cargo.toml` — every line number cited above verified directly
- Empirical sqlite3 run (this session): json_each EXISTS predicate in CTE-join + plain-WHERE contexts; LIKE over-match reproduction; NULL-disable; empty-array; case-sensitivity
- `gsd-tools query package-legitimacy` (crates.io registry): cargo-xwin OK, cargo-zigbuild OK
- `.planning/milestones/v1.0-phases/02-semantic-search-interop-release/02-05-SUMMARY.md` — the four-layer test pattern + exhaustive-mapper precedent
- Local environment probes: cargo 1.94.1, gh 2.55.0 authenticated, no `.cargo/` dir, no graph.json

### Secondary (MEDIUM confidence)
- [cargo-xwin README](https://github.com/rust-cross/cargo-xwin) (fetched this session) — install, `rustup component add llvm-tools`, implicit license acceptance, clang-cl backend, Ninja-only-for-CMake
- `.planning/research/STACK.md` / `ARCHITECTURE.md` / `PITFALLS.md` (2026-07-12 milestone research) — cargo-zigbuild no-Windows fact, CFLAGS shim mapping, pin triangle, validation-crate rejection; built on, not repeated

### Tertiary (LOW confidence)
- Ubuntu runner-image tool inventory (clang preinstalled) and `gcc-mingw-w64-x86-64` apt name — flagged in Assumptions Log A1/A2; self-verifying inside the spike run

## Metadata

**Confidence breakdown:**
- Validation seam (API-02): HIGH — every call path read in source; clamp sites, DTOs, and mappers enumerated with line numbers
- Tag matching (API-03): HIGH — replacement SQL empirically executed against a structural mirror of all three sites
- Spike toolchain: MEDIUM — tool identities/versions/prereqs verified; whether this workspace's C code compiles for Windows is the spike's own question (by design)
- Pitfalls: HIGH — grounded in read source + 02-05/mcp-hub precedents

**Research date:** 2026-07-12
**Valid until:** ~2026-08-11 for the code-level findings (stable, single-repo); re-check cargo-xwin/zig versions and sqlite-vec PR #199 status if the spike slips past that

---
*Phase: 3 — API Hardening & Toolchain Spikes*
*Researched: 2026-07-12*
