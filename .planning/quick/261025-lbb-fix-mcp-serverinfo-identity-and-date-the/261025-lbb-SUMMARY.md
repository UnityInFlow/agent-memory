---
phase: quick-261025-lbb
plan: 01
subsystem: mcp
tags: [mcp, serverInfo, initialize, changelog, v0.1.0]
status: complete
requires: []
provides:
  - "MCP initialize reports agent-memory's own identity (name, version, title, websiteUrl)"
  - "stdio regression test initialize_reports_agent_memory_server_info_over_stdio"
  - "CHANGELOG 0.1.0 dated 2026-10-25 with a serverInfo Fixed bullet"
affects: [phase-6-v0.1.0-release]
tech-stack:
  added: []
  patterns:
    - "rmcp #[non_exhaustive] Implementation built through Implementation::new + with_* builders"
key-files:
  created: []
  modified:
    - crates/agent-memory/src/mcp.rs
    - crates/agent-memory/tests/tools.rs
    - CHANGELOG.md
decisions:
  - "serverInfo name is the literal product/binary name \"agent-memory\", not CARGO_PKG_NAME (which would be the crates.io package name agent-memory-mcp)"
  - "serverInfo version and websiteUrl come from env!(CARGO_PKG_VERSION) / env!(CARGO_PKG_REPOSITORY) of package agent-memory-mcp, so they stay single-sourced from the manifests"
  - "serverInfo title \"Agent Memory\" (planner discretion: human-readable display title)"
metrics:
  duration: "~6 min"
  completed: 2026-10-25
  tasks: 3
  files: 3
actuals:
  tokens: 1500
  tasks: 3
  commits: 2
---

# Phase quick-261025-lbb Plan 01: MCP serverInfo identity + dated 0.1.0 CHANGELOG Summary

agent-memory's MCP `initialize` reply now names agent-memory 0.1.0, with a title and the repository URL, where it used to report the rmcp SDK (`rmcp` / `1.8.0`). A real-binary stdio test pins the fix, and the CHANGELOG's 0.1.0 entry is dated 2026-10-25.

## Live serverInfo capture

