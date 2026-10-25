# Phase 5: Portable Export & Import - Research

**Researched:** 2026-10-25
**Domain:** Rust / serde JSONL interchange format + SQLite store extension + CLI atomic file write + service-seam error redaction (agent-memory workspace, no new crates)
**Confidence:** HIGH (every load-bearing claim is either read from this repo's source this session or verified by a scratch probe against the pinned serde 1.0.228 / serde_json 1.0.150)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Surface & CLI shape (DIST-04 / INTEROP-02)
- **D-01:** Export and import are **CLI subcommands only** in v0.1.0: `agent-memory export [--output PATH]` and `agent-memory import --from jsonl PATH` (the second value of the existing `--from` dispatch beside `gsd-state`). No MCP tool and no REST endpoint for either. Rationale: an MCP export would route the corpus through the JSON-RPC stdout channel (MCP-05 purity), a REST import is an arbitrary-file-read primitive on an unauthenticated daemon (PITFALLS security row), and both roadmap criteria describe a file written and read by the user. The **error contract still lives in core**: every malformed-file rejection is `MemoryError::InvalidArgument` (Phase 3 D-04/D-06), so the two-tier taxonomy holds and any later transport inherits 400/`invalid_params` structurally; the CLI exits non-zero printing that message. — **Reversibility:** reversible — adding a transport later is additive.
- **D-02:** `export` writes to **stdout by default**; `--output PATH` writes **temp-file-in-the-same-directory + rename** (atomic; the target is replaced if it exists), **mode 0600 on Unix** (the corpus is T-01-03-sensitive; never looser than the DB), using the same `cfg(unix)` / `cfg(not(unix))` split `config.rs` already has (Windows is one seam, Phase 6). stdout is sanctioned for CLI subcommands (the `import` precedent); MCP-05 applies only to `serve`. Export streams one line at a time — never buffers the corpus as one JSON document.

#### JSONL format v1 — the frozen contract (DIST-04)
- **D-03:** **Line 1 is a header record**, mandatory: `{"record":"header","format":"agent-memory-export","version":1,"exported_at":<unix secs>,"embedding_model":"nomic-embed-text","embedding_dim":768,"schema_version":3,"agent_memory_version":"<CARGO_PKG_VERSION>"}`. `embedding_model`/`embedding_dim` are **read from the `meta` table** (the 0002 pin), not from constants, so the header always states what the source DB actually pinned. — **Reversibility:** one-way — the moment one user writes a file, format v1 is a compatibility contract; any field removal or meaning change needs `version: 2` plus a reader that still accepts 1.
- **D-04:** **Memory record:** `{"record":"memory","key":<source row id>,"type":"DECISION","content":"…","tags":[…],"source":<string|null>,"scope":<string|null>,"created_at":<unix>,"last_accessed":<unix>,"access_count":<n>,"expires_at":<unix|null>}`. `key` is the source database's id used **only as an export-local key** — it is never written as an id on import (PITFALLS 18). **Excluded as derived data:** `decay_score` (recomputed on read), `base_weight` (a function of `type`), `embedding_status`, and every vector (PITFALLS 17; ARCHITECTURE Anti-Pattern 4). Field order is the declaration order above (serde struct), golden-tested like the v1.0 envelopes.
- **D-05:** **Link record:** `{"record":"link","from":<key>,"to":<key>,"kind":"relates_to|supersedes|caused_by","created_at":<unix>}`, emitted **after all memory records**, endpoints exactly as stored (04-06 canonical `(min,max)` for `relates_to`; request order for directed kinds). `created_at` travels because D-07 of Phase 4 orders expansion by edge recency.
- **D-06:** Export writes **every row present in the database** (no `--scope`/`--type` filters, no expiry skipping at export time — the importer owns expiry policy) in ascending id order, through one streaming read-pool query per record type. Filters are deferred (additive).

#### Import identity, dedup & edge remapping (INTEROP-02)
- **D-07:** The idempotency key stays the existing INTEROP-01 key **`(source, mem_type, content)`** — no new identity column, **no migration 0004**. `Store::exists` is generalised to return the matching row's id (`Option<i64>`, NULL-safe `source IS ?1` kept, CR-01) so a duplicate maps its export key to the **existing** row. Consequence, accepted and documented: two source rows identical on that key collapse to one on import (counted as a duplicate). `source` is **preserved verbatim** (no `jsonl` provenance stamp), so the round trip is faithful and a `gsd-state` row carried in a JSONL file dedups against an earlier direct GSD import.
- **D-08:** **Two-pass, validate-before-write import:** (1) parse and validate the whole file into typed records (header, memories, links) — any error aborts with nothing written; (2) memories pass: per record, skip-and-count if expired (D-10), else dedup-or-insert, building `key → row id`; (3) links pass through the map via the existing `Store::link` (idempotent `INSERT OR IGNORE` + endpoint pre-check), with the record's `created_at` as `now`. A link whose endpoint was **skipped as expired is dropped and counted** (`links_dropped`); a link whose key **does not appear in the file at all is malformed** (whole-file rejection, D-11). All writes go through the existing writer lane; batching into transactions is the planner's call.
- **D-09:** Imported rows go through the shared Phase 3 seam like every insert path: the `type` and `kind` wire strings parse via `MemoryType::try_from` / `LinkKind::try_from`, empty/whitespace `content` is `InvalidArgument`, self-links are `InvalidArgument` (Phase 4 D-02), tags must be a JSON array of strings. `expires_at` is absolute, so `validate_ttl` does not apply; a non-null `expires_at` must be an integer.

#### Timestamp & lifecycle policy on import (INTEROP-02; PITFALLS 19)
- **D-10:** Per column, written into the format doc: **`created_at` preserved** (provenance); **`expires_at` preserved**, and a row with `expires_at <= import-time now` is **skipped and counted (`skipped_expired`) — never inserted** for the sweep to delete later; **`last_accessed` refreshed to import-time now** so imported memories start decay-fresh (PITFALLS 19: a preserved months-old `last_accessed` would make a restored corpus unfindable by rank); **`access_count` preserved** (provenance only — `decay_score` depends solely on `last_accessed`, so this changes no ranking); link `created_at` preserved (D-05). The decay-never-deletes invariant is untouched. If the header's `exported_at` is more than one hour in the future, the CLI prints a clock-skew **warning** to stderr and continues.

#### Malformed-file policy & the import report (INTEROP-02 criterion 4)
- **D-11:** **Strict, all-or-nothing validation — the opposite of the tolerant `gsd-state` scraper, deliberately:** JSONL is agent-memory's own contract, so a defect means a corrupted or hand-edited file and a half-restored corpus is worse than a clear refusal. Rejected as `InvalidArgument` naming the 1-based line: missing/invalid header, `format` ≠ `agent-memory-export`, non-integer or `< 1` `version`, `version > 1` (message says to upgrade agent-memory), a line that is not a JSON object, an unknown `record` value, an unknown field (`deny_unknown_fields`, the 04-07 precedent), a wrong field type, an invalid `type`/`kind`, empty content, a self-link, a link key absent from the file. Message shape follows Phase 3 D-05: `invalid argument: line 42: unknown record type 'memo'`. **Nothing is written when validation fails.** Expired rows are **not** malformed (D-10). The `gsd-state` importer's tolerance rules are unchanged.
- **D-12:** `ImportReport` grows **additively**: `imported`, `skipped_duplicates` (existing), plus `skipped_expired`, `links_imported`, `links_skipped_duplicates`, `links_dropped`. The `gsd-state` path reports zeros for the new fields. The CLI prints every count on one line plus, for JSONL, a line stating how many rows are pending embedding (D-13).

#### Embeddings on import (INTEROP-02 criterion 3)
- **D-13:** **JSONL import never embeds inline.** Every imported row lands `embedding_status = 0` and is re-embedded by the **existing sweep backfill** (128 per tick; the first tick fires on every `serve`/`serve-rest` start) — exactly one re-embed mechanism, zero mixing risk (PITFALLS 17), the CLI never blocks on Ollama, and the criterion holds literally regardless of embedder state. Cost, accepted and documented in the README: a large restore is keyword-only until backfill catches up (128 rows per hourly tick after the first). The `gsd-state` importer keeps its existing one-shot best-effort embed. A faster drain is deferred (see Deferred Ideas).

#### Write-path storage-error redaction (user-approved scope addition; 04-REVIEW IN-01)
- **D-14:** Generalise `redact_search_error` into a **service-seam `redact_storage_error(op, e)`** applied to the result of **every public `MemoryService` method** (`store`, `list`, `search`, `update`, `link`, `unlink`, `forget`, `import`, the new `export`, `sweep`). Storage-engine variants (`Sqlite | Pool | Join | Migration`) are logged at `error` level with full detail, then replaced by `MemoryError::Internal("<op> failed due to a storage error (details are in the server log)")`; client tier (`InvalidType | InvalidQuery | InvalidArgument`) and crate-authored text (`Internal`, `SchemaTooNew`) pass through unchanged. `SEARCH_STORAGE_FAILURE` keeps its exact current text (it already fits the pattern), so Phase 4's golden assertions still hold. The two transport mappers stay as they are (their exhaustive matches still compile), which means **MCP, REST — including `/health` — and the CLI are covered by one change at one seam**, mirroring Phase 3 D-06 and Phase 4 T-04-19. The README sentence that scopes redaction to search is widened to every surface. — **Reversibility:** reversible.

### Claude's Discretion
- How the import insert carries preserved `created_at`/`access_count` into the store: extend `NewMemory` with optional overrides (ARCHITECTURE §4) or add a dedicated `ImportedMemory` + `Store::insert_imported` — pick whichever touches fewer existing call sites while keeping `insert`'s single-writer-transaction shape.
- Transaction batching for the import passes (per row vs chunked) and the export read (one statement vs paged) — streaming on both sides is the requirement, the chunk size is not.
- Temp-file naming for `--output`; whether to reuse the backup temp-path helper from `sqlite.rs`.
- Whether the header also carries `exported_rows`/`exported_links` counts (nice for a quick integrity check; additive within v1 only if decided now, since D-11 rejects unknown fields later).
- Exact rustdoc placement of the format spec (`crates/agent-memory-core/src/export.rs` module doc + a README "Export & import" section is the minimum).
- Zero-code README tip documenting `VACUUM INTO` as a same-machine *backup* (FEATURES anti-pattern row) — include if cheap.

### Deferred Ideas (OUT OF SCOPE)
- `--include-embeddings` export flag with enforced model/dim header check on import (FEATURES P2) — only when a same-model offline-restore use case appears.
- Export filters (`--scope`, `--type`) — additive; D-06 exports everything.
- MCP tool / REST endpoint for export or import — would need an in-body payload design and a stdout-purity story; D-01 keeps both CLI-only.
- `import --embed-now` or a one-shot `backfill` subcommand that drains pending embeddings immediately after a large restore — mitigates the D-13 latency (128 rows per hourly tick) without a second re-embed mechanism.
- Cross-tool import adapters (mem0 / Chroma JSONL dialects) — no standard exists; wait for demand (FEATURES).
- A stable per-memory identity column (UUID) for dedup that survives content edits — would need migration 0004 and changes the idempotency contract; D-07 keeps the content key.
- Cross-process exclusive lock on the pre-migration backup (04-REVIEW WR-01 residual) and the 04-REVIEW IN-02/IN-04/IN-05/IN-06 items — unchanged, still deferred; IN-05's `SchemaTooNew` re-probe fix is cheap if a planner wants to ride it along, but it is not Phase 5 scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| DIST-04 | User can export all memories to a versioned JSONL file (header records format version + embedding model/dim; embeddings excluded as derived data; relation edges included with export-local key remapping) | Patterns 1 (record enum, verified field order), 4 (streaming boxed-writer export on one read snapshot), 7 (atomic 0600 `--output`), 5b (`meta` read for the header); Pitfalls 5, 7, 8; Code Examples 1, 4, 5 |
| INTEROP-02 | User can import that JSONL on another machine — idempotent re-run, imported rows land `embedding_status = 0` and re-embed via the existing sweep backfill, expired items skipped and counted | Patterns 2 (two-step header parse), 3 (per-line parse with own line counter), 5 (`ImportedMemory` + `insert_imported`), 6 (`find_by_key` dedup → key map), 6b (`LinkOutcome::Linked { created }` for `links_skipped_duplicates`); Pitfalls 1–4, 6, 9–11; Code Examples 2, 3, 6 |
| D-14 (scope addition, 04-REVIEW IN-01) | Redact raw SQLite text on every `MemoryService` path, both transports + CLI | Pattern 8 (`redact_storage_error(op, e)` with byte-identical `SEARCH_STORAGE_FAILURE`), the public-method inventory, the existing-assertion inventory, the RENAME-table fault-injection recipe; Code Example 7 |
</phase_requirements>

## Summary

Phase 5 is almost entirely in-repo engineering against code that already exists: the `MemoryService::import` → `Store::exists` → `Store::insert` seam, the `Store::link` idempotent edge insert, the `meta` pin table, the `redact_search_error` helper, the `config.rs` `cfg(unix)` split and the `sqlite.rs` temp-then-rename backup shape. No new crate is needed or recommended: serde 1.0.228 + serde_json 1.0.150 (already pinned in `Cargo.lock`) and `std` cover the record enum, per-line JSONL reading/writing, and the 0600 atomic file write. The one genuinely uncertain technical question — whether `#[serde(deny_unknown_fields)]` works on an internally tagged `#[serde(tag = "record")]` enum — was settled empirically this session with a scratch crate against the exact pinned versions: **it works** (the only documented serde limitation is `flatten`), but with one consequence the planner must design around: for errors detected inside a variant (unknown field, wrong type) serde_json reports `line 0, column 0`, so D-11's `line N:` prefix must come from the importer's own 1-based line counter, never from `serde_json::Error::line()`.

Three design recommendations fall out of counting call sites rather than taste. (1) Carry preserved `created_at`/`access_count` with a new `ImportedMemory` struct + `Store::insert_imported` — extending `NewMemory` would touch 15 struct-literal construction sites (12 in core tests, `gsd_state.rs`, `mcp.rs`, `handlers.rs`), the dedicated type touches zero, and `Store` has exactly one implementor. (2) Generalise `Store::exists` into `find_by_key(...) -> Result<Option<i64>>`: it has one caller (`service.rs:413`) and one impl; no test calls it directly. (3) D-12's `links_skipped_duplicates` cannot be counted with today's `LinkOutcome::Linked` (a duplicate and a fresh insert are indistinguishable); the least-churn honest fix is `LinkOutcome::Linked { created: bool }` — 7 sites (1 producer, 2 transport matches that become `Linked { .. }`, 4 test asserts that become *stronger*). The wire shape is unchanged because transports echo request ids, not the outcome.

D-14 is a mechanical generalisation with a precise compatibility constraint: four existing tests assert on the exact `SEARCH_STORAGE_FAILURE` text (`hybrid.rs:729,760` by equality; `handlers.rs:881` and `mcp.rs:1336` by `contains`), and the new `format!("{op} failed due to a storage error (details are in the server log)")` with `op = "search"` is byte-identical to the current constant, so nothing moves. The fault-injection recipe for the new redaction tests is already proven in three layers (`ALTER TABLE … RENAME TO …_offline` on a raw side connection); for write paths and `/health` the lever is renaming `memories` (or `memory_links` for `link`/`unlink`). Note the repo's `pre-bash-rust.sh` hook blocks any Bash command whose text matches `drop table` — another reason to keep using RENAME, not DROP, in test SQL.

**Primary recommendation:** Build format v1 as one `#[serde(tag = "record", deny_unknown_fields)]` enum in `crates/agent-memory-core/src/export.rs` with `String` wire fields for `type`/`kind` (converted through `MemoryType::try_from` / `LinkKind::try_from` in a pure `import/jsonl.rs` validator that owns the line counter); stream export through a `Box<dyn Write + Send>` moved into one `spawn_blocking` hop that reads memories then links on ONE read-pool connection inside one read transaction; add `ImportedMemory`/`insert_imported`, `find_by_key`, and `LinkOutcome::Linked { created }` to the store; wrap every public `MemoryService` method with `redact_storage_error(op, e)`; keep `main.rs` glue thin (file read, 0600 temp+rename, report printing).

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Format v1 record definitions (header/memory/link structs, serde attrs, golden field order) | Core library (`agent-memory-core::export`) | — | Single definition of the contract; unit-testable in-process; both the exporter and the importer compile against it |
| Export streaming read (memories asc id, then links) | Store (`SqliteStore`, read pool) | Core service (`MemoryService::export` orchestrates, maps io errors) | Only the store knows the SQL/snapshot semantics; the service owns the `spawn_blocking` + redaction seam |
| Header `embedding_model`/`embedding_dim` | Store (`meta` table read) | Core service (parses `'768'` → integer) | D-03: header must state what the source DB pinned, not a constant |
| `agent_memory_version` in the header | CLI binary (passes `env!("CARGO_PKG_VERSION")` in) | Core (accepts it as a parameter) | The binary's version is the user-facing one; core and binary are separate crates with separate `version =` lines (both `0.0.1` today) |
| stdout vs `--output` temp+rename+0600 | CLI binary (`main.rs` / a small file helper beside `config.rs`) | — | OS/filesystem concern; mirrors the `config.rs` `cfg(unix)` split; core stays I/O-agnostic like `gsd_state.rs` |
| JSONL parse + strict validation (D-08 pass 1, D-09, D-11) | Core library (`import/jsonl.rs`, pure, no I/O) | — | Mirrors `parse_gsd_state`: the CLI reads the file, core validates; errors are `MemoryError::InvalidArgument` so any future transport inherits 400/`invalid_params` |
| Memories pass (expiry skip, dedup via `find_by_key`, `insert_imported`, key→id map) | Core service (`MemoryService::import_jsonl`) | Store (writer lane) | Same seam as `import()`; the service owns the clock (`now`) and the report |
| Links pass (`Store::link` with preserved `created_at`, drop/duplicate counting) | Core service | Store (`link` on writer lane) | D-08 locks `Store::link`; the service maps keys → ids |
| Import report printing, pending-embedding line, clock-skew warning | CLI binary (`run_import`) | Core (`ImportReport` fields; `tracing::warn` for skew) | Presentation is the CLI's; counts come from core |
| Storage-error redaction on every path (D-14) | Core service seam | — (transport mappers unchanged) | One change covers MCP, REST incl. `/health`, and CLI — Phase 3 D-06 / Phase 4 T-04-19 pattern |
| Re-embedding imported rows | Existing sweep backfill (`MemoryService::sweep`, `BACKFILL_BATCH = 128`) | — | D-13: exactly one re-embed mechanism; import never calls the embedder |

## Standard Stack

### Core (all already in the workspace — no installs this phase)
| Library | Version (Cargo.lock) | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `serde` (derive) | 1.0.228 `[VERIFIED: Cargo.lock]` | `Serialize`/`Deserialize` for the format v1 record enum; `deny_unknown_fields`; `rename_all` | Already a workspace dependency of both crates (`serde.workspace = true`) `[VERIFIED: crates/agent-memory-core/Cargo.toml, crates/agent-memory/Cargo.toml]` |
| `serde_json` | 1.0.150 `[VERIFIED: Cargo.lock]` | `to_writer` per record + `\n`; `from_str::<Record>` per line | Already a workspace dependency; the only JSON codec in the tree |
| `rusqlite` (bundled, functions, backup) | 0.39.0 `[VERIFIED: Cargo.lock]` | Streaming `query_map` reads, writer-lane transactions, `meta` read | Pin triangle must not move (REQUIREMENTS Out of Scope) |
| `std` (`std::fs::OpenOptions`, `std::os::unix::fs::OpenOptionsExt::mode`, `std::fs::rename`, `std::io::{BufWriter, Write}`, `std::io::Stdout::lock`) | rustc 1.94.1 `[VERIFIED: rustc --version]` | 0600 temp file, atomic replace, buffered streaming writer | No dependency needed; `config.rs` already uses the sibling `DirBuilderExt::mode` under `cfg(unix)` `[VERIFIED: crates/agent-memory/src/config.rs:38-48]` |
| `tracing` | workspace | `error!` log line carrying the redacted storage detail; `warn!` for clock skew | Existing logging channel (stderr only) |

### Supporting (test-side, already present)
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tempfile` | 3.27.0 `[VERIFIED: Cargo.lock]` (dev-dependency of both crates) | Temp DB dirs and export files in tests | Tests only — it is NOT a runtime dependency of the binary, so `NamedTempFile::persist` is not available for `--output` without promoting it (rejected: std suffices) `[VERIFIED: crates/agent-memory/Cargo.toml dev-dependencies]` |
| `FakeEmbedder` / `TestClock` (`test-clock` feature) | in-repo | Deterministic `exported_at`, expiry fixtures, backfill proof | Every core test `[VERIFIED: crates/agent-memory-core/src/embed/mod.rs:92-110, src/clock.rs:29-44]` |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| One `#[serde(tag = "record")]` enum | Three separate structs + a `serde_json::Value` pre-read of `record` | Two parses per line; the enum gives the `unknown variant 'memo', expected one of header, memory, link` message for free and one definition of the format |
| `from_str::<Record>(line)` per line with own counter | `serde_json::Deserializer::from_reader(file).into_iter::<Record>()` (StreamDeserializer) | StreamDeserializer's `line()` is real for syntax errors but **0 for in-variant data errors** (probe); the per-line approach gives correct `line N:` for every rejection class and makes "a line that is not a JSON object" trivially checkable |
| std `OpenOptions` + `mode(0o600)` + `rename` | `tempfile::NamedTempFile::new_in(dir)` + `persist` | Would promote `tempfile` to a runtime dep for one helper; CONTEXT requires a reason for any dependency — none |
| `Box<dyn Write + Send>` moved into `spawn_blocking` | `tokio::task::block_in_place` with `&mut dyn Write` | `block_in_place` is unusual in this codebase; the boxed writer keeps the `spawn_blocking` convention every other service method uses |
| `LinkOutcome::Linked { created: bool }` | A second store method `link_imported` returning `bool` | D-08 locks "the existing `Store::link`"; the enum field touches 7 sites, a sibling method duplicates the endpoint pre-check |

**Installation:** none — `cargo build --workspace --locked` must keep passing with an unchanged `Cargo.lock`.

**Version verification:** `grep -A1 'name = "serde"' Cargo.lock` → `1.0.228`; `serde_json` → `1.0.150`; `rusqlite` → `0.39.0`; `tempfile` → `3.27.0` `[VERIFIED: Cargo.lock, 2026-10-25]`.

## Package Legitimacy Audit

Not applicable — **this phase installs no external packages**. Every dependency used (`serde`, `serde_json`, `rusqlite`, `tracing`, `std`) is already pinned in the committed `Cargo.lock`; `cargo test --workspace --locked` will fail if a plan accidentally adds one, which is the intended guard.

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Project Constraints (from CLAUDE.md)

From `/Users/jirihermann/Documents/workspace-1-ideas/unity-in-flow-ai/10-agent-memory/CLAUDE.md` and the ecosystem `CLAUDE.md` one level up (both read this session):

- Rust stable, edition 2021; `clap` derive for CLI; `serde` + `serde_json`; `tokio` for async; `anyhow` in the binary, `thiserror` in the library.
- `cargo fmt` before every commit; `cargo clippy --workspace --all-targets --locked -- -D warnings` must pass; `cargo test --workspace --locked` green before every commit (orchestrator's wording; CI runs `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` `[VERIFIED: .github/workflows/ci.yml:32,38]`).
- **No `unwrap()` in production code** — use `?` or handle the error. (`insert` uses `unwrap_or_else` for tags JSON, which is the accepted shape.)
- Pattern match exhaustively — no catch-all `_` unless truly needed (the error mappers and `redact_search_error` rely on this: a new `MemoryError` variant must fail to compile until tiered).
- Test coverage > 80 % on core logic; CI gate `cargo llvm-cov --workspace --fail-under-lines 80` `[VERIFIED: ci.yml:44]`. Coverage-bearing tests run in-process; spawned-binary tests contribute zero coverage (a SIGKILL'd child never flushes profile data — `handlers.rs:456-460` comment).
- Zero cloud dependency; no secrets committed.
- **CI is GitHub-hosted on purpose** (`runs-on: ubuntu-latest`, `permissions: contents: read`) — never change runners; `release.yml` is tag-triggered, SHA-pinned, no org secrets.
- Document public items with `///`; KDoc/JSDoc/rustdoc on all public APIs.
- No new crates without a reason (CONTEXT.md reinforces: prefer none).
- Shipped migrations 0001–0003 are FROZEN; `LATEST_SCHEMA_VERSION = 3`; this phase ships no migration.
- Bash safety hook `.claude/hooks/pre-bash-rust.sh` blocks: force-push, `cargo publish`, `rm -rf *.db`, and any command text matching `drop (table|database|schema)` (case-insensitive) `[VERIFIED: .claude/hooks/pre-bash-rust.sh]`. Planner note: test SQL must use `ALTER TABLE … RENAME` (the existing T-04-19 recipe), and executors must not `grep`/`echo` a literal DROP in a shell command.
- GSD workflow enforcement: edits happen through `/gsd-execute-phase` / `/gsd-quick`.

## Architecture Patterns

### System Architecture Diagram

```
EXPORT  (agent-memory export [--output PATH])
────────────────────────────────────────────────────────────────────────────────
 main.rs                     MemoryService::export(out, version)        SqliteStore
 ┌─────────────────────┐     ┌──────────────────────────────┐     ┌────────────────────┐
 │ choose sink:        │     │ now = clock.now()            │     │ ONE pooled read    │
 │  stdout.lock()  ──┐ │     │ spawn_blocking {             │     │ conn, ONE read tx  │
 │  OR temp file   ──┼─┼──►  │   store.export_rows(|rec|    │ ──► │  SELECT meta pin   │
 │  (0600,create_new)│ │     │     to_writer(out, rec)?;    │     │  SELECT memories   │
 │ BufWriter(Box<dyn │ │     │     out.write_all(b"\n")?    │ ◄── │    ORDER BY id     │
 │  Write+Send>)     │ │     │   )                          │     │  SELECT links      │
 └──────────┬────────┘ │     │   out.flush()                │     │    ORDER BY from,to│
            │          │     │ }.map_err(redact("export"))  │     │    ,kind           │
            ▼          │     └──────────────────────────────┘     └────────────────────┘
 Ok  → sync_all, rename(temp → PATH)      io::Error → MemoryError::Internal (crate text)
 Err → remove temp, exit non-zero         Sqlite/Pool/Join → redacted "export failed …"

IMPORT  (agent-memory import --from jsonl PATH)
────────────────────────────────────────────────────────────────────────────────
 main.rs            import/jsonl.rs (pure)              MemoryService::import_jsonl
 ┌──────────────┐   ┌────────────────────────────┐     ┌──────────────────────────────────┐
 │ read_to_string│──►│ PASS 1 (no writes)         │     │ now = clock.now()                │
 │ (anyhow ctx) │   │ line 1: lenient probe       │     │ skew warn if exported_at>now+3600│
 └──────────────┘   │   {record,format,version}   │     │ PASS 2 (writer lane, 1 hop):     │
                    │   → version>1 "upgrade" msg │     │  for mem: expires_at<=now?       │
                    │ line 1: strict Record::Header│    │    skipped_expired++ , no map    │
                    │ lines 2..: first non-ws '{'?│ ──► │   else find_by_key → Some(id)?   │
                    │   from_str::<Record>(line)  │     │    dup++, map[key]=id            │
                    │   type/kind via try_from    │     │   else insert_imported → id      │
                    │   content.trim non-empty    │     │    imported++, map[key]=id       │
                    │   from!=to; keys unique;    │     │ PASS 3 (writer lane, 1 hop):     │
                    │   link keys ∈ memory keys   │     │  for link: both keys in map?     │
                    │ any Err → InvalidArgument   │     │   store.link(a,b,kind,created_at)│
                    │   "line N: …" (own counter) │     │    Linked{created:true} → +1     │
                    └────────────────────────────┘     │    Linked{created:false}→ dup+1   │
                                                        │   else links_dropped++           │
                                                        │ }.map_err(redact("import"))      │
                                                        └──────────────┬───────────────────┘
                                                                       ▼
                                      ImportReport{imported, skipped_duplicates, skipped_expired,
                                                   links_imported, links_skipped_duplicates, links_dropped}
                    main.rs prints one line of counts + "N rows pending embedding (backfilled by the sweep)"
                    rows land embedding_status = 0 → existing sweep backfill (128/tick) re-embeds locally
```

### Recommended Project Structure

```
crates/agent-memory-core/src/
├── export.rs              # NEW: format v1 — Record enum (Header/Memory/Link), FORMAT_NAME,
│                          #      FORMAT_VERSION = 1, module doc = the format spec; golden tests
├── import/
│   ├── mod.rs             # add `pub mod jsonl;` + a doc note that jsonl is STRICT (D-11) unlike gsd_state
│   ├── gsd_state.rs       # unchanged
│   └── jsonl.rs           # NEW: parse_jsonl(text) -> Result<JsonlFile, MemoryError>  (pure, no I/O)
├── service.rs             # redact_storage_error(op, e); export(); import_jsonl(); ImportReport +4 fields
├── domain.rs              # ImportedMemory; LinkOutcome::Linked { created: bool }
└── store/
    ├── mod.rs             # find_by_key (replaces exists); insert_imported; export_rows; embedding_pin
    └── sqlite.rs          # impls; insert/insert_imported share one private row-insert helper
crates/agent-memory-core/tests/
├── export_import.rs       # NEW: round trip, overlapping-rowid kill-test, expiry fixture, idempotency,
│                          #      malformed-file table, pending→sweep→searchable
├── redaction.rs           # NEW (or extend hybrid.rs): one write path + list redacted, no "sqlite" text
└── import.rs              # unchanged gsd-state proofs (still pass: new report fields are zeros)
crates/agent-memory/src/
├── main.rs                # Command::Export { output }, `jsonl` arm in run_import, report printing
├── export_file.rs         # NEW (or in config.rs): create_private_temp (cfg split) + rename helper
└── rest/handlers.rs, mcp.rs  # unchanged mappers; add in-process tests: /health + a write path redacted
crates/agent-memory/tests/
└── cli.rs                 # NEW, realism-only: spawn `export --output` then `import --from jsonl`
README.md                  # "Export & import" section; widen the REST redaction sentence (line 462)
```

### Pattern 1: One internally tagged, strict record enum (format v1)

**What:** A single `Record` enum with `#[serde(tag = "record", rename_all = "lowercase", deny_unknown_fields)]`, struct variants `Header`, `Memory`, `Link`. Wire `type`/`kind` are `String` so conversion runs through the Phase 3 seam (`MemoryType::try_from` / `LinkKind::try_from`) and produces the established messages.

**When to use:** Always — it is the single definition of the contract for both directions.

**Verified facts (scratch probe, serde 1.0.228 / serde_json 1.0.150, 2026-10-25):** `[VERIFIED: /tmp/serde-probe run output]`
- `deny_unknown_fields` on the tagged enum **rejects unknown fields inside every variant**: `unknown field 'extra', expected 'format' or 'version'` / `unknown field 'bogus', expected one of 'key', 'content', 'source'`.
- Unknown tag value → `unknown variant 'memo', expected one of 'header', 'memory', 'link' at line 1 column 16` (position kept).
- **In-variant errors report `line 0 column 0`** (unknown field, `invalid type: string "1", expected i64`, `invalid type: floating point 1.5, expected u32`, `invalid value: integer -1, expected u32`) — the internally-tagged path buffers the object into serde's `Content`, losing position.
- A missing `Option<T>` field deserializes as `None` (so `source`/`scope`/`expires_at` may be absent in a hand-edited file and still parse — the exporter always writes them explicitly as `null`, so round-trip output is canonical).
- A non-object line `[1,2]` → `invalid type: integer 1, expected variant identifier` (opaque — pre-check the first non-whitespace byte is `{` to say `line N: expected a JSON object`).
- Trailing text after the object → `trailing characters at line 1 column 55`.
- Serialization emits the tag **first**, then fields in declaration order: `{"record":"memory","key":7,"content":"c","source":null}` — so D-04's documented order is exactly the struct's declaration order with `record` first.

Serde's only documented `deny_unknown_fields` limitation is `flatten` (not used here): "this attribute is not supported in combination with `flatten`, neither on the outer struct nor on the flattened field" `[CITED: serde.rs/container-attrs.html via Context7 /websites/serde_rs]`. Internally tagged enums support struct variants, newtype variants containing structs/maps, and unit variants — not tuple variants `[CITED: serde.rs/enum-representations.html]`.

**Example:**
```rust
// crates/agent-memory-core/src/export.rs  (shape verified by the probe; field names per D-03/D-04/D-05)
use serde::{Deserialize, Serialize};

pub const FORMAT_NAME: &str = "agent-memory-export";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "lowercase", deny_unknown_fields)]
pub enum Record {
    Header {
        format: String,
        version: u32,            // u32 rejects -1 and 1.5 at serde level; version == 0 needs an explicit check
        exported_at: i64,
        embedding_model: String,
        embedding_dim: u32,
        schema_version: i64,
        agent_memory_version: String,
    },
    Memory {
        key: i64,
        #[serde(rename = "type")]
        mem_type: String,        // String on the wire → MemoryType::try_from in jsonl.rs (D-09)
        content: String,
        tags: Vec<String>,       // "must be a JSON array of strings" falls out of the type
        source: Option<String>,
        scope: Option<String>,
        created_at: i64,
        last_accessed: i64,
        access_count: i64,
        expires_at: Option<i64>,
    },
    Link {
        from: i64,
        to: i64,
        kind: String,            // → LinkKind::try_from (D-09)
        created_at: i64,
    },
}
```

### Pattern 2: Two-step header parse (lenient probe, then strict)

**What:** Parse line 1 twice: first into a lenient `HeaderProbe { record: String, format: String, version: serde_json::Value }` **without** `deny_unknown_fields` (serde ignores unknown fields by default `[CITED: serde.rs/container-attrs.html]`), check `record == "header"`, `format == FORMAT_NAME`, `version` is an integer ≥ 1, then `version > FORMAT_VERSION` → the D-11 "upgrade" message; only then parse line 1 strictly as `Record::Header`.

**Why:** With a single strict parse, a future v2 header that adds a field fails first with `unknown field …` and the user never sees `file is format version 2 but this binary reads up to version 1 — upgrade agent-memory`. The probe is what makes D-11's version message reachable. `version: 0` passes a `u32` field, so the `< 1` rule is an explicit check either way.

**Example:**
```rust
#[derive(Deserialize)]
struct HeaderProbe { record: String, format: String, version: serde_json::Value }

fn check_header_line(line: &str) -> Result<(), MemoryError> {
    let probe: HeaderProbe = serde_json::from_str(line)
        .map_err(|e| invalid_line(1, format!("missing or invalid header ({e})")))?;
    if probe.record != "header" { return Err(invalid_line(1, "first record must be the header")); }
    if probe.format != FORMAT_NAME {
        return Err(invalid_line(1, format!("format must be '{FORMAT_NAME}' (got '{}')", probe.format)));
    }
    let version = probe.version.as_u64()
        .ok_or_else(|| invalid_line(1, format!("version must be a positive integer (got {})", probe.version)))?;
    if version < 1 { return Err(invalid_line(1, "version must be >= 1 (got 0)")); }
    if version > u64::from(FORMAT_VERSION) {
        return Err(invalid_line(1, format!(
            "file is format version {version} but this binary reads up to version {FORMAT_VERSION} — upgrade agent-memory")));
    }
    Ok(())
}

fn invalid_line(line_no: usize, msg: impl std::fmt::Display) -> MemoryError {
    // Display of InvalidArgument is "invalid argument: {0}" → "invalid argument: line 42: …" (D-11 shape)
    MemoryError::InvalidArgument(format!("line {line_no}: {msg}"))
}
```
`MemoryError::InvalidArgument`'s Display is `"invalid argument: {0}"` `[VERIFIED: crates/agent-memory-core/src/domain.rs:389-390]` — quote: `#[error("invalid argument: {0}")]` / `InvalidArgument(String),`.

### Pattern 3: Per-line deserialization with the importer's own 1-based counter

**What:** `for (idx, line) in text.lines().enumerate() { let line_no = idx + 1; … serde_json::from_str::<Record>(line) … }`. Every rejection is wrapped as `line {line_no}: {serde message}`; because serde's own `line()` is always `1` (per-line input) or `0` (in-variant data errors), **never surface `e.line()`**. Optionally strip serde's trailing ` at line 1 column N` suffix (present only on syntax/variant errors) or keep it — tests assert on the `line N:` prefix and the field name, not full equality (CONTEXT `<specifics>`).

**Pass-1 checks per line (D-09/D-11), in this order:** empty/whitespace-only or first non-ws byte ≠ `{` → "expected a JSON object"; `from_str::<Record>` → serde message; `Record::Header` after line 1 → "duplicate header"; `Memory`: `MemoryType::try_from(&mem_type)` (its `InvalidType` text re-wrapped under `InvalidArgument("line N: …")`), `content.trim().is_empty()` → "content must be a non-empty string" (the `validate_patch` wording `[VERIFIED: domain.rs:476-478]`), duplicate `key` → malformed (ASSUMED-reasonable extension of D-11: an ambiguous key map is a corrupted file; see A3); `Link`: `LinkKind::try_from(&kind)` (already `InvalidArgument` naming the allowed set `[VERIFIED: domain.rs:166-168]`), `validate_link_endpoints(from, to)` for self-links `[VERIFIED: domain.rs:490-497]`, then after the loop: every link `from`/`to` must be in the memory-key set → "link key 7 does not appear in the file". Memory records after the first link record are allowed by the parser but the exporter never writes them that way (D-05 ordering is an exporter guarantee; the importer only needs the key set complete before the links pass).

`serde_json::Error::line()` docs: "One-based line number at which the error was detected. Characters in the first line of the input (before the first newline character) are in line 1." `[CITED: docs.rs/serde_json error.rs via Context7 /websites/rs_serde_json]` — correct for syntax errors, but the probe shows `0` for data errors raised from buffered `Content`, hence the own counter.

### Pattern 4: Streaming export through a boxed writer on ONE read snapshot

**What:** `MemoryService::export(&self, out: Box<dyn Write + Send>, agent_memory_version: String) -> Result<ExportReport, MemoryError>` runs one `spawn_blocking` hop that calls `store.export_rows(&mut |record| { serde_json::to_writer(&mut out, &record)?; out.write_all(b"\n") })` and finally `out.flush()`. `Store::export_rows` takes **one** pooled connection, opens **one** read transaction (`conn.unchecked_transaction()` — in WAL mode a reader sees a consistent snapshot), reads the `meta` pin, then `SELECT … FROM memories ORDER BY id`, then `SELECT from_id, to_id, kind, created_at FROM memory_links ORDER BY from_id, to_id, kind`, invoking the visitor per row (rusqlite `query_map` steps rows lazily — nothing is buffered).

**Why one snapshot:** two statements on two pooled connections can straddle a concurrent `link` whose endpoint was inserted after the memories SELECT ran, producing a file whose link references a key absent from the file — which the strict importer then rejects (D-11). Inside one snapshot, the `memory_links` FK guarantees every endpoint is also exported. `[ASSUMED]` that `rusqlite::Connection::unchecked_transaction` is the right call on a pooled `&Connection` (the existing code uses `conn.transaction()` on the `&mut` writer `[VERIFIED: sqlite.rs:614,956]`); alternative: `execute_batch("BEGIN")`/`COMMIT`.

**Errors:** `serde_json::Error`/`io::Error` from the writer → `MemoryError::Internal(format!("export write failed: {e}"))` — crate-authored text, passes redaction unchanged; `Sqlite`/`Pool`/`Join` → `redact_storage_error("export", e)`.

**Sinks (main.rs):** stdout → `Box::new(BufWriter::new(std::io::stdout().lock()))` — `Stdout::lock` returns `StdoutLock<'static>` `[CITED: doc.rust-lang.org/std/io/struct.Stdout.html]`, so it is movable into the blocking task; `--output` → `Box::new(BufWriter::new(file.try_clone()?))` so `main.rs` keeps the original `File` for `sync_all()` and the rename after `Ok` (fsync is durability, not atomicity — optional; the backup helper does not fsync `[VERIFIED: sqlite.rs:284-291]`).

**Ordering:** memories ascending `id` (D-06); links by `(from_id, to_id, kind)` — D-05 fixes only "after all memory records"; a deterministic link order makes the export→import→export diff test and golden files stable (planner's call, recommended).

### Pattern 5: `ImportedMemory` + `Store::insert_imported` (recommended over extending `NewMemory`)

**Call-site count (the discretion criterion):** `NewMemory { … }` struct-literal constructions: **15** — `tests/hybrid.rs:73`, `relations.rs:64`, `backfill_race.rs:166`, `store.rs:30` and `:106`, `fallback.rs:19`, `decay.rs:29`, `validation.rs:26`, `semantic.rs:21`, `update.rs:105`, `ttl.rs:31`, `import.rs:110` (12 in core tests), `src/import/gsd_state.rs:178`, `crates/agent-memory/src/mcp.rs:285`, `crates/agent-memory/src/rest/handlers.rs:203` `[VERIFIED: grep -rn --include='*.rs' "NewMemory {" crates, signatures excluded]`. `NewMemory` has no `Default` (and `MemoryType` has no sensible default) `[VERIFIED: domain.rs:245-253]` — quote: `#[derive(Debug, Clone)] pub struct NewMemory { pub content: String, pub mem_type: MemoryType, pub tags: Vec<String>, pub source: Option<String>, pub scope: Option<String>, pub ttl_secs: Option<i64>, }` — so two new `Option` fields mean 15 edits. A dedicated type touches **0** existing sites. `Store` has exactly one implementor (`impl Store for SqliteStore`, `sqlite.rs:595`) and no test fakes `[VERIFIED: grep "impl Store for"]`, so a new trait method costs one impl.

**Shape:**
```rust
// domain.rs
/// A memory restored from a format-v1 file (INTEROP-02). Unlike NewMemory it carries absolute
/// timestamps: created_at/access_count/expires_at are preserved, last_accessed is the import-time
/// `now` (D-10), and the row ALWAYS lands embedding_status = 0 (D-13).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedMemory {
    pub content: String, pub mem_type: MemoryType, pub tags: Vec<String>,
    pub source: Option<String>, pub scope: Option<String>,
    pub created_at: i64, pub access_count: i64, pub expires_at: Option<i64>,
}
// store/mod.rs
fn insert_imported(&self, m: ImportedMemory, now: i64) -> Result<i64, MemoryError>;
```
`insert_imported` mirrors `insert`'s single writer transaction and its exact INSERT column list `[VERIFIED: sqlite.rs:615-631]` — quote of the current statement: `"INSERT INTO memories (mem_type, content, tags, source, scope, base_weight, decay_score, access_count, created_at, last_accessed, expires_at, embedding_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1.0, 0, ?7, ?7, ?8, ?9)"` — with `access_count = ?`, `created_at = ?`, `last_accessed = now`, `embedding_status = 0`, `base_weight` from `mem_type.is_pinned()` (2.0 / 1.0 `[VERIFIED: sqlite.rs:603]`), `decay_score = 1.0`. Refactor the body into one private `insert_row(tx, …)` helper so both inserts share one statement (avoids drift). The `memories_ai` FTS trigger fires on the insert `[CITED: sql/0001_init.sql]`, so imported rows are keyword-searchable immediately.

### Pattern 6: `find_by_key` replaces `exists` (D-07)

`Store::exists` has **one caller** (`service.rs:413` — `if store.exists(draft.source.as_deref(), draft.mem_type, &draft.content)?`) and **one impl** (`sqlite.rs:751-772`); no test calls it directly `[VERIFIED: grep "\.exists(" crates — the other hits are `Path::exists`]`. Change the signature to `fn find_by_key(&self, source: Option<&str>, mem_type: MemoryType, content: &str) -> Result<Option<i64>, MemoryError>` with the SQL `SELECT id FROM memories WHERE source IS ?1 AND mem_type = ?2 AND content = ?3 LIMIT 1` (keep `IS` — CR-01 NULL-safety `[VERIFIED: sqlite.rs:765-767]`), `.optional()?`; `import()` becomes `.is_some()`. Keep the trait doc's CR-01 paragraph. Within the JSONL memories pass, a second file record identical on the key (or identical to a row inserted earlier in the same pass — the per-row tx has committed) maps to the existing id and counts as `skipped_duplicates` (D-07's accepted collapse).

### Pattern 6b: `LinkOutcome::Linked { created: bool }` so `links_skipped_duplicates` is countable

Today `Store::link` runs `INSERT OR IGNORE` and returns `Ok(LinkOutcome::Linked)` whether or not a row was inserted (`changes() == 0 is still Linked` `[VERIFIED: sqlite.rs:974-989]`). D-12 requires `links_skipped_duplicates`, D-08 requires using `Store::link`, so the outcome must carry `created: tx.changes() == 1` (read `changes()` before `commit`). `LinkOutcome` is `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum LinkOutcome { Linked, NotFound { missing_id: i64 }, }` `[VERIFIED: domain.rs:176-183]`. Sites to update: producer `sqlite.rs:989`; transport matches `mcp.rs:438` and `handlers.rs:370` (become `LinkOutcome::Linked { .. } =>`, wire JSON unchanged); test asserts `tests/relations.rs:139,149,791,794` (become `Linked { created: true }` on first link and `Linked { created: false }` on the D-02 duplicate — a stronger idempotency proof) `[VERIFIED: grep "LinkOutcome::Linked" crates]`. Not a wire change: `LinkOutcome` carries no stored ids and transports echo request ids.

### Pattern 7: Atomic `--output` write with std only (binary crate)

```rust
// crates/agent-memory/src/export_file.rs (or beside create_data_dir in config.rs — same cfg split)
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// `{target}.export-tmp-{pid}` in the SAME directory, so the final rename never crosses a filesystem
/// (std::fs::rename "will not work if the new name is on a different mount point").
pub fn export_temp_path(target: &Path) -> PathBuf {
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("export");
    target.with_file_name(format!("{name}.export-tmp-{}", std::process::id()))
}

#[cfg(unix)]
pub fn create_private(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

#[cfg(not(unix))]
pub fn create_private(path: &Path) -> std::io::Result<File> {
    // Windows ACL inheritance is the Phase 6 seam (DIST-03); same split as config.rs::create_data_dir.
    OpenOptions::new().write(true).create_new(true).open(path)
}
// main.rs flow: temp = export_temp_path(&out); file = create_private(&temp)?; run export into
// BufWriter(file.try_clone()?); on Ok → file.sync_all()?; std::fs::rename(&temp, &out)?;
// on Err → let _ = std::fs::remove_file(&temp); propagate.
```
`OpenOptionsExt::mode`: "Sets the mode bits that a new file will be created with. If a new file is created as part of an `OpenOptions::open` call then this specified `mode` will be used as the permission bits for the new file. If no `mode` is set, the default of `0o666` will be used. The operating system masks out bits with the system's `umask`" `[CITED: doc.rust-lang.org/std/os/unix/fs/trait.OpenOptionsExt.html]` — umask can only remove bits, so the result is never looser than 0600. `std::fs::rename`: "Renames a file or directory to a new name, replacing the original file if `to` already exists" … "corresponds to the `rename` function on Unix, and `MoveFileExW` with a fallback to `SetFileInformationByHandle` on Windows" … "This will not work if the new name is on a different mount point" `[CITED: doc.rust-lang.org/std/fs/fn.rename.html]`. `create_new` makes a stale temp from a crashed run an explicit error rather than a silent truncate; the pid suffix makes that rare.

**Reuse verdict on the `sqlite.rs` backup helpers:** reuse the *shape*, not the code. `backup_temp_path` builds `{file_name}.backup-tmp-{LATEST_SCHEMA_VERSION:04}-{pid}` `[VERIFIED: sqlite.rs:140-145]`, `copy_into_temp` is the SQLite Online Backup API, `discard_backup_temp` removes SQLite sidecars, and all three are private `fn`s inside the core store `[VERIFIED: sqlite.rs:131-176]`. None applies to a plain text file written by the binary crate; the verify-then-rename + remove-temp-on-failure discipline (`sqlite.rs:265-291`) is what to mirror.

### Pattern 8: `redact_storage_error(op, e)` at the service seam (D-14)

**Current state** `[VERIFIED: crates/agent-memory-core/src/service.rs:33-60]` — quotes: `pub const SEARCH_STORAGE_FAILURE: &str = "search failed due to a storage error (details are in the server log)";` and the helper `fn redact_search_error(e: MemoryError) -> MemoryError { match e { MemoryError::InvalidType(_) | MemoryError::InvalidQuery(_) | MemoryError::InvalidArgument(_) => e, MemoryError::Internal(_) | MemoryError::SchemaTooNew { .. } => e, MemoryError::Sqlite(_) | MemoryError::Pool(_) | MemoryError::Join(_) | MemoryError::Migration(_) => { tracing::error!(error = %e, "search failed with a storage error"); MemoryError::Internal(SEARCH_STORAGE_FAILURE.to_string()) } } }`. It is applied in exactly one place: `search()` → `self.search_unredacted(args).await.map_err(redact_search_error)` `[VERIFIED: service.rs:299-303]`.

**`MemoryError` variants (complete)** `[VERIFIED: domain.rs:375-426]`: `InvalidType(String)`, `InvalidQuery(String)`, `InvalidArgument(String)`, `Sqlite(#[from] rusqlite::Error)`, `Pool(#[from] r2d2::Error)`, `Join(#[from] tokio::task::JoinError)`, `Migration(#[from] rusqlite_migration::Error)`, `Internal(String)`, `SchemaTooNew { db_version: i64, binary_version: i64 }`. D-14 adds none.

**Public `MemoryService` methods and today's error flow** `[VERIFIED: service.rs]`:

| Method | Lines | Error path today | D-14 op string |
|--------|-------|------------------|----------------|
| `store(new)` | 209-232 | `validate_ttl?` then `spawn_blocking(insert).await.map_err(Join)?` — raw | `"store"` |
| `list(args)` | 244-265 | `validate_limit?`, WR-04 check, `spawn_blocking(list)`, optional `expand_related` — raw | `"list"` |
| `search(args)` | 299-303 | **already redacted** via `search_unredacted` | `"search"` (text must stay byte-identical) |
| `import(drafts)` | 394-463 | `validate_ttl` per draft, two `spawn_blocking` hops — raw | `"import"` |
| `forget(id)` | 573-578 | `spawn_blocking(forget)` — raw | `"forget"` |
| `link(from,to,kind)` | 592-604 | `validate_link_endpoints?`, `spawn_blocking(link)` — raw | `"link"` |
| `unlink(from,to,kind)` | 610-620 | raw | `"unlink"` |
| `update(id, patch)` | 634-666 | `validate_patch?`, embed, `spawn_blocking(update)` — raw | `"update"` |
| `sweep(now)` | 688-744 | three `spawn_blocking` hops — raw (callers only log) | `"sweep"` |
| `export(...)` / `import_jsonl(...)` | new | — | `"export"` / `"import"` |

**Shape:** keep the body of each method in a private `*_unredacted` fn (the `search` precedent) and make the public fn `self.x_unredacted(..).await.map_err(|e| redact_storage_error("x", e))`. Methods have multiple `?` exits, so a tail `.map_err` on the last expression would miss early `Join` returns — the inner-fn shape is the safe one. Message: `format!("{op} failed due to a storage error (details are in the server log)")`; keep `pub const SEARCH_STORAGE_FAILURE` and add a unit test asserting `redact_storage_error("search", MemoryError::Internal-free storage variant).to_string() == format!("internal error: {SEARCH_STORAGE_FAILURE}")` so the constant and the template can never drift. Log line: `tracing::error!(op, error = %e, "storage error")`.

**Existing assertions that must keep passing** `[VERIFIED: grep]`: `tests/hybrid.rs:729` and `:760` — `matches!(err, MemoryError::Internal(ref m) if m == SEARCH_STORAGE_FAILURE)` plus `!err.to_string().to_lowercase().contains("sqlite")`; `rest/handlers.rs:881` — `error.contains("search failed due to a storage error")`; `mcp.rs:1336` — `err.message.contains("search failed due to a storage error")`; `tests/rest.rs:219`, `tests/tools.rs:328`, `handlers.rs:767,827`, `mcp.rs:1206,1286` — `!…contains("sqlite error")` on search responses; `handlers.rs:1794` — `Internal("writer lock poisoned")` passes through the REST mapper (still true: `Internal` passes through the new seam unchanged, and the mapper is untouched). The transport mappers `map_mcp_error` `[CITED: mcp.rs:35-50]` and `map_memory_error` `[CITED: rest/handlers.rs:167-182]` match every variant explicitly and stay as they are.

**Fault injection recipe (in-process, deterministic)** — the T-04-19 pattern `[CITED: tests/hybrid.rs:712-735; handlers.rs:851-889; mcp.rs:1309-1344]`: open a raw side connection (`register_vec_extension()` first, then `rusqlite::Connection::open(&db_path)`), run `execute_batch("ALTER TABLE memories RENAME TO memories_offline;")`, then call the service/handler. Effects: `store` → INSERT fails `no such table: memories` → `Sqlite` → redacted `store failed …`; `/health` → `service.list(limit 1)` `[CITED: handlers.rs:437-445]` fails the same way → 500 `{"error": "internal error: list failed due to a storage error (…)"}` with no `sqlite` substring; `link`/`unlink` → rename `memory_links` instead. Renaming `memories` is safe for the trigger set (modern SQLite rewrites trigger references on RENAME) and already-open pooled connections re-prepare on `SQLITE_SCHEMA` — the hybrid tests prove the same store instance keeps working after a rename. **Do not use DROP** (the repo hook blocks it in shell text; RENAME is the precedent). `chmod`/read-only tricks are unreliable because the writer connection already holds an open fd.

### Anti-Patterns to Avoid
- **Surfacing `serde_json::Error::line()` in messages:** it is `1` for every per-line parse and `0` for in-variant data errors (probe). Use the importer's counter.
- **Strict header parse before the version check:** a v2 header fails on `unknown field` and the "upgrade agent-memory" message is unreachable (Pattern 2).
- **Two export SELECTs on two pooled connections:** a concurrent `link` can produce a dangling key that the strict importer rejects (Pattern 4).
- **Deserializing `type`/`kind` straight into `MemoryType`/`LinkKind`:** D-09 locks `try_from`; serde's `unknown variant` text would replace the established `kind must be one of relates_to, supersedes, caused_by (got '…')` message.
- **Writing `INSERT INTO memories (id, …)`** with the file's `key` — PITFALLS 18; `key` is a map key only.
- **Importing links before the memories pass completes** — endpoints that deduped to existing rows must map to the existing id (PITFALLS 18 interplay).
- **Calling the embedder anywhere in `import_jsonl`** — D-13; `insert_imported` hard-codes `embedding_status = 0`.
- **Applying `validate_ttl` to `expires_at`** — it is absolute (D-09); the only lifecycle rule is `expires_at <= now` → skip.
- **A tail `.map_err(redact…)` after a `?`-laden body** — early returns escape unredacted; use the `*_unredacted` inner-fn shape.
- **`unwrap()` / `expect()` in `main.rs`/core production paths** — CLAUDE.md; use `?` with `anyhow::Context` in the binary.
- **Buffering the corpus** (`Vec<Record>` then `to_string`) on export — D-02; stream per row. (Holding pass-1 records in memory on import is accepted by D-08's validate-before-write.)

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Tagged-record JSON encoding/decoding | A manual `serde_json::Value` walker that reads `record` and dispatches | `#[serde(tag = "record", deny_unknown_fields)]` enum | Probe-verified messages for unknown variant/field/type; one definition for both directions |
| Unknown-field rejection | Field-name whitelists | `deny_unknown_fields` (04-07 precedent) | Already the project's pattern for new v0.1.0 DTOs |
| JSON array-of-strings validation for `tags` | Manual `Value::Array` checks | `tags: Vec<String>` | serde enforces element types; the store already serializes tags with `serde_json::to_string(&new.tags)` `[VERIFIED: sqlite.rs:602]` |
| Edge idempotency | A pre-check SELECT per link | Existing `Store::link` `INSERT OR IGNORE` on PK `(from_id, to_id, kind)` + `changes()` | PK already enforces it `[VERIFIED: sql/0003_relations.sql:19]`; `relates_to` canonicalisation happens inside `link` `[VERIFIED: sqlite.rs:982]` |
| Dedup identity | A UUID/hash column (migration 0004) | `find_by_key` on `(source, mem_type, content)` | D-07 locked; deferred idea |
| Re-embedding on import | An inline batch embed | Existing sweep backfill (`BACKFILL_BATCH = 128` `[VERIFIED: service.rs:749]`, guarded `insert_embedding`) | D-13; CR-03 race safety already proven in `tests/backfill_race.rs` |
| Atomic replace | Write-then-copy or truncate-in-place | temp in same dir + `std::fs::rename` | POSIX rename atomicity; mirrors the backup helper |
| Private file creation | `set_permissions` after create | `OpenOptions::mode(0o600)` at creation | No window where the file is 0644 |
| Clock for `exported_at`/import `now` | `SystemTime::now()` in core | Injected `Clock` (`self.clock.now()`) | Deterministic tests with `TestClock` (`clock.rs:29-44`) |

**Key insight:** every hard part of this phase (identity, idempotent edges, re-embedding, snapshot reads, the error taxonomy) already has a tested mechanism in the repo; the phase is wiring a format onto them, and every "shortcut" that bypasses one of those mechanisms re-opens a closed review finding (CR-01, CR-03, WR-03, T-04-13, T-04-19).

## Common Pitfalls

### Pitfall 1: Line numbers vanish inside tagged variants
**What goes wrong:** `invalid argument: line 0: unknown field 'bogus'` — or no line at all.
**Why it happens:** Internally tagged enums buffer the object into serde `Content`; the position is lost for data errors (probe-verified: `line 0 col 0`).
**How to avoid:** Own 1-based counter from `text.lines().enumerate()`; format `line {n}: {e}`; never print `e.line()`.
**Warning signs:** A test expecting `line 3:` seeing `line 1:`/`line 0:`.

### Pitfall 2: The "upgrade agent-memory" message is unreachable
**What goes wrong:** A v2 file with a new header field reports `unknown field …` instead of D-11's version message.
**Why it happens:** `deny_unknown_fields` fires before any version comparison.
**How to avoid:** Pattern 2's lenient probe first; a test that feeds `{"record":"header","format":"agent-memory-export","version":2,"new_field":1}` and asserts the upgrade text.
**Warning signs:** Only `version: 2` without extra fields is tested.

### Pitfall 3: `links_skipped_duplicates` always 0 (or always equals `links_imported`)
**What goes wrong:** Re-importing the same file reports every link as imported again, violating criterion 2's "idempotent" report.
**Why it happens:** `LinkOutcome::Linked` hides whether `INSERT OR IGNORE` inserted.
**How to avoid:** Pattern 6b (`created: bool` from `tx.changes()` before commit); test: second import → `links_imported == 0`, `links_skipped_duplicates == N`, `SELECT count(*) FROM memory_links` unchanged (raw side connection, `relations.rs::edge_count` shape).

### Pitfall 4: Phantom edges in a non-empty target DB
**What goes wrong:** Link `(from: 3, to: 7)` attaches to whatever rows 3 and 7 are in the target.
**Why it happens:** Treating `key` as an id, or running the links pass with a stale/incomplete map.
**How to avoid:** Keys only ever index the `HashMap<i64, i64>` built in pass 2; pass 3 looks up both ends and drops (`links_dropped`) when either is missing (expired endpoint); kill-test per CONTEXT `<specifics>`: pre-seed the target with rows whose ids overlap the file's keys, import, assert `edges_touching(pre_existing_id) == 0`.

### Pitfall 5: Dangling-key export from a split read
**What goes wrong:** The exporter's own output fails the importer with `link key N does not appear in the file`.
**Why it happens:** Memories and links SELECTed on different connections while another process writes.
**How to avoid:** Pattern 4 — one connection, one read transaction, memories then links. Test is hard to make deterministic; a code-review check plus the FK argument suffices (document in the trait doc).

### Pitfall 6: Expired rows imported "successfully" then swept
**What goes wrong:** Report says `imported: 3`, an hour later `memory_list` shows 2 (PITFALLS 19a).
**Why it happens:** Preserving `expires_at` without the import-time check.
**How to avoid:** `expires_at.is_some_and(|t| t <= now)` → `skipped_expired += 1`, no insert, no map entry. Note the sweep's own predicate is `expires_at < now` (strict) `[VERIFIED: store/mod.rs:219]`; D-10's `<=` is the locked import rule — document both in the format spec so the boundary case is deliberate.

### Pitfall 7: Export file briefly world-readable or left half-written
**What goes wrong:** A 0644 file for a moment, or a truncated `PATH` after a crash.
**Why it happens:** `File::create` (0666 & umask) then chmod; or writing directly to `PATH`.
**How to avoid:** Pattern 7: `create_new` + `mode(0o600)` on the temp, rename after flush; remove the temp on any error; test with `std::os::unix::fs::PermissionsExt::mode() & 0o777 == 0o600` under `#[cfg(unix)]`, and a test that an existing `PATH` is replaced.

### Pitfall 8: `BufWriter` dropped without flush / stdout EPIPE
**What goes wrong:** Last records missing from the file; `export | head` panics or errors noisily.
**Why it happens:** `BufWriter` flushes on drop but swallows errors; a closed pipe returns `BrokenPipe`.
**How to avoid:** Explicit `out.flush()` inside the blocking task before returning `Ok`; map `io::Error` → `MemoryError::Internal("export write failed: …")`; in `main.rs`, treat `ErrorKind::BrokenPipe` as a quiet non-zero exit (ASSUMED-nice-to-have, A6).

### Pitfall 9: `CARGO_PKG_VERSION` ambiguity
**What goes wrong:** The header says `0.0.1` from the core crate while the binary is `0.1.0` (or vice versa) after a partial bump.
**Why it happens:** Two crates, two `version =` lines (`crates/agent-memory-core/Cargo.toml:3`, `crates/agent-memory/Cargo.toml:3`, both `0.0.1` `[VERIFIED]`); `env!` resolves to whichever crate compiles it.
**How to avoid:** Pass the version string from `main.rs` (`env!("CARGO_PKG_VERSION")` of the binary) into `MemoryService::export`; tests pass a literal. Phase 6's release checklist bumps both crates together.

### Pitfall 10: `deny_unknown_fields` strictness vs `Option` leniency
**What goes wrong:** A hand-edited file omitting `source` parses (as `None`) while the same file with `"sourc":null` is rejected — surprising but consistent.
**Why it happens:** serde treats missing `Option<T>` as `None` (probe); `deny_unknown_fields` only governs extra keys.
**How to avoid:** Accept it (document: "optional fields may be omitted; the exporter always writes them"), or add `#[serde(deserialize_with = "…")]`/a required marker — not worth it. Golden tests pin the exporter's canonical output.

### Pitfall 11: Report arithmetic drift
**What goes wrong:** `imported + skipped_duplicates + skipped_expired != memory records` or `links_* != link records`.
**Why it happens:** Counting in two places or skipping a branch.
**How to avoid:** Count in exactly one match per pass; test asserts the sums for every fixture, including the gsd-state path's zeros (`import()` must set the four new fields to 0 — `ImportReport` is constructed only at `service.rs:425` and `:459` `[VERIFIED: grep "ImportReport {"]`, so no test literal breaks when fields are added).

### Pitfall 12: Coverage gate regression from CLI-only tests
**What goes wrong:** `cargo llvm-cov --fail-under-lines 80` drops because new `main.rs` glue and `export_file.rs` are exercised only by spawned-binary tests (zero profile data).
**Why it happens:** `handlers.rs:456-460` explains the SIGKILL/profile issue.
**How to avoid:** Keep `main.rs` arms thin; unit-test `export_temp_path`/`create_private` in the binary crate (plain `#[test]` with tempdir); put all format logic in core tests.

## Code Examples

### 1. Golden field-order test for a memory record (D-04)
```rust
// crates/agent-memory-core/src/export.rs  #[cfg(test)]
#[test]
fn memory_record_serializes_in_the_documented_order() {
    let rec = Record::Memory { key: 7, mem_type: "DECISION".into(), content: "c".into(),
        tags: vec!["a".into()], source: None, scope: Some("s".into()),
        created_at: 1, last_accessed: 2, access_count: 3, expires_at: None };
    assert_eq!(serde_json::to_string(&rec).expect("serialize"),
        r#"{"record":"memory","key":7,"type":"DECISION","content":"c","tags":["a"],"source":null,"scope":"s","created_at":1,"last_accessed":2,"access_count":3,"expires_at":null}"#);
}
// Probe-verified: tag first, then declaration order; Option::None → null.
```

### 2. Malformed-file table test shape (D-11)
```rust
// crates/agent-memory-core/tests/export_import.rs
const HEADER: &str = r#"{"record":"header","format":"agent-memory-export","version":1,"exported_at":1000,"embedding_model":"nomic-embed-text","embedding_dim":768,"schema_version":3,"agent_memory_version":"0.0.1"}"#;
const MEM1: &str = r#"{"record":"memory","key":1,"type":"DECISION","content":"a","tags":[],"source":null,"scope":null,"created_at":1,"last_accessed":1,"access_count":0,"expires_at":null}"#;

#[test]
fn malformed_files_are_rejected_naming_the_line() {
    let cases: &[(&str, &str, &str)] = &[
        ("no header",        &format!("{MEM1}\n"),                                              "line 1:"),
        ("bad format",       "{\"record\":\"header\",\"format\":\"x\",\"version\":1}\n",         "line 1: format"),
        ("version 0",        "{\"record\":\"header\",\"format\":\"agent-memory-export\",\"version\":0}\n", "line 1:"),
        ("version 2 + field","{\"record\":\"header\",\"format\":\"agent-memory-export\",\"version\":2,\"later\":1}\n", "upgrade agent-memory"),
        ("not an object",    &format!("{HEADER}\n[1,2]\n"),                                     "line 2: expected a JSON object"),
        ("unknown record",   &format!("{HEADER}\n{{\"record\":\"memo\",\"key\":1}}\n"),         "line 2:"),
        ("unknown field",    &format!("{HEADER}\n{}\n", MEM1.replace("\"expires_at\":null", "\"expires_at\":null,\"bogus\":1")), "line 2: unknown field"),
        ("bad type",         &format!("{HEADER}\n{}\n", MEM1.replace("\"key\":1", "\"key\":\"1\"")), "line 2:"),
        ("bad kind",         &format!("{HEADER}\n{MEM1}\n{{\"record\":\"link\",\"from\":1,\"to\":1,\"kind\":\"loves\",\"created_at\":1}}\n"), "kind must be one of"),
        ("self link",        &format!("{HEADER}\n{MEM1}\n{{\"record\":\"link\",\"from\":1,\"to\":1,\"kind\":\"relates_to\",\"created_at\":1}}\n"), "must differ"),
        ("dangling key",     &format!("{HEADER}\n{MEM1}\n{{\"record\":\"link\",\"from\":1,\"to\":9,\"kind\":\"relates_to\",\"created_at\":1}}\n"), "does not appear"),
    ];
    for (name, text, needle) in cases {
        let err = parse_jsonl(text).expect_err(name);
        assert!(matches!(err, MemoryError::InvalidArgument(_)), "{name}: {err:?}");
        assert!(err.to_string().contains(needle), "{name}: {err}");
    }
}
```

### 3. Non-empty-DB overlapping-rowid kill-test (PITFALLS 18) — shape
```rust
#[tokio::test]
async fn importing_into_a_non_empty_db_never_attaches_phantom_edges() {
    // Source: 3 memories (ids 1..=3) + link 1→2. Target: pre-seed 3 unrelated memories so ids 1..=3 are taken.
    let src = harness(); /* store a,b,c; link(a,b,RelatesTo) */ let file = export_to_string(&src).await;
    let dst = harness(); let pre: Vec<i64> = /* store x,y,z */;
    let report = dst.service.import_jsonl(parse_jsonl(&file).expect("valid")).await.expect("import");
    assert_eq!((report.imported, report.links_imported), (3, 1));
    for id in pre { assert_eq!(edges_touching(&dst.db_path, id), 0, "pre-existing row {id} gained an edge"); }
    // Then: export dst, import again → imported 0, skipped_duplicates 3, links_imported 0, links_skipped_duplicates 1.
}
```
(`edges_touching` is the raw-connection helper already in `tests/relations.rs:95-104` `[CITED]`; copy it or lift it into a shared test helper.)

### 4. Export sink selection in `main.rs`
```rust
Command::Export { output } => {
    let (sink, finish): (Box<dyn Write + Send>, Box<dyn FnOnce() -> anyhow::Result<()>>) = match output {
        None => (Box::new(BufWriter::new(std::io::stdout().lock())), Box::new(|| Ok(()))),
        Some(path) => {
            let temp = export_file::export_temp_path(&path);
            let file = export_file::create_private(&temp)
                .with_context(|| format!("creating {}", temp.display()))?;
            let writer = BufWriter::new(file.try_clone().context("duplicating export handle")?);
            (Box::new(writer), Box::new(move || { file.sync_all()?; std::fs::rename(&temp, &path)?; Ok(()) }))
        }
    };
    let report = service.export(sink, env!("CARGO_PKG_VERSION").to_string()).await.context("exporting")?;
    finish()?;                               // on Err above: remove_file(temp) best-effort before returning
    eprintln!("exported {} memories and {} links", report.memories, report.links);  // stderr: stdout may be the file
}
```

### 5. `meta` pin read for the header (D-03)
```rust
// store/mod.rs
/// The 0002 embedding pin: `(embedding_model, embedding_dim)` from `meta`. A missing or non-integer
/// value is a crate-authored `Internal` (never a client error) — the table is written only by 0002.
fn embedding_pin(&self) -> Result<(String, u32), MemoryError>;
// sqlite.rs
let model: String = conn.query_row("SELECT value FROM meta WHERE key = 'embedding_model'", [], |r| r.get(0))?;
let dim_text: String = conn.query_row("SELECT value FROM meta WHERE key = 'embedding_dim'", [], |r| r.get(0))?;
let dim = dim_text.parse::<u32>()
    .map_err(|e| MemoryError::Internal(format!("meta.embedding_dim is not an integer ({dim_text:?}): {e}")))?;
```
`meta` is `(key TEXT PRIMARY KEY, value TEXT NOT NULL)` seeded with `('embedding_model', 'nomic-embed-text'), ('embedding_dim', '768')` `[VERIFIED: crates/agent-memory-core/sql/0002_embeddings.sql:21-26]` — values are TEXT, hence the parse. `schema_version` in the header is `LATEST_SCHEMA_VERSION` (3).

### 6. Pending → sweep → searchable proof (criterion 3)
```rust
#[tokio::test]
async fn imported_rows_land_pending_and_the_sweep_backfills_them() {
    let h = harness_with(Arc::new(FakeEmbedder::with_vectors(HashMap::new()))); // embedder UP, still never called by import
    h.service.import_jsonl(parsed).await.expect("import");
    assert_eq!(status(&h, id), 0);                       // raw: SELECT embedding_status (backfill_race.rs:205 shape)
    assert_eq!(vec_rows(&h, id), 0);                     // no vector travelled or was written inline
    h.service.sweep(h.clock.now()).await.expect("sweep"); // BACKFILL_BATCH = 128 per tick
    assert_eq!(status(&h, id), 1);
    let out = h.service.search(SearchArgs { query: "…".into(), ..Default::default() }).await.expect("search");
    assert_eq!(out.search_mode, SearchMode::Hybrid);
}
```

### 7. D-14 redaction test for a write path and `/health`
```rust
// core: tests/redaction.rs
#[tokio::test]
async fn a_storage_fault_on_store_is_redacted() {
    let h = harness();
    raw_conn(&h.db_path).execute_batch("ALTER TABLE memories RENAME TO memories_offline;").expect("rename");
    let err = h.service.store(new_memory("x")).await.expect_err("insert must fail");
    assert!(matches!(err, MemoryError::Internal(ref m) if m == "store failed due to a storage error (details are in the server log)"), "{err:?}");
    assert!(!err.to_string().to_lowercase().contains("sqlite"));
}
// binary: rest/handlers.rs tests — health_handler(State(test_state(&dir))) after the same rename → 500, body["error"] has no "sqlite"
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `redact_search_error` on `search` only | `redact_storage_error(op, e)` on every public service method | This phase (D-14, closes 04-REVIEW IN-01) | `/health`, write paths and the CLI never leak `sqlite error: …`; README sentence widened |
| `Store::exists -> bool` | `Store::find_by_key -> Option<i64>` | This phase (D-07) | Dedup hits map export keys to existing ids |
| `LinkOutcome::Linked` (unit) | `LinkOutcome::Linked { created: bool }` | This phase (D-12 countability) | Transport JSON unchanged; D-02 idempotency test becomes stronger |
| Import = `gsd-state` only, tolerant | `--from jsonl` strict sibling | This phase (D-11) | Two importers with opposite tolerance philosophies, both documented |
| Backup = same-machine `.backup-pre-*` copy | Portable JSONL export (vectors excluded) | This phase (DIST-04) | README should contrast "backup" (`VACUUM INTO`/backup file) vs "portable export" (CONTEXT discretion) |

**Deprecated/outdated:** none in scope. The 04-REVIEW IN-01 suggested fix location ("move the redaction into the two transport mappers") is superseded by D-14's service-seam placement (one change covers the CLI too).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `rusqlite::Connection::unchecked_transaction()` (or an explicit `BEGIN`) on a pooled read connection gives the single WAL snapshot Pattern 4 needs | Pattern 4 | Low — fallback is `execute_batch("BEGIN")`/`COMMIT` or a single JOIN-free statement pair on one connection without a tx (WAL readers are still snapshot-consistent per statement; the risk is only the straddle) |
| A2 | Modern bundled SQLite rewrites trigger bodies on `ALTER TABLE memories RENAME`, so the fault-injection rename does not itself error | Pattern 8 recipe | Low — the hybrid tests already rename `memories_fts`/`vec_memories`; if `memories` RENAME misbehaves, rename `memory_links` for link paths and use `PRAGMA writable_schema`-free alternatives such as renaming `memories_fts` for `/health` via a search-based probe |
| A3 | Rejecting a duplicate `key` within one file as malformed is within D-11's "strict" intent (not an enumerated case) | Pattern 3 | Low — alternative is last-wins; the exporter never produces duplicates (ids are unique) |
| A4 | Empty/whitespace-only lines are "not a JSON object" and therefore malformed (D-11), including a blank trailing line | Pattern 3 | Low — if the user prefers tolerance of blank lines, it is one `continue`; the exporter never emits them |
| A5 | Deterministic link ordering `(from_id, to_id, kind)` is acceptable under D-05 ("after all memory records" is the only locked ordering) | Pattern 4 | None — ordering is a planner discretion |
| A6 | Treating `BrokenPipe` on stdout as a quiet non-zero exit is desirable | Pitfall 8 | Cosmetic |
| A7 | Passing `agent_memory_version` from the binary (not `env!` in core) matches D-03's intent of "<CARGO_PKG_VERSION>" | Pattern 4 / Pitfall 9 | None functionally while both crates are `0.0.1`; matters at the Phase 6 bump |
| A8 | `access_count` needs no extra bounds check (any `i64` accepted) | Pattern 3 | Negligible — provenance-only field (D-10) |

## Open Questions

1. **Header `exported_rows`/`exported_links` counts (CONTEXT discretion)**
   - What we know: additive only if decided now (D-11 rejects unknown fields later); the exporter knows the counts only after streaming, but the header is line 1.
   - What's unclear: whether a pre-count query (`SELECT count(*)`) inside the same read snapshot is worth two extra statements.
   - Recommendation: include them — two `count(*)` on the same snapshot are cheap, and the importer can then reject a truncated file (`line N: file ends after 12 of 40 declared memory records`) — a real integrity check for a copied file. If omitted, it is gone for v1.

2. **Where the clock-skew warning lives**
   - What we know: D-10 says "the CLI prints … to stderr"; core has `tracing::warn!` (stderr in the binary) and the injected clock.
   - Recommendation: emit it in `import_jsonl` via `tracing::warn!` (testable with the `TestClock`), which reaches stderr through the binary's subscriber — the CLI needs no extra code.

3. **`LinkOutcome` change scope**
   - What we know: 7 sites; not a wire change.
   - What's unclear: whether the user considers `LinkOutcome` part of the Phase 4 "closed" contract.
   - Recommendation: proceed (it is `pub` in core, not on the wire); mention in the plan's summary.

4. **`SqliteStore::open` errors on the CLI** still print raw `sqlite error:` text via anyhow (e.g. unreadable DB path). D-14 scopes redaction to `MemoryService` methods; `open` is outside. Recommendation: leave as is (local stderr, no transport), note in the README sentence as "every service response".

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| rustc / cargo | build + tests | ✓ | 1.94.1 (2026-03-25) `[VERIFIED]` | — |
| serde / serde_json (locked) | format v1 | ✓ | 1.0.228 / 1.0.150 `[VERIFIED: Cargo.lock]` | — |
| cargo-llvm-cov | coverage gate (CI + local) | ✓ | 0.8.7 `[VERIFIED]` | CI installs it with `cargo install cargo-llvm-cov --locked` |
| jq | `.claude/hooks/pre-bash-rust.sh` | ✓ | 1.6 | — |
| Ollama | none — D-13 forbids inline embedding; tests use `FakeEmbedder` | not required | — | — |
| Node/gsd-tools | planning tooling only | ✓ | — | — |

**Missing dependencies with no fallback:** none.
**Missing dependencies with fallback:** none.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) + `#[tokio::test]`; `FakeEmbedder`/`TestClock` via the `test-clock` feature self-dev-dependency `[VERIFIED: crates/agent-memory-core/Cargo.toml dev-dependencies]` |
| Config file | none — workspace `Cargo.toml` |
| Quick run command | `cargo test -p agent-memory-core --test export_import --test import --test redaction` |
| Full suite command | `cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` |
| Coverage gate | `cargo llvm-cov --workspace --fail-under-lines 80` (CI-enforced `[VERIFIED: .github/workflows/ci.yml:44]`) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DIST-04 | Header line carries format/version/`meta` model+dim/schema_version/agent_memory_version; `exported_at` from the injected clock | unit (core, in-process) | `cargo test -p agent-memory-core --test export_import header_` | ❌ Wave 0 |
| DIST-04 | Memory/link record golden field order (D-04/D-05); no `decay_score`/`base_weight`/`embedding_status`/vector fields | unit (core, `export.rs` `#[cfg(test)]`) | `cargo test -p agent-memory-core --lib export::` | ❌ Wave 0 |
| DIST-04 | Every row exported, ascending id; links after memories; `relates_to` endpoints as stored `(min,max)` | unit (core) | `cargo test -p agent-memory-core --test export_import export_` | ❌ Wave 0 |
| DIST-04 | `--output`: temp in same dir, 0600 on Unix, existing target replaced, temp removed on failure; stdout default | unit (binary crate, `#[cfg(unix)]` perms) + realism CLI | `cargo test -p agent-memory --bin agent-memory export_file::` ; `cargo test -p agent-memory --test cli` | ❌ Wave 0 |
| INTEROP-02 | Round trip into an EMPTY and a NON-EMPTY overlapping-rowid DB: counts, key→id remap, zero phantom edges; export→import→export diff modulo `key`/`last_accessed` | integration (core, real temp DB) | `cargo test -p agent-memory-core --test export_import round_trip_` | ❌ Wave 0 |
| INTEROP-02 | Idempotent re-import: `imported 0`, `skipped_duplicates N`, `links_imported 0`, `links_skipped_duplicates M`, edge count unchanged | integration (core) | `cargo test -p agent-memory-core --test export_import idempotent_` | ❌ Wave 0 |
| INTEROP-02 | Expiry fixture: one expired (skipped, counted, never inserted), one ancient (listable, low decay, `last_accessed` refreshed), one fresh; link to expired endpoint → `links_dropped` | integration (core, `TestClock`) | `cargo test -p agent-memory-core --test export_import expired_` | ❌ Wave 0 |
| INTEROP-02 | Imported rows `embedding_status = 0`, no vec rows; `sweep` backfills; then hybrid-searchable | integration (core) | `cargo test -p agent-memory-core --test export_import pending_` | ❌ Wave 0 |
| INTEROP-02 | Malformed-file table (header/format/version 0/version 2+field/non-object/unknown record/unknown field/bad type/bad kind/empty content/self-link/dangling key/duplicate key) → `InvalidArgument` with `line N:`; nothing written (row count unchanged) | unit (core, `jsonl.rs` + `export_import.rs`) | `cargo test -p agent-memory-core --test export_import malformed_` | ❌ Wave 0 |
| INTEROP-02 | gsd-state path unchanged and reports zeros for the four new fields | existing + 1 assertion | `cargo test -p agent-memory-core --test import` | ✅ `tests/import.rs` (extend) |
| INTEROP-02 | CLI: `import --from jsonl` prints all six counts + pending line; `--from bogus` still bails; exit non-zero on malformed file | realism (spawned binary) | `cargo test -p agent-memory --test cli` | ❌ Wave 0 |
| D-14 | `redact_storage_error("search", storage)` text == `SEARCH_STORAGE_FAILURE` (byte-identical); client/Internal/SchemaTooNew pass through | unit (core, `service.rs` `#[cfg(test)]`) | `cargo test -p agent-memory-core --lib redact_` | ❌ Wave 0 |
| D-14 | A write path (`store`) and `list` on a renamed `memories` table → `Internal("<op> failed …")`, no `sqlite` substring | integration (core) | `cargo test -p agent-memory-core --test redaction` | ❌ Wave 0 |
| D-14 | REST `/health` and `POST /api/memories` → 500 JSON `{"error"}` without `sqlite`; MCP `memory_store` → `internal_error` without `sqlite` | in-process handler/MCP unit tests | `cargo test -p agent-memory --bin agent-memory storage_failure` | partial (extend `handlers.rs`/`mcp.rs` test modules; T-04-19 search tests exist) |
| D-14 | Existing Phase 4 golden assertions still green (`hybrid.rs:729,760`, `handlers.rs:881`, `mcp.rs:1336`) | existing | `cargo test -p agent-memory-core --test hybrid && cargo test -p agent-memory --bin agent-memory` | ✅ |

### Sampling Rate
- **Per task commit:** `cargo test -p agent-memory-core --test export_import --test import --test redaction` (< 30 s) — plus `cargo clippy -p agent-memory-core --all-targets -- -D warnings`
- **Per wave merge:** `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --check`
- **Phase gate:** full suite + `cargo llvm-cov --workspace --fail-under-lines 80` green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/agent-memory-core/tests/export_import.rs` — covers DIST-04 + INTEROP-02 (round trip, kill-tests, expiry fixture, idempotency, malformed table, pending→sweep); harness copied from `tests/relations.rs:33-61` (`Harness { service, clock, db_path, _dir }`) with `raw_conn`/`edge_count`/`edges_touching` helpers
- [ ] `crates/agent-memory-core/tests/redaction.rs` — covers D-14 on `store` and `list` (RENAME recipe)
- [ ] `crates/agent-memory-core/src/export.rs` `#[cfg(test)]` — golden serialization + `deny_unknown_fields` probes (the 11 probe cases from this research as the table)
- [ ] `crates/agent-memory/tests/cli.rs` — realism-only spawned-binary round trip (`CARGO_BIN_EXE_agent-memory`, `--db` on a tempdir — the `tests/tools.rs:12-13` pattern)
- [ ] In-process `/health` + `store` redaction tests appended to `rest/handlers.rs` and `mcp.rs` test modules (`test_state`/`test_server` harnesses already exist)
- [ ] Framework install: none — cargo test is built-in

*Task-level TDD (Phase 3/4 precedent): each new test file lands RED inside the owning task before its implementation.*

## Security Domain

### Applicable ASVS Categories (Level 1)

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | CLI-only surfaces on the user's own machine (D-01); no new network endpoint |
| V3 Session Management | no | — |
| V4 Access Control | yes | Export file `0600` at creation (`OpenOptionsExt::mode`), temp in the DB's own directory (never `/tmp`), never looser than the DB (T-01-03 twin); import path accepted only from the CLI argument — no REST/MCP file-path primitive (PITFALLS security row) |
| V5 Input Validation | yes | serde typed records + `deny_unknown_fields`; `MemoryType::try_from` / `LinkKind::try_from`; `validate_link_endpoints`; whole-file validate-before-write; all SQL parameterised (`params![]`) — `kind`/`type` never reach SQL unparsed |
| V6 Cryptography | no | none — no hashing/signing in scope (checksums are a Phase 6 release concern) |
| V7 Error Handling & Logging | yes | D-14: storage-engine text only in the stderr log at `error` level; clients see `"<op> failed due to a storage error (details are in the server log)"`; client errors echo only client input and public bounds |
| V12 Files & Resources | yes | `create_new` (no clobbering a stranger's file through a temp-name race), same-directory rename (no cross-FS partial copy), `read_to_string` only on the user-supplied path; file content is stored strictly as data (import/mod.rs invariant) |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| World-readable export of a sensitive corpus | Information Disclosure | `mode(0o600)` at creation; README documents Windows ACL inheritance as Phase 6 |
| Hand-edited file injecting junk `type`/`kind` into SQL | Tampering | closed enums via `try_from`; parameterised INSERTs |
| Rowid collision / phantom edges in the target DB | Tampering (data integrity) | export-local keys + two-pass map; kill-test |
| Model-mismatched vectors poisoning `vec_memories` | Tampering | no vectors in the file; re-embed via sweep against the local pin |
| Raw SQLite text (table/column names) to clients | Information Disclosure | D-14 redaction at the service seam; tests assert no `sqlite` substring |
| Half-written export left on crash | Denial of Service (data loss) | temp + rename; remove temp on error |
| Half-restored corpus on a malformed file | Tampering / DoS | validate-before-write (D-08/D-11); row-count-unchanged assertion in tests |
| Resource exhaustion on import of a huge file | DoS | accepted: pass 1 holds parsed records in memory (D-08); content size bounded only by what `store` accepts — document |
| Sweep deleting "successfully imported" expired rows | Repudiation (UX) | import-time `expires_at <= now` skip + `skipped_expired` count |

## Sources

### Primary (HIGH confidence — read with the Read tool this session)
- `crates/agent-memory-core/src/service.rs` (redaction helper 33-60, public methods, `import()` 394-463, `ImportReport` 162-168, `BACKFILL_BATCH` 749)
- `crates/agent-memory-core/src/domain.rs` (`MemoryError` 375-426, `NewMemory` 245-253, `LinkOutcome` 176-183, `MemoryType`/`LinkKind` + `try_from`, `validate_*`)
- `crates/agent-memory-core/src/store/mod.rs` (trait, `exists` doc 81-91, `link` 174-194, `sweep_expired` 219-228)
- `crates/agent-memory-core/src/store/sqlite.rs` 120-445 (backup helpers, struct, `open`), 595-654 (`insert`), 735-804 (`pending_embeddings`, `exists`, `list`), 945-994 (`link`)
- `crates/agent-memory-core/src/import/{mod.rs,gsd_state.rs}`, `crates/agent-memory/src/{main.rs,config.rs}`, `crates/agent-memory-core/tests/import.rs`, `sql/0002_embeddings.sql`, `sql/0003_relations.sql`, `lib.rs`
- `.planning/phases/05-portable-export-import/05-CONTEXT.md`, `.planning/REQUIREMENTS.md`, `.planning/STATE.md`
- Scratch probe `/tmp/serde-probe` (serde `=1.0.228`, serde_json `=1.0.150`, `cargo run --offline`) — 11 deserialization cases + serialization order + `Deserializer::from_str` + trailing characters

### Secondary (MEDIUM confidence — official docs via Context7, or repo files viewed via grep/sed)
- Context7 `/websites/serde_rs` — container-attrs (`deny_unknown_fields`, `flatten` limitation), enum-representations (internally tagged)
- Context7 `/websites/rs_serde_json` — `Error::line/column/classify`, `Deserializer::from_str/from_reader`
- Repo via grep/sed: `.planning/research/ARCHITECTURE.md` §4 + Anti-Pattern 4; `PITFALLS.md` 17–19 + Security/Performance/UX rows; `04-REVIEW.md` IN-01/WR-02; `tests/hybrid.rs:712-770`; `rest/handlers.rs:167-182,437-454,851-889`; `mcp.rs:35-50,1309-1344`; `rest/mod.rs:34-48`; `tests/relations.rs:1-104`; `embed/mod.rs:85-160`; `sql/0001_init.sql`; `.github/workflows/ci.yml`; `.claude/hooks/pre-bash-rust.sh`; `README.md` 414-535, 557-567; `04-RESEARCH.md`/`04-VALIDATION.md` Validation sections

### Tertiary (LOW confidence per the classify-confidence seam — WebFetch of official std docs; content is authoritative, provider tier is not)
- https://doc.rust-lang.org/std/fs/fn.rename.html — replace semantics, syscalls, mount-point caveat
- https://doc.rust-lang.org/std/os/unix/fs/trait.OpenOptionsExt.html — `mode`, umask
- https://doc.rust-lang.org/std/io/struct.Stdout.html — `lock() -> StdoutLock<'static>`

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — no new packages; versions read from `Cargo.lock`; serde behaviour probe-verified on the pinned versions
- Architecture: HIGH — every integration point read in source this session; call-site counts are grep-verified; the only assumptions (A1–A8) are low-risk and listed
- Pitfalls: HIGH for 1–6, 9, 11, 12 (source/probe-backed); MEDIUM for 7, 8, 10 (std docs + reasoning)
- D-14 compatibility: HIGH — the exact assertion sites and the byte-identical message template are verified

**Research date:** 2026-10-25
**Valid until:** 2026-11-24 (stable: no external moving parts; re-check only if the serde/serde_json pins change or a migration lands)
