# Phase 3: API Hardening & Toolchain Spikes - Context

**Gathered:** 2026-07-12
**Status:** Ready for planning
**Mode:** auto (user delegated decisions; every choice below is the recommended option, logged for audit)

<domain>
## Phase Boundary

Invalid inputs (out-of-bounds `limit`/`ttl_secs`) rejected cleanly at ONE shared seam (`MemoryService`) as client errors on both transports; tag filters match exact tags instead of substrings; and the Windows/musl cross-compile spikes run with a recorded toolchain verdict so Phase 6 starts with the feasibility question answered. Requirements: API-02, API-03. No new features — hardening + de-risking only.

</domain>

<decisions>
## Implementation Decisions

### Validation bounds (API-02)
- **D-01:** `limit` valid range is **1..=200** — floor 1 (zero/negative rejected), ceiling aligned with the existing `MAX_KNN_K = 200` hard cap so a valid `limit` can never exceed what the KNN leg can honor. Omitted `limit` keeps defaulting to `DEFAULT_SEARCH_LIMIT = 50` (unchanged behavior). The current silent `.max(0)` clamp in `service.rs:168` is REMOVED — out-of-range now rejects, per the no-silent-clamp requirement.
- **D-02:** `ttl_secs` valid range is **1..=3_155_760_000** (~100 years) — rejects zero, negative (which currently creates an already-expired row via `now + ttl`), and extremes that could overflow `expires_at` arithmetic. Omitted `ttl_secs` = no expiry (unchanged).
- **D-03:** Bounds live as **named `pub` consts in `agent-memory-core`** next to `DEFAULT_SEARCH_LIMIT`/`MAX_KNN_K`, and are documented in the README API tables. Both transports and all future surfaces (update/link/import in Phases 4-5) validate against the same consts.

### Error taxonomy & message shape (API-02)
- **D-04:** New variant **`MemoryError::InvalidArgument(String)`** in `domain.rs`, client tier — joins `InvalidType | InvalidQuery` in `map_memory_error` → `ApiError::BadRequest` (REST 400) and `map_mcp_error` → `invalid_params` (MCP). Both matches are exhaustive, so the compiler forces every mapper to classify the new variant — same pattern proven by 02-05.
- **D-05:** Message shape is **field + allowed range + offending value**: e.g. `invalid argument: limit must be between 1 and 200 (got 0)`. Deterministic, greppable, and safe to surface verbatim on both transports (no internal detail leaks — value echoes only what the client sent).
- **D-06:** Validation happens at the **`MemoryService` seam** (before any store call), NOT per-transport. Serde-level `deny_unknown_fields` may be added on REST DTOs if cheap, but the contract lives in core — transports stay thin adapters.

### Exact tag matching (API-03)
- **D-07:** Replace all **three** `tags LIKE '%' || ? || '%'` predicates (`sqlite.rs:279` knn_search, `:369` list, `:414` keyword search) with `EXISTS (SELECT 1 FROM json_each(m.tags) WHERE json_each.value = ?)` — tags are already stored as a JSON array string, so `json_each` equality is exact per-element matching with a bound parameter (no injection surface change).
- **D-08:** Matching is **case-sensitive, whole-tag equality** (`rust` ≠ `Rust` ≠ `rustling`). Case-folding is NOT added — it would be a second silent behavior change; documented in the README and v0.1.0 release notes alongside the substring→exact change itself.
- **D-09:** A regression test locks each of the three sites (a `rustling`-tagged row must no longer match `tag=rust` in semantic, keyword-fallback, and list paths).

### Toolchain spikes & CI reality (Phase 6 de-risk)
- **D-10:** **Spikes run on GitHub-hosted `ubuntu-latest`** in a secretless, `contents: read`, manually-triggered (`workflow_dispatch`) spike workflow. Rationale — this is now the only runnable option: the repo went public with v0.0.1 and the org runner group's `allows_public_repositories: false` (enforced 2026-07-09, ecosystem OPS-02) means NO self-hosted job (orangepi included) will ever pick up jobs from this repo again. This is the ecosystem's sanctioned D-02 exception (public/secretless CI on GitHub-hosted).
- **D-11:** Spike matrix and verdict: (a) **musl leg** — zigbuild `x86_64/aarch64-unknown-linux-musl` with target-suffixed `CFLAGS_<triple>="-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int32_t=uint32_t -Du_int64_t=uint64_t"`; (b) **Windows leg** — `cargo-xwin` → `x86_64-pc-windows-msvc` first, `mingw-w64` → `x86_64-pc-windows-gnu` as the fallback leg in the same matrix. The verdict (which legs build green, exact flags) is **recorded in the phase SUMMARY and as a comment in the spike workflow file** — Phase 6 consumes it directly.
- **D-12:** **Flag for Phase 6 (decision deferred, groundwork noted):** the existing `ci.yml`/`release.yml` target self-hosted runners and therefore currently never run on this public repo — CI has been silently dead since publication. Phase 3 spikes will prove GitHub-hosted builds work; Phase 6 decides whether to move `release.yml` to GitHub-hosted (viable — release needs only `GITHUB_TOKEN`) and/or restore a workflow-restricted runner-group exception. Migrating `ci.yml` to a D-02 split (hosted, secretless) SHOULD ride along in this phase if trivially cheap, since a dead CI gate undermines every later phase.

