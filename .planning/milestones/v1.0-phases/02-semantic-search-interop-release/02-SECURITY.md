---
phase: 02
slug: semantic-search-interop-release
status: verified
threats_open: 0
asvs_level: 1
created: 2026-07-12
---

# Phase 02 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> Register origin: plan-time — all five plans (02-01…02-05) carried `<threat_model>` blocks. Every threat has a disposition; summaries confirm mitigations applied as registered; 02-VERIFICATION.md proved the key controls live.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| MCP client → stdio server | untrusted tool arguments (query strings, ids, types, filters) | untrusted client input |
| HTTP client → REST daemon | untrusted request bodies/query params (loopback by default, unauthenticated by design) | untrusted client input |
| core → Ollama daemon | memory content leaves the process over localhost HTTP for embedding | user memory content |
| REST daemon ↔ MCP stdio processes | two processes share one SQLite file (WAL), same user, no privilege difference | shared local state |
| filesystem → parser | untrusted/messy STATE.md file content crosses into typed drafts | user-owned file content |
| imported content → downstream agents | memory content is later injected into agent contexts via search results | potential prompt-injection carrier |
| release artifacts → end users | users download and execute binaries built by CI | executable binaries |
| CI workflow → third-party tooling | zig tarball, cargo-zigbuild, gh-release action run inside the release pipeline | supply-chain surface |
| private working tree → public repo | publishing exposes the full git history publicly | repo history |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-02-01 | Tampering | knn_search / vec insert SQL | mitigate | every KNN parameter bound (query blob via bytemuck, k, filters); no user input formatted into SQL | closed |
| T-02-02 | Tampering (SSRF-style) | OllamaClient base URL | mitigate | base URL only from --ollama-url flag / AGENT_MEMORY_OLLAMA_URL env (default localhost:11434), never from request payloads | closed |
| T-02-03 | Denial of Service | embed HTTP calls / KNN k | mitigate | 10s reqwest timeout; k capped at 200; search limit capped at DEFAULT_SEARCH_LIMIT | closed |
| T-02-04 | Information Disclosure | memory content → Ollama | accept | localhost-only by default per zero-cloud doctrine; remote URLs are an explicit user override — documented | closed |
| T-02-05 | Tampering (integrity) | vec_memories ↔ memories drift | mitigate | explicit vec deletes in forget() and sweep_expired(); drift kill-test (forgotten id never resurfaces semantically) | closed |
| T-02-SC (02-01) | Tampering (supply chain) | sqlite-vec, reqwest, bytemuck | mitigate | all passed RESEARCH Package Legitimacy Audit (OK verdicts); versions pinned in workspace Cargo.toml | closed |
| T-02-10 | Info Disclosure / Elevation | REST bind address | mitigate | default 127.0.0.1:7437; ensure_bind_allowed refuses non-loopback without --allow-remote + logged warning; bind-guard tests live (02-VERIFICATION truth 11) | closed |
| T-02-11 | Tampering | request deserialization → SQL | mitigate | serde-typed DTOs; MemoryType::try_from validates before SQL; all SQL parameterized end-to-end | closed |
| T-02-12 | Denial of Service | request bodies / result sizes | mitigate | axum default ~2MB body limit retained; list/search limit caps from core | closed |
| T-02-13 | Tampering (CSRF/DNS-rebinding) | browser-origin requests to localhost | accept | single-user local tool, no auth at loopback by design (documented in README); revisit if --allow-remote gains real use | closed |
| T-02-SC (02-02) | Tampering (supply chain) | axum | mitigate | passed Package Legitimacy Audit (OK, 7.2M dl/wk); version pinned | closed |
| T-02-20 | Tampering | parse_gsd_state / exists / insert | mitigate | tolerant scanner (skip + count, never execute); dedup SELECT and inserts fully parameterized; content strictly data | closed |
| T-02-21 | Elevation (prompt-injection carrier) | imported memory content consumed by agents | transfer/accept | content never executed or interpreted by agent-memory; composes with ecosystem injection-scanner (documented, no in-phase code) | closed |
| T-02-22 | Denial of Service | oversized items / huge files | mitigate | MAX_IMPORT_CONTENT_BYTES = 8192 per item with skip-count; whole-file size accepted (local, user-owned CLI input) | closed |
| T-02-23 | Repudiation | provenance of imported rows | mitigate | every imported row carries source = "gsd-state" + tag "gsd" + scope — auditable and bulk-removable | closed |
| T-02-SC (02-03) | Tampering (supply chain) | new dependencies | mitigate | none added — hand-rolled scanner per RESEARCH (pulldown-cmark deliberately avoided) | closed |
| T-02-30 | Tampering | release artifacts | mitigate | SHA256SUMS.txt published with Release; Homebrew formula pins per-arch sha256; local checksum verification before formula written; assets re-verified live via gh (02-VERIFICATION) | closed |
| T-02-31 | Tampering (supply chain) | zig + cargo-zigbuild + gh-release action | mitigate | ZIG_VERSION 0.14.1 from official ziglang.org tarball; cargo-zigbuild --locked pinned 0.23.0; softprops/action-gh-release@v3 — versions pinned, never latest | closed |
| T-02-32 | Information Disclosure | repo publication (.planning history included) | mitigate | pre-push secret scan for token patterns run before gh repo create; secrets only via env vars; untracked local files (.mcp.json, CLAUDE.md, .claude/) left untracked | closed |
| T-02-33 | Spoofing | release creation | mitigate | Release created only by tag-triggered workflow with repo-scoped GITHUB_TOKEN; tag pushed by authenticated gh session | closed |
| T-02-SC (02-04) | Tampering (supply chain) | package installs | mitigate | no new crates; CI tools pinned as above | closed |
| T-02G-01 | Information Disclosure | rest/handlers.rs map_memory_error | mitigate | FTS5 parse errors mapped to InvalidQuery → 400 with controlled message (WR-05); proven live at 4 layers (02-VERIFICATION truth 10) | closed |
| T-02G-02 | Tampering (result integrity) | sqlite.rs fn search (keyword path) | mitigate | bound ?10 tag predicate — parameterized, no string formatting, no injection surface added (CR-01); regression test live 4/4 | closed |
| T-02G-03 | DoS (agent retry loops) | mcp.rs error mapping | mitigate | exhaustive two-tier map_mcp_error restores honest error taxonomy (WR-04); unit test live | closed |
| T-02G-SC | Tampering (supply chain) | cargo installs | accept | zero new dependencies — all crates already pinned and audited in 02-01/02-02 | closed |

