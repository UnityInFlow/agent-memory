# Roadmap: agent-memory

## Milestones

- ✅ **v1.0 MVP** — Phases 1-2 (shipped 2026-07-12; product release v0.0.1 on GitHub + Homebrew) — [details](milestones/v1.0-ROADMAP.md)
- 🚧 **v1.1 Hardening & Interop** — Phases 3-6 (in progress; ships as product release v0.1.0)

## Overview

v1.1 closes the deferred v2 backlog on the shipped v0.0.1 codebase: better recall (decay-aware hybrid RRF search), a richer MCP surface (`memory_update`, flat typed relations), hardened input validation at the shared service seam, and wider distribution (musl + Windows binaries, portable JSONL export/import) — shipping as product release v0.1.0. The build order follows the research-verified dependency chain: the validation seam lands first (every later surface routes new inputs through it) alongside the milestone's only feasibility unknown, the Windows/musl cross-compile spikes; the relations schema and update semantics land next with hybrid RRF running as an independent parallel track; export freezes its format v1 only after relations exist so link edges are in the contract from day one; and distribution ships last on the toolchains the Phase 3 spikes already proved.

## Phases

<details>
<summary>✅ v1.0 MVP (Phases 1-2) — SHIPPED 2026-07-12</summary>

- [x] Phase 1: Core Memory Foundation (3/3 plans) — completed 2026-06-25
- [x] Phase 2: Semantic Search, Interop & Release (5/5 plans) — completed 2026-07-12

Full phase details: [milestones/v1.0-ROADMAP.md](milestones/v1.0-ROADMAP.md)

</details>

**Phase Numbering:**

- Integer phases (3, 4, 5, 6): Planned milestone work
- Decimal phases (3.1, 3.2): Urgent insertions (marked INSERTED)

- [ ] **Phase 3: API Hardening & Toolchain Spikes** - Out-of-bounds limit/ttl rejected as client errors and exact tag matching at the shared service seam, plus early Windows/musl cross-compile spikes answering the milestone's only feasibility unknown
- [ ] **Phase 4: Memory Update, Relations & Hybrid Search** - `memory_update` with same-transaction re-embed, flat typed links with cascade, migration 0003 hygiene, and decay-aware hybrid RRF as a parallel track
- [ ] **Phase 5: Portable Export & Import** - Versioned JSONL export (link edges included, embeddings excluded) and idempotent cross-machine import with sweep-backfill re-embedding
- [ ] **Phase 6: Distribution & v0.1.0 Release** - musl + Windows binaries on the spike-chosen toolchains and the checksummed v0.1.0 release with Homebrew update and behavioral-change release notes

## Phase Details

### Phase 3: API Hardening & Toolchain Spikes

**Goal**: As an agent or REST client, I want invalid inputs rejected cleanly at one shared seam and tag filters to match tags exactly, so that bad requests never surface as server faults or over-matched results on any transport.
**Mode:** mvp
**Depends on**: Nothing within this milestone (builds on shipped v1.0)
**Requirements**: API-02, API-03
**Success Criteria** (what must be TRUE):

  1. Sending an out-of-bounds `limit` or `ttl_secs` (zero, negative, absurd extremes) to any surface returns REST 400 / MCP `invalid_params` with a descriptive message — never a 500 and never a silent clamp — while valid boundary values still succeed.
  2. Searching or listing with `tag=rust` returns only memories tagged exactly `rust` — a memory tagged `rustling` no longer matches (json_each equality replaces LIKE substring; behavioral change recorded for the v0.1.0 release notes).
  3. The same invalid input produces the same client-error classification on both MCP and REST because validation lives once at the `MemoryService` seam (`MemoryError::InvalidArgument` through the proven two-tier taxonomy) — new v1.1 surfaces (update/link/export/import) must route through this seam as they land.
  4. The Windows (cargo-xwin/msvc vs mingw-w64/gnu) and musl CFLAGS-shim cross-compile spikes have run in CI with a recorded toolchain verdict, so Phase 6 starts with the feasibility question already answered.

**Plans**: 2 plans

Plans:

- [x] 03-01-PLAN.md — Validation seam (`MemoryError::InvalidArgument` + pub bounds consts, all 7 surfaces incl. the import bypass, clamp removed) + json_each exact tag matching at all 3 predicate sites + doc/README contract updates (API-02, API-03)
- [ ] 03-02-PLAN.md — CI revival (ci.yml → GitHub-hosted ubuntu-latest, D-12) + hosted 4-leg Windows/musl cross-compile spike dispatched live with the D-11 toolchain verdict recorded for Phase 6

### Phase 4: Memory Update, Relations & Hybrid Search