### Claude's Discretion
- Whether `deny_unknown_fields` lands on REST DTOs this phase (nice-to-have, not a criterion).
- Exact const names and module placement for the bounds.
- Whether the spike workflow reuses `spike-cross-compile.yml` or adds a new file.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Milestone research (decides the patterns this phase implements)
- `.planning/research/SUMMARY.md` — synthesized v1.1 research; sequencing constraints and resolved Windows-cost conflict
- `.planning/research/STACK.md` — cargo-xwin 0.23.0 verdict path, musl CFLAGS shim mapping, pin-triangle do-not-touch list
- `.planning/research/ARCHITECTURE.md` — `InvalidArgument` at the service seam, exhaustive-mapper enforcement pattern, json_each tag fix
- `.planning/research/PITFALLS.md` — silent-clamp vs reject, ttl overflow, spike/public-repo runner conflict

### Prior art in this repo (proven patterns to mirror)
- `.planning/milestones/v1.0-phases/02-semantic-search-interop-release/02-05-SUMMARY.md` — the 02-05 two-tier error-mapping pattern this phase extends (`InvalidQuery` precedent for `InvalidArgument`)
- `crates/agent-memory-core/src/domain.rs` — `MemoryError` taxonomy (client vs internal tiers)
- `crates/agent-memory/src/mcp.rs` + `crates/agent-memory/src/rest/handlers.rs` — the two exhaustive mappers that must gain the new arm
- `.github/workflows/spike-cross-compile.yml` — v1.0 darwin spike workflow shape (currently self-hosted; see D-10)

### Ecosystem policy
- Wrapper `CLAUDE.md` (unity-in-flow-ai) Decisions Log — OPS-01/OPS-02/D-02: public-repo runner policy and the sanctioned GitHub-hosted exception underpinning D-10/D-12

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `map_memory_error` (handlers.rs) + `map_mcp_error` (mcp.rs): exhaustive matches — adding `InvalidArgument` forces compile-time coverage on both transports.
- `DEFAULT_SEARCH_LIMIT`/`MAX_KNN_K` consts (sqlite.rs:28,32): the bounds consts join them.
- 02-05's four-layer test pattern (core unit → in-process handler → MCP unit → spawned-binary HTTP) is the template for API-02's proof.

### Established Patterns
- Bad client input must NEVER surface as 500/internal_error (locked contract from 02-05).
- All SQL parameterized; the tag-fix keeps bound parameters (json_each value comparison).
- Coverage-bearing tests run in-process; spawned-binary tests are realism-only.

### Integration Points
- `service.rs:168` — the silent `.max(0)` clamp to remove; validation inserts just before this.
- `sqlite.rs:217` — `expires_at = now + ttl` (unvalidated arithmetic ttl bound protects).
- `sqlite.rs:279/369/414` — the three LIKE predicates to replace.

</code_context>

<specifics>
## Specific Ideas

- Error strings must be stable enough to grep in tests: assert on the `limit must be between` prefix, not full string equality.
- The spike workflow must be dispatchable independently of CI so re-runs are cheap while iterating on flags.

</specifics>

<deferred>
## Deferred Ideas

- Request-level `mode` override for search (`keyword|semantic|hybrid`) — already captured as SEARCH-06 (future).
- Case-insensitive tag matching / tag normalization on write — revisit only if exact-match causes real friction (would be its own behavioral change).
- Full `ci.yml` D-02 split rework if it turns out non-trivial — Phase 6 owns the release-side runner decision (D-12).

</deferred>

---

*Phase: 3-API Hardening & Toolchain Spikes*
*Context gathered: 2026-07-12*
