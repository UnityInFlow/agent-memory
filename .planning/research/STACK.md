# Stack Research

**Domain:** Local-first Rust daemon — MCP server + REST API, embedded SQLite store with on-device (Ollama) semantic search
**Researched:** 2026-06-24
**Confidence:** HIGH (core crates verified against crates.io live versions; integration nuances verified against upstream docs/issues)

> **Verdict in one line:** Build on **`rmcp` (official MCP SDK) + `axum` + `rusqlite` (bundled) + `sqlite-vec` + `reqwest` direct to Ollama**, all on one `tokio` runtime, packaged with **`cargo-dist`** (with a self-hosted-runner caveat). This matches the ecosystem's existing Rust conventions (mcp-hub already uses `axum 0.8`, `clap 4`, `tokio 1`, `thiserror 2`) and keeps the tool single-binary, zero-cloud, zero-config.

---

## Recommended Stack

### Core Technologies

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| **`rmcp`** | `1.8` (`features = ["server", "transport-io", "transport-streamable-http-server", "macros"]`) | The MCP server — the primary interface (`memory_store/search/list/forget`) | The **official** Model Context Protocol Rust SDK (`modelcontextprotocol/rust-sdk`), 4.7M+ downloads, implements the latest protocol revision with backward compat. Provides **stdio** (`transport-io`) for the standard `claude_desktop`/Cursor launch model **and** **Streamable HTTP** (`transport-streamable-http-server`) as a `tower`/`axum`-nestable service. `#[tool]` / `#[tool_router]` macros remove handwritten JSON-RPC plumbing. No credible alternative for a tool that must be a first-class MCP citizen. |
| **`tokio`** | `1` (`features = ["full"]`) | Async runtime hosting both the MCP server and the REST API in one process | Ecosystem standard and already used across mcp-hub/injection-scanner. rmcp, axum, and reqwest are all tokio-native, so one runtime serves everything — no second executor, no blocking-thread juggling. |
| **`rusqlite`** | `0.40` (`features = ["bundled"]`) | Embedded SQLite store (memories, embeddings, metadata) | `bundled` compiles SQLite **into the binary** → zero system dependency, identical behavior on every prebuilt target, no "install sqlite first" step. Synchronous API is the right fit for a single-process local store (wrap writes in `tokio::task::spawn_blocking` or a single writer task). Directly supports loading the `sqlite-vec` extension via `sqlite3_auto_extension`. |
| **`sqlite-vec`** | `0.1.9` | Vector similarity search inside SQLite (semantic memory recall) | Pure-C, dependency-free successor to the **deprecated** `sqlite-vss`; runs anywhere SQLite runs. Registered as an auto-extension, stores 768-dim `nomic-embed-text` vectors in a `vec0` virtual table and does KNN with `MATCH`. Keeps search **in the database** — no separate vector store, no extra process. (See Version Compatibility for the rusqlite 0.34+ registration nuance.) |
| **`axum`** | `0.8` | Secondary REST API for non-MCP integrations | Already the ecosystem's chosen web framework (mcp-hub pins `axum 0.8`). Critically, rmcp's `StreamableHttpService` **is a `tower` service you nest directly into an axum `Router`** — so the MCP HTTP transport and the REST endpoints share **one router, one listener, one runtime**. `actix-web` would force a second, non-tower stack. |
| **`reqwest`** | `0.12` (pin `0.12`, not `0.13`) | HTTP client to local Ollama `/api/embed` | Direct, dependency-light, fully under our control for the *one* call we make (request an embedding). Avoids an extra abstraction layer and an unmaintained-wrapper risk (see What NOT to Use re: `ollama-rs`). |
| **`clap`** | `4` (`features = ["derive", "env"]`) | CLI surface (`serve`, `import --from gsd-state`, config flags) | Ecosystem standard; `derive` + `env` matches mcp-hub. |
| **`serde` / `serde_json`** | `1` / `1` | (De)serialization of MCP payloads, REST bodies, Ollama JSON, config | Ecosystem standard, required transitively by rmcp/axum anyway. |
| **`anyhow` / `thiserror`** | `1` / `2` | Errors: `anyhow` in the binary, `thiserror` typed errors in the library | Mandated by ecosystem CLAUDE.md; mcp-hub already on `thiserror 2`. |

