# Phase 5: Portable Export & Import - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-25
**Phase:** 5-Portable Export & Import
**Mode:** `--auto` — the user delegated every gray area to the recommended option. No AskUserQuestion was issued; each row marked ✓ is the recommended default, chosen by Claude and logged here for audit.
**Areas discussed:** Surface & CLI shape, JSONL format v1, Import identity & edge remapping, Timestamp & lifecycle policy, Malformed-file policy & report, Embeddings on import, Write-path storage-error redaction

---

## Surface & CLI shape

| Option | Description | Selected |
|--------|-------------|----------|
| CLI-only (`export` / `import --from jsonl`) | File in, file out; core raises `InvalidArgument` so the taxonomy holds for any later transport | ✓ |
| CLI + MCP tools | `memory_export`/`memory_import` over stdio | |
| CLI + REST endpoints | `GET /api/export`, `POST /api/import` | |

**Choice:** CLI-only (recommended). MCP export would push the corpus through the JSON-RPC stdout channel; REST import is an arbitrary-file-read primitive on an unauthenticated daemon (PITFALLS security row).
`[auto] Surface — Q: "Where do export/import live?" → Selected: "CLI-only" (recommended default)`

| Option | Description | Selected |
|--------|-------------|----------|
| stdout default, `--output PATH` atomic + 0600 | Unix convention, pipe-friendly; file path gets temp+rename and owner-only mode | ✓ |
| `--output` required | Forces a file; refuses stdout | |
| Default path in the data dir | Guesses a location; risks a shared/tmp default | |

`[auto] Surface — Q: "Export destination?" → Selected: "stdout default, --output atomic 0600" (recommended default)`

## JSONL format v1

| Option | Description | Selected |
|--------|-------------|----------|
| Header record + `record`-tagged lines; model/dim from `meta`; derived fields excluded | `{"record":"header",…}` then memories then links; `key` = source id as export-local key | ✓ |
| No header, infer from first record | Smaller but unversioned — a shipped tool's export is a contract | |
| Include `decay_score`/`base_weight`/`embedding_status` | "Round-trip everything" | |

`[auto] Format — Q: "Header and record shape?" → Selected: "versioned header, tagged records, derived fields excluded" (recommended default)`

| Option | Description | Selected |
|--------|-------------|----------|
| Export everything present, no filters, importer owns expiry | One streaming query per record type | ✓ |
| `--scope`/`--type` filters in v1 | Partial exports | |
| Skip expired rows at export time | Policy split across both sides | |

`[auto] Format — Q: "Export scope?" → Selected: "everything, no filters" (recommended default)`

## Import identity & edge remapping

| Option | Description | Selected |
|--------|-------------|----------|
| Reuse `(source, mem_type, content)` key; `exists` → id-returning lookup; no migration | Zero new schema; duplicate maps to existing row id | ✓ |
| New UUID identity column (migration 0004) | Survives content edits; changes the idempotency contract | |
| Preserve source ids | Collides in a non-empty DB (PITFALLS 18) | |

`[auto] Identity — Q: "Idempotency key for JSONL rows?" → Selected: "existing content key" (recommended default)`

| Option | Description | Selected |
|--------|-------------|----------|
| Two-pass validate-then-write: memories build key→id map, links through the map; expired-endpoint links dropped+counted; unknown key = malformed | PITFALLS 18 shape | ✓ |
| Single pass, edges inline | Edges can precede the dedup result of their endpoints | |
| Drop unknown-key links silently | Hides corruption | |

`[auto] Identity — Q: "How are edges remapped?" → Selected: "two-pass through key→id map" (recommended default)`

## Timestamp & lifecycle policy

| Option | Description | Selected |
|--------|-------------|----------|
| `created_at` preserve; `expires_at` preserve + skip-and-count already-expired; `last_accessed` refresh to now; `access_count` preserve; link `created_at` preserve; skew → warning | PITFALLS 19 recommendation | ✓ |
| Preserve everything verbatim | Restored corpus arrives fully decayed and unfindable by rank; expired rows "eaten" by the sweep later | |
| Refresh everything to now | Loses provenance | |

`[auto] Lifecycle — Q: "Per-column timestamp policy?" → Selected: "preserve created/expires, refresh last_accessed, skip expired" (recommended default)`

## Malformed-file policy & report

| Option | Description | Selected |
|--------|-------------|----------|
| Strict all-or-nothing `InvalidArgument` naming the line; `deny_unknown_fields`; nothing written on failure | Our own contract — half-restores are worse than refusals | ✓ |
| Tolerant skip-and-count (gsd-state style) | Silent data loss on a corrupted backup | |
| Best-effort: import valid rows, report bad ones | Partial state, non-idempotent recovery | |

`[auto] Malformed — Q: "Reject or tolerate bad rows?" → Selected: "strict, all-or-nothing" (recommended default)`

| Option | Description | Selected |
|--------|-------------|----------|
| Extend `ImportReport` additively (`skipped_expired`, `links_imported`, `links_skipped_duplicates`, `links_dropped`) | One report type; gsd path reports zeros | ✓ |
| Separate `JsonlImportReport` | Two shapes for one concept | |

`[auto] Report — Q: "Report shape?" → Selected: "extend ImportReport" (recommended default)`

## Embeddings on import

| Option | Description | Selected |
|--------|-------------|----------|
| Never embed inline — rows land `embedding_status = 0`, sweep backfill re-embeds | Criterion 3 literal; one mechanism; CLI never blocks on Ollama | ✓ |
| Best-effort inline batch embed (gsd-state style) | Faster semantic availability, but blocks the CLI and contradicts the criterion's stated landing state | |
| Import then drain backfill in the CLI | Immediate availability; extra scope — deferred as `--embed-now` | |

`[auto] Embeddings — Q: "Embed on import?" → Selected: "never inline; sweep backfill" (recommended default)`

## Write-path storage-error redaction (user-approved scope addition)

| Option | Description | Selected |
|--------|-------------|----------|
| Service-seam `redact_storage_error(op)` on every public `MemoryService` method | One seam covers MCP, REST (incl. `/health`) and the CLI; mirrors Phase 4's search redaction | ✓ |
| Redact in the two transport mappers | Two files, CLI uncovered (04-REVIEW IN-01's suggestion) | |
| Leave deferred | Contradicts the user's scope addition | |

`[auto] Redaction — Q: "Where does write-path redaction live?" → Selected: "service seam" (recommended default)`

---

## Claude's Discretion

- `NewMemory` override fields vs a dedicated `ImportedMemory` + `Store::insert_imported`.
- Transaction batching for import passes; paging for the export read.
- Temp-file naming for `--output`; reuse of the backup temp-path helper.
- Whether the header carries `exported_rows`/`exported_links` counts.
- Rustdoc/README placement of the format spec; the optional `VACUUM INTO` backup tip.

## Deferred Ideas

- `--include-embeddings` with model/dim check; export filters; MCP/REST export/import; `import --embed-now` / `backfill` subcommand; cross-tool import dialects; UUID identity column (migration 0004); 04-REVIEW residuals (WR-01 cross-process lock, IN-02/IN-04/IN-05/IN-06).
