# Changelog

All notable changes to agent-memory are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While the version is `0.x`, a minor bump (`0.0.x` → `0.1.0`) may change contracts;
every such change is listed under **Changed** and in the README's
*Behavioral changes in v0.1.0* section.

## [0.1.0] - Unreleased

Generated from `git log v0.0.1..main` (133 commits). Highlights: update and
relations tools, hybrid search, and portable JSONL export/import.

### Added

- **`memory_update` MCP tool and `PATCH /api/memories/{id}`.** Merge-patch
  semantics: an omitted field is unchanged, `null` clears it. A content change
  re-embeds in the same writer transaction, and the edit counts as a fresh
  access. Unknown or immutable fields are refused on both transports.
- **Relations: `memory_link` / `memory_unlink`** with three kinds:
  `relates_to` (symmetric), `supersedes` and `caused_by`. Also exposed as
  `POST /api/links` and `DELETE /api/links/{from_id}/{to_id}/{kind}`. Linking
  twice is idempotent, a self-link is a client error, and a missing endpoint is
  reported as data rather than a 500. `memory_forget` and the TTL sweep remove
  a memory's links with it.
- **`expand_links`** on `memory_search` and `memory_list` attaches up to 10
  newest related memories per result, labelled by direction (`superseded_by`,
  `caused` for incoming edges). `memory_list` with `expand_links` requires a
  `limit`.
- **Hybrid search.** When Ollama is reachable, keyword (FTS5 bm25) and semantic
  (KNN) rankings are fused with Reciprocal Rank Fusion (k = 60), then
  decay-weighted, reported as `"search_mode": "hybrid"`.
- **Forgiving keyword leg.** The raw FTS5 query runs first. If FTS5 cannot parse
  it, it is retried once with its words quoted and OR-joined, so punctuated,
  hyphenated and colon-bearing queries (`sqlite-vec`, `TODO: auth`) are
  answered instead of failing.
- **Portable JSONL export/import (format v1).** `agent-memory export [--output PATH]`
  writes a header, then one record per memory, then one record per link, all
  from one consistent read snapshot. Embedding vectors and other derived fields
  are excluded. `agent-memory import --from jsonl FILE` validates the whole file
  before it writes anything. It is idempotent on re-import, safe on a non-empty
  database, and remaps file-local link keys to target ids. Expired rows are
  skipped and counted. Restored rows are re-embedded against the importing
  database's own model by the background sweep. Export and import are CLI
  subcommands only (no MCP tool, no REST route).
- **Private, atomic `--output`.** The export is written to a sibling temp file
  created with mode `0600`, then `sync_all`-ed and renamed over the target. No
  failure path leaves a half-written file.
- **Schema migration 0003 (`memory_links`)** with an automatic pre-migration
  online backup (`memory.db.backup-pre-NNNN-YYYYMMDD`). If the backup cannot be
  written, the database is not migrated. Concurrent first opens are safe.
- **Newer-schema refusal.** A database written by a newer agent-memory is
  refused at startup and left byte-untouched. The error names both schema
  versions.
- **Release provenance.** Every release tarball and `SHA256SUMS.txt` carries a
  signed SLSA build-provenance attestation, verifiable with
  `gh attestation verify agent-memory-<triple>.tar.gz --repo UnityInFlow/agent-memory` (#1).

### Changed

- **The binary crate is now `agent-memory-mcp`.** The crates.io name
  `agent-memory` belongs to an unrelated project. Install with
  `cargo install agent-memory-mcp`. The executable, the Homebrew formula and
  the release tarball names stay `agent-memory`. `agent-memory-core` is
  published beside it (#9).
- **Tag filtering is an exact, case-sensitive match** instead of a substring
  match: `tag=rust` no longer matches `rustling`. This applies to search (both
  modes) and list.
- **Out-of-bounds `limit` (1–200) and `ttl_secs` (1–3155760000) are client
  errors** (HTTP 400 / MCP `invalid_params`) carrying the valid range, instead
  of being clamped or accepted.
- **`search_mode: "semantic"` is no longer emitted.** With an embedder,
  searches are `"hybrid"`. `"keyword"` (embedder down) and the envelope shape
  are unchanged. Result order can differ from v0.0.1's embedding-only ranking.
- **Release pipeline** moved to GitHub-hosted runners, triggered only by exact
  `vX.Y.Z` tags. All actions are pinned to commit SHAs (#1).

### Fixed

- The keyword fallback (Ollama down) now honours the tag filter.
- A malformed or over-deep FTS5 expression is classified as client input, never
  a server fault.
- Re-importing a GSD `STATE.md` with no source is idempotent (NULL-safe
  dedup).
- The background embedding backfill never overwrites a row that was edited or
  forgotten while its vector was being computed.
- `relates_to` is stored as one symmetric relation and can be unlinked from
  either end.
- Pre-migration backups are race-safe across processes opening the same
  database.

### Security

- **Storage-engine errors are redacted on every surface.** `store`, `list`
  (including `/health`), `search`, `update`, `link`/`unlink`, `forget`,
  `import`, `export` and the sweep now report
  `<operation> failed due to a storage error (details are in the server log)`
  instead of echoing SQLite text with table and column names. The detail is
  only in the server's stderr log.
- The JSONL importer treats its input as untrusted. Hostile input (deep
  nesting, NUL bytes, overflowing numbers, negative `access_count`) is either
  rejected with a line-numbered error or stored as plain data. It never
  panics. File keys can never address
  existing rows. Model-mismatched vectors cannot be imported, because the format
  has no vector field.
- Added [`SECURITY.md`](SECURITY.md) with a private vulnerability-reporting
  route.

## [0.0.1] - 2026-07-03

First release.

### Added

- MCP stdio server with `memory_store`, `memory_search`, `memory_list` and
  `memory_forget` over embedded SQLite (no cloud, no account).
- Six memory types: `DECISION`, `PATTERN`, `ERROR`, `TODO`, `ARCHITECTURE`,
  `CONSTRAINT`.
- Exponential decay scoring, which fades ranking without deleting anything.
  Optional TTL expiry with a background sweep.
- Semantic search via local Ollama (`nomic-embed-text`), with automatic FTS5
  keyword fallback.
- `agent-memory import --from gsd-state .planning/STATE.md`.
- REST API (`agent-memory serve-rest`), bound to loopback by default.
  Non-loopback addresses are refused unless `--allow-remote` is passed, because
  the API has no authentication.
- Pre-built binaries for macOS (arm64/x86_64) and Linux gnu (x86_64/aarch64),
  plus the Homebrew formula `unityinflow/tap/agent-memory`.

[0.1.0]: https://github.com/UnityInFlow/agent-memory/compare/v0.0.1...v0.1.0
[0.0.1]: https://github.com/UnityInFlow/agent-memory/releases/tag/v0.0.1
