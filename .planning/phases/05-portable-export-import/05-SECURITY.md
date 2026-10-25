---
phase: 05
slug: portable-export-import
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
block_on: high
created: 2026-10-25
---

# Phase 05 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> Register merged from the `<threat_model>` blocks of 05-01..05-05 PLAN.md (authored at plan
> time) and the `## Threat Flags` of every SUMMARY (none new). Verified at ASVS L1 (grep depth)
> against `main` @ `b1053c5`.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| MCP stdio / REST loopback client → service error text | untrusted reader of error bodies | storage-engine detail (table/column names) — must not cross |
| service → stderr log | trusted operator channel | full engine error text |
| database (0700 dir) → export file on disk | the corpus leaves the DB's protection | whole memory corpus (private) |
| store read pool ↔ concurrent writers | WAL snapshot per read transaction | memories + links consistency |
| user-supplied JSONL file → parser | untrusted bytes (hand-edited, truncated, hostile, skewed clock) | memories, links, timestamps |
| file-local keys ↔ target database ids | two id spaces that must never be confused | link endpoints |
| realism run → local Ollama / loopback REST daemon | test-only, synthetic fixtures | synthetic text |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-05-01 | Information Disclosure | engine text in service errors + `/health` | medium | mitigate | `redact_storage_error` (`service.rs:64`) wraps every public service op (store/list/update/link/unlink/forget/import/sweep/search/export); `tests/redaction.rs`; `health_storage_failure_is_500_without_sqlite_text` | closed |
| T-05-02 | Information Disclosure | export file world-readable / in shared tmp | high | mitigate | `export_file.rs:127-128` `create_new(true).mode(0o600)`; temp `{target}.export-tmp-{pid}` beside the target; unit test on mode | closed |
| T-05-03 | DoS (data loss) | half-written export target | medium | mitigate | `sync_all` + `rename` in `commit` (`export_file.rs:90-94`); temp removed on every failure path; commit/abort tests | closed |
| T-05-04 | Tampering | vectors travelling in the file | high | mitigate | `Record` has no vector field, `deny_unknown_fields` (`export.rs:114`); `export_rows` never reads `vec_memories` (`sqlite.rs:874`); `insert_imported` writes `embedding_status = 0`; `memory_records_carry_exactly_the_documented_keys` | closed |
| T-05-05 | Tampering (integrity) | file keys treated as target ids | high | mitigate | keys index only the pass-2 `HashMap<key,id>` (`service.rs:646`); unmapped/collapsed endpoints counted `links_dropped`; overlapping-rowid kill-test (`edges_touching`) | closed |
| T-05-06 | Tampering | hand-edited `type`/`kind`/`tags` reaching SQL | medium | mitigate | `MemoryType::try_from` / `LinkKind::try_from` at the seam; `tags: Vec<String>`; bound `params![]` only | closed |
| T-05-07 | DoS | parser panic on hostile input | medium | mitigate | serde_json recursion limit; no `unwrap` on untrusted data; `hostile_input_never_panics` (`jsonl.rs:628`); invalid UTF-8 refused in CLI | closed |
| T-05-08 | Tampering / DoS | half-restored corpus on malformed file | high | mitigate | `parse_jsonl` completes before `open_service` (`main.rs:282-285`); `nothing_is_written_when_validation_fails` | closed |
| T-05-09 | Repudiation (UX) | expired rows reported imported | low | mitigate | `expires_at <= now` → `skipped_expired` (`service.rs:651`); boundary test | closed |
| T-05-10 | DoS | pass 1 holds a huge file in RAM | low | accept | see Accepted Risks AR-05-01 | closed |
| T-05-11 | EoP / Information Disclosure | export/import as MCP tool or REST route | high | mitigate (by design) | CLI-only (D-01); grep for `memory_export`/`memory_import`/`/api/export`/`/api/import` in `crates/agent-memory/src` is empty; README states the rule | closed |
| T-05-12 | Information Disclosure | stdout export in scrollback | low | accept | see AR-05-02 | closed |
| T-05-13 | Tampering | source-clock skew | low | mitigate | `clock_skew_message` + `CLOCK_SKEW_TOLERANCE_SECS = 3600` (`service.rs:205-213`) with unit test | closed |
| T-05-14 | Information Disclosure | `SqliteStore::open` failures on CLI stderr | low | accept | see AR-05-03 | closed |
| T-05-15 | Tampering (integrity) | divergent `memories` INSERTs | low | mitigate | single private `insert_row` (`sqlite.rs:622`); exactly one `INSERT INTO memories` in `src` | closed |
| T-05-16 | Repudiation | restored row swept later | low | mitigate (contract) | `store/mod.rs:52-53` fixes `<= now` (import skip) vs `< now` (sweep) | closed |
| T-05-17 | Information Disclosure | engine text on import fault | medium | mitigate | `redact_storage_error("import", _)`; `import_jsonl_storage_failure_is_redacted` | closed |
| T-05-18 | Repudiation (UX) | misread report counts | low | mitigate | README count table (`README.md:711` "not an error"); CLI test pins wording | closed |
| T-05-19 | Tampering (integrity) | export straddling a concurrent `link` | medium | mitigate | one `unchecked_transaction` on one pooled connection spans counts and both scans (`sqlite.rs:837-843`); 05-05 race proof | closed |
| T-05-20 | Information Disclosure | engine text on export fault | medium | mitigate | `redact_storage_error("export", _)`; `export_storage_failure_is_redacted` | closed |
| T-05-21 | DoS (test hang) | race test deadlock | low | mitigate | `recv_timeout(10 s)` on both hand-offs (`tests/export_import.rs:473,504`) | closed |
| T-05-22 | EoP / Information Disclosure | realism run binds the REST daemon | low | mitigate | `127.0.0.1:0`, throwaway DB, `trap cleanup EXIT` (`05-05-realism.sh:70,107`) | closed |
| T-05-23 | Information Disclosure | fixture text to local Ollama / temp dir | low | accept | see AR-05-04 | closed |
| T-05-24 | Repudiation (evidence) | realism checks loosened to pass | low | mitigate | checks fixed in committed `05-05-realism.sh`; raw output in 05-05-SUMMARY | closed |
| T-05-SC | Tampering | cargo supply chain | low | accept | see AR-05-05 | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

