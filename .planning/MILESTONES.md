# Milestones

## v1.0 MVP (Shipped: 2026-07-12)

**Phases completed:** 2 phases, 8 plans, 19 tasks

**Key accomplishments:**

- An rmcp stdio MCP server exposing memory_store + memory_list over a WAL+single-writer SQLite store (schema, migrations, FTS5 mirror) with stdout-purity, injectable Clock, and durable-across-restart persistence — the thin end-to-end slice.
- FTS5 keyword `memory_search` ranked by a negated-bm25 × on-read-recomputed-decay blend (decay surfaced on every result, recency-bumped on retrieval) plus delete-by-id `memory_forget` with clean not-found — completing the four-tool offline MCP surface (store → search → forget over stdio).
- A background `tokio::time::interval` sweep that deletes TTL-expired rows then materializes every survivor's decay score (decay NEVER deletes — only TTL and `memory_forget` remove), wired before `serve(stdio())` with stderr-only logging, plus the self-hosted-runner CI workflow (fmt/clippy/test + >80% llvm-cov gate) and a README — completing and shipping the Phase-1 memory foundation.
- Ollama (nomic-embed-text) semantic search over a sqlite-vec vec0 sidecar with similarity×decay blending, degrading loudly-but-gracefully to the Phase-1 FTS5 keyword path whenever the embedder is unreachable — every result carries `search_mode`
- axum 0.8 `serve-rest` daemon mirroring the four memory operations + /health as thin adapters over the same MemoryService and WAL SQLite file the MCP stdio server uses, loopback-bound by default with an explicit `--allow-remote` escape hatch
- One-command GSD STATE.md import: tolerant line-scanner parser → typed gsd-tagged memories, exact-key dedup proven idempotent through the real binary (second run `imported: 0`), with one best-effort batch embed
- v0.0.1 publicly released: orangepi zigbuild pipeline shipped checksummed tarballs for all 4 DIST-01 triples after the darwin spike gate passed, and `brew install unityinflow/tap/agent-memory` installs a working MCP server on this Mac from the new formula
- Keyword-fallback search now binds the tag filter (bound ?10 predicate mirroring knn_search) and malformed FTS5 queries surface as MemoryError::InvalidQuery → REST 400 / MCP invalid_params via a shared two-tier error mapping — closing both 02-VERIFICATION.md gaps (CR-01, WR-05, WR-04)

---
