# Phase 4: Memory Update, Relations & Hybrid Search - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-07-12
**Phase:** 4-Memory Update, Relations & Hybrid Search
**Areas discussed:** Link kind vocabulary, 1-hop expansion shape, Update patch semantics, Migration backup UX

---

## Link kind vocabulary

| Option | Description | Selected |
|--------|-------------|----------|
| Closed enum (Recommended) | Fixed set validated at the seam — unknown kind returns InvalidArgument with the allowed list | ✓ |
| Free-form string | Any non-empty string; maximum flexibility, typo-fragmentation risk | |
| Recommended set + freeform | Documented canonical kinds, any validated string accepted | |

**User's choice:** Closed enum

| Option | Description | Selected |
|--------|-------------|----------|
| Minimal: 3 kinds (Recommended) | relates_to, supersedes, caused_by | ✓ |
| Broader: 5-6 kinds | Adds blocks, derived_from, contradicts | |
| Single kind: relates_to | Mechanism only, semantics later | |

**User's choice:** Minimal 3 kinds

| Option | Description | Selected |
|--------|-------------|----------|
| Idempotent + strict self (Recommended) | Duplicate = no-op success (UNIQUE in 0003); self-link = InvalidArgument; unlink nonexistent = not-found shape | ✓ |
| Strict everywhere | All three cases error | |
| Lenient everywhere | All three cases succeed | |

**User's choice:** Idempotent + strict self

| Option | Description | Selected |
|--------|-------------|----------|
| Stored directed, expanded both ways (Recommended) | Edge keeps from→to + kind; expansion returns neighbors in either direction with kind + direction annotation | ✓ |
| Strictly directed | Expansion only follows from→to | |
| Undirected | All edges symmetric | |

**User's choice:** Stored directed, expanded both ways

---

## 1-hop expansion shape

| Option | Description | Selected |
|--------|-------------|----------|
| Opt-in flag (Recommended) | expand_links: true (default false) on search/list, both transports; v1.0 envelope unchanged unless asked | ✓ |
| Always expanded | Every result always carries related memories | |
| Separate tool/endpoint | Dedicated memory_related(id) call | |

**User's choice:** Opt-in flag

| Option | Description | Selected |
|--------|-------------|----------|
| Nested summaries (Recommended) | related: [{id, kind, direction, type, snippet, tags}] — trimmed view | ✓ |
| Nested full memories | Complete MemoryView per neighbor | |
| Flat id+kind edges only | related: [{id, kind, direction}] | |

**User's choice:** Nested summaries

| Option | Description | Selected |
|--------|-------------|----------|
| No bumps for expanded (Recommended) | Bumps only for post-truncation returned ids; expansion is a pure read | ✓ |
| Bump expanded too | Neighbors count as accessed | |
| You decide | Claude picks during planning | |

**User's choice:** No bumps for expanded

| Option | Description | Selected |
|--------|-------------|----------|
| Fixed cap const (Recommended) | pub const (e.g. MAX_EXPANDED_LINKS = 10) next to existing consts; most-recent edges win | ✓ |
| Caller-tunable parameter | expand_limit validated at the seam | |
| Uncapped | All 1-hop neighbors returned | |

**User's choice:** Fixed cap const

---

## Update patch semantics

| Option | Description | Selected |
|--------|-------------|----------|
| Omitted=unchanged, null=clear (Recommended) | JSON-merge-patch; content never null/empty; double-Option serde pattern | ✓ |
| Sentinel values | ttl_secs: 0 / tags: [] mean clear — contradicts Phase 3 seam contract | |
| No clearing in v0.1.0 | Patch only sets, never clears | |

**User's choice:** Omitted=unchanged, null=clear

| Option | Description | Selected |
|--------|-------------|----------|
| Bump on update (Recommended) | Update is the strongest freshness signal; reuses bump_access | ✓ |
| Separate updated_at only | New column, no ranking path consumes it | |
| No effect on decay | Just-corrected memory can rank stale | |

**User's choice:** Bump on update

| Option | Description | Selected |
|--------|-------------|----------|
| Full updated view (Recommended) | Complete post-update MemoryView incl. embedding_status | ✓ |
| Minimal ack | {id, updated: true} | |
| Changed-fields diff | old → new per changed field | |

**User's choice:** Full updated view

---

## Migration backup UX

| Option | Description | Selected |
|--------|-------------|----------|
| Sibling timestamped, keep last 1 per version (Recommended) | memories.db.backup-pre-0003-{YYYYMMDD} via SQLite online backup API; one per schema version | ✓ |
| Simple copy, keep forever | Unbounded accumulation | |
| Backups subdirectory | Rotation in a second location | |

**User's choice:** Sibling timestamped, keep last 1 per version

| Option | Description | Selected |
|--------|-------------|----------|
| Standard for all future migrations (Recommended) | Generic: back up whenever pending migrations exist and DB non-empty | ✓ |
| 0003 only | Hard-coded to this migration | |
| Opt-in flag | Default off | |

**User's choice:** Standard for all future migrations

| Option | Description | Selected |
|--------|-------------|----------|
| Refuse with upgrade hint (Recommended) | Detect newer schema at startup; clean exit naming both versions + brew upgrade command | ✓ |
| Read-only degraded mode | Serve reads, refuse writes — undesigned forward-compat promise | |
| Generic error | Raw rusqlite_migration error | |

**User's choice:** Refuse with upgrade hint

---

## Claude's Discretion

- Exact `MAX_EXPANDED_LINKS` value and const naming/placement
- Relations table/column naming and index layout in 0003 (UNIQUE(from_id, to_id, kind) required)
- Backup failure handling (abort vs proceed — lean abort-with-message)
- RRF implementation details within locked requirement mechanics
- Snippet truncation rule for expanded summaries

## Deferred Ideas

- Request-level `mode` override (SEARCH-06 — already tracked)
- Caller-tunable `expand_limit` parameter
- `updated_at` metadata column
- Broader relation vocabulary (blocks, derived_from, contradicts)
- Read-only degraded mode for newer-schema DBs
