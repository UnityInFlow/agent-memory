# Pitfalls Research

**Domain:** Local-first Rust agent-memory daemon (MCP server + REST, embedded SQLite, local Ollama embeddings, exponential decay + TTL)
**Researched:** 2026-06-24
**Confidence:** HIGH (MCP stdio + sqlite-vec findings verified against upstream issues/docs; ecosystem CI pitfalls confirmed from CLAUDE.md Decisions Log)

> Scope note: this file is domain-specific. Generic Rust advice (use `?`, no `unwrap()`, clippy clean) lives in the ecosystem CLAUDE.md and is not repeated here.

## Critical Pitfalls

### Pitfall 1: Logging to stdout corrupts the MCP stdio transport

**What goes wrong:**
Over MCP stdio transport, **stdout is the JSON-RPC channel** — every byte must be a valid framed JSON-RPC message. A single `println!`, a `tracing` subscriber defaulting to stdout, a `dbg!`, a panic backtrace, or a dependency that prints a banner will corrupt the stream. Clients fail with `Parse error: Unexpected token … is not valid JSON` and `Method not found: notifications/initialized`. This is documented as the single most common MCP server bug.

**Why it happens:**
The default for almost every Rust logging setup (`tracing_subscriber::fmt()`, `env_logger`) writes to stdout. It works fine when you test the binary by hand, then silently breaks the moment an MCP client attaches over stdio. ANSI color codes make it worse.

**How to avoid:**
- Route **all** logging to **stderr**: `tracing_subscriber::fmt().with_writer(std::io::stderr).init()`. Disable ANSI when not a TTY.
- Install a panic hook that writes to stderr, never stdout.
- Audit every transitive dependency for stray stdout writes (Ollama client banners, progress bars). Disable progress/indicatif output in daemon mode.
- Add a CI/integration test that runs the server over a stdio pipe, sends `initialize`, and asserts **every line on stdout parses as JSON-RPC**.

**Warning signs:**
Server works in `--help` / manual runs but the MCP client reports a parse error on connect; intermittent failures that correlate with a log line firing.

**Phase to address:** Phase 1 (MCP server foundation) — make stderr-only logging a hard gate before any tool is added.

---

### Pitfall 2: Blocking the async event loop with SQLite and Ollama calls

**What goes wrong:**
`rusqlite` is **synchronous and blocking**. The Ollama embedding HTTP call can take tens to hundreds of ms. If either runs directly inside a `tokio` async tool handler (rmcp is tokio-based), it blocks the runtime worker thread, stalling concurrent MCP/REST requests and the background decay task. Under stdio that can also delay protocol responses enough to look like a hang.

**Why it happens:**
The natural way to write a handler is `let row = conn.query_row(...)?;` inside an `async fn`. It compiles, it works in single-request tests, and the contention only shows up when search + store + decay overlap.

**How to avoid:**
- Run all `rusqlite` work inside `tokio::task::spawn_blocking` (or a dedicated blocking DB thread/actor with a channel). Never call blocking DB APIs directly in an async handler.
- Use a non-blocking HTTP client (`reqwest` async) for Ollama; never block on embeddings inside the handler thread.
- Keep the decay background job off the request path entirely (separate task, see Pitfall 7).

**Warning signs:**
Latency spikes when multiple tools fire at once; the daemon "freezes" during a large import; `tokio` worker threads pinned at 100%.

**Phase to address:** Phase 1 (set the blocking-DB + async-HTTP boundary as the architectural rule from the first handler).

---

### Pitfall 3: sqlite-vec / native-extension portability across the prebuilt-binary matrix

**What goes wrong:**
Vector search needs `sqlite-vec`. The naive approach ships a loadable extension (`.so`/`.dylib`/`.dll`) and `load_extension()`s it at runtime. This is a portability nightmare: you must ship the **correct arch-specific** extension for each of macOS arm64/x86_64, Linux x86_64/aarch64 (gnu **and** musl), and Windows; `load_extension` is disabled by default and must be explicitly enabled; and Homebrew/standalone installs break when the extension isn't found relative to the binary.

**Why it happens:**
Most tutorials demonstrate runtime extension loading because it's the easiest local setup. It does not survive being packaged as a single distributed binary across 6+ target triples.

