# agent-memory: todo list (2026-10-24)

Source: `docs/reviews/2026-10-24-launch-plan.md`. Tracking issue [#7](https://github.com/UnityInFlow/agent-memory/issues/7).
Board: [agent-memory delivery board](https://github.com/orgs/UnityInFlow/projects/12).
Shared conventions and owner defaults: `PORTFOLIO-DECISIONS-2026-10-24.md` in the workspace root.

Working copy: this checkout is on `feat/phase-04-update-relations-hybrid-search` (= PR #6, `0f1a9fd`), which is the code v0.1.0 ships from. `origin/main` is still the v0.0.1 surface.

## Done 2026-10-24

- [x] Plan written and verified on this tree (217 tests, clippy and fmt clean)
- [x] Issues #7 to #15 created; #2 and #5 linked
- [x] GitHub topics set: `mcp`, `mcp-server`, `claude-code`, `codex`, `agent-memory`, `rust`, `sqlite`
- [x] Delivery board created and linked to the repo; all issues and PR #6 on it

## First 2 hours (do before anything else)

- [ ] Merge PR #6 into `main` (`gh pr merge 6 --squash` or merge commit; your call) ([#8](https://github.com/UnityInFlow/agent-memory/issues/8))
- [ ] Decide the crate name: default `agent-memory-mcp`; the tag's `Cargo.toml` is what gets published ([#9](https://github.com/UnityInFlow/agent-memory/issues/9))
- [ ] Bump both crates to `0.1.0`, add `version = "0.1.0"` to the two path dependencies, `cargo update -p chacha20`, commit ([#8](https://github.com/UnityInFlow/agent-memory/issues/8))
- [ ] `git tag v0.1.0 && git push origin v0.1.0`; watch the hosted release run, darwin legs are hard-required ([#8](https://github.com/UnityInFlow/agent-memory/issues/8))
- [ ] `gh release edit v0.1.0 --generate-notes` as the interim changelog ([#8](https://github.com/UnityInFlow/agent-memory/issues/8))

## v0.1.0 launch (about 17 h)

- [ ] Add a `.gitignore` with `target/` (the repo has none; a fresh clone shows `target/` as untracked after `cargo test`) ([#8](https://github.com/UnityInFlow/agent-memory/issues/8))
- [ ] Read `docs/reviews/2026-10-25-fable-analysis.md` "Pre-tag fixes": schema enums, `limit` range, `rust-version`, `deny.toml`, `cargo update` the three advisories, release profile, crate `exclude` (about 1 h, before tagging)
- [ ] `CHANGELOG.md` from `git log v0.0.1..main` and `SECURITY.md`; closes #2 ([#8](https://github.com/UnityInFlow/agent-memory/issues/8), 3 h total)
- [ ] Rename binary crate to `agent-memory-mcp`, keep `[[bin]] name = "agent-memory"`; add `readme`, `keywords`, `categories`; visible README line `mcp-name: io.github.UnityInFlow/agent-memory` ([#9](https://github.com/UnityInFlow/agent-memory/issues/9), 3 h)
- [ ] MANUAL: first `cargo publish` of `agent-memory-core` then `agent-memory-mcp` from your machine; then set up crates.io Trusted Publishing and `publish-crates.yml` ([#9](https://github.com/UnityInFlow/agent-memory/issues/9))
- [ ] MANUAL: Homebrew tap `Formula/agent-memory.rb` to 0.1.0 with the four sha256 from `SHA256SUMS.txt`; `brew audit --strict`; test upgrade from 0.0.1 migrates the DB ([#10](https://github.com/UnityInFlow/agent-memory/issues/10), 0.5 h)
- [ ] README rewrite: new top 20 lines, "Connect your runtimes" with tested Claude Code / Codex / Cursor snippets, REST and GSD import below the fold ([#11](https://github.com/UnityInFlow/agent-memory/issues/11), 3 h)
- [ ] Demo: `docs/demo/two-runtimes.tape` with VHS, GIF under 2 MB at `docs/demo/two-runtimes.gif` ([#12](https://github.com/UnityInFlow/agent-memory/issues/12), 2 h)
- [ ] `server.json` (`registryType: cargo`, identifier `agent-memory-mcp`) and `mcp-publisher publish`; `glama.json`; awesome-mcp-servers PR; mcp.so submit ([#13](https://github.com/UnityInFlow/agent-memory/issues/13), 2 h)
- [ ] r/ClaudeAI post from the draft in the plan; cross-post r/ChatGPTCoding and r/mcp on day 2 ([#15](https://github.com/UnityInFlow/agent-memory/issues/15), 2 h)
- [ ] First week: answer within 2 h, patch tags `v0.1.x`, daily log in `docs/reviews/launch-log.md` ([#15](https://github.com/UnityInFlow/agent-memory/issues/15), 30 min/day)

## Stop rule

Day 30 after the first directory listing: fewer than 25 stars and fewer than 3 external issues or PRs.
Then archive with a pointer and leave crates.io and the tap installable. Nothing below before that gate.

## Later, only if demand

- [ ] Smithery listing via MCPB bundle ([#14](https://github.com/UnityInFlow/agent-memory/issues/14), 3 h)
- [ ] v0.2.0: Phase 5 export/import, Phase 6 musl/Windows, formula auto-bump
- [ ] Show HN with a story at day 30 to 60; decay-vs-deletion post with numbers

## Open owner decisions

1. Crate name (default `agent-memory-mcp`).
2. Ship from PR #6 now, re-scope Phase 5 and 6 to v0.2.0 (default yes).
3. REST mode: keep the code, drop it from the story (default yes).
