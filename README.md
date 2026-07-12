# agent-memory

**Persistent, cross-runtime, typed memory for AI agents — local, offline, no cloud.**

Tool 10 in the [UnityInFlow](https://github.com/UnityInFlow) ecosystem.

AI agents forget everything between sessions. `agent-memory` is a small
[MCP](https://modelcontextprotocol.io) stdio server backed by an embedded SQLite
database: any MCP-speaking runtime can store, search, list, and forget structured
memories, and **every runtime pointed at the same database file shares the same
memory** — so a decision recorded by one agent is recalled by the next, across
sessions and across tools.

- **Local & offline.** Embedded SQLite, zero accounts, zero cloud. Your memory
  never leaves your machine.
- **Typed memories.** Six categories — `DECISION`, `PATTERN`, `ERROR`, `TODO`,
  `ARCHITECTURE`, `CONSTRAINT` — so retrieval is structured, not a soup of text.
- **Decay, not deletion.** Memories fade in ranking over time (exponential decay;
  high-value pinned types fade slower) but are **never** deleted by decay. The only
  ways a memory is removed are an explicit TTL expiry or `memory_forget`.
- **Search by meaning.** Semantic search via local
  [Ollama](https://ollama.com) embeddings when available, with automatic
  fallback to SQLite FTS5 keyword ranking when it isn't — both blended with
  decay. No Ollama, no network? Everything still works.

## Install

### Homebrew (macOS + Linux)

```bash
brew install unityinflow/tap/agent-memory
```

### Pre-built binaries

Download the tarball for your platform from the
[latest release](https://github.com/UnityInFlow/agent-memory/releases/latest):

| Platform | Asset |
|----------|-------|
| macOS arm64 (Apple Silicon) | `agent-memory-aarch64-apple-darwin.tar.gz` |
| macOS x86_64 (Intel) | `agent-memory-x86_64-apple-darwin.tar.gz` |
| Linux x86_64 (gnu) | `agent-memory-x86_64-unknown-linux-gnu.tar.gz` |
| Linux aarch64 (gnu) | `agent-memory-aarch64-unknown-linux-gnu.tar.gz` |
| Linux x86_64 (musl, static) | `agent-memory-x86_64-unknown-linux-musl.tar.gz` (when available) |
| Linux aarch64 (musl, static) | `agent-memory-aarch64-unknown-linux-musl.tar.gz` (when available) |

Verify the checksum against `SHA256SUMS.txt` (shipped with every release),
then extract:

```bash
shasum -a 256 -c SHA256SUMS.txt --ignore-missing   # (sha256sum on Linux)
tar -xzf agent-memory-<triple>.tar.gz
./agent-memory --version
```

### From source

Requires a stable Rust toolchain (edition 2021).

```bash
git clone https://github.com/UnityInFlow/agent-memory
cd agent-memory
cargo build --release
# binary at target/release/agent-memory
```

## Run

```bash
# Serve the MCP stdio server (creates the DB on first run).
agent-memory serve

# Choose the database file (default: your OS data dir; honors $AGENT_MEMORY_DB).
agent-memory --db /path/to/memory.db serve
AGENT_MEMORY_DB=/path/to/memory.db agent-memory serve
```

The server speaks JSON-RPC over **stdout**; all logs go to **stderr**, so stdout
stays a pure protocol channel.

## MCP configuration (`.mcp.json`)

Point every runtime at **one** database via `AGENT_MEMORY_DB` and they share memory:

```json
{
  "mcpServers": {
    "agent-memory": {
      "command": "agent-memory",
      "args": ["serve"],
      "env": {
        "AGENT_MEMORY_DB": "/Users/you/.agent-memory/memory.db"
      }
    }
  }
}
```

Drop the same block into each agent runtime's MCP config; because they open the
same `AGENT_MEMORY_DB` file (WAL-mode SQLite with a serialized writer), a memory
stored in one is immediately retrievable in another.

## The four tools

| Tool | Purpose | Validated parameters |
|------|---------|----------------------|
| `memory_store` | Persist a typed memory; returns its id. | `ttl_secs`: optional integer, valid range 1–3155760000 (~100 years); omitted = no expiry |
| `memory_search` | Semantic (embedding) search when Ollama is reachable, FTS5 keyword otherwise — ranked by relevance × decay, recency-bumped on retrieval. | `limit`: optional integer, valid range 1–200; omitted defaults to 50. `tag`: exact tag match (case-sensitive) |
| `memory_list` | List memories newest-first, with optional type/tag/scope filters. | `limit`: optional integer, valid range 1–200; omitted = unlimited. `tag`: exact tag match (case-sensitive) |
| `memory_forget` | Delete a memory by id (clean not-found for unknown ids). | — |

Out-of-bounds values are rejected as client errors (`invalid_params` over MCP)
with a message naming the field, the valid range, and the offending value —
e.g. `invalid argument: limit must be between 1 and 200 (got 0)`. They are
never silently clamped and never produce an internal error.

### `memory_store`

```json
{
  "content": "We chose io.github.unityinflow as the Maven group.",
  "type": "DECISION",
  "tags": ["maven", "release"],
  "scope": "kore-runtime",
  "ttl_secs": null
}
```

`content` and `type` are required; `tags`, `source`, `scope`, and `ttl_secs` are
optional. A non-null `ttl_secs` makes the memory expire that many seconds after it
is stored — the background sweep then removes it. A present `ttl_secs` must be
between 1 and 3155760000 (~100 years); anything else is rejected with the range
in the error message.

### `memory_search`

```json
{ "query": "maven group", "type": "DECISION", "limit": 10 }
```

Returns an envelope carrying the mode actually used plus the ranked results,
each with a freshly-recomputed `decay_score`:

```json
{ "search_mode": "semantic", "results": [ ... ] }
```

`search_mode` is `"semantic"` when the query was answered via embeddings and
`"keyword"` when the FTS5 path served it (Ollama absent or unreachable). A
query that matches nothing returns an empty list, never an error. A present
`limit` must be between 1 and 200 (omitted defaults to 50); a `tag` filter
matches whole tags exactly and case-sensitively.

### `memory_list`

```json
{ "type": "TODO", "scope": "kore-runtime", "limit": 20 }
```

A present `limit` must be between 1 and 200 (omitted returns everything); a
`tag` filter matches an exact tag (case-sensitive).

### `memory_forget`

```json
{ "id": 42 }
```

Returns `{ "id": 42, "deleted": true }`, or `deleted: false` for an unknown id.

## Semantic search (optional Ollama)

Out of the box, search uses SQLite FTS5 keyword ranking — fully offline, no
extra software. To rank by *meaning* instead, run a local
[Ollama](https://ollama.com) and pull the embedding model:

```bash
ollama pull nomic-embed-text
```

That's the whole setup. When Ollama is reachable, stores embed content
automatically and searches use vector KNN (cosine) blended with decay. Point
at a non-default Ollama with either:

```bash
agent-memory --ollama-url http://localhost:11434 serve
AGENT_MEMORY_OLLAMA_URL=http://localhost:11434 agent-memory serve
```

**Graceful fallback, always.** If Ollama is stopped, absent, or the model
isn't pulled:

- stores still succeed — content is queued (`embedding_status = 0`) and
  backfilled by the background sweep once Ollama returns;
- searches transparently fall back to the FTS5 keyword path and report it via
  `"search_mode": "keyword"`;
- nothing errors, nothing blocks. The tool works fully offline without Ollama.

## REST API

For non-MCP integrations, the same store and logic are exposed over HTTP:

```bash
agent-memory serve-rest                       # binds 127.0.0.1:7437 by default
agent-memory serve-rest --addr 127.0.0.1:8080 # custom loopback bind
```

| Method | Path | Purpose | Validated parameters |
|--------|------|---------|----------------------|
| `POST` | `/api/memories` | Store a memory (same fields as `memory_store`) | `ttl_secs`: 1–3155760000 when present; omitted = no expiry |
| `GET` | `/api/memories` | List, with optional `type`/`tag`/`scope`/`limit` query params | `limit`: 1–200 when present; omitted = unlimited. `tag`: exact tag (case-sensitive) |
| `POST` | `/api/search` | Search — returns the same `{search_mode, results}` envelope | `limit`: 1–200 when present; omitted defaults to 50. `tag`: exact tag (case-sensitive) |
| `DELETE` | `/api/memories/{id}` | Forget by id (404 with a structured body for unknown ids) | — |
| `GET` | `/health` | Liveness + embedder reachability status | — |

Out-of-bounds `limit`/`ttl_secs` values return **HTTP 400** with a JSON body
whose `error` field names the field, range, and offending value — shaped
`invalid argument: limit must be between 1 and 200 (got 0)` — never a 500 and
never a silent clamp. The MCP transport rejects the same inputs identically as
`invalid_params`, because validation lives once at the shared service seam.

The REST daemon and the MCP stdio server can run concurrently against the same
database file (WAL-mode SQLite with a serialized writer).

**Security:** the API is unauthenticated and binds **loopback-only by
default**. Binding a non-loopback address requires the explicit
`--allow-remote` flag, and the server logs a loud warning naming the exposure
when you do. **Never expose the REST API publicly** — anyone who can reach it
can read and delete every memory. If you need remote access, keep it behind a
VPN/SSH tunnel or an authenticating reverse proxy.

## Behavioral changes in v0.1.0

Two deliberate contract changes from v0.0.1 (both apply identically to MCP and
REST):

1. **Tag filtering is now an exact, case-sensitive match** instead of a
   substring match — `tag=rust` no longer matches a memory tagged `rustling`,
   and `Rust` does not match `rust`. This applies to search (semantic and
   keyword paths) and list.
2. **Out-of-bounds `limit` and `ttl_secs` now return client errors** (HTTP 400
   / MCP `invalid_params`) carrying the valid range, instead of being silently
   clamped or accepted. Valid boundaries (`limit` 1–200, `ttl_secs`
   1–3155760000) and omitted values behave exactly as before.

## Import from GSD

Bootstrap your memory from an existing [GSD](https://github.com/gsd-build/get-shit-done)
project's state:

```bash
agent-memory import --from gsd-state .planning/STATE.md
```

| STATE.md section | Imported as |
|------------------|-------------|
| `### Decisions` bullets | `DECISION` |
| `### Blockers/Concerns` bullets | `CONSTRAINT` |
| `### Pending Todos` bullets | `TODO` |
| `## Deferred Items` table rows | `TODO` (tagged `deferred`) |

Every imported memory carries `source = "gsd-state"`, the `gsd` tag, and a
scope (the project directory containing `.planning` by default; override with
`--scope`). Re-running the same import is a **no-op**: unchanged items are
deduplicated on the exact (source, type, content) key and reported as skipped.

## Lifecycle: TTL & decay

A background task sweeps on an interval while the server runs:

1. **TTL expiry** — rows whose `expires_at` is in the past are deleted.
2. **Decay materialization** — every survivor's `decay_score` is recomputed so
   ranking stays cheap between reads. Pinned types (`DECISION`, `ARCHITECTURE`,
   `CONSTRAINT`) decay markedly slower.
3. **Embedding backfill** — memories stored while Ollama was unreachable are
   embedded in batches once it comes back.

**Decay never deletes.** A memory with no TTL stays retrievable forever, however
low its decay score falls — it simply ranks lower. Removal happens only via TTL or
`memory_forget`.

## Development

```bash
cargo test --workspace          # unit + integration (incl. stdout-purity)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo llvm-cov --workspace --fail-under-lines 80   # >80% coverage gate (CI-enforced)
```

CI runs fmt-check, clippy `-D warnings`, the full test suite, and the coverage gate
on the UnityInFlow self-hosted runners. See [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

MIT © Jiří Hermann
