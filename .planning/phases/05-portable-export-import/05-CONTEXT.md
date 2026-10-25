# Phase 5: Portable Export & Import - Context

**Gathered:** 2026-10-25
**Status:** Ready for planning
**Mode:** auto (user delegated: resolve every gray area by recommendation; every choice below is the recommended option, logged for audit). ROADMAP success criteria are settled and were not re-discussed.

<domain>
## Phase Boundary

A user can move an agent's whole memory between machines with no cloud: `agent-memory export` writes a versioned JSONL file (header with format version + embedding model/dim, one record per memory, one record per relation edge keyed by export-local keys, no embedding vectors), and `agent-memory import --from jsonl` recreates memories and edges on another machine — into an empty or a non-empty database — idempotently, with imported rows landing `embedding_status = 0` for the existing sweep backfill, expired rows skipped and counted, and malformed files rejected as client errors through the Phase 3 seam. Requirements: DIST-04, INTEROP-02.

**User-approved scope addition (folds the Phase 4 deferral, 04-REVIEW IN-01):** every non-search service path — `store`, `list` (incl. `expand_links`), `update`, `link`/`unlink`, `forget`, `import`, the new `export`, and `/health` — still lets raw SQLite text (`sqlite error: …`, table/column names) reach the client on a storage fault. Import adds another write path through the same mapping, so this phase redacts storage-engine text on EVERY service path, on both transports, the way Phase 4 did for search.

Not in this phase: embedding vectors in the file, export filters, MCP/REST export or import endpoints, cross-tool import dialects, Windows/musl binaries and the v0.1.0 release (Phase 6).

</domain>

<decisions>
## Implementation Decisions

### Surface & CLI shape (DIST-04 / INTEROP-02)
- **D-01:** Export and import are **CLI subcommands only** in v0.1.0: `agent-memory export [--output PATH]` and `agent-memory import --from jsonl PATH` (the second value of the existing `--from` dispatch beside `gsd-state`). No MCP tool and no REST endpoint for either. Rationale: an MCP export would route the corpus through the JSON-RPC stdout channel (MCP-05 purity), a REST import is an arbitrary-file-read primitive on an unauthenticated daemon (PITFALLS security row), and both roadmap criteria describe a file written and read by the user. The **error contract still lives in core**: every malformed-file rejection is `MemoryError::InvalidArgument` (Phase 3 D-04/D-06), so the two-tier taxonomy holds and any later transport inherits 400/`invalid_params` structurally; the CLI exits non-zero printing that message. — **Reversibility:** reversible — adding a transport later is additive.
- **D-02:** `export` writes to **stdout by default**; `--output PATH` writes **temp-file-in-the-same-directory + rename** (atomic; the target is replaced if it exists), **mode 0600 on Unix** (the corpus is T-01-03-sensitive; never looser than the DB), using the same `cfg(unix)` / `cfg(not(unix))` split `config.rs` already has (Windows is one seam, Phase 6). stdout is sanctioned for CLI subcommands (the `import` precedent); MCP-05 applies only to `serve`. Export streams one line at a time — never buffers the corpus as one JSON document.

### JSONL format v1 — the frozen contract (DIST-04)
- **D-03:** **Line 1 is a header record**, mandatory: `{"record":"header","format":"agent-memory-export","version":1,"exported_at":<unix secs>,"embedding_model":"nomic-embed-text","embedding_dim":768,"schema_version":3,"agent_memory_version":"<CARGO_PKG_VERSION>"}`. `embedding_model`/`embedding_dim` are **read from the `meta` table** (the 0002 pin), not from constants, so the header always states what the source DB actually pinned. — **Reversibility:** one-way — the moment one user writes a file, format v1 is a compatibility contract; any field removal or meaning change needs `version: 2` plus a reader that still accepts 1.
- **D-04:** **Memory record:** `{"record":"memory","key":<source row id>,"type":"DECISION","content":"…","tags":[…],"source":<string|null>,"scope":<string|null>,"created_at":<unix>,"last_accessed":<unix>,"access_count":<n>,"expires_at":<unix|null>}`. `key` is the source database's id used **only as an export-local key** — it is never written as an id on import (PITFALLS 18). **Excluded as derived data:** `decay_score` (recomputed on read), `base_weight` (a function of `type`), `embedding_status`, and every vector (PITFALLS 17; ARCHITECTURE Anti-Pattern 4). Field order is the declaration order above (serde struct), golden-tested like the v1.0 envelopes.
- **D-05:** **Link record:** `{"record":"link","from":<key>,"to":<key>,"kind":"relates_to|supersedes|caused_by","created_at":<unix>}`, emitted **after all memory records**, endpoints exactly as stored (04-06 canonical `(min,max)` for `relates_to`; request order for directed kinds). `created_at` travels because D-07 of Phase 4 orders expansion by edge recency.
- **D-06:** Export writes **every row present in the database** (no `--scope`/`--type` filters, no expiry skipping at export time — the importer owns expiry policy) in ascending id order, through one streaming read-pool query per record type. Filters are deferred (additive).

