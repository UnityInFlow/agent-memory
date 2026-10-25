# agent-memory: Fable analysis (2026-10-25)

Author: Claude Fable 5.1 (model id claude-fable-5-1), working with the owner in Claude Code.
Status: opinion document. A second opinion from Claude Opus is requested; the prompt for it is in the appendix.
Inputs: repo at 0f1a9fd (PR #6 branch), the launch plan `docs/reviews/2026-10-24-launch-plan.md`, `docs/TODO.md`, web research on 2026-10-25 (links inline).

## Summary in ten lines

1. Verdict: ship v0.1.0 from PR #6 this week as planned; the code is in better shape than the repo's zero stars suggest (217 tests, clippy clean, no audit vulnerabilities, migration with backup).
2. Killer risk: the hosts now remember on their own. Claude Code ships auto memory by default (typed notes, per repository, `~/.claude/projects/<project>/memory/`, https://code.claude.com/docs/en/memory) and Codex has `~/.codex/memory/` with `/m_update` and `/m_drop` (https://codex.danielvaughan.com/2026/04/10/codex-cli-memory-extensions-cross-session-persistence/). "Agents forget" is no longer the problem statement.
3. What survives: neither native store is searchable by query, shared across tools, or portable; both are per-repo flat Markdown. A shared, typed, decaying, queryable store that any MCP host reads is still unoccupied in Rust and almost unoccupied as a product (engrim is the closest, 299 stars, Python).
4. The category is crowded in count and thin in quality: 138 entries under "Knowledge & Memory" on awesome-mcp-servers (counted today), most with under 10 stars; 2026 Show HN posts about MCP memory servers mostly scored 1 to 8 points.
5. The pitch must change from "your agents forget" to "your agents each remember in private; this is the one they share", and auto memory should be treated as a feeder (import it), not a competitor.
6. Blocking defects for launch: none. Pre-launch fixes worth one hour: JSON schema enums for `type` and `kind`, a range on `limit`, `cargo-deny` with the advisory list, `rust-version`.
7. Real risks after launch: unbounded `memory_list` without `limit` (service.rs:242-258), one full-table `UPDATE` per hour per process (sqlite.rs:1257-1264), flat KNN in sqlite-vec 0.1.9 at 100k rows (about 300 MB of vectors scanned per query).
8. Stop rule: I disagree with stars as the gate. Count external issues or PRs and install evidence (release downloads, crates.io downloads); stars in this category are noise.
9. Discipline: 65 of 97 commits since v0.0.1 are docs-prefixed. Squash-merge PR #6, then hold docs to one commit per release.
10. Day 30 question is not "did it get stars" but "did one stranger point two tools at one file and come back". That is what the metrics below measure.

## Where it stands

Verified today on this checkout (`0f1a9fd`, branch `feat/phase-04-update-relations-hybrid-search`):

| Fact | Evidence |
|---|---|
| Tests | `cargo test --workspace`: 217 passed, 0 failed, 2 ignored (`semantic.rs:321` live Ollama, `migration_hygiene.rs:128` fixture generator) |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings`: clean |
| Audit | `cargo audit`: 0 vulnerabilities, 3 warnings: `anyhow 1.0.102` unsound (RUSTSEC-2026-0190), `tokio 1.52.3` unsound (RUSTSEC-2026-0386), `chacha20 0.10.1` yanked; 202 crates in `Cargo.lock` |
| Size | 14,190 lines of Rust, of which 8,052 in `src/` (core 4,250, binary 3,802) |
| Tools | 7 MCP tools: `memory_store`, `memory_list`, `memory_search`, `memory_forget`, `memory_update`, `memory_link`, `memory_unlink` (`crates/agent-memory/src/mcp.rs:274,299,332,368,394,420,458`) |
| PR #6 | OPEN, MERGEABLE, no review, 42 commits, +13,184 / -302, 43 files |
| GitHub | 0 stars, 0 forks, topics set, 12 open issues including PR #6 |
| Release pipeline | `release.yml`: tag filter `vX.Y.Z` only (lines 3-12), cargo-zigbuild from one hosted Linux runner, SLSA attestation (303-308), no org secrets (19-40), darwin legs hard-required (51-58) |
| CI | `ci.yml`: fmt, clippy `-D warnings`, build, test, `cargo llvm-cov --fail-under-lines 80` |
| Crate name | both crates still `version = "0.0.1"`, `agent-memory` taken on crates.io (plan), default rename `agent-memory-mcp`; path dependency at `crates/agent-memory/Cargo.toml:15` has no `version`, so `cargo publish` of the binary fails until fixed |
| Tap | `UnityInFlow/homebrew-tap` `Formula/agent-memory.rb` pins `version "0.0.1"`, last tap commit `bfacf54` 2026-07-18 |
| Missing files | `CHANGELOG.md`, `SECURITY.md`, `server.json`, `glama.json`, `deny.toml`, `rust-version` in any `Cargo.toml`, `rust-toolchain.toml` |
| Commit mix | 97 commits since `v0.0.1`: 65 `docs`, 12 `feat`, 8 `fix`, 6 `test`, 4 `ci`, 2 `chore` |

The MCP SDK is `rmcp 1.8.0` (`Cargo.lock:1174`), the official Rust SDK. Embeddings are pinned to `nomic-embed-text` at 768 dimensions (`crates/agent-memory-core/src/embed/ollama.rs:16`, `embed/mod.rs:20`, `sql/0002_embeddings.sql` `FLOAT[768]`, `meta` table rows).

## Market and alternatives

| Project | Stars / last release | Storage | Typed | Decay | Multi-client | Local-first | Licence | Evidence |
|---|---|---|---|---|---|---|---|---|
| Claude Code auto memory (native) | n/a, on by default | Markdown files per repo, `MEMORY.md` index, first 200 lines or 25 KB loaded | yes: `user`, `feedback`, `project`, `reference` | no | no, Claude Code only, machine-local | yes | n/a | https://code.claude.com/docs/en/memory |
| Codex CLI memory (native) | n/a | `~/.codex/memory/` flat files, `/m_update`, `/m_drop`, usage-aware selection v0.106.0+ | no | no | no | yes | n/a | https://codex.danielvaughan.com/2026/04/10/codex-cli-memory-extensions-cross-session-persistence/ (third party; OpenAI changelog not found) |
| Cursor Memories (native) | removed in 2.1 per third party; docs URL now redirects to the docs root | n/a | n/a | n/a | n/a | n/a | n/a | https://www.memorylake.ai/blogs/cursor-forgets-previous-sessions, https://cursor.com/docs/context/memories (partially unverified) |
| `@modelcontextprotocol/server-memory` | repo 91.5k, v2.1.0 2026-10-25 | one JSONL knowledge-graph file | entity/observation only | no | any MCP host | yes | MIT | https://github.com/modelcontextprotocol/servers; growth complaint https://glama.ai/mcp/servers/@bneil/mcp-memory-pouchdb/inspect |
| mem0 / OpenMemory MCP | 67.8k, ts-v3.3.1 2026-09-25 | vector DB plus LLM extraction (Docker, Qdrant, LLM key; unverified today, both README URLs 404) | no | no | yes | partial | Apache-2.0 | https://github.com/mem0ai/mem0; dedup races #6515, #7123, compaction request #5850 |
| timgordontg/engrim | 299, v1.4.11 2026-09-22 | SQLite WAL, FTS5 plus model2vec, `~/.engrim/memory.db` | yes: decision, fact, feedback, state, user, reference | no (supersede, never erase) | Claude Code hooks, Cursor MCP, Codex hooks, 5 more | yes | MIT | https://github.com/timgordontg/engrim |
| thedotmack/claude-mem | 102.9k, v15.0.2 2026-10-23 | SQLite plus Chroma, hook capture | observation types | no | Claude Code, Codex, Copilot, OpenCode, more | needs an LLM provider (hosted observer trial, own key, or Anthropic plan) | Apache-2.0 | https://github.com/thedotmack/claude-mem |
| basicmachines-co/basic-memory | 4.3k, v0.24.1 2026-10-23 | Markdown files plus SQLite index | bracketed observation categories, typed relations | no | Claude, Codex, Cursor, VS Code, Obsidian | yes, cloud optional at 15 USD/mo | AGPL-3.0 | https://github.com/basicmachines-co/basic-memory |
| getzep/graphiti MCP | 32.2k, v0.31.2 2026-10-20 | FalkorDB or Neo4j, LLM key | entities/facts | bi-temporal `valid_at`/`invalid_at`, no decay | any host | no | Apache-2.0 | https://github.com/getzep/graphiti/tree/main/mcp_server |
| topoteretes/cognee MCP | 32.7k, v1.6.6rc1 2026-10-23 | SQLite, LanceDB, Ladybug locally; LLM key or MCP sampling, embeddings provider | no | no | any host | partial | Apache-2.0 | https://github.com/topoteretes/cognee/tree/main/cognee-mcp |
| letta-ai/letta | 25.2k, 0.16.8 2026-05-14 | server platform, DB backed | memory blocks | no | via API/MCP | server, cloud-first (unverified detail) | Apache-2.0 | https://github.com/letta-ai/letta |
| doobidoo/mcp-memory-service | 2.0k, v12.3.1 2026-10-23 | sqlite-vec or Cloudflare | tags | consolidation scripts | Claude Code and pipelines | yes | Apache-2.0 | https://github.com/doobidoo/mcp-memory-service |

Category context: punkpeye/awesome-mcp-servers "Knowledge & Memory" holds 138 entries today (https://github.com/punkpeye/awesome-mcp-servers); sampled new entries have 0 to 10 stars (whitemagic 0, compound-memory 1, seamless 6, genome 10). Only one is Rust. On HN, 2026 "Show HN" posts about MCP memory servers scored 1 to 8 points with two outliers at 70 (OKF plus SQLite FTS5, https://news.ycombinator.com/item?id=49286073) and 73 (a Rust MCP server, https://news.ycombinator.com/item?id=49292584); data from the Algolia HN API.

The wedge is the intersection nobody else ships: typed memories with a closed enum, exponential decay that re-ranks but never deletes, typed relations with `supersedes` and `caused_by`, one SQLite file many hosts open concurrently, full offline operation with FTS5 and an optional local embedder, a single static binary. engrim is the closest competitor: same SQLite plus hybrid idea, same hosts, better client coverage through hooks, no decay, Python runtime. The honest comparison is with engrim and basic-memory, not with mem0 or claude-mem, whose scale comes from LLM-driven capture that this project deliberately does not do.

## Design

Tool surface. Names are consistent and the descriptions are unusually good (`mcp.rs:333` explains hybrid, fallback, FTS5 retry and the filter-after-KNN caveat). Three schema gaps: `type` is a `String` (`mcp.rs:68`) and `kind` is a `String` (`mcp.rs:233,248`), so schema-driven clients are not told the closed vocabulary; `limit` on `SearchArgs` has no range in its doc (`mcp.rs:131-133`) while `ListArgs` does. Fix with enums at the transport, keeping the service seam unchanged:

```rust
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum WireType { Decision, Pattern, Error, Todo, Architecture, Constraint }
/// Cap the number of returned rows (1-200). Omitted means 50.
#[schemars(range(min = 1, max = 200))]
pub limit: Option<i64>,
```

Result shapes. `memory_search` returns `{search_mode, results}` as a single text content block (`mcp.rs:363-365`); fine for today's hosts. Add `total_candidates` so an agent can tell "nothing matched" from "filtered to zero after the 4x KNN cut". There is no pagination anywhere; with `MAX_LIMIT = 200` (`domain.rs:346`) that is acceptable for search, not for `memory_list` without `limit`, which returns every row (`service.rs:242-258`); 100k rows is a 37 MB JSON answer (measured below). Default `limit` to 50 on list in 0.1.0 and document the change. `expand_links` is well bounded (`MAX_EXPANDED_LINKS = 10`, `domain.rs:371`; snippet 160 chars, `domain.rs:227`).

Memory types. Six is the right count and the pinned set (`DECISION`, `ARCHITECTURE`, `CONSTRAINT`, `domain.rs:29-34`) is sensible. Missing: a `PREFERENCE` or `USER` type. Both native stores lead with user preferences (Claude Code `user`/`feedback`, engrim `user`/`feedback`), which is what agents most want to recall across tools. Adding a variant is additive on the wire; do it in 0.1.0 before the enum is public.

Decay. `score = exp(-ln2 * elapsed / half_life)` with a 30-day half-life and 6x for pinned (`decay.rs:16,20,42-51`) is clean and tested. Two problems: `last_accessed` is bumped on every search hit (`service.rs:561-571`), so a memory that keeps appearing in results never decays even if it is never useful, and there is no feedback path ("this was wrong"). Cheap fix: bump only the top 5 returned ids, and let `memory_update` with an empty patch count as a confirmation.

Hybrid ranking. RRF with `k = 60` (`service.rs:119`), both legs oversampled to `limit * 4` capped at 800 (`sqlite.rs:44,655,879`), then `rrf * decay` (`service.rs:514-528`). Multiplying a score in `[2/61, 1/260]` by decay in `(0, 1]` works but is arbitrary: a 60-day-old exact hit (decay 0.25) ranks below a fresh paraphrase hit. Expose `RankWeights` (`decay.rs:64-78`) through `--half-life-days` and `--decay-weight` CLI flags; the structs exist, the flags do not. Also the FTS retry turns `don't` into `"don" OR "t"` (`sqlite.rs:1701`); drop tokens shorter than two characters.

Migrations. Append-only chain, `user_version` probe, pre-migration backup through the Online Backup API with a verify-then-rename step and concurrent-upgrade tolerance (`sqlite.rs:224-335`); a v0.0.1 fixture is diffed in `migration_hygiene.rs`. This is the best part of the codebase and the thing to brag about in the CHANGELOG.

REST mirror. Keep the code, drop it from the story (agree with the plan). Beyond that: mark `serve-rest` deprecated in `CHANGELOG.md` and move it behind a Cargo feature in 0.2.0, which also removes axum from the default build (1,808 lines in `rest/handlers.rs` is 45 percent of the binary crate's source).

DB path and locking. Default `<data_dir>/agent-memory/memory.db` with mode 0700 (`config.rs:24-47`) is right; the README's `~/.agent-memory/memory.db` example contradicts it, pick one. Concurrency: WAL, `busy_timeout 5000`, `synchronous NORMAL`, `foreign_keys ON` on every connection (`sqlite.rs:89-93`), one writer `Mutex` per process, r2d2 read pool at its default size of 10 (`sqlite.rs:431-434`). Two agents writing at once: the second process waits up to 5 s for the WAL write lock, then receives `SQLITE_BUSY`, which surfaces as `internal_error`. Realistic writes take milliseconds, so this only bites during the hourly sweep's full-table `UPDATE` (`sqlite.rs:1257-1264`, interval `main.rs:34`), which at 100k rows holds the lock for about 180 ms per process per hour. Acceptable, but every MCP process runs its own sweep, so N hosts means N sweeps; elect one with `BEGIN IMMEDIATE` plus a `meta` row `last_sweep_at` and skip when another process swept in the last 50 minutes.

## Improve

Code smells and risks, with locations:

- `service.rs:242-258`: `memory_list` without `limit` is unbounded. Default to 50.
- `service.rs:561-571`: fire-and-forget `bump_access` on every search hit means each read is also a write; two agents searching concurrently serialise on the writer lock. Batch bumps per process and flush every few seconds, or bump only the top results.
- `sqlite.rs:1257-1264`: full-table rescore each hour in every process; see the election proposal above. Alternatively compute decay on read only (already done at `service.rs:358-361`) and drop materialisation, which exists only to serve `ORDER BY decay_score` paths that the search no longer uses.
- `sqlite.rs:655-673`: KNN filters (`type`, `scope`, `tag`) are applied after the `k = limit*4` cut, so a filtered semantic query over a large corpus can return nothing while matches exist. sqlite-vec 0.1.9 has no metadata pre-filter; the mitigation is to raise `k` when a filter is present (for example `limit * 16`).
- `embed/ollama.rs:16,20`: the model name is a compile-time constant and the 10 s timeout is per batch of up to 128 (`service.rs:749`). A cold Ollama can take longer than 10 s to load a model; the first backfill then fails and retries next sweep, fine, but a search during that window degrades to keyword silently. Log the mode switch once per process at info level.
- `mcp.rs:68,233`: strings where enums belong (above).
- `Cargo.toml` (workspace): no `rust-version`, no `[profile.release]` with `lto`, `codegen-units = 1`, `strip = true`; the static binary could be smaller.
- `crates/agent-memory/Cargo.toml:15`: path dependency without `version`, blocks publish.
- Both crates lack `exclude`, so a publish ships `.planning/` if it is ever committed to the crate directory (it is not today, but the portfolio convention asks for the guard).

Missing tests (what exists is strong on contract, weak on environment):

- Concurrency: the only multi-process test is the concurrent first open (`migration_hygiene.rs:387`). Add two processes (or two `SqliteStore` instances) interleaving `store`, `search`, `update` and `sweep` for 10 s with no `SQLITE_BUSY` surfacing.
- Corrupt DB: no test opens a truncated file, a non-SQLite file, or a DB whose `vec_memories` table is missing (the smoke query at `sqlite.rs:426-428` would error, but the error text is untested).
- Ollama down mid-query: `fallback.rs` covers a failing embedder; add a slow embedder that exceeds `REQUEST_TIMEOUT` and assert `search_mode: "keyword"` within 11 s.
- Scale: nothing above a few hundred rows. Add an ignored 10k-row test that asserts search under 100 ms and sweep under 200 ms.
- Migration from 0.0.1 data is covered by the fixture; add the upgrade path through the Homebrew binary once 0.1.0 exists (manual, in the release checklist).

Performance, measured today with a Python sqlite3 script in the scratchpad (FTS5 plus decay query, no sqlite-vec available to Python, so vectors are estimated):

| Rows | FTS5 plus decay query | `materialize_decay` UPDATE | list without limit | DB size | Vectors (f32[768]) |
|---|---|---|---|---|---|
| 10,000 | 8 ms | 24 ms | 19 ms | 4 MB | plus 31 MB |
| 100,000 | 80 ms | 180 ms | 200 ms | 37 MB | plus 307 MB |

sqlite-vec 0.1.9 is flat brute-force KNN (https://d-central.tech/self-hosted-vector-databases/; releases https://github.com/asg017/sqlite-vec/releases), so at 100k rows every semantic query scans about 300 MB; expect 100 to 400 ms on a laptop (estimate, not measured). At 10k everything is comfortably interactive. A realistic single developer accumulates 1k to 10k memories per year, so the engine choice holds for v0.1 and v0.2. Watch sqlite-vec maintenance: last stable 2026-03-31, last push 2026-05-18, ANN only in 0.1.10 alphas. If it stalls, the swap candidates are usearch (HNSW, Rust bindings) or LanceDB (hybrid search built in, heavier); neither is needed before 50k rows.

## Build

What is right: a two-crate workspace with the service seam in `agent-memory-core`, pinned action SHAs in both workflows, the zigbuild matrix with hard-required darwin legs, SLSA attestation, a tag filter that excludes milestone tags, no org secrets, 80 percent line coverage gate.

What to add before the tag:

1. `rust-version = "1.85"` (or whatever `cargo msrv` reports) in `[workspace.package]`; CI job on that toolchain.
2. `deny.toml` with `advisories.yanked = "deny"`, `advisories.unsound = "warn"`, licence allow-list (MIT, Apache-2.0, BSD-3, ISC, Unicode-3.0, Zlib), bans on duplicate `libsqlite3-sys`; `cargo deny check` in `ci.yml` (`cargo-deny` is installed locally, `cargo-semver-checks` and `release-plz` are not).
3. `cargo update -p chacha20 -p anyhow -p tokio` so the first public `cargo audit` is clean.
4. `[profile.release] lto = "fat", codegen-units = 1, strip = true, panic = "abort"`; measure tarball size before and after.
5. Crate rename to `agent-memory-mcp` with `[[bin]] name = "agent-memory"` kept, `version = "0.1.0"` on both path dependencies, `readme`, `keywords = ["mcp", "memory", "sqlite", "claude-code", "codex"]`, `categories = ["command-line-utilities", "database"]`, `exclude = [".planning/", ".claude/", "docs/", "CLAUDE.md"]`.
6. A separate `publish-crates.yml` on the same tag with `id-token: write` and `rust-lang/crates-io-auth-action`, publishing core then binary; first publish by hand (portfolio convention S2). Publishing cannot be retried for a version, so run `cargo publish --dry-run` for both crates in CI on every PR.
7. `cargo-semver-checks` as a non-blocking job for `agent-memory-core` from 0.2.0 on; before 1.0 it only reports.
8. Keep releases manual (tag push), not release-plz: one release every few months does not justify a bot that opens PRs.

Registry mechanics, confirmed today against https://raw.githubusercontent.com/modelcontextprotocol/registry/main/docs/modelcontextprotocol-io/package-types.mdx: `registryType: "cargo"` is supported, crates.io only, schema `https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json`, package fields `identifier`, `version`, `transport: {type: "stdio"}`, no `runtimeHint`. Ownership is proven by a visible `mcp-name: io.github.UnityInFlow/agent-memory` line in the crate README; crates.io strips HTML comments. The same doc offers `registryType: "mcpb"` for a prebuilt binary from GitHub Releases, which removes the Rust-toolchain requirement for registry users and is the same bundle Smithery wants (`smithery mcp publish ./server.mcpb -n org/name`, https://smithery.ai/docs/build/publish.md). Glama needs `glama.json` with `$schema` and `maintainers` (https://glama.ai/blog/2025-07-08-what-is-glamajson). PulseMCP ingests the official registry (https://docs.alpic.ai/guides/distributing-your-mcp-server). Homebrew core notability numbers were not found on the fetched docs page (unverified); the tap is the right home either way.

## Develop

Finish Phase 4 as v0.1.0 and nothing else. The ROADMAP (`.planning/ROADMAP.md:31-32`) still says v0.1.0 ships after Phases 5 and 6; edit those two lines to v0.2.0 in the release commit so the plan and the tag agree.

Branch strategy: squash-merge PR #6 into `main` as one commit ("Phase 4: update, relations, hybrid search"). That collapses 31 docs commits and resets the docs share on `main`. Then one short-lived branch per launch issue (#8 to #15), each merged by squash, each under 300 lines of diff. Tag from `main` only.

Docs share: 65 of 97 commits since v0.0.1 are `docs(...)`. Under the 20 percent cap, each release gets at most one docs commit for CHANGELOG plus README and one review file in `docs/reviews/`. `.planning/` updates go into the same commit as the code they describe, never alone.

Issue hygiene: #7 to #15 already exist with estimates. Add a `v0.1.0` milestone and a `v0.2.0` milestone, move Phase 5 and 6 items to the latter, label external reports `external` on arrival so the stop rule can be counted with one query (`gh issue list --label external`).

Tests as the gate: `cargo test --workspace` plus `cargo clippy -D warnings` plus `cargo deny check` is the merge gate; `cargo publish --dry-run` is the tag gate; `gh attestation verify` on the four tarballs is the release gate. Add the three new environment tests (concurrency, corrupt DB, slow embedder) in a follow-up `v0.1.1`, not before the tag.

Working with an agent: give it one issue at a time with the exact file list, require the test first, and refuse any PR that adds a planning artefact. The agent is good at tests and docs; the owner should keep the decisions in this section and the next.

## Deliver

Launch sequence (D0 = the day PR #6 merges):

| Day | Action |
|---|---|
| D0 | Squash-merge #6; bump versions; rename crate; `rust-version`, `deny.toml`, `cargo update`; `CHANGELOG.md`, `SECURITY.md`; tag `v0.1.0`; watch the darwin legs |
| D0 | `cargo publish` core then `agent-memory-mcp` by hand; set up trusted publishing afterwards |
| D1 | Tap formula to 0.1.0; test `brew upgrade` from 0.0.1 on one machine (migration 0003 with backup) |
| D1 to D2 | README top (below), "Connect your runtimes" snippets tested against current Claude Code, Codex and Cursor; REST and GSD import moved under `docs/` |
| D2 | Demo GIF (below) |
| D3 | `server.json` (cargo package first, mcpb second once a bundle exists), `mcp-publisher publish`; `glama.json`; awesome-mcp-servers PR; mcp.so submit |
| D5 to D7 (Tue to Thu, 14:00 to 16:00 CET) | r/ClaudeAI post; r/ChatGPTCoding and r/mcp two days later |
| D7 to D37 | daily log in `docs/reviews/launch-log.md`; patch tags as needed |
| D37 | stop rule check |

README top. The plan's 20 lines are close; three changes: lead with the native-memory reality instead of "agents forget", add the mandatory "what it is not" line, and pin the test number. Replace the plan's block with:

````markdown
# agent-memory

**The memory your coding agents share.** Claude Code and Codex each remember in private.
agent-memory is one local SQLite file that every MCP host reads and writes: typed
memories, decay instead of deletion, typed relations, keyword and semantic search.
No cloud, no account, no LLM in the loop.

![Claude Code stores a decision, Codex recalls it](docs/demo/two-runtimes.gif)

```bash
brew install unityinflow/tap/agent-memory
claude mcp add --scope user agent-memory -e AGENT_MEMORY_DB=$HOME/.local/share/agent-memory/memory.db -- agent-memory serve
codex  mcp add agent-memory --env AGENT_MEMORY_DB=$HOME/.local/share/agent-memory/memory.db -- agent-memory serve
```

Tell Claude Code "remember: we chose SQLite over Postgres because this must run offline".
Ask Codex "why SQLite?". Same file, same answer.

- Typed: `DECISION`, `PATTERN`, `ERROR`, `TODO`, `ARCHITECTURE`, `CONSTRAINT`.
- Decay re-ranks, it never deletes; only a TTL or `memory_forget` removes.
- Relations: `supersedes`, `caused_by`, `relates_to`; follow them with `expand_links`.
- Hybrid search (FTS5 plus local Ollama embeddings, RRF) when Ollama is up; FTS5 alone when it is not.
- One static Rust binary, 217 tests, MIT.

Not: an auto-capture hook, a knowledge graph, a cloud service, or a replacement for CLAUDE.md.
````

Verify the `claude mcp add` and `codex mcp add` flag spellings against the installed CLIs on D1; the plan's `--env` for Claude Code is the long form of `-e` and should be confirmed, not assumed. Use the real default path, not `~/.agent-memory`.

Demo tape. The plan's two-pane VHS tape drives two interactive TUIs; VHS types keystrokes blind and the Claude Code and Codex TUIs redraw, so the recording will be flaky and over 2 MB. Record non-interactive calls instead: pane 1 `claude -p "Remember: we chose SQLite WAL over Postgres because the memory store must run offline"` (expect a `memory_store` line), pane 2 `codex exec "Why did we pick SQLite for memory?"` (expect the `DECISION` back), end card with the brew line. Under 20 s, deterministic, re-recordable; keep REST out.

First-week routine: 30 minutes a day; answer every comment inside two hours during the two post windows; reproduce bugs the same day; `v0.1.x` patch tags; log stars, release-asset downloads (`gh api repos/UnityInFlow/agent-memory/releases`), crates.io downloads, Glama views, external issues.

Metric that means "continue" at day 30: at least 3 external issues or PRs, or at least 50 release-asset plus crates.io downloads that are not CI, or one stranger describing two hosts on one file. Stars are recorded but do not decide.

r/ClaudeAI post, ready to paste (title under 300 characters, body under 250 words):

Title: Claude Code and Codex each have their own memory now. I built a tiny Rust MCP server so they share one (local SQLite, typed, decay instead of deletion)

Body:

> Claude Code's auto memory and Codex's `/m_update` are nice, but each tool keeps its notes to itself, per repo, as Markdown you cannot query. I wanted one memory both of them read and write.
>
> agent-memory is a single-binary MCP stdio server over an embedded SQLite file. Point every runtime at the same `AGENT_MEMORY_DB` and a decision recorded in Claude Code is recalled in Codex a minute later. [GIF]
>
> What is different from the official memory server, mem0 or claude-mem:
> - memories are typed (DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT), so recall is structured;
> - decay instead of deletion: stale memories sink in ranking, nothing disappears unless you say so or a TTL expires;
> - typed relations (`supersedes`, `caused_by`), so a superseded decision stays findable but ranks below its successor;
> - local and offline: FTS5 keyword search always works; with Ollama running you get hybrid keyword plus semantic (RRF);
> - no LLM in the loop, no hooks, no account; Rust, `brew install unityinflow/tap/agent-memory`, MIT.
>
> Honest limits: no Windows binary yet (`cargo install agent-memory-mcp` works), you decide what to store (no auto-capture), young project: 217 tests, few users.
>
> Would you point two agents at one file? What would you need before you did?

## Fable's opinion

1. Ship v0.1.0 from PR #6 within the week; waiting for Phases 5 and 6 buys features no first visitor will ask for, and the code quality already exceeds the category median.
2. Change the pitch from "agents forget" to "agents remember in private"; Claude Code auto memory is on by default and the old problem statement reads as uninformed.
3. Disagree with the plan's stop rule: 25 stars in a 138-entry category where fresh entries sit at 0 to 10 stars measures posting luck, so gate on external issues and install evidence instead.
4. Disagree with the plan's two-pane interactive demo: record `claude -p` and `codex exec` instead, because VHS cannot drive two redrawing TUIs reliably and the GIF budget is 2 MB.
5. Add a `PREFERENCE` memory type before the enum is public; user preferences are what both native stores lead with and what agents actually want from a shared store.
6. Default `memory_list` to a limit of 50 now; an unbounded tool result is the one defect a stranger can hit on day one with a big import.
7. Treat native memory as a feeder: an `import --from claude-auto-memory ~/.claude/projects/<p>/memory/` sub-command is a two-hour job on the existing `import` seam (`import/gsd_state.rs` is the template) and turns the existential risk into the best onboarding path; schedule it for v0.1.1, not before the tag.
8. Squash-merge PR #6; a 42-commit, 31-docs-commit history on `main` is the visible symptom of the planning-tool habit the portfolio decisions already cap at 20 percent.
9. Add `cargo-deny` and `rust-version` before the first public tag; a clean `cargo audit` on launch day is cheap credibility for a security-adjacent tool.
10. Keep REST but deprecate it in the 0.1.0 CHANGELOG and feature-gate it in 0.2.0; it is 45 percent of the binary crate's source for a use case the launch story drops.
11. Do not add an ANN index or swap sqlite-vec before 50k rows; flat KNN is correct and fast at the scale one developer reaches in a year, and the maintenance risk is watched, not acted on.
12. HN only with a story at day 30 to 60, as the plan says; the 2026 HN data shows memory MCP posts are a lottery with a 1 to 8 point mode.

## Open questions for a second opinion

1. Does the native-memory shift (Claude Code auto memory, Codex memory) make a shared MCP memory more valuable (interop) or less (good enough per tool), and what single README sentence tests that with strangers?
2. Is `rrf_score * decay_score` (`service.rs:527`) the right fusion, or should decay enter RRF as a third ranked leg so a fresh exact hit cannot be outranked by a stale paraphrase?
3. Should `memory_list` without `limit` default to 50 in 0.1.0 (a behavioural change) or stay unbounded until 0.2.0 with a documented warning?
4. `PREFERENCE` type now (public enum, cheap) or never (keeps the project-knowledge focus and avoids overlap with CLAUDE.md)?
5. Is the per-process hourly full-table rescore worth keeping at all, given decay is recomputed on read, or should `materialize_decay` be deleted along with the `idx_memories_decay` index?
6. Stop rule: stars, external issues, downloads, or a qualitative signal; which one would you defend to the owner on day 30?
7. Registry package: `cargo` first (toolchain required) or `mcpb` first (prebuilt binary, also unlocks Smithery), given most Claude Code users do not have rustup?
8. Is the two-crate split worth its publish friction for a project this size, or should `agent-memory-core` stay unpublished until someone asks for it?

## Appendix: prompt for Claude Opus

```text
You are Claude Opus, giving a second opinion on the open-source project agent-memory
(https://github.com/UnityInFlow/agent-memory), a Rust MCP stdio server over SQLite that
gives Claude Code, Codex and Cursor one shared typed memory file. Today is 2026-10-25.

Read, in this order:
1. docs/reviews/2026-10-25-fable-analysis.md (Fable's analysis; this file)
2. docs/reviews/2026-10-24-launch-plan.md (the launch plan and issue index)
3. docs/TODO.md
Verify any code claim you rely on against the checkout at 0f1a9fd (file:line anchors are
given); run `cargo test --workspace` once. Do web research where Fable marked a claim
"unverified" or where you doubt a market claim; cite URLs inline.

Produce docs/reviews/2026-10-25-opus-analysis.md, 150 to 250 lines, plain Markdown with
tables, no em-dashes anywhere, with this header:

# agent-memory: Opus analysis (2026-10-25)
Author: Claude Opus, second opinion requested by Fable's analysis of the same date.
Status: opinion document.

Then these H2 sections, in order:
1. Answers to the eight open questions in Fable's "Open questions for a second opinion",
   numbered, each with a verdict in one sentence and the reasoning in at most four.
2. Disagreements with Fable: every numbered statement in "Fable's opinion" you reject,
   quoted, with why; state explicitly which of Fable's two disagreements with the launch
   plan you side with and which you do not.
3. Your own ranked next three actions for the owner (a solo developer with an agent,
   about 17 hours available), each with effort and the metric that shows it worked.
4. One paragraph: what you would tell the owner if the day-30 gate fails.

Rules: every market claim has a URL, every code claim has file:line, write "unverified"
where you could not check, do not restate either document, do not edit code or any other
file, do not commit. Mark the document "Opus analysis" in its title and header.
```
