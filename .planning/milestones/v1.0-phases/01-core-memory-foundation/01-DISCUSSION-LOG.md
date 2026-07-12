# Phase 1: Core Memory Foundation - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-06-24
**Phase:** 1-core-memory-foundation
**Areas discussed:** DB location & sharing, Search ranking blend, MCP tool schema shape, Decay defaults & pinning

> SPEC.md was loaded — the what/why was locked, so discussion was HOW-only. Pure mechanics (crate layout, dependency versions, migration lib, connection-pool crate, coverage tool) were intentionally left to research/planner.

---

## DB location & sharing

| Option | Description | Selected |
|--------|-------------|----------|
| Global DB + override | One global DB in OS data dir (`dirs`), overridable by `AGENT_MEMORY_DB`/`--db`; `scope` = logical column filter | ✓ |
| Per-project DB | Default to `./.agent-memory/memory.db` in the project | |

**User's choice:** Global DB + override
**Notes:** Maximizes the cross-runtime "store in Claude Code, recall in Cursor" value; per-project isolation still possible via `--db`/env. → D-01, D-02, D-03

---

## Search ranking blend

| Option | Description | Selected |
|--------|-------------|----------|
| Relevance × decay | FTS5/BM25 relevance blended with decay_score, pinned types floated, both surfaced | ✓ |
| Pure relevance | BM25 only; decay surfaced but not ranked | |

**User's choice:** Relevance × decay
**Notes:** Exact blend math left to planner. → D-04

---

## MCP tool schema shape

| Option | Description | Selected |
|--------|-------------|----------|
| Rich, sensible defaults | store(content, type, tags?, source?, scope?, ttl?); results include decay_score + timestamps; bare {content,type} still works | ✓ |
| Minimal | content + type only; metadata later | |

**User's choice:** Rich, sensible defaults
**Notes:** Optional fields default; invalid type rejected cleanly (no panic). → D-05, D-06, D-07

---

## Decay defaults & pinning

| Option | Description | Selected |
|--------|-------------|----------|
| Pin high-value + default half-life | ~30d default half-life; DECISION/ARCHITECTURE/CONSTRAINT decay slower/never; half-life + pinned types configurable | ✓ |
| Uniform decay | All types same fixed half-life; no pinning in v0.0.1 | |

**User's choice:** Pin high-value + default half-life
**Notes:** Decay still only down-ranks, never deletes (SPEC STORE-04). → D-08, D-09

---

## Claude's Discretion

- Crate/workspace mechanics, dependency versions, migration library, connection-pool approach, exact decay/ranking math, config-file format, coverage tool — deferred to research/planner (noted in CONTEXT.md `<decisions>` → Claude's Discretion).

## Deferred Ideas

- Hybrid (keyword+semantic) RRF ranking fusion — v2 (SEARCH-04)
- Per-runtime/per-project DB auto-detection beyond explicit `--db`/env — later UX
- `memory_update` / relation tools — v2 (MCP-06)