### Import identity, dedup & edge remapping (INTEROP-02)
- **D-07:** The idempotency key stays the existing INTEROP-01 key **`(source, mem_type, content)`** — no new identity column, **no migration 0004**. `Store::exists` is generalised to return the matching row's id (`Option<i64>`, NULL-safe `source IS ?1` kept, CR-01) so a duplicate maps its export key to the **existing** row. Consequence, accepted and documented: two source rows identical on that key collapse to one on import (counted as a duplicate). `source` is **preserved verbatim** (no `jsonl` provenance stamp), so the round trip is faithful and a `gsd-state` row carried in a JSONL file dedups against an earlier direct GSD import.
- **D-08:** **Two-pass, validate-before-write import:** (1) parse and validate the whole file into typed records (header, memories, links) — any error aborts with nothing written; (2) memories pass: per record, skip-and-count if expired (D-10), else dedup-or-insert, building `key → row id`; (3) links pass through the map via the existing `Store::link` (idempotent `INSERT OR IGNORE` + endpoint pre-check), with the record's `created_at` as `now`. A link whose endpoint was **skipped as expired is dropped and counted** (`links_dropped`); a link whose key **does not appear in the file at all is malformed** (whole-file rejection, D-11). All writes go through the existing writer lane; batching into transactions is the planner's call.
- **D-09:** Imported rows go through the shared Phase 3 seam like every insert path: the `type` and `kind` wire strings parse via `MemoryType::try_from` / `LinkKind::try_from`, empty/whitespace `content` is `InvalidArgument`, self-links are `InvalidArgument` (Phase 4 D-02), tags must be a JSON array of strings. `expires_at` is absolute, so `validate_ttl` does not apply; a non-null `expires_at` must be an integer.

### Timestamp & lifecycle policy on import (INTEROP-02; PITFALLS 19)
- **D-10:** Per column, written into the format doc: **`created_at` preserved** (provenance); **`expires_at` preserved**, and a row with `expires_at <= import-time now` is **skipped and counted (`skipped_expired`) — never inserted** for the sweep to delete later; **`last_accessed` refreshed to import-time now** so imported memories start decay-fresh (PITFALLS 19: a preserved months-old `last_accessed` would make a restored corpus unfindable by rank); **`access_count` preserved** (provenance only — `decay_score` depends solely on `last_accessed`, so this changes no ranking); link `created_at` preserved (D-05). The decay-never-deletes invariant is untouched. If the header's `exported_at` is more than one hour in the future, the CLI prints a clock-skew **warning** to stderr and continues.

### Malformed-file policy & the import report (INTEROP-02 criterion 4)
- **D-11:** **Strict, all-or-nothing validation — the opposite of the tolerant `gsd-state` scraper, deliberately:** JSONL is agent-memory's own contract, so a defect means a corrupted or hand-edited file and a half-restored corpus is worse than a clear refusal. Rejected as `InvalidArgument` naming the 1-based line: missing/invalid header, `format` ≠ `agent-memory-export`, non-integer or `< 1` `version`, `version > 1` (message says to upgrade agent-memory), a line that is not a JSON object, an unknown `record` value, an unknown field (`deny_unknown_fields`, the 04-07 precedent), a wrong field type, an invalid `type`/`kind`, empty content, a self-link, a link key absent from the file. Message shape follows Phase 3 D-05: `invalid argument: line 42: unknown record type 'memo'`. **Nothing is written when validation fails.** Expired rows are **not** malformed (D-10). The `gsd-state` importer's tolerance rules are unchanged.
- **D-12:** `ImportReport` grows **additively**: `imported`, `skipped_duplicates` (existing), plus `skipped_expired`, `links_imported`, `links_skipped_duplicates`, `links_dropped`. The `gsd-state` path reports zeros for the new fields. The CLI prints every count on one line plus, for JSONL, a line stating how many rows are pending embedding (D-13).

