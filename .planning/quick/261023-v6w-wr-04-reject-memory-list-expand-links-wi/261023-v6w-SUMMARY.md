---
phase: quick-261023-v6w
plan: 01
subsystem: memory-service / MCP + REST list transport
status: complete
tags: [WR-04, expand_links, limit, validation, review-closure]
requires: [Phase 04 (expand_links, validate_limit, InvalidArgument tiering)]
provides: [memory_list expand_links requires a limit; 200 x 10 bound on every expanded response]
affects: [MemoryService::list, memory_list MCP tool, GET /api/memories, README]
tech-stack:
  added: []
  patterns: [single validation rule at the shared service seam, reuse of the client-tier InvalidArgument variant]
key-files:
  created: []
  modified:
    - crates/agent-memory-core/src/service.rs
    - crates/agent-memory-core/src/domain.rs
    - crates/agent-memory-core/tests/validation.rs
    - crates/agent-memory-core/tests/relations.rs
    - crates/agent-memory/src/mcp.rs
    - crates/agent-memory/src/rest/handlers.rs
    - crates/agent-memory/tests/tools.rs
    - crates/agent-memory/tests/rest.rs
    - README.md
    - .planning/phases/04-memory-update-relations-hybrid-search/04-REVIEW.md
    - .planning/STATE.md
key-decisions:
  - "WR-04 resolved by user decision option-a (reject): memory_list with expand_links and no limit is MemoryError::InvalidArgument (MCP invalid_params / REST 400), enforced once in MemoryService::list"
requirements: [WR-04]
metrics:
  completed: 2026-10-24
  duration: ~40 min
actuals:
  tokens: 9600
  tasks: 3
  commits: 2
---

# Quick Task 261023-v6w: Reject memory_list expand_links without a limit (WR-04) Summary

`MemoryService::list` now rejects `expand_links: true` with no `limit` using the existing `MemoryError::InvalidArgument`. The check runs before any store hop, and the message reads `invalid argument: limit is required when expand_links is true; pass a limit between 1 and 200`. Over MCP stdio that is a JSON-RPC -32602 error, and over REST it is a 400 `{"error"}`. As a result, the T-04-11 `200 x 10` expansion bound now covers every expanded response.

## Commits

| Task | Commit | Subject |
|------|--------|---------|
| 1+2 | `969e891` | fix(quick-261023-v6w): reject memory_list expand_links without a limit (WR-04) |
| 3 | `766b8f0` | docs(quick-261023-v6w): mark WR-04 resolved (option a, reject) in 04-REVIEW |

Task 1 had no commit of its own, as the plan required: until Task 2 refitted the inline tests, the library tests stayed red. `.planning/STATE.md` line 122 is edited but left uncommitted, because the orchestrator commits it.

## Gate results (before the code commit)

- `cargo fmt --all -- --check`: **clean**
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: **clean, 0 warnings**
- `cargo test --workspace --locked`: **217 passed, 0 failed, 2 ignored** across 18 test binaries (exit 0)

TDD RED was confirmed before the fix. All three new tests failed on their rejection assertion against the old code:
- the service seam returned `Ok(rows)`;
- stdio returned a `result` with no `error`;
- REST returned 200 instead of 400.

## Changes

- **service.rs:249-254**: the guard, placed after `validate_limit`, so a range error still wins for a bad limit. `MIN_LIMIT` and `MAX_LIMIT` are imported for the `format!`. The `list` doc, the inline comment and the `ListArgs.expand_links` doc now state the rule.
- **domain.rs:362-371**: the `MAX_EXPANDED_LINKS` doc is rewritten as the settled option-a contract, and the "deferred API-contract" sentence is dropped.
- **mcp.rs:300**: the tool description now says "expand_links requires a limit (1-200) ... rejected as invalid_params". The MCP `ListArgs` `limit`/`expand_links` docs are updated (`SearchArgs` is untouched). The inline test `expand_links_from_raw_args_attaches_related_on_search_and_list` now asserts the in-process invalid_params error, then lists with `limit: 10`.
- **handlers.rs**: the `ListQuery` docs and the `list_handler` doc are updated (`SearchRequest` is untouched). Two inline tests now pass `&limit=10`. A new inline test, `list_with_expand_links_and_no_limit_returns_400_never_500` (`:981`), was added.
- **tests**:
  - `validation.rs:128` `expand_links_list_without_a_limit_is_rejected_at_the_service_seam`
  - `tools.rs:487` `memory_list_with_expand_links_requires_a_limit_over_stdio`
  - `rest.rs:316` `rest_list_with_expand_links_requires_a_limit`
  - The `relations.rs` `list_args` helper now uses `limit: expand_links.then_some(MAX_LIMIT)`.
- **README.md**:
  - the `memory_list` row in the MCP tools table;
  - the `memory_list` limit sentence;
  - the Expansion rule bullet (`:363`);
  - the Bounds row (`:378`);
  - the REST `GET /api/memories` row;
  - one sentence added to the out-of-bounds paragraph;
  - `&limit=20` added to the curl example.
  - "Behavioral changes in v0.1.0" is unchanged, as the plan specified.
- Both transport mappers are unchanged: `map_mcp_error` at `mcp.rs:39` and `map_memory_error` at `handlers.rs:171`. No new `MemoryError` variant was added, and no `unwrap()` was added to production code.

## Evidence used in the 04-REVIEW.md WR-04 row

- `service.rs:249-254`: the guard
- `mcp.rs:39` and `handlers.rs:171`: the InvalidArgument arms of the two mappers
- `mcp.rs:300`: the tool description
- `domain.rs:362-371`: the `MAX_EXPANDED_LINKS` doc
- `README.md:363`: the rule bullet; `README.md:378`: the Bounds row
- the four tests named above; `.planning/STATE.md:122`; commit `969e891`

## Deviations from Plan

1. **Task 3 commit excludes STATE.md (orchestrator constraint).** The plan has the Task 3 commit cover both `04-REVIEW.md` and `STATE.md`. The orchestrator's instructions say not to commit `.planning/STATE.md`. So `766b8f0` touches only `04-REVIEW.md`, and the line-122 edit is still in the working tree for the orchestrator's docs commit. Consequently, the last check in the plan's Task 3 verify, which expects both paths in HEAD, is satisfied only once the orchestrator commits. Every other Task 3 check passes.
2. **The tracer gate was not a human checkpoint.** `workflow.auto_advance` is false, which would normally mean a `checkpoint:human-verify` after the tracer task. The orchestrator's instruction was to execute every task and stop only for a genuine human decision. The tracer `<verify>` is fully automated and passed end-to-end (validation 5, relations 27, tools 7, rest 3). Also, the plan forbids a tracer commit, so there was nothing to checkpoint on. I continued to Task 2.
3. **Scoped README rewrap.** The edited `memory_list` limit sentence was re-wrapped to keep the paragraph's line width. The text is as the plan specified.

## Known Stubs

None.

## Threat Flags

None. This change shrinks the attack surface (T-261023-01 mitigated). The rejection message is static, built only from public bounds (T-261023-02).

## Self-Check: PASSED

- All 11 modified files exist, and the 9 code/README files are in `969e891`.
- `969e891` and `766b8f0` are both on local `main`. Nothing was pushed or tagged.
- None of the user's local files were staged: `CLAUDE.md`, `.claude/`, `.mcp.json`, `.gitignore`, `10-agent-memory.md`, the harness guide, `.planning/research/.cache/` and `.planning/config.json` are still untracked or unstaged.
