---
phase: 3
slug: api-hardening-toolchain-spikes
status: verified
threats_open: 0
asvs_level: high
created: 2026-07-12
---

# Phase 3 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| HTTP client → REST API | Untrusted request bodies/query params (limit, ttl_secs, tag) cross into handlers (loopback-guarded, unauthenticated by design) | Untrusted client input |
| MCP client → stdio tools | Untrusted tool arguments cross into the service | Untrusted client input |
| CLI import file → import() | Parsed drafts reach store.insert without passing store() — the import bypass this phase closed | Untrusted file content |
| Public repo → GitHub Actions | Anyone can open PRs against this public repo; workflows define what code from which triggers can run with which permissions | Untrusted fork code |
| CI job → external tool sources | crates.io (cargo-xwin/cargo-zigbuild/cargo-llvm-cov), ziglang.org (zig tarball), Ubuntu apt (mingw), Microsoft (xwin-fetched MSVC CRT/SDK) cross into the build environment | Third-party build tooling |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-03-01 | Denial of Service | service.rs seam / sqlite.rs ttl arithmetic | mitigate | Bounds consts + `validate_limit`/`validate_ttl` (domain.rs:118-188; `MAX_TTL_SECS = 3_155_760_000` keeps `now + ttl` below `i64::MAX`); seam calls at all four service methods; negative SQL `LIMIT -N` unreachable; test-locked incl. `i64::MAX`/`i64::MIN` (tests/validation.rs) | closed |
| T-03-02 | Information Disclosure | Error message bodies (both transports) | mitigate | D-05 messages echo only client-sent value + public range consts (domain.rs:168-183); internal tier stays generic per 02-05 contract at both mappers (handlers.rs:87-98, mcp.rs:32-43) | closed |
| T-03-03 | Tampering (result integrity) | sqlite.rs tag predicates (3 sites) | mitigate | `json_each` equality EXISTS at all 3 sites (sqlite.rs:284, 377, 425); `tags LIKE` = 0; tag remains a bound parameter with unchanged indices; per-site rust/rustling/Rust regression tests | closed |
| T-03-04 | Information Disclosure | Server-fault probing via crafted inputs | mitigate | `InvalidArgument` in client arm of both mappers → REST 400 / MCP `invalid_params`; in-process and real-HTTP 400-never-500 proofs (handlers.rs:444-500, tests/rest.rs:220-264) | closed |
| T-03-05 | Elevation of Privilege | Both workflow files on a public repo | mitigate | Secretless: `permissions: contents: read` on both; spike is `workflow_dispatch`-only; zero `secrets.` references; OPS-02 keeps org self-hosted runners unreachable from this repo | closed |
| T-03-06 | Tampering (supply chain) | CI-side build tooling | mitigate | cargo-xwin/cargo-zigbuild pinned 0.23.0 installed `--locked --version`; zig 0.14.1 from official ziglang.org tarball; never `latest`; spike ships no artifacts (no upload/release steps) | closed |
| T-03-07 | Repudiation | D-11 verdict record | mitigate | VERDICT comment cites date 2026-07-12 + concrete run URL (actions/runs/29196226289) in workflow header and 03-02-SUMMARY.md | closed |
| T-03-SC-01 | Tampering | cargo installs (plan 03-01) | accept | Zero new dependencies — git-verified empty diff on all Cargo manifests across the 8 phase commits; rusqlite 0.39 pin triangle unmoved | closed |
| T-03-SC-02 | Tampering | cargo installs (plan 03-02) | mitigate | All installs are CI-runner-side pinned build tools; `cargo-llvm-cov` install carried over byte-identical (git show 8b0337e); Cargo manifests untouched | closed |

*Status: open · closed*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-03-01 | T-03-SC-01 | Plan 03-01 adds zero new dependencies (Cargo manifests git-verified untouched); the rusqlite 0.39/rusqlite_migration 2.5/r2d2_sqlite 0.34 pin triangle must not move (STATE.md 01-01), so no package-legitimacy checkpoint was needed | Plan 03-01 threat model (plan-time) | 2026-07-12 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-07-12 | 9 | 9 | 0 | gsd-security-auditor (assume-open verification, ASVS high) |

**Informational notes (non-blocking):**
1. Internal-tier 500 bodies carry the error Display string (preserved 02-05 contract on a loopback-guarded API) — revisit if `--allow-remote` deployment becomes common.
2. `release.yml` untouched by this phase (git-verified) — Phase 6 owns the release-side runner decision.
3. The zig tarball fetch is HTTPS + pinned version but has no checksum verification — adding a SHA-256 pin is a hardening upgrade for the Phase 6 release workflow.

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-07-12