**How to avoid:**
- Use the **`sqlite-vec` Rust crate**, which embeds the C source and **statically links via the `cc` crate at build time** (register with `sqlite3_auto_extension` / `register_auto_extension`). No runtime `.so`, no per-arch extension file, one self-contained binary. This is the decisive mitigation.
- Pin `rusqlite` with the `bundled` feature so SQLite itself is compiled in (no reliance on the host's system SQLite version, which may predate `vec0`).
- **Caveat that intersects this ecosystem:** static linking means the build now needs a working **C compiler for every target**. The ecosystem already cross-compiles Rust via `cargo-zigbuild` on the `orangepi` ARM64 runner (Hetzner X64 fleet intermittently offline — see CLAUDE.md). Zig provides the C cross-toolchain, so this is workable, but it must be **proven on day one for all target triples**, not discovered at release.

**Warning signs:**
`load_extension` errors on a user's machine; "no such module: vec0"; works on the dev Mac but not on a Linux musl binary; Homebrew bottle can't find the dylib.

**Phase to address:** Phase 1 for the static-link decision (it shapes the schema and search code); revalidate on the full target matrix in the release/CI phase.

---

### Pitfall 4: Ollama not installed / not running / model not pulled — and silent fallback

**What goes wrong:**
Semantic search depends on a local Ollama serving `nomic-embed-text`. Users frequently (a) don't have Ollama installed, (b) have it installed but not running, or (c) have never `ollama pull nomic-embed-text`. The dangerous failure mode is a **silent fallback**: the daemon stores memories with null/zero embeddings, search returns garbage or nothing, and the user has no idea their corpus is now unsearchable.

**Why it happens:**
Treating Ollama as an always-available dependency. Swallowing the connection error to "keep working." Storing a memory even when embedding failed.

**How to avoid:**
- **Fail loud, not silent.** On startup, probe Ollama (`/api/tags`) and report clearly: not running / model missing / OK. Provide an actionable message (`ollama pull nomic-embed-text`).
- Design `memory_store` so a failed embedding is an explicit, recoverable state: either reject the store with a clear error, or store the text + mark `embedding_status = pending` and backfill later — **never** silently store a fake vector.
- Make embeddings **optional but explicit**: keyword/FTS search must work with zero embeddings (see Pitfall 9), so the tool is useful even without Ollama, but the user is told semantic search is degraded.
- Record the **embedding model + dimension** alongside each vector (see Pitfall 5).

**Warning signs:**
`memory_search` returns empty/irrelevant results; embeddings column full of zeros or nulls; no error ever surfaced to the agent.

**Phase to address:** Phase 2 (embeddings + semantic search) — startup health-check and explicit failure contract.

---

### Pitfall 5: Embedding dimension mismatch and model version drift invalidating stored vectors

**What goes wrong:**
`vec0` tables require a **fixed dimension at table-creation time** (e.g. `embedding float[768]`). Two related failures: (1) the configured model's output dimension doesn't match the column → insert/query errors or silent truncation; (2) the user (or a future release) swaps the embedding model, or Ollama updates `nomic-embed-text`, so **new vectors are not comparable to old ones**. Cosine similarity across two model versions is meaningless, silently degrading recall.

**Why it happens:**
Hardcoding a dimension; assuming the embedding model is immutable; not storing which model produced each vector.

**How to avoid:**
- Store `embedding_model` and `embedding_dim` columns (or a corpus-level metadata row). On startup, compare configured model/dim against the stored corpus; refuse to mix or trigger a re-embed.
- Verify the model's actual output dimension at runtime (embed a probe string) and assert it equals the `vec0` column width before serving.
- Provide a `reindex`/`re-embed` path for model changes rather than silently appending incompatible vectors.
- Default to one pinned model (`nomic-embed-text`, 768-dim) and make changing it a deliberate, corpus-wide operation.

**Warning signs:**
Insert errors mentioning dimension; search quality drops after an `ollama` upgrade; mixed-dimension data in the table.

**Phase to address:** Phase 2 (embeddings) — model/dim metadata as part of the schema, not an afterthought.

---

### Pitfall 6: Cosine similarity on un-normalized vectors

**What goes wrong:**
Computing cosine similarity (or using `vec0`'s distance) without L2-normalizing vectors, or mixing normalized and raw vectors, produces ranking that's skewed by vector magnitude rather than direction. Results look "kind of relevant" but recall/ordering is subtly wrong — the hardest class of bug to notice.

**Why it happens:**
Assuming the embedding model already returns unit vectors (not guaranteed), or mixing `vec_distance_cosine` vs `vec_distance_L2` without understanding the implication. Normalizing at query time but not at store time (or vice versa).

**How to avoid:**
- Pick one convention and enforce it: normalize on store **and** query, or rely consistently on `vec0`'s cosine distance. Document it.
- Use `vec_distance_cosine` explicitly rather than defaulting to L2 if the intent is angular similarity.
- Add a golden-set test: a handful of known query→expected-memory pairs that must rank correctly.

**Warning signs:**
Top results that are "in the ballpark" but the obviously-correct memory ranks third; longer memories systematically out-ranking short relevant ones.

**Phase to address:** Phase 2 (semantic search).

---

### Pitfall 7: Decay that nukes useful memories / decay–TTL–forget interactions

**What goes wrong:**
Exponential decay tuned too aggressively silently buries (or, if coupled to deletion, erases) memories the user still needs — DECISION/ARCHITECTURE/CONSTRAINT entries that are *rarely accessed but high-value*. Worse is conflating three distinct mechanisms: **decay** (ranking signal), **TTL** (hard expiry), and **explicit forget** (user delete). If decay ever deletes rows, or TTL silently removes a pinned decision, users lose trust and stop relying on the tool.

**Why it happens:**
Treating decay as garbage-collection rather than a ranking weight. One global half-life applied to all memory types. No "pin / never-decay" concept for foundational entries.

**How to avoid:**
- **Decay affects ranking only — never deletes.** Deletion is exclusively TTL (explicit, per-memory) or `memory_forget` (explicit user action). Keep the three concepts orthogonal and documented.
- Per-type decay policy: CONSTRAINT/ARCHITECTURE/DECISION decay slowly or are pinnable; TODO/ERROR may decay faster.
- "Touch on access" — reading/matching a memory refreshes its recency so genuinely-used memories don't fade.
- Make decay parameters (half-life) configurable and conservative by default.

**Warning signs:**
Users report "it forgot my architecture decision"; relevant old memories never surface; rows disappearing without an explicit forget/TTL.

**Phase to address:** Phase 1 (schema must separate `decay_score`, `expires_at`/TTL, and last-accessed from the start; retrofitting the distinction later is a migration).

---

### Pitfall 8: Recompute cost, clock issues, and non-deterministic decay tests

**What goes wrong:**
(a) Recomputing decay scores for the whole corpus on a timer is wasteful and, if done on the request path, causes latency spikes. (b) Decay is a function of `now`, so using `SystemTime::now()` directly inside the algorithm makes it **untestable and non-deterministic**, and is vulnerable to clock skew / DST / the machine sleeping. (c) Storing naive local timestamps instead of UTC corrupts decay math across timezones and sleep/wake.

**Why it happens:**
"Run decay as a background thread, update daily" (the spec's own todo) implemented as a full-table UPDATE; `now()` hardcoded; timestamps stored as local strings.

**How to avoid:**
- **Compute decay lazily at query time** from `stored_at`/`last_accessed` rather than materializing a periodically-rewritten `decay_score` column — or if materializing, only touch rows that changed. This avoids the full-table churn and the daily-job clock dependency.
- Inject a **`Clock` trait** (real impl = `Utc::now`, test impl = fixed/advanceable). All decay math takes the clock as a parameter. This makes decay tests deterministic.
- Store **all timestamps as UTC** (Unix epoch ms or RFC3339-UTC). Never local time.
- Handle "machine was asleep for 3 days" gracefully — decay is a smooth function of elapsed time, which it should already be if computed from stored timestamps.

**Warning signs:**
Flaky decay tests; scores that jump after a timezone change or laptop sleep; CPU spike once a day.

**Phase to address:** Phase 1 (Clock trait + UTC + lazy decay are foundational and make >80% coverage on decay achievable — see Pitfall 12).

---

### Pitfall 9: No keyword/hybrid fallback — poor recall on short memories

**What goes wrong:**
Pure semantic (vector) search has weak recall on **short memories** (a 4-word TODO, an error code, an exact identifier the user remembers verbatim). Embeddings of very short text are noisy, and exact-match queries ("find the memory mentioning `S005`") often *miss* under cosine ranking. A semantic-only tool feels unreliable for the exact recall agents need most.

**Why it happens:**
"Semantic search" sounds strictly better than keyword, so FTS is skipped. Short-text embedding weakness isn't obvious until real usage.

**How to avoid:**
- Implement **hybrid search**: SQLite **FTS5** (keyword/BM25) + `vec0` (semantic), fused (e.g. reciprocal rank fusion or weighted). FTS5 is built into bundled SQLite — cheap to add.
- Hybrid also gives a working search path when Ollama is absent (ties into Pitfall 4).
- Boost exact identifier/substring matches.

**Warning signs:**
Searching a known exact phrase doesn't return the memory containing it; short TODOs never surface.

**Phase to address:** Phase 2 (search) — design FTS5 + vector together; FTS5 schema can land in Phase 1 to avoid a migration.

---

### Pitfall 10: Cross-platform binary failures — Windows `cfg(unix)` and the cross-compile matrix

**What goes wrong:**
The ecosystem has **already been burned** by this: mcp-hub's Windows cross-build failed because CLI-only deps (chrono/rand/owo_colors/comfy_table/dialoguer/dirs) were used **unguarded** while only `cfg(unix)`-gated, so they didn't link on Windows (CLAUDE.md Decisions Log, June 2026 — Windows + macOS deferred to v2). agent-memory uses similar crates (`dirs` for the SQLite path, possibly colored CLI output). Same trap: a daemon/CLI that builds on Linux/macOS silently won't link on Windows.

**Why it happens:**
Developing on macOS/Linux; Windows only exercised at release; platform-specific deps not consistently `cfg`-gated; the self-hosted runner fleet has no Windows host, so Windows is cross-built and rarely smoke-tested.

**How to avoid:**
- Decide the Windows story **up front**: either (a) gate platform-specific code properly and CI-cross-build Windows from the start, or (b) **explicitly defer Windows to v2** like mcp-hub did, and document it — don't discover it at release.
- Centralize OS-specific paths (use `dirs`/`directories` consistently, not ad-hoc `cfg(unix)`).
- For the data dir, handle Windows path conventions (`%APPDATA%`) from the schema-init code path.
- Plan the release matrix around **`cargo-zigbuild` on `orangepi`** (proven for injection-scanner darwin + mcp-hub Linux), and remember the **`cc`-compiled sqlite-vec** (Pitfall 3) must cross-compile its C for every triple — verify early.

**Warning signs:**
`cargo build --target x86_64-pc-windows-*` fails on unresolved symbols / unsatisfied deps; "works on my Mac"; no Windows binary on the release.

**Phase to address:** Release/CI phase — but the **decision** (support vs defer Windows) belongs in Phase 1 so deps are gated correctly throughout.

---

### Pitfall 11: SQLite writer contention ("database is locked") in a long-running daemon

**What goes wrong:**
A daemon with concurrent MCP store calls + REST writes + a decay/TTL background task hits SQLite's single-writer limit. With default `rollback journal` mode and no busy timeout, concurrent writes throw `SQLITE_BUSY` / "database is locked", surfacing to agents as random tool failures.

**Why it happens:**
Default `rusqlite` connection settings; opening many connections that all try to write; running decay UPDATEs concurrently with stores; no `busy_timeout`.

**How to avoid:**
- Enable **WAL mode** (`PRAGMA journal_mode=WAL`) — allows concurrent readers with one writer; far better for a daemon. Set `PRAGMA synchronous=NORMAL` (safe with WAL) and a `PRAGMA busy_timeout` (e.g. 5s).
- **Serialize writes** through a single owning thread/actor (one write connection), with a read pool (e.g. `r2d2`/`deadpool` for reads). This avoids multi-writer contention entirely and pairs naturally with the spawn_blocking boundary (Pitfall 2).
- Don't open a fresh connection per request; don't let the decay job and store handlers fight for the write lock — schedule decay writes through the same write actor.
- Checkpoint WAL periodically so the `-wal` file doesn't grow unbounded.

**Warning signs:**
Intermittent "database is locked" under concurrent use; growing `*.db-wal` file; failures during a large import.

**Phase to address:** Phase 1 (connection strategy + WAL + write-serialization is foundational; retrofitting a pool is invasive).

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Runtime `load_extension` for sqlite-vec instead of static link | Quick local setup | Per-arch extension shipping nightmare; broken Homebrew/portable binary | Never for distribution; OK only in a throwaway spike |
| Materialized `decay_score` rewritten by a daily full-table job | Matches the spec's literal wording | Full-table churn, clock-dependent, non-deterministic tests | Only if rows-touched is bounded and clock is injected |
| Silently store a zero/null embedding when Ollama is down | "Keeps working" | Corpus silently unsearchable; user trust destroyed | Never — fail loud or mark pending |
| Single embedding model hardcoded with no model/dim metadata | Less schema | Model drift silently invalidates all vectors | Acceptable for v0.0.1 only if reindex path exists and dim is asserted |
| Semantic-only search, skip FTS5 | Less code | Poor exact/short recall; no fallback without Ollama | Never — FTS5 is cheap and built in |
| Defer Windows binary to v2 | Smaller release matrix | Some users excluded | Acceptable (precedent: mcp-hub) **if documented and deps stay correctly gated** |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| MCP stdio client | Logging/banners on stdout | All logs → stderr; assert stdout is pure JSON-RPC in CI |
| Ollama API | Assume installed + running + model pulled; swallow errors | Startup health probe (`/api/tags`); explicit "model missing → `ollama pull`" message; never silent fallback |
| sqlite-vec | Ship a runtime extension per arch | Static-link via the `sqlite-vec` crate (`cc` build) into one binary |
| rusqlite | Default journal mode + per-request connections | WAL + busy_timeout + serialized single-writer + read pool |
| GSD STATE.md import | Assume a fixed format; choke on partial/edge files | Tolerant parser; treat malformed sections as skippable; import is idempotent (re-import doesn't duplicate) |
| Self-hosted CI | `ubuntu-latest`/`macos-latest` | `arc-runner-unityinflow` (X64) / `orangepi` (ARM64); zigbuild for cross — Hetzner fleet may be offline, plan for orangepi-only serial builds |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Blocking DB/HTTP in async handler | Latency spikes under concurrency; pinned worker threads | `spawn_blocking` for DB; async reqwest for Ollama | As soon as 2+ tools overlap |
| Full-table decay recompute | Daily CPU spike; lock contention with stores | Lazy decay at query time from timestamps | A few thousand memories |
| Brute-force cosine over all rows (no `vec0` index) | Search latency grows linearly | Use `vec0` virtual table KNN, not a manual scan | Low thousands of memories |
| WAL never checkpointed | `-wal` file grows unbounded; disk pressure | Periodic `wal_checkpoint(TRUNCATE)` | Long-running daemon over days |
| Re-embedding on every search | High Ollama load; slow search | Embed once on store; cache query embeddings | Immediately under real use |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| REST API bound to `0.0.0.0` with no auth | Local memory corpus exposed on the network | Bind to `127.0.0.1` by default; require explicit opt-in + token for non-loopback |
| Stored memory content rendered/executed downstream (prompt-injection carrier) | A poisoned memory steers a future agent | Treat memory content as untrusted data; never auto-execute; this composes with the ecosystem's injection-scanner |
| `load_extension` left enabled | Arbitrary extension load = code execution vector | Static-link sqlite-vec; keep `load_extension` disabled |
| DB file world-readable | Other local users read the agent's memory | Create the DB dir with restrictive perms (0700) |
| Secrets accidentally stored as memories | Tokens persisted in plaintext SQLite | Optional secret-pattern guard on store (reuse injection-scanner regexes); document that storage is plaintext |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| Tool schemas that confuse the agent (vague names, overloaded params, no examples) | Agent calls the wrong tool or malforms args; memory underused | Crisp tool descriptions; enum the memory `type`; small, well-documented param sets with examples in the schema |
| `memory_store`/`memory_search` return walls of unstructured text | Bloats the agent's context, defeats the purpose | Return compact, ranked, typed results with IDs; let the agent fetch detail on demand |
| Silent embedding failure | Agent "remembers" but can never recall | Surface degraded-search state in the tool response |
| No way to pin/never-forget a key decision | Foundational context decays away | First-class pin flag; per-type decay (Pitfall 7) |
| `memory_forget` ambiguous (soft vs hard delete) | User unsure if data is gone | Explicit, documented semantics; consider soft-delete + purge |

## "Looks Done But Isn't" Checklist

- [ ] **MCP server:** Often missing — stdout purity. Verify with a piped `initialize` test that asserts every stdout line is valid JSON-RPC.
- [ ] **Semantic search:** Often missing — keyword/FTS5 fallback and short-text recall. Verify with a golden query set including exact-identifier queries.
- [ ] **Embeddings:** Often missing — Ollama-down behavior and model/dim metadata. Verify by killing Ollama and confirming a loud, recoverable error (not a stored zero-vector).
- [ ] **Decay:** Often missing — separation of decay (rank) vs TTL (expire) vs forget (delete), and pinning. Verify decay never deletes a row.
- [ ] **Cross-platform binary:** Often missing — Windows link + sqlite-vec C cross-compile. Verify all target triples build and the binary runs `--version` per arch (host-arch-aware, per ecosystem precedent).
- [ ] **Concurrency:** Often missing — WAL + write serialization. Verify with a concurrent store/search/decay stress test (no "database is locked").
- [ ] **GSD import:** Often missing — idempotency. Verify re-importing the same STATE.md doesn't duplicate memories.
- [ ] **Timestamps:** Often missing — UTC everywhere + injectable clock. Verify decay tests are deterministic with a fixed clock.

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Stdout-corrupted MCP | LOW | Move logging to stderr, add panic hook, add the stdout-purity test |
| Decay deleted useful memories | HIGH (data loss) | If decay ever deleted: restore from backup; redesign so decay only ranks. Prevent by never coupling decay to deletion |
| Model drift invalidated vectors | MEDIUM | Re-embed corpus with the pinned model via a `reindex` command; add model/dim metadata going forward |
| sqlite-vec won't load on a target | MEDIUM | Switch to static-link crate; rebuild matrix; ship single self-contained binary |
| "database is locked" in prod | MEDIUM | Enable WAL + busy_timeout; route all writes through one actor; add checkpointing |
| Windows build fails at release | MEDIUM | Either gate deps and fix cross-build, or formally defer Windows to v2 and document (mcp-hub precedent) |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| 1 Stdout corrupts MCP | Phase 1 | Piped `initialize` test asserts pure JSON-RPC on stdout |
| 2 Blocking the event loop | Phase 1 | Concurrent-handler stress test; no worker-thread starvation |
| 3 sqlite-vec portability | Phase 1 (decision) + Release | Single static binary; `vec0` works on all triples |
| 4 Ollama down / silent fallback | Phase 2 | Kill Ollama → loud recoverable error; no zero-vectors stored |
| 5 Dim mismatch / model drift | Phase 2 | Dim asserted at startup; model metadata stored; reindex path exists |
| 6 Un-normalized cosine | Phase 2 | Golden-set ranking test passes |
| 7 Decay nukes memories | Phase 1 (schema) | Decay never deletes; pin + per-type policy work |
| 8 Recompute/clock/non-determinism | Phase 1 | Injectable clock; UTC timestamps; deterministic decay tests |
| 9 No hybrid fallback | Phase 1 (FTS5 schema) + Phase 2 | Exact-phrase + short-memory recall test passes |
| 10 Windows / cross-compile | Phase 1 (decision) + Release | All triples build; deps correctly `cfg`-gated |
| 11 SQLite writer contention | Phase 1 | Concurrent store/search/decay test; no `SQLITE_BUSY` |

## Sources

- [stdio transport: log output on stdout breaks MCP JSON-RPC; use stderr (dirmacs/daedra #4)](https://github.com/dirmacs/daedra/issues/4) — HIGH
- [MCP server stdio mode corrupted by stdout log messages (ruvnet/claude-flow #835)](https://github.com/ruvnet/claude-flow/issues/835) — HIGH
- [The Complete MCP Debugging Guide (ChatForest)](https://chatforest.com/guides/mcp-debugging-guide/) — MEDIUM
- [asg017/sqlite-vec — static link via cc, runs anywhere](https://github.com/asg017/sqlite-vec) — HIGH
- [Using sqlite-vec in Rust (Alex Garcia)](https://alexgarcia.xyz/sqlite-vec/rust.html) — HIGH
- [Compiling sqlite-vec (Alex Garcia)](https://alexgarcia.xyz/sqlite-vec/compiling.html) — HIGH
- [Update for use with current rusqlite — register_auto_extension (sqlite-vec #206)](https://github.com/asg017/sqlite-vec/issues/206) — MEDIUM
- [Official Rust MCP SDK (rmcp)](https://github.com/modelcontextprotocol/rust-sdk) — HIGH
- [Build MCP Servers in Rust (MCPcat guide)](https://mcpcat.io/guides/building-mcp-server-rust/) — MEDIUM
- [rusqlite — bundled SQLite + WAL guidance](https://github.com/rusqlite/rusqlite) — HIGH
- UnityInFlow CLAUDE.md Decisions Log (mcp-hub Windows `cfg(unix)` failure; cargo-zigbuild on orangepi; Hetzner fleet offline) — HIGH (project-internal, verified)

---
*Pitfalls research for: local-first Rust agent-memory daemon (MCP + SQLite + Ollama + decay)*
*Researched: 2026-06-24*