*Status: open · closed*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-02-01 | T-02-04 | Memory content sent to Ollama over localhost HTTP — zero-cloud doctrine keeps it local by default; remote URL is an explicit user override, documented | plan 02-01 (user-approved plans) | 2026-07-03 |
| AR-02-02 | T-02-13 | No auth on loopback REST — single-user local tool by design; documented in README; revisit if --allow-remote gains real use | plan 02-02 (user-approved plans) | 2026-07-03 |
| AR-02-03 | T-02-21 | Imported memory content as prompt-injection carrier — never executed/interpreted by agent-memory; mitigation transferred to ecosystem injection-scanner | plan 02-03 (user-approved plans) | 2026-07-03 |
| AR-02-04 | T-02G-SC | No package-legitimacy checkpoint in gap-closure plan — zero new dependencies added | plan 02-05 (user-approved plans) | 2026-07-03 |

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-07-12 | 25 | 25 | 0 | Claude (gsd-secure-phase, plan-time register short-circuit) |

Evidence basis: plan-time `<threat_model>` registers in all five PLAN.md files; SUMMARY.md Threat Flags sections (02-02, 02-04, 02-05: "None — mitigations applied as registered"; 02-01, 02-03 predate the Threat Flags convention but their mitigations are verified in 02-VERIFICATION.md); 02-VERIFICATION.md re-verification (15/15 truths, live test runs: bind-guard, parameterized keyword/tag predicate, two-tier error taxonomy at 4 layers, release assets + checksums re-queried via gh).

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-07-12
