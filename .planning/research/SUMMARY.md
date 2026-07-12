# Project Research Summary

**Project:** agent-memory — v1.1 "Hardening & Interop" milestone
**Domain:** Local-first Rust + SQLite + MCP memory daemon (shipped v0.0.1) — hybrid RRF search, memory_update/relations, REST input hardening, Windows/musl binaries, portable export
**Researched:** 2026-07-12
**Confidence:** HIGH

## Executive Summary

This milestone adds six features to a shipped, well-seamed codebase — and the research verdict is that almost all of it is **zero new runtime dependencies**. Hybrid search is rank-based RRF (k=60, the universal convention) fused in Rust over the two existing, individually-tested store legs; memory_update and flat id-addressed relations are one appended migration (`0003_links.sql`) plus new methods on the existing `Store` trait; REST hardening is manual bounds checks at the `MemoryService` seam surfaced through the proven two-tier error taxonomy (no validation crate); portable export is versioned JSONL via already-pinned `serde_json`, embeddings excluded by default and re-embedded on import via the existing sweep-backfill machinery. The only additions are build-side: a Windows cross-compile toolchain and a 4-macro CFLAGS shim for musl.

The two distribution facts every researcher independently verified: (1) **cargo-zigbuild cannot build Windows targets** (upstream README: Linux and macOS only), so Windows is a differently-tooled matrix leg — mingw-w64 (`x86_64-pc-windows-gnu`, one apt package on the existing orangepi runner) or cargo-xwin (msvc) — and needs an early toolchain **spike** because the bundled sqlite3.c + sqlite-vec.c must compile for the Windows target; (2) **sqlite-vec PR #199 (musl typedef fix) is still unmerged** and the pinned crate 0.1.9 ships the broken C, so musl builds need target-scoped `CFLAGS_<triple>` typedef defines — zero code change, trivially removable when upstream releases.

One important conflict was resolved during research: FEATURES (and the v1.0 decision log, extrapolating from the mcp-hub precedent) rated Windows as a HIGH-cost cfg(unix) refactor. ARCHITECTURE **read the actual code** and found the codebase is already ~Windows-clean — the only platform-conditional code is `config.rs` (with the `cfg(not(unix))` fallback already written); no unix-only deps, no signal handling, portable shutdown. **Resolve in ARCHITECTURE's favor:** DIST-03 is ~90% release-pipeline work, ~10% code (switch to `data_local_dir()` for `%LOCALAPPDATA%`, README notes). The toolchain spike remains the safety check on the residual risk (C-code cross-compile + untestable-at-runtime binary). Key sequencing constraint agreed by three of four files: **relations schema must land before the export format freezes**, so the JSONL v1 contract carries link records from day one.

## Key Findings

### Recommended Stack

No Cargo.toml changes for features 1–3 and export. The standing pin triangle (rusqlite 0.39 / rusqlite_migration 2.5 / r2d2_sqlite 0.34 / libsqlite3-sys 0.37, bundled SQLite 3.51.3) stays untouched — the bundled engine already has window functions, FULL OUTER JOIN, and VACUUM INTO. Do NOT bump to rusqlite 0.40 (links-conflict), do NOT use sqlite-vec 0.1.10-alpha, do NOT reintroduce chrono default features (breaks the zig darwin cross-link).

**Core technologies:**
- RRF fusion: pure Rust/SQL on existing engine — zero new deps; k=60, weights 1.0/1.0
- `rusqlite_migration 2.5`: migration 0003 for `memory_links` — append-only, shipped migrations frozen
- **New (build-side only):** cargo-xwin 0.23 or mingw-w64 for Windows; `CFLAGS_x86_64/aarch64_unknown_linux_musl="-Du_int8_t=uint8_t …"` shim for musl
- Explicitly NOT added: validation crate (garde/validator), export framework, base64 (defer with `--include-embeddings`)

Details: [STACK.md](./STACK.md)

### Expected Features

**Must have (table stakes):**
- Hybrid RRF (k=60, rank-based, over-fetch 2–4× per leg, dedup-by-summing, degrades to keyword-only without Ollama, mode reported honestly in `SearchOutcome`)
- `memory_update(id, partial_patch)` with re-embed on content change (mem0/Letta-proven shape)
- `memory_link/unlink` flat id-addressed relations + 1-hop `related` in results + `ON DELETE CASCADE` cleanup on forget AND TTL sweep
- REST boundary hardening: 400-reject out-of-range limit/ttl (never clamp silently), exact tag match via `json_each` (behavioral change — release notes)
- JSONL export with version header + idempotent round-trip import with re-embed
- musl + Windows binaries with checksums