### Supporting Libraries

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| **`rusqlite_migration`** | `2` | Versioned, ordered schema migrations over the bundled SQLite connection | From day one. Lightweight (just `M::up(...)` SQL steps + `user_version`), no async, no macros — correct weight for a single-file embedded DB. Preferred over `refinery` here (see Alternatives). |
| **`tower` / `tower-http`** | `0.5` / `0.6` | Middleware for the axum router (CORS, timeout, tracing) shared with the MCP HTTP service | When exposing the HTTP transport / REST surface. mcp-hub already uses `tower-http 0.6` with `timeout, cors`. |
| **`tracing` / `tracing-subscriber`** | `0.1` / `0.3` (`env-filter`) | Structured logging for a long-running daemon | Always — observability-first is an ecosystem principle; matches mcp-hub. |
| **`chrono`** | `0.4` | Timestamps for `created_at` / `last_accessed`, decay + TTL math | Decay scoring and TTL expiry both need wall-clock deltas. (`time` 0.3 is an acceptable substitute if you prefer fewer transitive deps.) |
| **`dirs`** | `5`/`6` | Resolve default DB path (`~/.local/share/agent-memory/…`) | Zero-config default location for the SQLite file. mcp-hub uses `dirs 5`. |
| **`zerocopy`** | `0.8` | Cast `Vec<f32>` embeddings to `&[u8]` BLOB and back without unsafe | When writing/reading raw vectors to SQLite (sqlite-vec expects little-endian f32 byte slices). Avoids hand-rolled `unsafe transmute`. |
| **`uuid`** | `1` (`v4`) | Stable memory IDs | When generating memory record identifiers. |
| **`tokio-cron-scheduler`** *(optional)* | `0.13` | Daily decay-score recompute as a background task | Only if you want a scheduler abstraction. A plain `tokio::time::interval` loop in a spawned task is simpler and usually sufficient — prefer it unless cron semantics are genuinely needed. |

### Development Tools

| Tool | Purpose | Notes |
|------|---------|-------|
| **`cargo-dist`** | Cross-platform prebuilt binaries + GitHub Release + **Homebrew formula generation** | Latest `0.32.0` (Dec 2025); actively maintained. Generates the release workflow, archives, checksums, shell/PowerShell installers, **and a Homebrew tap formula** from `Cargo.toml` metadata — directly satisfies "pre-built binaries + Homebrew". **Caveat:** its generated `release.yml` defaults to `ubuntu-latest`/`macos-latest`, which is **banned org-wide**. You must override `[workspace.metadata.dist] github-custom-runners` (or post-generate patch `runs-on:` to `arc-runner-unityinflow` / `orangepi`). See Stack Patterns. |
| **`cargo-zigbuild`** | Cross-compile all target triples from one Linux host (incl. ARM64 `orangepi`) | **Proven in this ecosystem** — used to ship injection-scanner v0.0.2 (incl. apple-darwin) and mcp-hub v0.1.1 from the `orangepi` runner when the Hetzner X64 fleet was offline. Pairs with either cargo-dist or a hand-rolled matrix. ⚠️ **`bundled` rusqlite + `sqlite-vec` compile C code** — confirm the zig C toolchain links the bundled SQLite/vec C for each cross target early (this is the single biggest release risk; see Pitfalls). |
| **`cargo clippy -- -D warnings`** | Lint gate | Mandated; must pass before commit. |
| **`cargo fmt`** | Format | Mandated before every commit. |
| **`cargo nextest`** *(optional)* | Faster test runner for the >80% coverage gate | Nice-to-have; `cargo test` is fine. |

## Installation

```bash
# Add to Cargo.toml [dependencies]
cargo add rmcp --features server,transport-io,transport-streamable-http-server,macros
cargo add tokio --features full
cargo add rusqlite --features bundled
cargo add sqlite-vec
cargo add rusqlite_migration
cargo add axum
cargo add tower-http --features timeout,cors
cargo add reqwest@0.12 --no-default-features --features json,rustls-tls
cargo add clap --features derive,env
cargo add serde --features derive
cargo add serde_json anyhow
cargo add thiserror@2
cargo add tracing tracing-subscriber --features tracing-subscriber/env-filter
cargo add chrono dirs uuid@1 zerocopy

# Dev / release tooling (installed on the runner / dev machine, not deps)
cargo install cargo-dist
cargo install cargo-zigbuild   # already used in this ecosystem
```

