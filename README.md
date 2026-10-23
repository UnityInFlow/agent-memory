# agent-memory

**Persistent, cross-runtime, typed memory for AI agents — local, offline, no cloud.**

Tool 10 in the [UnityInFlow](https://github.com/UnityInFlow) ecosystem.

AI agents forget everything between sessions. `agent-memory` is a small
[MCP](https://modelcontextprotocol.io) stdio server backed by an embedded SQLite
database: any MCP-speaking runtime can store, search, list, update, and forget
structured memories, and **every runtime pointed at the same database file shares the same
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

## The seven tools

| Tool | Purpose | Validated parameters |
|------|---------|----------------------|
| `memory_store` | Persist a typed memory; returns its id. | `ttl_secs`: optional integer, valid range 1–3155760000 (~100 years); omitted = no expiry |
| `memory_search` | Semantic (embedding) search when Ollama is reachable, FTS5 keyword otherwise — ranked by relevance × decay, recency-bumped on retrieval. | `limit`: optional integer, valid range 1–200; omitted defaults to 50. `tag`: exact tag match (case-sensitive). `expand_links`: optional boolean, default `false` |
| `memory_list` | List memories newest-first, with optional type/tag/scope filters. | `limit`: optional integer, valid range 1–200; omitted = unlimited. `tag`: exact tag match (case-sensitive). `expand_links`: optional boolean, default `false` |
| `memory_update` | Patch a memory in place by id — content, tags, scope, TTL (clean not-found for unknown ids). | `content`: never null or empty. `ttl_secs`: 1–3155760000 when set; `null` = no expiry. At least one field besides `id` |
| `memory_link` | Link two memories with a typed, directed relation (idempotent; clean not-found for unknown ids). | `kind`: one of `relates_to`, `supersedes`, `caused_by`. `from_id` ≠ `to_id` |
| `memory_unlink` | Remove a relation (clean not-found when no such edge exists). | `kind`: one of `relates_to`, `supersedes`, `caused_by` |
| `memory_forget` | Delete a memory by id (clean not-found for unknown ids). Its relations go with it. | — |

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

Both `memory_search` and `memory_list` accept `"expand_links": true` to attach
each result's linked memories — see [Relations](#relations-memory_link--memory_unlink).

### `memory_forget`

```json
{ "id": 42 }
```

Returns `{ "id": 42, "deleted": true }`, or `deleted: false` for an unknown id.

### `memory_update`

Patch an existing memory **in place** — its id never changes. The arguments
are a JSON merge-patch: each field you send is applied, each field you omit is
left alone, and an explicit `null` clears the field where clearing is legal.

```json
{ "id": 42, "content": "We chose io.github.unityinflow as the Maven group (Sonatype-verified).", "ttl_secs": null }
```

| Field | Omitted | `null` | A value |
|-------|---------|--------|---------|
| `content` | unchanged | **rejected** (`invalid_params`) | replaced, then re-indexed and re-embedded |
| `tags` | unchanged | cleared to `[]` | replaced (the whole list) |
| `scope` | unchanged | reset to global (`null`) | replaced |
| `ttl_secs` | unchanged (the existing expiry stays) | expiry removed — the memory never expires | a new TTL counted from **now** (1–3155760000) |

`type` and `source` cannot be changed; to retype a memory, store a new one and
forget the old. A patch with no field besides `id`, empty or whitespace-only
`content`, or an out-of-bounds `ttl_secs` is rejected as `invalid_params`
before anything is written.

On success the result is the full updated memory — the same fields `memory_list`
returns — plus `embedding_status`:

```json
{ "id": 42, "content": "We chose io.github.unityinflow as the Maven group (Sonatype-verified).",
  "type": "DECISION", "tags": ["maven", "release"], "scope": "kore-runtime",
  "decay_score": 1.0, "created_at": 1767225600, "last_accessed": 1767312000,
  "embedding_status": 1 }
```

- **New content changes the meaning, everywhere.** The keyword index and the
  embedding are replaced in the same transaction, so a search for the old
  meaning stops matching the memory and the new meaning finds it.
- **`embedding_status`** is `1` when the stored vector matches the current
  content and `0` when a re-embed is pending — the update succeeded while Ollama
  was unreachable, and the background sweep re-embeds it once Ollama is back.
  Until then semantic search cannot match it by meaning; keyword-fallback
  search already sees the new content.
- **An update counts as a use.** It refreshes `last_accessed` (and the access
  count), so an edited memory ranks as current rather than decayed.

For an unknown id the result is `{ "id": 42, "updated": false, "reason": "not_found" }`
— a normal tool result, not an error.

### Relations (`memory_link` / `memory_unlink`)

Connect memories with flat, typed relations — no graph model, just one directed
edge `from_id -[kind]-> to_id` per link:

| `kind` | Reads as | Seen from `to_id` (expansion) |
|--------|----------|-------------------------------|
| `relates_to` | `from_id` relates to `to_id` (symmetric "see also") | `relates_to` |
| `supersedes` | `from_id` replaces the older `to_id` | `superseded_by` |
| `caused_by` | `from_id` was caused by `to_id` | `caused` |

```json
{ "from_id": 57, "to_id": 42, "kind": "supersedes" }
```

`memory_link` returns `{ "from_id": 57, "to_id": 42, "kind": "supersedes", "linked": true }`.

- **Idempotent.** Linking the same `(from_id, to_id, kind)` again is a no-op
  success — safe to retry. A different `kind` between the same pair is a
  separate relation.
- **Rejected as `invalid_params`:** a self-link (`from_id and to_id must differ
  (got 57)`) and an unknown kind (`kind must be one of relates_to, supersedes,
  caused_by (got 'blocks')`).
- **Unknown ids are a clean not-found:** `{ ..., "linked": false, "reason":
  "not_found", "missing_id": 42 }` names the id that does not exist. Nothing is
  written.

`memory_unlink` takes the same three fields; direction and kind must match the
stored edge. It returns `"unlinked": true`, or `"unlinked": false, "reason":
"not_found"` when no such edge exists.

**Relations never outlive their memories.** Forgetting a memory, or its TTL
expiring, removes every relation touching it in the same transaction.

**Expansion.** Pass `"expand_links": true` to `memory_search` or
`memory_list` and every result gains a `related` list of trimmed 1-hop
summaries, following relations in both directions:

```json
{ "id": 42, "content": "...", "type": "DECISION", "...": "...",
  "related": [
    { "id": 57, "kind": "superseded_by", "direction": "in",
      "type": "DECISION", "snippet": "Maven group is io.github.unityinflow (Sonatype-verified).", "tags": ["maven"] }
  ] }
```

- `direction` is `out` when the result is the relation's `from_id` and `in`
  when it is the `to_id`; `kind` reads naturally from the result's side (the
  table above).
- `snippet` is the neighbor's content cut to 160 characters, with a trailing
  `…` when cut. Fetch the neighbor by id if you need all of it.
- At most `MAX_EXPANDED_LINKS = 10` neighbors per result; when a memory has
  more, the most recently created relations win.
- Expansion is strictly one hop and a pure read: neighbors are **not**
  recency-bumped, so expanding never changes how anything decays.
- A result with no relations gets `"related": []`. Without the flag there is no
  `related` member at all — the response is exactly the v0.0.1 shape.

#### Bounds

| Bound | Value | Applies to |
|-------|-------|------------|
| `limit` | 1–200 (`MAX_LIMIT`) | `memory_search` (default 50), `memory_list` (default unlimited) |
| `ttl_secs` | 1–3155760000 (~100 years) | `memory_store`, `memory_update` |
| `MAX_EXPANDED_LINKS` | 10 neighbors per result, most recent first | `expand_links` on search and list |
| snippet length | 160 characters (+ `…` when cut) | `related[].snippet` |

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
| `GET` | `/api/memories` | List, with optional `type`/`tag`/`scope`/`limit`/`expand_links` query params | `limit`: 1–200 when present; omitted = unlimited. `tag`: exact tag (case-sensitive). `expand_links=true` attaches `related` summaries |
| `POST` | `/api/search` | Search — returns the same `{search_mode, results}` envelope | `limit`: 1–200 when present; omitted defaults to 50. `tag`: exact tag (case-sensitive). `"expand_links": true` attaches `related` summaries |
| `PATCH` | `/api/memories/{id}` | Update in place — JSON merge-patch body with the same fields and semantics as `memory_update`; 200 returns the updated memory with `embedding_status` | as `memory_update`; 404 `{"id", "updated": false, "reason": "not_found"}` for unknown ids |
| `DELETE` | `/api/memories/{id}` | Forget by id (404 with a structured body for unknown ids) | — |
| `POST` | `/api/links` | Link — JSON body `{from_id, to_id, kind}`, same semantics as `memory_link`; 200 (also for a duplicate) | 400 for a self-link, an unknown `kind` or a bad body; 404 `{..., "linked": false, "reason": "not_found", "missing_id"}` for unknown ids |
| `DELETE` | `/api/links/{from_id}/{to_id}/{kind}` | Unlink — same semantics as `memory_unlink` | 400 for an unknown `kind`; 404 `{..., "unlinked": false, "reason": "not_found"}` when no such edge exists |
| `GET` | `/health` | Liveness + embedder reachability status | — |

Out-of-bounds `limit`/`ttl_secs` values return **HTTP 400** with a JSON body
whose `error` field names the field, range, and offending value — shaped
`invalid argument: limit must be between 1 and 200 (got 0)` — never a 500 and
never a silent clamp. The MCP transport rejects the same inputs identically as
`invalid_params`, because validation lives once at the shared service seam. A
`PATCH` body that is not valid JSON, has a wrongly typed field, or is sent
without `Content-Type: application/json` is also a 400 with the same
`{"error": ...}` body.

```bash
curl -X PATCH localhost:7437/api/memories/42 \
  -H 'content-type: application/json' \
  -d '{"tags": ["maven"], "ttl_secs": null}'

curl -X POST localhost:7437/api/links \
  -H 'content-type: application/json' \
  -d '{"from_id": 57, "to_id": 42, "kind": "supersedes"}'

curl 'localhost:7437/api/memories?type=DECISION&expand_links=true'
```

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

Reads and updates both count as use: a search result is recency-bumped when it
is returned, and `memory_update` resets the memory's decay clock. A TTL can be
added, replaced (counted from the time of the update) or removed with
`memory_update`.

## Development

```bash
cargo test --workspace          # unit + integration (incl. stdout-purity)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo llvm-cov --workspace --fail-under-lines 80   # >80% coverage gate (CI-enforced)
```

CI runs fmt-check, clippy `-D warnings`, the full test suite, and the coverage gate
on GitHub-hosted runners (secretless, read-only token). See [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

MIT © Jiří Hermann