### Embeddings on import (INTEROP-02 criterion 3)
- **D-13:** **JSONL import never embeds inline.** Every imported row lands `embedding_status = 0` and is re-embedded by the **existing sweep backfill** (128 per tick; the first tick fires on every `serve`/`serve-rest` start) — exactly one re-embed mechanism, zero mixing risk (PITFALLS 17), the CLI never blocks on Ollama, and the criterion holds literally regardless of embedder state. Cost, accepted and documented in the README: a large restore is keyword-only until backfill catches up (128 rows per hourly tick after the first). The `gsd-state` importer keeps its existing one-shot best-effort embed. A faster drain is deferred (see Deferred Ideas).

### Write-path storage-error redaction (user-approved scope addition; 04-REVIEW IN-01)
- **D-14:** Generalise `redact_search_error` into a **service-seam `redact_storage_error(op, e)`** applied to the result of **every public `MemoryService` method** (`store`, `list`, `search`, `update`, `link`, `unlink`, `forget`, `import`, the new `export`, `sweep`). Storage-engine variants (`Sqlite | Pool | Join | Migration`) are logged at `error` level with full detail, then replaced by `MemoryError::Internal("<op> failed due to a storage error (details are in the server log)")`; client tier (`InvalidType | InvalidQuery | InvalidArgument`) and crate-authored text (`Internal`, `SchemaTooNew`) pass through unchanged. `SEARCH_STORAGE_FAILURE` keeps its exact current text (it already fits the pattern), so Phase 4's golden assertions still hold. The two transport mappers stay as they are (their exhaustive matches still compile), which means **MCP, REST — including `/health` — and the CLI are covered by one change at one seam**, mirroring Phase 3 D-06 and Phase 4 T-04-19. The README sentence that scopes redaction to search is widened to every surface. — **Reversibility:** reversible.

### Claude's Discretion
- How the import insert carries preserved `created_at`/`access_count` into the store: extend `NewMemory` with optional overrides (ARCHITECTURE §4) or add a dedicated `ImportedMemory` + `Store::insert_imported` — pick whichever touches fewer existing call sites while keeping `insert`'s single-writer-transaction shape.
- Transaction batching for the import passes (per row vs chunked) and the export read (one statement vs paged) — streaming on both sides is the requirement, the chunk size is not.
- Temp-file naming for `--output`; whether to reuse the backup temp-path helper from `sqlite.rs`.
- Whether the header also carries `exported_rows`/`exported_links` counts (nice for a quick integrity check; additive within v1 only if decided now, since D-11 rejects unknown fields later).
- Exact rustdoc placement of the format spec (`crates/agent-memory-core/src/export.rs` module doc + a README "Export & import" section is the minimum).
- Zero-code README tip documenting `VACUUM INTO` as a same-machine *backup* (FEATURES anti-pattern row) — include if cheap.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Milestone research (decides the patterns this phase implements)
- `.planning/research/SUMMARY.md` — "Phase 3: Export/Import (JSONL)" section (header, no vectors, key→id remap, timestamp policy, rich report) and the pin-triangle do-not-touch list
- `.planning/research/ARCHITECTURE.md` §4 "Export / Import (DIST-04)" — streaming JSONL, `Store::export_rows`, `import/jsonl.rs` beside `gsd_state.rs`, same `MemoryService::import` dedup path, timestamp-preservation note; Anti-Pattern 4 (never export vectors)
- `.planning/research/PITFALLS.md` — Pitfall 17 (model-pinned vectors), 18 (rowid collisions + edge remap, two-pass import), 19 (absolute timestamps: preserve/skip/refresh), Security rows (export 0600; import file-path only on the CLI), Performance row (stream, never through MCP stdout)
- `.planning/research/FEATURES.md` §3 "Portable export/import formats" — JSONL precedent, header fields, embeddings-excluded-by-default, round-trip idempotency

