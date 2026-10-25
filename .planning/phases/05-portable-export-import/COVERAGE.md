# Phase 5 — API Coverage Declaration

**Phase:** 05-portable-export-import
**Declared:** 2026-10-25 (planner, at plan-phase time)

## Declaration

No external API integration: Phase 5 adds file-based JSONL export/import over the existing embedded SQLite store and reuses the existing Ollama path untouched.

## Reasoning

- **Surfaces added:** two CLI subcommands (`agent-memory export [--output PATH]`, `agent-memory import --from jsonl PATH`). Both are local file I/O plus SQLite reads/writes through the existing `Store` trait and `MemoryService` seam (05-CONTEXT.md D-01).
- **No new SDK, HTTP client, or network endpoint.** The only network-capable component in the workspace is the Ollama embedder, and the JSONL importer never calls it (D-13): imported rows land `embedding_status = 0` and are re-embedded by the pre-existing sweep backfill, whose Ollama usage is unchanged.
- **No new MCP tool and no new REST route** (D-01), so the two transports' API surface is unchanged; the only transport-visible change is the storage-error redaction text widened to every path (D-14), which alters error message bodies, not endpoints or schemas.
- **No new crate dependency** (RESEARCH "Standard Stack" / "Package Legitimacy Audit": not applicable — serde, serde_json, rusqlite, tracing and std only; `Cargo.lock` unchanged).
- **No schema migration** (0001–0003 frozen, `LATEST_SCHEMA_VERSION = 3`).

The API-coverage gate therefore has nothing external to cover for this phase; the format v1 file contract is documented in `crates/agent-memory-core/src/export.rs` (module doc) and the README "Export & import" section (plan 05-04).