Related hardening closed by the phase code review (05-REVIEW.md, all fixed before this audit):
WR-02 (unbounded `access_count` could make every later export fail — now rejected `< 0` on
import and the bump saturates, `3fdadad`), WR-03 (dedup probe matched expired-but-unswept rows,
`db11cc8`), IN-02 (redaction proofs for export/import, `1173fde`).

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-05-01 | T-05-10 | Inherent to validate-before-write (D-08); a 4 MiB content line is tested to parse; documented in README. A size cap is not in v0.1.0 scope. | plan 05-03 / 05-04 threat model | 2026-10-25 |
| AR-05-02 | T-05-12 | User-directed output; README points to `--output` (0600, atomic) as the private alternative. | plan 05-02 / 05-04 threat model | 2026-10-25 |
| AR-05-03 | T-05-14 | Startup, operator-facing local stderr outside the `MemoryService` seam; no transport carries it. README scopes redaction to served requests and CLI import/export. | plan 05-01 threat model (orchestrator resolution 4) | 2026-10-25 |
| AR-05-04 | T-05-23 | Synthetic engineering-note fixtures only; nothing user-private; temp dir OS-managed. | plan 05-05 threat model | 2026-10-25 |
| AR-05-05 | T-05-SC | Zero new dependencies in Phase 5; `--locked` builds fail on any `Cargo.lock` drift; rusqlite/libsqlite3-sys pin untouched. | plans 05-01..05-05 threat model | 2026-10-25 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-10-25 | 25 | 25 | 0 | /gsd-secure-phase 5 (orchestrator, ASVS L1 grep-depth short-circuit; register authored at plan time, so no auditor spawn) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-10-25