**Before** (planner's probe of the pre-fix binary, 2026-10-25):

```json
{"name":"rmcp","version":"1.8.0"}
```

**After**: observed from a live stdio `initialize` against the binary rebuilt on HEAD `be9dc2d`. The binary's mtime is 1792938446, later than the Task 1 commit time of 1792938423, so the binary was not stale.

```json
{"name":"agent-memory","title":"Agent Memory","version":"0.1.0","websiteUrl":"https://github.com/UnityInFlow/agent-memory"}
```

Command used:

```bash
D=$(mktemp -d)
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"probe","version":"0.0.0"}}}' \
  | AGENT_MEMORY_OLLAMA_URL=http://127.0.0.1:9 target/debug/agent-memory serve --db "$D/memory.db" 2>/dev/null \
  | head -1 | jq -c '.result.serverInfo'
rm -rf "$D"
```

The rest of the same reply did not change: `protocolVersion` `"2025-11-25"`, `capabilities` `{"tools":{}}`, and the same `instructions` string.

## Tasks and commits

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | serverInfo identity: RED test, GREEN fix (tracer) | `8c49f243b10af1ed1cf239ba5fe30329ed2f2538` (`fix(mcp): report agent-memory serverInfo identity`) | crates/agent-memory/src/mcp.rs, crates/agent-memory/tests/tools.rs |
| 2 | Date the 0.1.0 CHANGELOG entry and add a Fixed bullet | `be9dc2d62408d540c78d69e56e610a351ab0277a` (`docs(changelog): date 0.1.0 as 2026-10-25`) | CHANGELOG.md |
| 3 | Final gates and live capture | (no code commit; this SUMMARY is for the orchestrator's docs commit) | this file |

Both commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

### RED (Task 1, before the fix)

`cargo test -p agent-memory-mcp --locked --test tools initialize_reports` failed:

```
thread 'initialize_reports_agent_memory_server_info_over_stdio' panicked at crates/agent-memory/tests/tools.rs:134:5:
assertion `left == right` failed
  left: Some("rmcp")
 right: Some("agent-memory")
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 7 filtered out
```

The red state was not committed, per the CLAUDE.md rule that failing tests are never committed.

### GREEN (Task 1, after the fix)

- `tests/tools.rs`: 8 passed / 0 failed / 0 ignored. This is the 7 existing tests plus the new one.
- `tests/stdio_purity.rs`: 1 passed / 0 failed / 0 ignored. The file itself is unchanged.

## What changed

- `crates/agent-memory/src/mcp.rs` (`get_info`): `info.server_info = Implementation::new("agent-memory", env!("CARGO_PKG_VERSION")).with_title("Agent Memory").with_website_url(env!("CARGO_PKG_REPOSITORY"));`. The comment above it gained one sentence explaining that rmcp's default Implementation reports the SDK's own identity. Capabilities and instructions are unchanged. The change adds no `unwrap()`/`expect()`, because the `env!` macros resolve at compile time.
- `crates/agent-memory/tests/tools.rs`:
  - `Server::initialize` now returns the initialize reply (`serde_json::Value`). The seven existing call sites are unchanged.
  - New test `initialize_reports_agent_memory_server_info_over_stdio` asserts `name`, `version` (against `env!("CARGO_PKG_VERSION")`, not a literal), `title` and `websiteUrl` at `/result/serverInfo`.
- `CHANGELOG.md`:
  - The heading now reads `## [0.1.0] - 2026-10-25`, and the placeholder word is gone from the file.
  - One bullet was appended to the end of `### Fixed`.
  - The "Generated from …" sentence and the `[0.1.0]:` compare-link footer are untouched.

**Where `env!("CARGO_PKG_VERSION")` resolves:** `mcp.rs` is `mod mcp;` of `src/main.rs` in package **agent-memory-mcp** (`crates/agent-memory/Cargo.toml`, version 0.1.0), not agent-memory-core. The integration test compiles in the same package, so it compares the same value. `CARGO_PKG_REPOSITORY` is inherited from the workspace (`https://github.com/UnityInFlow/agent-memory`).

**Title choice:** "Agent Memory" is a planner discretion choice, a human-readable display title for MCP client UIs. Description and icons were deliberately left unset.

## Final gates (HEAD `be9dc2d`)

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | clean, exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0, 0 warnings |
| `cargo test --workspace --locked` | exit 0: **21 test binaries (incl. 1 doc-test), 288 passed / 0 failed / 2 ignored** |
| `cargo build --locked -p agent-memory-mcp` | exit 0 |
| `Cargo.lock` | unchanged (`git diff --quiet adea39e HEAD -- Cargo.lock`) |

The test totals were summed from the raw `test result:` lines (run via `rtk proxy`). The two ignored tests are one in `migration_hygiene.rs` and one in `semantic.rs`, both pre-existing. Per binary:

- agent_memory_core unit tests: 63
- backfill_race: 7
- decay: 4
- export_import: 20
- fallback: 5
- hybrid: 22
- import: 2
- migration_hygiene: 8 + 1 ignored
- redaction: 10
- relations: 27
- semantic: 4 + 1 ignored
- store: 8
- ttl: 4
- update: 9
- validation: 5
- agent-memory unit tests (main.rs): 72
- cli: 6
- rest: 3
- stdio_purity: 1
- tools: 8
- doc-tests: 0

## Deviations from Plan

None. The plan was executed exactly as written.

## Threat model

- T-261025-lbb-01 (spoofing): mitigated. The reply now reports the true identity, pinned by the stdio test.
- T-261025-lbb-03 (stdout tampering): mitigated. `stdio_purity` still passes, and there is no new output path.
- T-261025-lbb-04 (dependency graph): mitigated. There are no new dependencies, every command used `--locked`, and Cargo.lock is unchanged.
- T-261025-lbb-02 (information disclosure): accepted, as planned.

No new threat surface.

## Known Stubs

None.

## Not done (by instruction)

Nothing was pushed, tagged or published. STATE.md and ROADMAP.md were not updated, and no docs artifacts were committed (the orchestrator owns the docs commit). The pre-existing modified and untracked files (`.planning/config.json`, `.claude/`, `.gitignore`, `.gsd/`, `.mcp.json`, `.planning/research/.cache/`, `10-agent-memory.md`, `CLAUDE.md`, `claude-code-harness-engineering-guide-v2.md`, `docs/`) were left unstaged.

## Self-Check: PASSED

- FOUND: crates/agent-memory/src/mcp.rs contains `Implementation::new("agent-memory", env!("CARGO_PKG_VERSION"))`
- FOUND: crates/agent-memory/tests/tools.rs contains `initialize_reports_agent_memory_server_info_over_stdio`
- FOUND: CHANGELOG.md contains `## [0.1.0] - 2026-10-25`
- FOUND: commit 8c49f24, commit be9dc2d
