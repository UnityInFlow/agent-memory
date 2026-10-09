# Stack Research

**Domain:** v1.1 milestone additions to a shipped local-first Rust MCP memory daemon — hybrid RRF search, memory_update/relations, REST input hardening, Windows + musl binaries, portable export
**Researched:** 2026-07-12
**Confidence:** HIGH for "no new crates needed" items (verified against the pinned graph and official sqlite-vec docs); MEDIUM for cross-compile toolchain claims (verified against upstream READMEs/PRs but not yet spiked on the actual runner)

> **Verdict in one line:** Almost everything in this milestone is **zero new runtime dependencies** — RRF is pure SQL on the already-bundled SQLite 3.51.3, update/relations is a `rusqlite_migration` step, REST hardening is manual bounds checks on the existing two-tier error taxonomy, and export is `serde_json` JSONL + `VACUUM INTO`. The only stack additions are **build-side**: `cargo-xwin 0.23` for Windows (cargo-zigbuild explicitly does NOT support Windows targets) and a `CFLAGS_<target>` typedef shim for musl (upstream fix PR #199 is still unmerged; crate 0.1.9 vendors the broken C).

---

## Recommended Stack

### Core Technologies (unchanged — validated in v1.0, listed for the pin contract)

| Technology | Version | Purpose | Why It Must Not Move |
|------------|---------|---------|----------------------|
| `rusqlite` | `0.39` (`bundled`, `functions`) | Embedded SQLite (ships **SQLite 3.51.3**) | Shares `libsqlite3-sys 0.37` with `r2d2_sqlite 0.34`; 0.40 pulls libsqlite3-sys 0.38 → `links = "sqlite3"` conflict. **Nothing in this milestone requires bumping it.** |
| `rusqlite_migration` | `2.5` | Schema migrations (relations table, any update-path columns) | 2.6 jumps to rusqlite ^0.40 — stay on 2.5 to hold the libsqlite3-sys 0.37 pin. |
| `sqlite-vec` | `0.1.9` | vec0 KNN sidecar | Still `max_stable_version` on crates.io (verified 2026-07-12; 0.1.10 is alpha-only, latest alpha.4 2026-05-18). Its only build-dep is `cc`, so it stays outside the libsqlite3-sys pin. The musl fix is a build-flag workaround, **not** a version bump. |
| `rmcp` | `1.8` | MCP server (gains `memory_update`, `memory_link`, `memory_relations` tools) | New tools are just more `#[tool]` methods on the existing router — no SDK change needed. |
| `axum` | `0.8` | REST mirror (gains PATCH/update + relations routes, hardened validation) | No change; validation is handler-level. |
| `serde` / `serde_json` | `1` / `1` | JSONL export/import records | Already pinned; JSONL is `to_writer` + `\n` per record. |
| `chrono` | `0.4` (`default-features=false`, `features=["now"]`) | UTC timestamps | **Do not touch** — the trimmed feature set is what keeps zig darwin cross-compiles linking; it also happens to be Windows-safe (no `iana-time-zone`, no `Local`). |

### New Additions (this milestone)

| Item | Version | Purpose | Kind |
|------|---------|---------|------|
| **`cargo-xwin`** | `0.23.0` (2026-06-16, verified crates.io) | Cross-compile `x86_64-pc-windows-msvc` from the ARM64 Linux runner | **Build tool only** — installed on the runner, not a dependency |
| **`clang` / `llvm` + `lld`** (Debian arm64 pkgs) | distro current | Backend `cargo-xwin` drives (`clang-cl` against the xwin-fetched MSVC CRT); clang cross-targets x86_64-windows from aarch64 hosts natively | Runner provisioning |
| **`CFLAGS_x86_64_unknown_linux_musl` / `CFLAGS_aarch64_unknown_linux_musl` env shim** | n/a | `-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int32_t=uint32_t -Du_int64_t=uint64_t` — the `cc` crate honors per-target CFLAGS, so the vendored `sqlite-vec.c` BSD typedefs resolve without forking anything | Release-workflow env var |
| **`base64`** *(optional, only if `--include-embeddings` export ships)* | `0.22` | Encode 768-dim f32 blobs into JSONL | Runtime dep — **defer unless the feature is confirmed**; default export should exclude embeddings and re-embed on import |

**Explicitly NOT added:** no validation crate (`validator`/`garde`/`axum-valid`), no new search crate, no export/serialization framework, no second SQLite stack. Rationale per feature below.

---

## Per-Feature Stack Analysis

### 1. Hybrid RRF search (SEARCH-04) — zero new dependencies

Pure SQL on the existing engine. The **official sqlite-vec example** (nbc-headlines, Context7-verified) is exactly this shape:

```sql
WITH vec_matches AS (
  SELECT memory_id, row_number() OVER (ORDER BY distance) AS rank_number, distance
  FROM vec_memories WHERE embedding MATCH :query_vec AND k = :k
),
fts_matches AS (
  SELECT rowid AS memory_id, row_number() OVER (ORDER BY rank) AS rank_number, rank AS score
  FROM fts_memories WHERE fts_memories MATCH :query LIMIT :k
)
SELECT ...,
  coalesce(1.0 / (:rrf_k + fts_matches.rank_number), 0.0) * :weight_fts
+ coalesce(1.0 / (:rrf_k + vec_matches.rank_number), 0.0) * :weight_vec AS combined_rank
FROM fts_matches
FULL OUTER JOIN vec_matches USING (memory_id)
JOIN memories ON memories.id = coalesce(fts_matches.memory_id, vec_matches.memory_id)
ORDER BY combined_rank DESC;
```

- **Engine requirements:** window functions (SQLite ≥ 3.25) and `FULL OUTER JOIN` (SQLite ≥ 3.39). libsqlite3-sys 0.37 bundles **SQLite 3.51.3** (verified docs.rs) — both available. No pin change.
- **Conventions:** `rrf_k = 60` (the literature default), equal weights 1.0/1.0 to start; expose weights as config later if recall tuning demands it.
- **Integration notes:** FTS5 `rank` is negative-BM25 (ascending = best), so `ORDER BY rank` is already correct. The existing decay blend (registered `exp` scalar function) composes on top: apply decay as a multiplier on `combined_rank`, keeping one ranking pipeline. Keep the existing keyword-only fallback path — RRF degrades to FTS-only when Ollama is absent (SC2 behavior preserved) simply because `vec_matches` is empty and `coalesce` handles it.

### 2. `memory_update` + relation/link tools (MCP-06) — zero new dependencies

- **Schema:** one new `rusqlite_migration 2.5` step: a `memory_relations(from_id, to_id, relation_type, created_at)` table with FKs to `memories` and `ON DELETE CASCADE` (so `memory_forget` and TTL sweep clean up edges for free). Pinned 2.5 handles this fine — additive migrations are its bread and butter.
- **Caveat:** SQLite enforces FKs only when `PRAGMA foreign_keys = ON` is set **per connection** — set it in the existing r2d2 pool's connection customizer (alongside the WAL/busy_timeout pragmas you already set), or the CASCADE silently never fires.
- **MCP surface:** new `#[tool]` methods on the existing rmcp 1.8 router; no SDK feature change. Reuse the shared `SearchOutcome`-style envelope so MCP/REST wire shapes stay aligned (existing decision).
- **Update semantics:** `memory_update` re-embedding on content change goes through the existing single-writer lane + Ollama client; nothing new. If content changes, the FTS5 row and vec0 row must be updated in the same transaction as the base row — same pattern the store already uses for insert.

### 3. REST input hardening (WR-01/02/07) — zero new dependencies; do NOT add a validation crate

- **Recommendation: manual validation at the shared store seam**, mapped through the existing two-tier taxonomy (`InvalidQuery → 400/invalid_params`). The surface is tiny — `limit` bounds (e.g. 1..=1000), `ttl_secs` bounds (reject 0/negative/absurd), exact tag-match semantics — three checks do not justify a derive-macro dependency tree.
- Add `#[serde(deny_unknown_fields)]` to REST request DTOs as part of hardening (free, catches client typos like `tags_` silently matching nothing).
- Because validation lives at the **store seam**, MCP and REST get identical enforcement for free — same argument that won for the error taxonomy in 02-05.
- `validator`/`garde` via `axum-valid 0.24` is the ecosystem answer **only when** you have many DTOs with cross-field rules. Revisit if the REST surface grows past ~10 endpoints.

### 4. Windows binaries (DIST-03) — build-tooling change, likely small code change

- **`cargo-zigbuild` cannot do this**: its README states only Linux and macOS targets are supported. Windows requires a different tool — this is a hard fact, not a preference.
- **Primary path: `cargo-xwin 0.23.0` → `x86_64-pc-windows-msvc`.** Same `rust-cross` org as cargo-zigbuild (consistent tooling family), runs on Linux hosts including aarch64 (it's a cargo subcommand needing only `clang`; clang cross-targets x86_64-windows from ARM64). It fetches the MSVC CRT/SDK via xwin — note the **Microsoft license acceptance** step (`--accept-license` / env) in CI. MSVC is the tier-1 Windows ABI users expect.
- **C-code risk (the real spike):** `libsqlite3-sys` bundled compiles `sqlite3.c` via `cc`, and rusqlite docs explicitly recommend `bundled` for Windows; `cc`-driven C under cargo-xwin (clang-cl) is that tool's core scenario. `sqlite-vec.c` must also compile under clang-cl — sqlite-vec upstream ships official Windows artifacts, so the C is MSVC-clean, but **spike the full workspace build first** before planning the release around it.
- **Fallback path: `x86_64-pc-windows-gnu` via mingw-w64** (`gcc-mingw-w64-x86-64` exists for arm64 Debian hosts). Well-trodden for rusqlite-bundled; larger binaries, but avoids the MSVC CRT licensing step entirely.
- **cfg(unix) audit (mcp-hub precedent) — expect this to be SMALL here:** agent-memory does not use the deps that sank mcp-hub (dialoguer/comfy_table/owo_colors); `dirs 6`, trimmed `chrono` (`now` works on Windows), rmcp stdio, axum, tokio are all Windows-clean. The likely offenders are direct `std::os::unix` usage — e.g. `PermissionsExt` 0o600 on the DB file and any Unix-path/loopback assumptions. Gate those behind `#[cfg(unix)]` with a Windows equivalent (or documented no-op) rather than refactoring dependencies.
- **Skip `aarch64-pc-windows-msvc`** this milestone — tiny audience, doubles the spike surface.

### 5. musl binaries — CFLAGS shim now, upstream later

- **Root cause confirmed:** `sqlite-vec.c` carries platform-conditional BSD `u_int*_t` typedef fallbacks that break on musl. Upstream fix **PR asg017/sqlite-vec#199 is still open/unmerged** (verified 2026-07-12); multiple users confirm it fixes Alpine builds; **no crate release contains it** — Rust crate stable remains 0.1.9 with the broken vendored C (a June 2026 PR comment says newest upstream C doesn't need the patch, but that hasn't reached the crate).
- **Recommended fix: per-target CFLAGS injection in the release workflow** — the `cc` crate (sqlite-vec's only build-dep) honors `CFLAGS_<target-with-underscores>`:

  ```yaml
  env:
    CFLAGS_x86_64_unknown_linux_musl: "-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int32_t=uint32_t -Du_int64_t=uint64_t"
    CFLAGS_aarch64_unknown_linux_musl: "-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int32_t=uint32_t -Du_int64_t=uint64_t"
  ```

  Zero code change, zero fork, trivially removable when upstream ships. (If the failure mode turns out to be *re*definition rather than missing definition, the `-D` mapping still works — C11 permits identical typedef redefinition. `-D_GNU_SOURCE` is a second-choice shim since musl defines `u_int*_t` under it, but it widens the macro surface; prefer the targeted `-D`s.)
- **Do NOT** fork/vendor sqlite-vec or move to 0.1.10-alpha for this — alpha in a release graph for a typedef workaround is bad trade.
- musl targets themselves go through the **existing cargo-zigbuild** path (zig ships musl headers/libc) — same runner, just two more triples in the matrix. Track PR #199 / crate 0.1.10-stable as the shim's retirement condition.

### 6. Portable export (DIST-04) — JSONL primary, `VACUUM INTO` as full-fidelity backup; zero new required deps

- **`agent-memory export` → JSONL** (one JSON object per line, `serde_json` already pinned): schema-versioned header line (`{"format":"agent-memory-export","version":1,...}`), then one record per memory (id, type, content, tags, timestamps, decay inputs, relations). **Exclude embeddings by default** — they are re-derivable from content via Ollama on import, model/dim may differ across machines, and it keeps exports human-readable/diffable/greppable. This is the *interop* format (feeds DIST-goal "portable file" and future cross-tool import).
- **Import** = the existing idempotent-import machinery (GSD import precedent) pointed at JSONL; re-embed on ingest with the same graceful no-Ollama fallback.
- **`agent-memory backup` → `VACUUM INTO 'file.db'`** (SQLite ≥ 3.27; bundled 3.51.3 has it): one SQL statement, safe against a live WAL database, produces a minimal single-file consistent snapshot — strictly better than copying the db file (torn-copy risk with WAL). Full fidelity including vec0/FTS shadow tables. This is the *backup* format. Both commands, ~zero new code surface, no new crates.
- **Optional:** `--include-embeddings` (base64 f32 blobs, add `base64 0.22`) only if a concrete offline-import-without-Ollama use case is confirmed — otherwise skip the dep.

---

## Installation

```bash
# Runtime dependencies: NO changes to Cargo.toml required for features 1–3, 6
# (optional, only if --include-embeddings export is confirmed)
# cargo add base64@0.22

# Runner provisioning (release workflow, not deps)
cargo install cargo-xwin --version 0.23.0   # Windows msvc cross
sudo apt install clang lld                   # cargo-xwin backend (arm64 host OK)
rustup target add x86_64-pc-windows-msvc x86_64-unknown-linux-musl aarch64-unknown-linux-musl
# cargo-zigbuild + zig 0.14.1 already provisioned (existing pipeline) — used for musl, NOT Windows
```

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| Pure-SQL RRF (one query, FULL OUTER JOIN CTEs) | Fetch two result sets, fuse in Rust | If the SQL becomes unreadable once decay-blend + tag-filter + RRF compose, in-Rust fusion of two simple queries is legitimate and testable — same complexity class, k is small. Not a dependency question either way. |
| Manual validation at the store seam | `axum-valid 0.24` + `garde`/`validator` | REST surface grows to many DTOs with cross-field rules; today it's 3 bounds checks. |
| `cargo-xwin` → windows-msvc | mingw-w64 → windows-gnu | If the clang-cl spike fails on `sqlite3.c`/`sqlite-vec.c`, or the MS CRT license step is unacceptable in CI. Known-good with rusqlite-bundled; ship gnu rather than slipping the milestone. |
| CFLAGS typedef shim for musl | Wait for upstream PR #199 / crate 0.1.10 stable | Only if it merges + releases before the milestone ships — then delete the shim instead of adding it. |
| JSONL export (embeddings excluded) | `VACUUM INTO` **as the export format** | If "portable file" is interpreted as *machine migration* rather than *interop*: VACUUM INTO preserves everything bit-perfectly but is opaque, version-locked to the schema, and not diffable. Recommendation: ship **both** commands with distinct names (export vs backup) so the semantics stay honest. |
| JSONL | SQL text dump (`.dump`-style) | Never for this tool — ties consumers to SQLite semantics, worse than JSONL for cross-tool interop, worse than VACUUM INTO for fidelity. |

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| **`cargo-zigbuild` for Windows targets** | Upstream README: only Linux and macOS targets supported. This will burn a spike for nothing. | `cargo-xwin 0.23` (msvc) or mingw-w64 (gnu) |
| **rusqlite 0.40 / rusqlite_migration 2.6 bump** | Pulls libsqlite3-sys 0.38 → `links = "sqlite3"` conflict with `r2d2_sqlite 0.34`. Nothing in this milestone needs it — SQLite 3.51.3 already has every SQL feature RRF requires. | Keep 0.39 / 2.5 pin |
| **`sqlite-vec 0.1.10-alpha.*`** to dodge the musl typedefs | Alpha in a release binary graph; the CFLAGS shim achieves the same with zero risk. | 0.1.9 + per-target CFLAGS |
| **Forking/vendoring sqlite-vec.c** for the musl fix | Permanent maintenance burden for a 4-macro workaround; upstream fix exists and will land eventually. | CFLAGS shim, tracked retirement condition |
| **`validator`/`garde` derive stack** for 3 bounds checks | Dependency tree + macro surface for what is one small function at the store seam; also splits validation away from the proven two-tier taxonomy. | Manual checks → `InvalidQuery → 400/invalid_params` |
| **Reintroducing `chrono` default features or `Local`** (tempting on Windows work) | `iana-time-zone → core-foundation-sys` breaks the zig darwin cross-link (02-04 spike failure, standing decision). | Stay UTC-only, `features=["now"]` |
| **Embedding blobs in default JSONL export** | Ties exports to embedding model/dim, bloats files ~4KB/record, kills diffability; embeddings are derivable data. | Re-embed on import; optional `--include-embeddings` flag if truly needed |
| **Raw file-copy "backup"** of a live WAL database | Torn copies — WAL content not yet checkpointed into the main file. | `VACUUM INTO` |

## Stack Patterns by Variant

**If the cargo-xwin spike passes cleanly (expected):**
- Windows release job = same ARM64 runner, `cargo xwin build --release --target x86_64-pc-windows-msvc`; smoke test must be host-arch-aware (cannot execute the .exe on the ARM64 Linux host — same lesson as mcp-hub's `Exec format error`; use `wine` only if provisioned, otherwise skip execution and verify artifact presence + size).

**If the cargo-xwin spike fails on the C code:**
- Fall back to windows-gnu via mingw-w64 in the same workflow slot; do not block the milestone re-debugging clang-cl. Document msvc as a follow-up (mirrors the mcp-hub HUB-V2 document-and-defer pattern, but with a shipping fallback instead of a deferral).

**If upstream sqlite-vec releases the musl fix mid-milestone:**
- Delete the CFLAGS env lines, rebuild musl targets, done. The shim is designed to be removed.

**If RRF + decay + tag-filter in one SQL statement gets unwieldy:**
- Keep the two CTE queries separate, fuse + decay-blend in Rust. The contract (SearchOutcome envelope) doesn't change; only the internals do.

## Version Compatibility

| Package A | Compatible With | Notes |
|-----------|-----------------|-------|
| `rusqlite 0.39` (bundled) | SQLite **3.51.3** | Verified docs.rs (libsqlite3-sys 0.37). Window functions (≥3.25), FULL OUTER JOIN (≥3.39), `VACUUM INTO` (≥3.27) all available — RRF and backup need no engine change. |
| `rusqlite 0.39` + `rusqlite_migration 2.5` + `r2d2_sqlite 0.34` | `libsqlite3-sys 0.37` | The standing pin triangle — any single bump breaks `links = "sqlite3"`. Unchanged this milestone. |
| `sqlite-vec 0.1.9` | musl targets | **Only with** the `CFLAGS_<target>` typedef shim (upstream PR #199 unmerged as of 2026-07-12). |
| `sqlite-vec 0.1.9` + `libsqlite3-sys 0.37` bundled | `x86_64-pc-windows-msvc` via cargo-xwin 0.23 | Expected-good (cc/clang-cl is cargo-xwin's core path; rusqlite recommends bundled on Windows; sqlite-vec ships official Windows artifacts) — **but unproven for this workspace; spike first.** |
| `cargo-zigbuild` (zig 0.14.1) | linux-gnu, linux-musl, apple-darwin | Existing pipeline + two musl triples. **NOT Windows.** |
| `chrono 0.4` (`now` only) | Windows msvc/gnu targets | `Utc::now` works without the `clock`/`iana-time-zone` chain; the darwin-motivated trim is Windows-safe too. |
| `memory_relations` FK CASCADE | r2d2 pool | Requires `PRAGMA foreign_keys = ON` per pooled connection (connection customizer), or CASCADE never fires. |

## Sources

- Context7 `/asg017/sqlite-vec` (nbc-headlines hybrid-search example) — exact RRF SQL pattern (CTEs, rrf_k=60, weighted coalesce, FULL OUTER JOIN) — MEDIUM
- [docs.rs libsqlite3-sys 0.37](https://docs.rs/crate/libsqlite3-sys/0.37.0) — bundled = SQLite 3.51.3 — HIGH (official docs)
- [rust-cross/cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild) — "only Linux and macOS targets are supported" — MEDIUM (upstream README via search)
- [rust-cross/cargo-xwin](https://github.com/rust-cross/cargo-xwin) + [crates.io](https://crates.io/crates/cargo-xwin) — 0.23.0 stable (2026-06-16), Linux-host Windows-msvc cross via xwin/clang-cl — MEDIUM
- [asg017/sqlite-vec PR #199](https://github.com/asg017/sqlite-vec/pull/199) — musl typedef fix, **still open/unmerged 2026-07-12**, confirmed working on Alpine by users; no release contains it — MEDIUM (verified directly on the PR page)
- [crates.io sqlite-vec versions](https://crates.io/api/v1/crates/sqlite-vec) — 0.1.9 remains max_stable (0.1.10-alpha.4, 2026-05-18) — HIGH (registry API)
- [rusqlite README](https://github.com/rusqlite/rusqlite) — bundled uses `cc`, recommended for Windows — HIGH (official)
- [axum validator example](https://github.com/tokio-rs/axum/blob/main/examples/validator/src/main.rs) + [axum-valid](https://github.com/gengteng/axum-valid) — validation-crate landscape (informed the "don't add one" call) — LOW/MEDIUM
- [SQLite VACUUM INTO discussions](https://oldmoe.blog/2024/04/30/backup-strategies-for-sqlite-in-production/) — live-WAL-safe single-statement backup semantics — LOW (cross-consistent with sqlite.org lang_vacuum)
- In-repo: `Cargo.toml` workspace pins + PROJECT.md Key Decisions (libsqlite3-sys pin triangle, chrono trim, error taxonomy, mcp-hub Windows precedent) — HIGH

---
*Stack research for: agent-memory v1.1 Hardening & Interop milestone*
*Researched: 2026-07-12 (supersedes 2026-06-24 v1.0 stack research, which is fully validated/shipped)*