**Goal**: As an agent, I want to update existing memories, link related ones, and get hybrid keyword+semantic recall, so that my memory stays current and connected and I find the right memory even when the phrasing differs.
**Mode:** mvp
**Depends on**: Phase 3 (shared `InvalidArgument` error seam — variant added once, reused by every mapper arm)
**Requirements**: MCP-06, MCP-07, STORE-05, SEARCH-04, SEARCH-05
**Success Criteria** (what must be TRUE):

  1. Calling `memory_update` with a partial patch (content, tags, scope, TTL) changes the memory in place; a content update re-mirrors FTS and invalidates/re-embeds the vector in the same writer transaction (embed outage falls back to `embedding_status = 0` + existing sweep backfill), the old meaning no longer matches semantically, and an unknown id returns the clean not-found shape.
  2. `memory_link`/`memory_unlink` create and remove a flat typed relation (from_id, to_id, kind) between two memories; search/list results can expand 1-hop related memories; forgetting or TTL-expiring a memory cascades its links — no orphan edges, no graph/entity model.
  3. With Ollama available, `memory_search` returns rank-fused results (RRF, k=60, duplicate ids sum contributions from both legs) labeled `search_mode: "hybrid"`; with Ollama unreachable, search degrades to keyword exactly as in v1.0 with an unchanged envelope shape.
  4. Hybrid ranking stays decay-aware — a fresh memory outranks a stale one at equal fused rank — and access bumps apply only to post-truncation returned ids.
  5. Upgrading a real v0.0.1 database applies migration 0003 only after a pre-migration backup; shipped 0001/0002 SQL is provably unchanged (fixture divergence test against a v0.0.1 database), and a newer-schema DB met by an older binary yields a friendly error, not a crash.

**Plans**: TBD

### Phase 5: Portable Export & Import

**Goal**: As a user, I want to export all my memories to a portable file and import them on another machine, so that my agent's memory moves with me — across machines, with no cloud sync.
**Mode:** mvp
**Depends on**: Phase 4 (relations schema must exist before the JSONL format v1 contract freezes — export must carry link edges from day one)
**Requirements**: DIST-04, INTEROP-02
**Success Criteria** (what must be TRUE):

  1. `export` writes a versioned JSONL file whose header records format version and embedding model/dim; embeddings are excluded as derived data; relation edges are included with export-local key remapping.
  2. Importing that file on another machine — including into a non-empty database — recreates memories and their link edges with correct key→id remapping, and re-running the same import is idempotent (no duplicates).
  3. Imported rows land with `embedding_status = 0` and become semantically searchable after the existing sweep backfill re-embeds them — no embedding vectors travel in the file.
  4. Expired items in the file are skipped and counted, and the import report states imported/skipped/duplicate counts; malformed files (bad header/version/rows) are rejected as client errors through the Phase 3 seam, never a 500 or crash.

**Plans**: TBD

### Phase 6: Distribution & v0.1.0 Release

**Goal**: As a user on Windows or musl-based Linux, I want to install agent-memory v0.1.0 from a pre-built binary, so that the memory daemon runs on my platform without a Rust toolchain.
**Mode:** mvp
**Depends on**: Phase 3 (spike toolchain verdicts), Phases 4-5 (shipped binary must contain all v1.1 features)
**Requirements**: DIST-05, DIST-03, DIST-06
**Success Criteria** (what must be TRUE):

  1. Linux musl tarballs (x86_64/aarch64) build green on the existing zigbuild pipeline via the target-suffixed CFLAGS typedef shim (`-Du_int*_t=uint*_t`) and ship checksummed on the release — shim documented as removable when sqlite-vec PR #199 lands in a pinned release.
  2. A Windows x86_64 binary is published on the release using the Phase 3 spike-chosen toolchain, with artifact-presence smoke only (no Windows runner in the fleet) and a best-effort label in the release notes.
  3. Product release v0.1.0 is published: checksummed tarballs for all supported targets, `brew install unityinflow/tap/agent-memory` installs the new version, and release notes call out the behavioral changes (exact tag match, new 400s on bounds, `"hybrid"` search mode).

**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 3 → 4 → 5 → 6

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Core Memory Foundation | v1.0 | 3/3 | Complete | 2026-06-25 |
| 2. Semantic Search, Interop & Release | v1.0 | 5/5 | Complete | 2026-07-12 |
| 3. API Hardening & Toolchain Spikes | v1.1 | 1/2 | In Progress|  |
| 4. Memory Update, Relations & Hybrid Search | v1.1 | 0/? | Not started | - |
| 5. Portable Export & Import | v1.1 | 0/? | Not started | - |
| 6. Distribution & v0.1.0 Release | v1.1 | 0/? | Not started | - |