> Pin `reqwest = "0.12"` explicitly. `0.13.x` exists on crates.io but `0.12` is the stable line the wider ecosystem (and `rustls`) is settled on; opt into `rustls-tls` to avoid a system OpenSSL dependency in prebuilt binaries.

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| `rmcp` (official SDK) | `rust-mcp-sdk` (rust-mcp-stack) | Mature community SDK with a different ergonomics model. Reasonable, but for a flagship MCP tool the **official** SDK is the safer long-term bet (protocol-version tracking, ecosystem gravity). Choose `rust-mcp-sdk` only if you hit a concrete rmcp limitation. |
| `rusqlite` (bundled) | `libsql` `0.9` | Use libsql if you later want Turso/embedded-replica sync or remote SQLite. Overkill for a strictly local, zero-cloud tool and a heavier dependency. |
| `rusqlite` (bundled) | `sqlx` `0.9` (sqlite) | Use sqlx if you want compile-time-checked queries and a fully async DB layer. But its async-over-SQLite adds complexity with no real win for a single-process embedded file, and the `sqlite-vec` auto-extension story is cleaner through rusqlite. |
| `sqlite-vec` | BLOB + in-Rust cosine similarity | Viable fallback if `sqlite-vec` fails to cross-compile for a target: store `f32` vectors as BLOBs, load candidates, cosine in Rust. Simple and dependency-free, but O(n) per query and no SQL-level KNN. Keep as the documented Plan B (HUB-style A4 fallback). |
| `reqwest` direct | `ollama-rs` `0.3` | Use `ollama-rs` if you need *broad* Ollama coverage (chat, streaming generate, model mgmt). We make essentially **one** call (embed); a typed wrapper adds a dependency + version-tracking burden for little gain. |
| `axum` | `actix-web` | Only if a non-tower stack were required — it isn't, and it would break the "rmcp HTTP service nests into the same router" win. |
| `cargo-dist` | Hand-rolled GitHub Actions matrix + `cargo-zigbuild` | Use hand-rolled if cargo-dist's runner-override story proves too fiddly against the self-hosted-only constraint. mcp-hub/injection-scanner **already ship this way** (zigbuild matrix on `orangepi`), so a hand-rolled path is a known-good, lower-magic option. Trade-off: you then write the Homebrew formula yourself. |

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| **`sqlite-vss`** | Deprecated/abandoned by its author (Faiss C++ integration pain); effort moved to sqlite-vec. | `sqlite-vec` |
| **Raw `sqlite3_auto_extension` + `std::mem::transmute`** for registering sqlite-vec | The pre-rusqlite-0.34 pattern **does not compile** against rusqlite 0.34+ (now `RawAutoExtension`). | Follow the current `sqlite-vec` Rust guide registration path for rusqlite 0.40. |
| **A separate vector database** (Qdrant/LanceDB/Chroma) | Reintroduces a service/process and breaks "zero cloud, zero account, single binary." | `sqlite-vec` inside the embedded DB |
| **`ubuntu-latest` / `macos-latest` in CI** (incl. cargo-dist's defaults) | **Banned org-wide**; cargo-dist emits these by default. | `arc-runner-unityinflow` (X64) / `orangepi` (ARM64) via custom-runner config |
| **`unwrap()` / `expect()` in daemon paths** | Ecosystem hard rule; a long-running daemon panic kills all sessions. | `?` + `thiserror`/`anyhow`, graceful error returns over MCP/REST |
| **A second async runtime / blocking threads for DB** | Fragments the executor. | One `tokio` runtime; `spawn_blocking` or a single writer task for rusqlite |
| **Ollama `/api/embeddings` (legacy, singular `prompt`)** | Older endpoint; the current batch-capable endpoint is `/api/embed` with `"input"`. | POST `/api/embed` `{ "model": "nomic-embed-text", "input": "..." }` → `embeddings: [[f32; 768]]` |

## Stack Patterns by Variant

**If you want the simplest single-process topology (recommended default):**
- Build **one `axum::Router`**: nest rmcp's `StreamableHttpService` at e.g. `/mcp`, mount REST routes at `/api/*`, bind one `tokio` listener.
- Also expose **stdio MCP** (`transport-io`) as the default `serve` mode for editor launch configs; HTTP is opt-in via a flag/port.
- Because all of rmcp, axum, reqwest, rusqlite live in one runtime, the decay/TTL sweeper is just a spawned `tokio::time::interval` task.

**If a target triple fails to build `sqlite-vec` C (cross-compile risk):**
- Fall back to **BLOB embeddings + in-Rust cosine** for that target (documented Plan B), keeping the same schema (`embedding BLOB`), so the storage format is forward-compatible.
- Validate the C-toolchain link for **every** target on `orangepi`/zigbuild **before** committing to cargo-dist, mirroring the HUB-V2 lesson where `cfg(unix)` deps blocked Windows.

**If Windows/macOS cross-compile is painful (ecosystem precedent: HUB-V2-01/02):**
- Ship **Linux x86_64 + aarch64 (gnu/musl)** first via zigbuild, document macOS/Windows as a follow-up — but note injection-scanner v0.0.2 *did* land apple-darwin via zigbuild on `orangepi`, so darwin is plausible from the start. Homebrew on Apple Silicon needs the darwin-aarch64 binary, so prioritize it if Homebrew is a launch goal.

## Version Compatibility

| Package A | Compatible With | Notes |
|-----------|-----------------|-------|
| `rusqlite 0.40` (`bundled`) | `sqlite-vec 0.1.9` | Registration API changed at **rusqlite 0.34** — use `RawAutoExtension`, not `transmute`. Follow the current sqlite-vec Rust guide; do not copy pre-0.34 snippets. |
| `rmcp 1.8` | `axum 0.8` + `tower 0.5`/`tower-http 0.6` | `StreamableHttpService` is a tower service nestable in an axum 0.8 router — versions align with mcp-hub's existing pins. |
| `rmcp 1.8` | (was `0.7→0.8` breaking) | rmcp had breaking changes across 0.7→0.8→1.x; pin a **specific** `1.x` and read release notes before bumping. |
| `reqwest 0.12` | `tokio 1`, `rustls` | Use `rustls-tls` (not default `native-tls`) so prebuilt binaries don't depend on a system OpenSSL. |
| `cargo-dist 0.32` | self-hosted runners | Requires `github-custom-runners` override (or post-gen patch); default `ubuntu-latest`/`macos-latest` violate org policy. |
| `nomic-embed-text` | `sqlite-vec vec0(embedding float[768])` | 768 dimensions (Matryoshka-capable 64–768; default 768). Schema must pin the dimension to match. |

## Sources

- [crates.io live API](https://crates.io/) — verified current stable versions: `rmcp 1.8.0`, `rusqlite 0.40.1`, `sqlite-vec 0.1.9`, `ollama-rs 0.3.5`, `axum 0.8.9`, `reqwest 0.13.4` (recommend pinning 0.12 line), `sqlx 0.9.0`, `libsql 0.9.30` — confidence HIGH
- [modelcontextprotocol/rust-sdk (rmcp) README](https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/README.md) — official SDK, feature flags, transports — confidence HIGH
- [docs.rs/rmcp](https://docs.rs/rmcp) — `StreamableHttpService`, transport feature names — confidence HIGH
- [Shuttle: Build a Streamable HTTP MCP Server in Rust](https://www.shuttle.dev/blog/2025/10/29/stream-http-mcp) — axum-nesting pattern for rmcp HTTP service — confidence MEDIUM
- [asg017/sqlite-vec](https://github.com/asg017/sqlite-vec) + [Using sqlite-vec in Rust](https://alexgarcia.xyz/sqlite-vec/rust.html) — successor to sqlite-vss, rusqlite auto-extension registration — confidence HIGH
- [sqlite-vec issue #206 — rusqlite 0.34 API change](https://github.com/asg017/sqlite-vec/issues/206) — `RawAutoExtension` registration nuance — confidence HIGH
- [rusqlite README + PR #176 (bundled)](https://github.com/rusqlite/rusqlite) — bundled feature semantics — confidence HIGH
- [ollama.com/library/nomic-embed-text](https://ollama.com/library/nomic-embed-text) — 768 dims, 8192 ctx, `/api/embed` `input` format — confidence HIGH
- [axodotdev/cargo-dist releases + CHANGELOG](https://github.com/axodotdev/cargo-dist/releases) — v0.32.0 (Dec 2025) active; Homebrew formula generation; custom runners — confidence HIGH
- Ecosystem precedent (in-repo): `07-mcp-hub/Cargo.toml` (axum 0.8, clap 4, tokio 1, thiserror 2, tower-http 0.6, dirs 5), `03-injection-scanner/Cargo.toml`, and CLAUDE.md Decisions Log (cargo-zigbuild on `orangepi`, HUB-V2 cross-compile lessons) — confidence HIGH

---
*Stack research for: local-first Rust MCP memory daemon*
*Researched: 2026-06-24*
