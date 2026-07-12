# Phase 3: API Hardening & Toolchain Spikes - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-07-12
**Phase:** 3-API Hardening & Toolchain Spikes
**Areas discussed:** Validation bounds, Error message shape, Tag matching semantics, Spike/CI runner strategy
**Mode:** auto — user delegated ("do it"); Claude selected the recommended option per area and logged alternatives.

---

## Validation bounds (limit / ttl_secs)

| Option | Description | Selected |
|--------|-------------|----------|
| Align with existing caps (limit 1..=200 = MAX_KNN_K; ttl 1..=~100y) | Bounds derived from constants already in the codebase; no new magic numbers | ✓ |
| Generous arbitrary caps (limit 1..=1000, ttl unlimited positive) | Looser, but 1000 exceeds what the KNN leg can honor — a "valid" limit would silently underdeliver | |
| Keep silent clamping, just fix negatives | Contradicts API-02's explicit "never a silent clamp" | |

**Auto-selected:** Align with existing caps.
**Notes:** [auto] Ceiling = MAX_KNN_K keeps "valid input" and "honorable input" the same set. Silent `.max(0)` clamp removed.

## Error message shape

| Option | Description | Selected |
|--------|-------------|----------|
| field + range + got-value ("limit must be between 1 and 200 (got 0)") | Deterministic, greppable, echoes only client-sent data | ✓ |
| Terse ("invalid limit") | Cheaper but agents/users can't self-correct without docs | |
| Structured error body with code/field/range JSON | Nicest for machines but changes the established error envelope — bigger change than the phase needs | |

**Auto-selected:** field + range + got-value.
**Notes:** [auto] Tests assert on stable prefix, not full equality.

## Tag matching semantics

| Option | Description | Selected |
|--------|-------------|----------|
| json_each equality, case-sensitive | Exact per-element match on the existing JSON-array storage; single behavioral change | ✓ |
| json_each equality + case-folding | Two behavioral changes at once; case policy deserves its own decision if ever needed | |
| Normalize tags on write + exact match | Rewrites stored data; migration burden out of proportion | |

**Auto-selected:** json_each equality, case-sensitive.
**Notes:** [auto] All three LIKE sites replaced; regression test per site; release-notes callout.

## Spike/CI runner strategy

| Option | Description | Selected |
|--------|-------------|----------|
| GitHub-hosted ubuntu-latest, secretless workflow_dispatch spike (D-02 exception) | Only runnable option: org policy (allows_public_repositories: false, enforced 2026-07-09) blocks all self-hosted jobs on this now-public repo | ✓ |
| Self-hosted orangepi as in v1.0 | Jobs would queue forever under the enforced policy | |
| Local spike on this darwin-arm64 machine | Non-reproducible, no CI artifact, and cargo-xwin/mingw setups differ from the release environment | |

**Auto-selected:** GitHub-hosted secretless spike workflow.
**Notes:** [auto] Surfaced a bigger finding: ci.yml/release.yml are silently dead on this public repo for the same reason — flagged as D-12 (CI fix should ride along if cheap; Phase 6 owns the release-runner decision).

## Claude's Discretion

- `deny_unknown_fields` on REST DTOs (optional this phase)
- Const naming/placement for bounds
- Reuse vs new spike workflow file

## Deferred Ideas

- SEARCH-06 request-level mode override (already in Future Requirements)
- Case-insensitive tag matching / tag normalization on write
- Full ci.yml D-02 split if non-trivial — Phase 6 owns the release-side decision
