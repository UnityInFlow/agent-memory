# agent-memory: launch plan for v0.1.0 (2026-10-24)

Tracking issue: [#7](https://github.com/UnityInFlow/agent-memory/issues/7). Steps: #8–#15. Facts below were re-verified today on this checkout; `file:line` anchors refer to the Phase 4 tree (`main` @ `0f1a9fd`).

## Where it stands

- Checkout: on `main`, **42 commits ahead of `origin/main`**, dirty (`.planning/config.json` modified; untracked `CLAUDE.md`, `.claude/`, `.mcp.json`, `.gitignore`, `10-agent-memory.md`, …). Local `main` tip `0f1a9fd` is **identical** to `origin/feat/phase-04-update-relations-hybrid-search` (PR #6, pushed 2026-10-24 07:47). `origin/main` is at `907161c` (2026-10-21). Nothing was pulled or switched for this plan.
- Tests on this tree: **217 passed / 0 failed / 2 ignored** (`cargo test --workspace`), clippy `-D warnings` clean, `cargo fmt --check` clean. The 76/1 count in the brief is `origin/main`; Phase 4 added ~141 tests (`hybrid.rs` 22, `relations.rs` 27, `update.rs` 9, `backfill_race.rs` 7, …).
- v0.0.1 → `origin/main`: 55 commits (exact tag match, 400 on out-of-range bounds, hosted-runner release pipeline with SLSA attestation: PR #3 closed #1). `origin/main` → Phase 4 branch: 42 commits, 31 code / 66 docs across the whole v0.0.1..main range of 97.
- Phase 4 adds: `memory_update` + `PATCH /api/memories/{id}`; `memory_link`/`memory_unlink` (`relates_to`, `supersedes`, `caused_by`) with cascade and `expand_links`; RRF hybrid search (`search_mode: "hybrid"`), forgiving FTS5 leg; migration 0003 with pre-migration backup and newer-schema refusal. README already has a "Behavioral changes in v0.1.0" section (`README.md:486`).
- Code: `crates/agent-memory-core` 4,250 src / 5,069 test LOC; `crates/agent-memory` 3,802 src / 1,069 test LOC. Both at `version = "0.0.1"` (`crates/*/Cargo.toml:3`).
- One release, v0.0.1 (2026-07-03), 4 tarballs + `SHA256SUMS.txt`. Tap formula `Formula/agent-memory.rb` pins `version "0.0.1"` (tap commit `aa76440`, 2026-07-03); `release.yml:25-26` says the formula is updated by hand, and there is no crates.io publish step anywhere.
- `cargo publish --dry-run`: core crate packages fine (one warning: yanked `chacha20 0.10.1` in `Cargo.lock`); binary crate fails: `agent-memory-core` path dependency has no version (`crates/agent-memory/Cargo.toml:15`).
- crates.io `agent-memory` is taken (unrelated "Self-hosted MCP memory server … HTTP-only … Vue3 UI", 0.1.0, 2026-09-25, 13 downloads). `agentmem` is taken by a competitor. Free today: `agent-memory-mcp`, `agent-memory-core`, `mcp-agent-memory`, `agentmemory`, `memsqlite`, `uif-agent-memory`.
- Repo hygiene: no `CHANGELOG.md`, no `SECURITY.md` (#2), no `server.json`, no `glama.json`. GitHub: 0 stars, 0 forks, no topics, 2 open issues (#2, #5), 1 open PR (#6).
- Portfolio docs: `00-MASTER-ANALYSIS.md:160` rates agent-memory 7/10 complexity, 9/10 value, and `:289` schedules "agent-memory v0.1: Rust binary + MCP server" for months 5–7. `PORTFOLIO-VERDICT.md:48` puts it in Tier 2 "harvest, then archive" into codememory. **This plan agrees with the MASTER scope (v0.1 = binary + MCP server) and disagrees with the VERDICT's immediate archive: but only conditionally:** the launch is ~17 hours of work and is framed as a 30-day demand test; if it fails the gate in "What comes after", follow the VERDICT. A close competitor already listed on awesome-mcp-servers (`timgordontg/engrim`: "cross-model, local-first … Claude Code, Cursor, Codex … SQLite FTS5 + embeddings via RRF; stdio MCP") shows both that the thesis has demand and that the window is now.

## Launch goal and definition of done

Ship **v0.1.0** publicly and get it in front of MCP users through the directories. Done when:

1. GitHub release v0.1.0 built from `main` **after PR #6 merges**, with `CHANGELOG.md` and `SECURITY.md` on `main`.
2. crates.io: `agent-memory-core` 0.1.0 and the binary crate under its new name, published from CI.
3. Homebrew tap formula at 0.1.0; `brew install unityinflow/tap/agent-memory` installs it.
4. README: 60-second quick start for Claude Code + Codex sharing one DB; 20 s demo GIF at the top.
5. Listed in ≥ 3 directories (official MCP Registry, Glama, awesome-mcp-servers, mcp.so).
6. One r/ClaudeAI post live, first-week response routine running.

## Depends on / unblocks

Depends on: PR #6 merged (the local `main` already is that branch). Unblocks nothing in the portfolio;
`codememory` is parked and does not consume this. Shared conventions (hosted runners, trusted
publishing, tap by hand, VHS, README top, MCP directories): `PORTFOLIO-DECISIONS-2026-10-24.md` in
the workspace root, "Shared conventions".

## First 2 hours (do before anything else)

1. Merge PR #6. Bump both crates to `0.1.0`, add `version = "0.1.0"` to the two path dependencies,
   `cargo update -p chacha20`, commit.
2. `git tag v0.1.0 && git push origin v0.1.0`; the hosted release pipeline builds the four tarballs.
   Use `gh release edit v0.1.0 --generate-notes` as the changelog for now; the hand-written
   `CHANGELOG.md` and `SECURITY.md` come in step 1 proper.
3. Set GitHub topics (`mcp`, `mcp-server`, `claude-code`, `codex`, `agent-memory`, `rust`, `sqlite`).

**Stop rule.** Day 30 after the first directory listing: fewer than 25 stars and fewer than 3 external
issues or PRs. Then follow `PORTFOLIO-VERDICT.md:48`: archive with a pointer, leave crates.io and the
tap installable. No Phase 5 or 6 work before that gate.

## Step-by-step plan

### 1. Decide branch scope and cut v0.1.0: #8: ~3 h
- **What:** merge PR #6, bump both crates to 0.1.0, write CHANGELOG + SECURITY.md, tag.
- **Why:** the Phase 4 branch is the real product (217 tests, 28/28 re-verification per #5); `origin/main` still has the 76-test v0.0.1-era surface. Waiting for Phase 5 (JSONL export) and Phase 6 (musl/Windows) per `.planning/ROADMAP.md:31-32` would push the launch months out for features no directory visitor will ask about first.
- **How:** merge #6 → `version = "0.1.0"` in `crates/agent-memory/Cargo.toml:3` and `crates/agent-memory-core/Cargo.toml:3` → `cargo update -p chacha20` → `CHANGELOG.md` from `git log v0.0.1..main` (headline entries listed in #8) → `SECURITY.md` (GitHub Security Advisories; REST is loopback-only by design) → do step 2 first → `git tag v0.1.0 && git push origin v0.1.0` (`release.yml:65-66` only matches `vX.Y.Z`). Watch the run: first darwin zigbuild from an x86_64 hosted runner; darwin legs are hard-required.
- **Done when:** `gh release view v0.1.0` shows the 4 required tarballs + `SHA256SUMS.txt`, `gh attestation verify` passes, CHANGELOG/SECURITY on `main`. Then close #2.

### 2. Naming and crates.io publish via CI: #9: ~3 h
- **What:** rename the binary crate to **`agent-memory-mcp`** (recommended; alternatives in #9), keep `agent-memory-core`, publish both from a tag-triggered workflow.
- **Why:** the name is gone; crates.io is also what the official MCP Registry lists (`registryType: cargo` is supported, crates.io only).
- **How:** `[package] name = "agent-memory-mcp"` (`Cargo.toml:2`); the executable stays `agent-memory` because `[[bin]] name = "agent-memory"` is already declared (`Cargo.toml:10-11`): `cargo install agent-memory-mcp` yields `agent-memory`, tarballs/brew unchanged. Add `version = "0.1.0"` to the path deps (lines 15 and 30), `readme`, `keywords`, `categories`. Add the visible README line `mcp-name: io.github.UnityInFlow/agent-memory` (crates.io strips HTML comments). Set up crates.io Trusted Publishing (GitHub OIDC) and a separate `publish-crates.yml` (`id-token: write`, `rust-lang/crates-io-auth-action`, publish core then binary) so `release.yml`'s "no org secrets" policy (`release.yml:20-40`) stays intact. First publish of each crate is manual from the owner's machine.
- **Done when:** both crates at 0.1.0 on crates.io; `cargo install agent-memory-mcp && agent-memory --version` → 0.1.0.

### 3. Homebrew formula bump: #10: ~0.5 h
- **What/How:** after the release run, copy the four sha256 from `SHA256SUMS.txt` into `Formula/agent-memory.rb`, `version "0.1.0"`, `brew audit --strict`, install on a clean machine, confirm `brew upgrade` from 0.0.1 migrates the DB (0003 with backup).
- **Why:** the formula is the headline install path in the README (`README.md:33`) and would otherwise install the 4-month-old binary.
- **Done when:** fresh `brew install unityinflow/tap/agent-memory` prints 0.1.0.

### 4. README rewrite: #11 (text) and #12 (demo): ~3 h + ~2 h
- **What:** replace `README.md:1-27` with the block below; add a "Connect your runtimes" section with tested Claude Code / Codex / Cursor snippets (today only a generic `.mcp.json` exists at `README.md:124-144`); move REST (`:414`) and GSD import (`:513`) below the fold or to `docs/`; embed the GIF.
- **Why:** line 5 ("Tool 10 in the UnityInFlow ecosystem") is noise to strangers; Install comes before any proof; directories scrape the README top.

Proposed top 20 lines:

````markdown
# agent-memory

**One memory file for all your coding agents.** Claude Code, Codex, Cursor: any MCP
host: store and recall typed memories in a local SQLite database. No cloud, no account,
works offline.

![Claude Code stores a decision, Codex recalls it](docs/demo/two-runtimes.gif)

```bash
brew install unityinflow/tap/agent-memory
claude mcp add --scope user agent-memory --env AGENT_MEMORY_DB=~/.agent-memory/memory.db -- agent-memory serve
codex  mcp add               agent-memory --env AGENT_MEMORY_DB=~/.agent-memory/memory.db -- agent-memory serve
```

Then tell Claude Code "remember: we chose SQLite over Postgres because this must run
offline", and ask Codex "why SQLite?". Same file, same memory.

- **Typed, not a text soup.** `DECISION`, `PATTERN`, `ERROR`, `TODO`, `ARCHITECTURE`, `CONSTRAINT`.
- **Decay, not deletion.** Old memories fade in ranking; only TTL or `memory_forget` removes them.
- **Relations.** `supersedes`, `caused_by`, `relates_to`: follow them with `expand_links`.
- **Search by meaning and keyword.** Hybrid RRF when local Ollama is up; FTS5 when it isn't.
- **Single static binary.** Rust, embedded SQLite + sqlite-vec, ~zero dependencies.
````

Two-runtimes demo script (`docs/demo/two-runtimes.tape`, VHS; details in #12):
1. `export AGENT_MEMORY_DB=~/.agent-memory/demo.db`
2. Pane 1, Claude Code: "Remember: we chose SQLite WAL mode over Postgres for the memory store because it must run offline." → `memory_store` (type `DECISION`) appears.
3. Pane 2, Codex: "Why did we pick SQLite for memory?" → `memory_search` returns the DECISION, `search_mode: "keyword"` (no Ollama) or `"hybrid"`.
4. End card: `brew install unityinflow/tap/agent-memory`. Keep REST out of the demo.

- **Done when:** a new reader reaches "stored in Claude Code, recalled in Codex" from the top screen alone; snippets verified against current CLI versions; `docs/demo/two-runtimes.gif` < 2 MB, re-recordable.

### 5. Directory submissions: #13 (core four): ~2 h
Order: crates.io (#9) → official registry → Glama + awesome PR + mcp.so same day. Smithery (#14) is in
"Later", not here.

| Directory | URL | Required | Steps |
|---|---|---|---|
| Official MCP Registry | registry.modelcontextprotocol.io | `server.json` (name `io.github.UnityInFlow/agent-memory`, `packages[0].registryType: "cargo"`, `identifier: "agent-memory-mcp"`, stdio transport, `AGENT_MEMORY_DB` env); crate README containing visible `mcp-name: io.github.UnityInFlow/agent-memory` | `brew install mcp-publisher` → `mcp-publisher init` → edit → `login github` → `validate` → `publish`. PulseMCP ingests this automatically (its own submissions are closed). |
| Glama | glama.ai/mcp/servers | `glama.json`: `{"$schema":"https://glama.ai/mcp/schemas/server.json","maintainers":["hermanngeorge15"]}` (schema's only required field) | Commit file; use "Add Server" on the listing if not auto-indexed; add score badge to README. |
| awesome-mcp-servers | github.com/punkpeye/awesome-mcp-servers | One line under `### 🧠 Knowledge & Memory`, alphabetical, `🦀 🏠 🍎 🐧` | Fork → edit README → PR (agent PRs fast-tracked with `🤖🤖🤖` in title). Entry text in #13. |
| mcp.so | mcp.so/submit | Repo URL only; it scrapes README `.mcp.json` | Paste URL, review auto-filled fields, submit; live after review. |

- **Done when:** visible on registry, Glama (claimed), awesome list (PR merged), mcp.so.

### 6. Post drafts: #15: ~2 h
**r/ClaudeAI title:** *I built a tiny Rust MCP server so Claude Code and Codex share one memory file (local SQLite, typed memories, decay instead of deletion)*

**Body (draft):**
> Every agent I use forgets everything between sessions, and each one that does remember keeps its own private notes. I wanted one memory both Claude Code and Codex read and write.
>
> agent-memory is a single-binary MCP stdio server over an embedded SQLite file. Point every runtime at the same `AGENT_MEMORY_DB` and a decision recorded in Claude Code is recalled in Codex a minute later. [GIF]
>
> What's different from mem0 / the official memory server:
> - memories are **typed** (DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT), so recall is structured;
> - **decay instead of deletion**: stale memories sink in ranking but nothing disappears unless you say so or a TTL expires;
> - typed **relations** (`supersedes`, `caused_by`) so a superseded decision is still findable but ranked below its successor;
> - **local and offline**: FTS5 keyword search always works; if Ollama is running you get hybrid keyword+semantic (RRF);
> - Rust, `brew install unityinflow/tap/agent-memory`, MIT.
>
> Install (3 lines) … Honest limits: no Windows binary yet (`cargo install` works), REST mirror is loopback-only and unauthenticated by design, young project: 217 tests but few users. What would make you point your agents at it?

**HN: later, not now.** HN is a one-shot; a Show HN with 0 stars, no demo and no external users gets buried, and you cannot repost the same project. Do it at day 30–60 with a story ("what 30 days of shared Claude Code + Codex memory taught me", or a decay-vs-deletion recall benchmark) once directory traffic has shaken out the first bugs.

### 7. First-week response plan: #15: ~30 min/day
- Pre-flight: #8, #10, #11, #12 done and at least the official registry listing (#13) live. Post Tue–Thu 14:00–16:00 CET.
- Day 0–1: answer every comment within 2 h; reproduce bugs same day; ship `v0.1.x` patch tags (pipeline ≈ 20 min).
- Day 2: shorter cross-posts to r/ChatGPTCoding (Codex users) and r/mcp.
- Day 3–7: turn "what would make you use it" into `enhancement` issues; one follow-up comment with what shipped.
- Log daily in `docs/reviews/launch-log.md`: stars, release asset downloads (`gh api repos/UnityInFlow/agent-memory/releases`), crates.io downloads, registry/Glama views, external issues.

**Total effort:** ≈ 17 h core (#8 3, #9 3, #10 0.5, #11 3, #12 2, #13 2, #15 2 + follow-up). Smithery (#14, 3 h) is gated below.

## Decisions the owner must make

Defaults are recorded in `PORTFOLIO-DECISIONS-2026-10-24.md`, "Owner decisions"; shared ones are S1 to S5 there.

1. **Crate name.** Recommended `agent-memory-mcp` (free, descriptive, binary stays `agent-memory`). Alternatives free today: `mcp-agent-memory`, `agentmemory`, `memsqlite`, `uif-agent-memory`. Avoid `agentmem` (taken by a competitor). Decide before tagging: the tag's `Cargo.toml` is what gets published.
2. **Wait for the hybrid-search branch or not.** It is not a wait: the branch *is* the local `main` and PR #6 is open. Recommendation: merge #6 and ship v0.1.0 from it; re-scope Phase 5 (export/import) and Phase 6 (musl/Windows) to v0.2.0 and update `.planning/ROADMAP.md:6,31-32` accordingly. The only reason to delay is if the first hosted-runner darwin build fails: fix forward, don't fall back to v0.0.1 code.
3. **REST mode in the launch story.** Recommendation: **keep the code, drop it from the story.** It is unauthenticated (`README.md:478-485`), the squatter crate's pitch is literally "HTTP-only", and it dilutes the "one MCP server, one file" message. One line under "Also" linking to `docs/rest.md`; keep `serve-rest` tested.
## Later, only if demand (gated on the day-30 stop rule)

- **Day 30 gate passed** (≥ 25 stars or ≥ 3 external issues/PRs): commit to v0.2.0 (Phase 5 export/import, Phase 6 musl/Windows, formula auto-bump via a tap-scoped PAT in a separate workflow); Smithery via an MCPB bundle (#14: `manifest.json`, `server.type: "binary"`, `smithery mcp publish`, reusable as a second `packages[]` entry in `server.json`).
- **Day 30 to 60:** Show HN with a story; a short blog post on decay-vs-deletion with numbers from the hybrid test-suite.
- Gate failed: harvest the store/decay/hybrid code into codememory only if codememory itself is revived (it is parked), otherwise archive with a pointer.

## Issue index

| # | Title | Step | Effort |
|---|---|---|---|
| [#7](https://github.com/UnityInFlow/agent-memory/issues/7) | Launch v0.1.0: release, rename crate, directories (2026-10-24 plan) | tracking | n/a |
| [#8](https://github.com/UnityInFlow/agent-memory/issues/8) | Merge Phase 4, add CHANGELOG + SECURITY.md, cut v0.1.0 | 1 | 3 h |
| [#9](https://github.com/UnityInFlow/agent-memory/issues/9) | Rename binary crate to agent-memory-mcp, publish to crates.io from CI | 2 | 3 h |
| [#10](https://github.com/UnityInFlow/agent-memory/issues/10) | Bump Homebrew tap formula to v0.1.0 | 3 | 0.5 h |
| [#11](https://github.com/UnityInFlow/agent-memory/issues/11) | README rewrite: quick start, Claude Code / Codex / Cursor snippets | 4 | 3 h |
| [#12](https://github.com/UnityInFlow/agent-memory/issues/12) | 20-second two-runtimes demo recording (VHS tape) | 4b | 2 h |
| [#13](https://github.com/UnityInFlow/agent-memory/issues/13) | Directory submissions: MCP Registry, Glama, awesome-mcp-servers, mcp.so | 5 | 2 h |
| [#14](https://github.com/UnityInFlow/agent-memory/issues/14) | Smithery listing via MCPB bundle | later, gated | 3 h |
| [#15](https://github.com/UnityInFlow/agent-memory/issues/15) | r/ClaudeAI post draft and first-week response plan | 6–7 | 2 h + 30 min/day |
| [#2](https://github.com/UnityInFlow/agent-memory/issues/2) | Repo hygiene: SECURITY.md, CHANGELOG.md (existing; closed by #8) | 1 | n/a |
| [#5](https://github.com/UnityInFlow/agent-memory/issues/5) / [PR #6](https://github.com/UnityInFlow/agent-memory/pull/6) | Phase 4 tracking / branch (existing; merge is the first action of #8) | 1 | n/a |
