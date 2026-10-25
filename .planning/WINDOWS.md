---
schema_version: 1
open_count: 1
waived_count: 0
fixed_count: 0
total_count: 1
last_updated: 2026-10-25T12:22:24.215Z
---

# Broken Windows Ledger

> Cross-phase defect register. With `workflow.windows_enforce` enabled, `/gsd-ship` blocks while `open_count > 0`.
> Waive with `gsd-tools windows waive <id> "<reason>"` (reason required).
> Mark fixed with `gsd-tools windows fixed <id>`.

| id | phase | kind | file | line | description | status | reason | recorded_at | resolved_at |
|----|-------|------|------|------|-------------|--------|--------|-------------|-------------|
| 1 | 05 | deviation | crates/agent-memory-core/src/import/jsonl.rs |  | 05-03: D-11 message wording differs from the plan's interface contract in three recorded ways (Option probe fields, serde position suffix stripped, InvalidArgument payload lifted) — documented in 05-03-SUMMARY.md | open |  | 2026-10-25T12:22:24.215Z |  |

````json
[
  {
    "id": 1,
    "kind": "deviation",
    "phase": "05",
    "file": "crates/agent-memory-core/src/import/jsonl.rs",
    "line": null,
    "description": "05-03: D-11 message wording differs from the plan's interface contract in three recorded ways (Option probe fields, serde position suffix stripped, InvalidArgument payload lifted) — documented in 05-03-SUMMARY.md",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-10-25T12:22:24.215Z",
    "resolved_at": null
  }
]
````