### Prior phase contracts (locked, must be honored)
- `.planning/phases/03-api-hardening-toolchain-spikes/03-CONTEXT.md` — D-04/D-05/D-06: `InvalidArgument` at the `MemoryService` seam, message shape `field + allowed set + offending value`, transports stay thin
- `.planning/phases/04-memory-update-relations-hybrid-search/04-CONTEXT.md` — D-01..D-03 (closed `LinkKind`, duplicate link = idempotent, directed storage), D-07 (edge recency ordering → link `created_at` must travel), D-11/D-12 (backup mechanism this phase inherits for free — no migration here)
- `.planning/phases/04-memory-update-relations-hybrid-search/04-REVIEW.md` — IN-01 (the raw-SQLite-text deferral this phase closes, with the mapper-vs-seam options), WR-02 (`deny_unknown_fields` precedent for new DTOs), IN-05 (SchemaTooNew re-probe note — do not regress)
- `.planning/STATE.md` — accumulated decisions: [03-01] `import()` validates per draft through the shared helper (the seam this phase extends); [Phase 04] 04-06 `relates_to` canonical storage; 04-07 `deny_unknown_fields` on new v0.1.0 DTOs only

### Code the phase extends
- `crates/agent-memory-core/src/service.rs` — `import()` (dedup partition → best-effort embed → insert), `ImportReport`, `redact_search_error` + `SEARCH_STORAGE_FAILURE` (the redaction helper D-14 generalises), `BACKFILL_BATCH`
- `crates/agent-memory-core/src/import/mod.rs` + `import/gsd_state.rs` — the importer module the `jsonl.rs` parser joins (keep the gsd tolerance rules intact)
- `crates/agent-memory-core/src/store/mod.rs` + `store/sqlite.rs` — `Store` trait (`insert`, `exists`, `link`, `pending_embeddings`), `exists`'s NULL-safe probe, the writer-mutex single-writer lane, the `backup_temp_path`/rename helpers (reusable shape for `--output`)
- `crates/agent-memory-core/src/domain.rs` — `MemoryError`, `NewMemory`, `MemoryType`/`LinkKind::try_from`, `validate_*` helpers
- `crates/agent-memory-core/sql/0002_embeddings.sql` (the `meta` table the header reads) and `sql/0003_relations.sql` (the `memory_links` shape link records mirror) — both FROZEN; this phase ships **no migration**
- `crates/agent-memory/src/main.rs` — `Command::Import { from, path, scope }` + `run_import` (the `--from` dispatch gains `jsonl`; `Export` is a new variant); `crates/agent-memory/src/config.rs` — the `cfg(unix)` 0700 pattern D-02 mirrors for 0600 files
- `crates/agent-memory/src/mcp.rs` `map_mcp_error` + `crates/agent-memory/src/rest/handlers.rs` `map_memory_error` — unchanged by D-14 but must keep compiling exhaustively; `health_handler` is one of the surfaces D-14 covers
- `crates/agent-memory-core/tests/import.rs` + `tests/fixtures/STATE.md` — the idempotent-import proof shape to mirror for JSONL (non-empty-DB and overlapping-rowid kill-tests per PITFALLS 18)
- `README.md` — "Import from GSD", "REST API" (the search-only redaction sentence to widen), "Behavioral changes in v0.1.0", "Lifecycle" (backfill cadence the D-13 note points at)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `MemoryService::import(drafts)` already does validate → dedup (`Store::exists`) → insert; the JSONL importer feeds a sibling path that adds key→id mapping, expiry skipping, and a links pass — same seam, same writer lane.
- `Store::exists(source: Option<&str>, mem_type, content)` with NULL-safe `source IS ?1` (CR-01) is the dedup probe to generalise into an id-returning lookup (D-07).
- `Store::link(from, to, kind, now)` already pre-checks endpoints and uses `INSERT OR IGNORE` on the `(from_id, to_id, kind)` primary key — link-record import is idempotent for free, and `now` lets the record's `created_at` be preserved.
- `redact_search_error` / `SEARCH_STORAGE_FAILURE` (service.rs:33-60): exhaustive-match redaction helper; D-14 parameterises it by operation.
- `backup_temp_path` + verify-then-rename in `sqlite.rs` (04-07): the atomic-write shape `--output` reuses.
- `config.rs` `cfg(unix)`/`cfg(not(unix))` `create_data_dir` (0700): the one-seam platform split to mirror for 0600 export files.
- `tests/import.rs` + `FakeEmbedder::failing()` harness: proves import succeeds and is keyword-searchable with Ollama dead — the exact fresh-machine story D-13 relies on.
- `meta` table (`embedding_model`, `embedding_dim`) — the header's source of truth (D-03).