**Should have (differentiators):**
- Decay multiplier on fused score — competitors lack recency in hybrid ranking and users request it (qmd #331)
- Embeddings-optional export with re-embed-on-import — genuinely portable across model versions
- Fully-local hybrid search in one static binary — the milestone's headline positioning

**Defer (v1.x/v2+):**
- Configurable k/weights, `--include-embeddings`, cross-encoder reranking, multi-hop graph queries (anti-feature until proven), entity/observation knowledge graph (rejected — 9-tool surface confuses agents), LLM write-arbitration (violates zero-cloud)

Details: [FEATURES.md](./FEATURES.md)

### Architecture Approach

Everything plugs into verified existing seams: fusion, update orchestration, and validation live in `MemoryService` (never in transports); new store methods on the `Store` trait; migration appended to the ordered list; JSONL import dispatched through the existing `--from` + idempotent `service.import` path. ARCHITECTURE explicitly recommends **service-level RRF fusion in Rust over the two existing store methods** — NOT the sqlite-vec blog's single-statement CTE (it would duplicate filter predicates and entangle the FTS5-error and degrade seams). `SearchMode::Hybrid` is an additive enum variant; envelope shape unchanged on both transports.

**Major components (new/modified):**
1. `service.rs` — RRF fusion fn + `SearchMode::Hybrid` + validation helpers + update/link orchestration
2. `sql/0003_links.sql` + `LinkKind` enum + 4 Store methods/tools/routes — relations
3. `export.rs` + `import/jsonl.rs` + `Store::export_rows` + timestamp-preserving `NewMemory` extension
4. `release.yml` — mingw Windows leg (zip, presence-only smoke, continue-on-error initially) + musl CFLAGS shim legs

Details: [ARCHITECTURE.md](./ARCHITECTURE.md)

### Critical Pitfalls

1. **Score-fusion instead of rank-fusion** — the two legs' score scales are incomparable; fuse on rank positions only, decide ONCE where decay applies (post-fusion multiplier OR per-leg, never both), golden-query fixture test.
2. **`memory_update` leaving a stale vector** — FTS trigger updates itself but vec0 ignores triggers; explicit DELETE + status-reset in the same writer tx; kill-test that the OLD meaning no longer matches semantically. Never implement update as DELETE+INSERT (rowid churn corrupts relations).
3. **Editing shipped migrations / no pre-migration backup** — rusqlite_migration has no checksums; freeze 0001/0002 forever, add a v0.0.1-fixture schema-divergence test, back up the DB before applying 0003+, friendly too-new-DB error.
4. **Windows-as-a-zigbuild-target assumption** — fails at first CI run; spike the toolchain first; `data_local_dir()` not roaming `data_dir()` (WAL corruption on roaming profiles).
5. **Export/import id remap + timestamp policy** — edges must map through an export-key→new-rowid table (naive id preservation silently corrupts graphs in non-empty DBs); decide expired-row and last_accessed policy explicitly (refresh recommended) or import "eats" memories.

Details: [PITFALLS.md](./PITFALLS.md) (19 pitfalls, phase-mapped)

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: API Hardening & Validation Seam
**Rationale:** Foundation — update/link/export all route new inputs through this seam; adding `MemoryError::InvalidArgument` early means every later mapper arm is written once. Small, high-certainty. Also pull the **Windows/musl toolchain spikes forward into this phase** (spike-cross-compile.yml) — the only unresolved feasibility question in the milestone should be answered in week 1.
**Delivers:** limit/ttl bounds → 400, exact tag-match (`json_each`, behavioral change), `deny_unknown_fields`, boundary test suite; spike results for mingw/xwin and musl CFLAGS.
**Addresses:** WR-01/02/07.
**Avoids:** per-transport validation drift (Anti-Pattern 3), late toolchain surprise (Pitfall 13).

### Phase 2: memory_update + Relations (parallel track: Hybrid RRF)
**Rationale:** Relations MUST precede export (format freeze); hybrid search touches only `service.rs` and shares no files with relations beyond domain.rs (error variants landed in Phase 1), so the two can run as parallel tracks.
**Delivers:** migration 0003 (+ migration-hygiene tests: fixture divergence, too-new error, pre-migration backup), `LinkKind`, update with re-embed-in-one-tx, cascade on forget AND sweep; RRF fusion (k=60, dedup-by-summing, symmetric over-fetch, bump only returned ids, mode honesty, InvalidQuery propagates 400 in all modes).
**Avoids:** Pitfalls 1–12.

### Phase 3: Export/Import (JSONL)
**Rationale:** Last core feature — exports the final v1.1 schema including link records.
**Delivers:** versioned JSONL export (no embeddings), `import --from jsonl` through the existing idempotent path with key→id remap for edges, timestamp policy (preserve created_at, skip-and-report expired, refresh last_accessed), rich import report.
**Avoids:** Pitfalls 17–19.

### Phase 4: Distribution & Release (Windows + musl + tag)
**Rationale:** Shipped binary must contain everything; spikes from Phase 1 de-risked the tooling.
**Delivers:** musl legs with CFLAGS shim (promote to required on green), mingw Windows leg (zip, `.exe`, presence smoke, `continue-on-error` first release, `%LOCALAPPDATA%` config branch, README Windows/musl sections), SHA256SUMS glob widened, release notes covering behavioral changes (tag exact-match, 400s, `"hybrid"` mode string).
**Avoids:** Pitfalls 13–16.

### Phase Ordering Rationale

- Validation seam first: everything downstream reuses it; error-variant additions in one place avoid domain.rs merge conflicts across parallel tracks.
- Relations before export: the JSONL v1 contract must carry links or format v2 arrives one phase later (FEATURES, ARCHITECTURE, and PITFALLS all independently require this).
- Distribution last but **spiked first**: build feasibility answered in Phase 1, release wiring in Phase 4.
- Hybrid RRF is dependency-free (no schema, no store changes) — the natural parallel track.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 4 (Windows):** toolchain choice mingw-w64 (gnu) vs cargo-xwin (msvc) is unproven for this workspace's C code on the ARM64 runner — the spike is the decision input. STACK leans xwin/msvc, ARCHITECTURE leans mingw/gnu (self-contained, no MSVC EULA); let the spike settle it, ship gnu rather than slip the milestone.
- **Phase 2 (update semantics):** patch-clear semantics (`Option<Option<T>>` vs explicit clear flags) and `last_accessed` vs `updated_at` decay policy — settle in discuss-phase, not research.

Phases with standard patterns (skip research-phase):
- **Phase 1 (hardening):** extends the proven 02-05 taxonomy; three bounds checks.
- **Phase 2 (RRF):** the exact SQL/Rust pattern is documented by the sqlite-vec author; math is 30 lines.
- **Phase 3 (export):** JSONL + existing import machinery; design decisions are enumerated, not open.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | "No new crates" verified against pinned graph + official docs; MEDIUM only on cross-compile toolchain (unspiked) |
| Features | MEDIUM | RRF/tool-shape findings cross-verified (LanceDB, sqlite-vec canonical, mem0/Letta/MCP reference); export format is convergent practice, not standard |
| Architecture | HIGH | Every integration point verified against the actual v1.0 source |
| Pitfalls | HIGH | Grounded in the shipped codebase + verified upstream state (PR #199, zigbuild targets, rusqlite_migration behavior); MEDIUM on Windows ACL specifics |

**Overall confidence:** HIGH

### Gaps to Address

- **Windows C-code cross-compile is unproven for this workspace** (all four files agree): spike `spike-cross-compile.yml` with mingw-w64 (and optionally cargo-xwin) in Phase 1. Fallback ladder: gnu → msvc → document-and-defer (mcp-hub HUB-V2 pattern, last resort).
- **Windows runtime is untestable in CI** (no Windows runner; cross-built .exe can't run on the Linux host): presence-only smoke + `continue-on-error` + "community-validated" release-note label; optional wine smoke, never gating.
- **sqlite-vec upstream fix timing:** if a fixed stable crate releases mid-milestone, bump the pin and delete the CFLAGS shim in the same commit; otherwise document the shim in README for `cargo install --target musl` source users.
- **Resolved conflict (recorded):** Windows cfg(unix) refactor cost — FEATURES/v1.0 decision log assumed HIGH per the mcp-hub precedent; ARCHITECTURE's code audit shows the risk was designed out in v1.0 (single seam in config.rs, fallback already written). Roadmap should budget Windows as pipeline work with a small code delta, with the spike as the safety check.
- **Decay placement in fusion** (Pitfall 1): per-leg decay-blended ranks vs post-fusion multiplier — FEATURES recommends post-fusion, PITFALLS warns against double-counting. Decide once in Phase 2 discuss-phase and document in the fusion function.

## Sources

### Primary (HIGH confidence)
- v1.0 codebase read directly 2026-07-12: `service.rs`, `sqlite.rs`, `migrations.rs`, `config.rs`, `mcp.rs`, REST handlers, SQL migrations, `Cargo.toml` pins, `release.yml`
- docs.rs libsqlite3-sys 0.37 (bundled SQLite 3.51.3); rusqlite README; rusqlite_migration docs (user_version-only tracking, too-new error)
- cargo-zigbuild README — Linux/macOS targets only (no Windows)
- sqlite-vec PR #199 — verified still open/unmerged 2026-07-12; crates.io registry (0.1.9 max stable)

### Secondary (MEDIUM confidence)
- Alex Garcia (sqlite-vec author) hybrid-search pattern; Context7 LanceDB RRFReranker (k=60 default); Cormack & Clarke SIGIR 2009
- mem0 / Letta / official MCP memory server / Zep-Graphiti tool-shape analysis (cross-verified)
- cargo-xwin 0.23 (crates.io + upstream README); rusqlite windows-gnu community reports
- Ecosystem precedents: mcp-hub Windows failure (03-04), OPS-01/OPS-02 runner policy, 02-05 error taxonomy

### Tertiary (LOW confidence)
- Export-format practice (ChromaDB Data Pipes, VACUUM-INTO-as-backup writeups) — convergent but no standard; needs no further validation beyond shipping the versioned header

---
*Research completed: 2026-07-12*
*Ready for roadmap: yes*
