# agent-memory

**Persistent, cross-runtime, typed memory for AI agents — local, offline, no cloud.**

Tool 10 in the [UnityInFlow](https://github.com/UnityInFlow) ecosystem.

AI agents forget everything between sessions. `agent-memory` is a small
[MCP](https://modelcontextprotocol.io) stdio server backed by an embedded SQLite
database: any MCP-speaking runtime can store, search, list, and forget structured
memories, and **every runtime pointed at the same database file shares the same
memory** — so a decision recorded by one agent is recalled by the next, across
sessions and across tools.

- **Local & offline.** Embedded SQLite, zero accounts, zero network. Your memory
  never leaves your machine.
- **Typed memories.** Six categories — `DECISION`, `PATTERN`, `ERROR`, `TODO`,
  `ARCHITECTURE`, `CONSTRAINT` — so retrieval is structured, not a soup of text.
- **Decay, not deletion.** Memories fade in ranking over time (exponential decay;
  high-value pinned types fade slower) but are **never** deleted by decay. The only
  ways a memory is removed are an explicit TTL expiry or `memory_forget`.
- **Keyword search now.** v0.0.1 ranks by SQLite FTS5 relevance blended with decay.
  Semantic (embedding) search is Phase 2.

> **v0.0.1 is keyword-only and works fully offline — no Ollama, no embeddings, no
> network calls.** Semantic search via local Ollama embeddings lands in Phase 2.

## Install / Build

Requires a stable Rust toolchain (edition 2021).

```bash
git clone https://github.com/UnityInFlow/agent-memory
cd agent-memory
cargo build --release
# binary at target/release/agent-memory
```

Pre-built binaries and a Homebrew formula ship with the tagged release.

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

| Tool | Purpose |
|------|---------|
| `memory_store` | Persist a typed memory; returns its id. |
| `memory_search` | Keyword (FTS5) search ranked by relevance × decay; recency-bumped on retrieval. |
| `memory_list` | List memories newest-first, with optional type/tag/scope filters. |
| `memory_forget` | Delete a memory by id (clean not-found for unknown ids). |

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
is stored — the background sweep then removes it.

### `memory_search`

```json
{ "query": "maven group", "type": "DECISION", "limit": 10 }
```

Returns ranked results, each carrying a freshly-recomputed `decay_score`. A query
that matches nothing returns an empty list, never an error.

### `memory_list`

```json
{ "type": "TODO", "scope": "kore-runtime", "limit": 20 }
```

### `memory_forget`

```json
{ "id": 42 }
```

Returns `{ "id": 42, "deleted": true }`, or `deleted: false` for an unknown id.

## Lifecycle: TTL & decay

A background task sweeps on an interval while the server runs:

1. **TTL expiry** — rows whose `expires_at` is in the past are deleted.
2. **Decay materialization** — every survivor's `decay_score` is recomputed so
   ranking stays cheap between reads. Pinned types (`DECISION`, `ARCHITECTURE`,
   `CONSTRAINT`) decay markedly slower.

**Decay never deletes.** A memory with no TTL stays retrievable forever, however
low its decay score falls — it simply ranks lower. Removal happens only via TTL or
`memory_forget`.

## Development

```bash
cargo test --workspace          # unit + integration (incl. stdout-purity)
cargo clippy --workspace -- -D warnings
cargo fmt --check
cargo llvm-cov --workspace --fail-under-lines 80   # >80% coverage gate (CI-enforced)
```

CI runs fmt-check, clippy `-D warnings`, the full test suite, and the coverage gate
on the UnityInFlow self-hosted runners.

## License

MIT © Jiří Hermann