### Established Patterns
- Bad client input never surfaces as 500/`internal_error`; validation lives once at the `MemoryService` seam, transports are thin (Phase 3 D-06). Malformed-file errors are `InvalidArgument` in core even though the CLI is the only caller (D-01).
- Exhaustive `match` on `MemoryError` in both transport mappers and in the redaction helper — new variants fail to compile until classified. D-14 adds no variant.
- All SQL parameterised; export reads and import writes keep bound parameters; `kind`/`type` wire strings never reach SQL unparsed.
- Shipped migrations are frozen (fixture divergence test) and `LATEST_SCHEMA_VERSION = 3` — this phase adds no schema, so neither moves.
- Coverage-bearing tests run in-process (core + handler tests); spawned-binary tests are realism-only. The CLI `export`/`import` glue in `main.rs` should stay thin so the format logic is unit-tested in core.
- `cargo fmt` + `clippy --workspace --all-targets --locked -- -D warnings` + `cargo test --workspace --locked` green before every commit; no `unwrap()` in production code.

### Integration Points
- `main.rs` `Command` enum: new `Export { output: Option<PathBuf> }` variant; `Import` gains the `jsonl` branch in `run_import`'s `--from` match (the `gsd-state`-only bail becomes a two-arm match).
- `service.rs`: new `export()` (streaming read) and `import_jsonl()`-style entry (or an extended `import` taking typed records) beside `import()`; every public method's error path routed through `redact_storage_error`.
- `store/mod.rs` trait: `export_memories`/`export_links` streaming reads, id-returning dedup lookup, an insert that accepts preserved `created_at`/`access_count`, and a `meta` read for the header.
- `import/jsonl.rs` (new) beside `gsd_state.rs`; `export.rs` (new) in core holding the record structs (serde `Serialize + Deserialize`, `deny_unknown_fields`) that are the single definition of format v1.
- README: new "Export & import" section (format v1 spec, timestamp policy table, pending-embedding note, strict-vs-tolerant importer note), REST section redaction sentence widened, lifecycle section cross-reference.

</code_context>

<specifics>
## Specific Ideas

- Error strings stay greppable and stable-prefixed (Phase 3 D-05): tests assert on `line 42:`-style prefixes and the field name, not on full equality.
- The `version > 1` refusal message names both versions and the fix, in the spirit of Phase 4 D-13: `file is format version 2 but this binary reads up to version 1 — upgrade agent-memory`.
- The round-trip kill-test (PITFALLS 18): export → import into a NON-empty DB whose rowids overlap the file's keys → assert no pre-existing memory gained a phantom edge, then export again and diff modulo `key`/`last_accessed`.
- The expiry kill-test (PITFALLS 19): a fixture with one expired, one ancient (retrievable via `list`, low decay) and one fresh row → report shows `skipped_expired: 1`, `imported: 2`.
- The redaction proof (D-14): one in-process test per surface class (a write path and `/health`) against a store whose underlying file is made unreadable or whose table is dropped, asserting the client body contains no `sqlite` substring while the log carries it — the T-04-19 pattern.

</specifics>

<deferred>
## Deferred Ideas

- `--include-embeddings` export flag with enforced model/dim header check on import (FEATURES P2) — only when a same-model offline-restore use case appears.
- Export filters (`--scope`, `--type`) — additive; D-06 exports everything.
- MCP tool / REST endpoint for export or import — would need an in-body payload design and a stdout-purity story; D-01 keeps both CLI-only.
- `import --embed-now` or a one-shot `backfill` subcommand that drains pending embeddings immediately after a large restore — mitigates the D-13 latency (128 rows per hourly tick) without a second re-embed mechanism.
- Cross-tool import adapters (mem0 / Chroma JSONL dialects) — no standard exists; wait for demand (FEATURES).
- A stable per-memory identity column (UUID) for dedup that survives content edits — would need migration 0004 and changes the idempotency contract; D-07 keeps the content key.
- Cross-process exclusive lock on the pre-migration backup (04-REVIEW WR-01 residual) and the 04-REVIEW IN-02/IN-04/IN-05/IN-06 items — unchanged, still deferred; IN-05's `SchemaTooNew` re-probe fix is cheap if a planner wants to ride it along, but it is not Phase 5 scope.

</deferred>

---

*Phase: 5-Portable Export & Import*
*Context gathered: 2026-10-25*
