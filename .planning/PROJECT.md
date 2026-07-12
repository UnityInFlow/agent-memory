# agent-memory

## What This Is

A persistent, structured memory layer for AI agents, exposed over a standard MCP server interface. Agents store and recall typed memories (decisions, patterns, errors, todos, architecture, constraints) that survive session boundaries and work across runtimes (Claude Code, Cursor, etc.). Everything runs locally — embedded SQLite, local Ollama embeddings — with zero cloud dependency and zero account required.

Tool 10 in the [UnityInFlow](https://github.com/UnityInFlow) ecosystem (Phase 3).

## Core Value

An agent can persist a structured memory and retrieve the right one later — across sessions and across tools — through a standard MCP interface, with no cloud. If everything else fails, store → search → recall must work.

## Requirements

### Validated

<!-- v0.0.1 shipped 2026-07-03 (Release v0.0.1, Homebrew tap); phases 1–2 verified 15/15 must-haves. -->

- ✓ SQLite backend — zero cloud, zero account required — Phase 1
- ✓ MCP server interface — `memory_store`, `memory_search`, `memory_list`, `memory_forget` — Phase 1
- ✓ Memory types — DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT — Phase 1
- ✓ Decay scoring — exponential decay, re-ranks but never deletes — Phase 1
- ✓ TTL support — expiry via hourly sweep; TTL and explicit forget are the only removal paths — Phase 1
- ✓ Semantic search via local Ollama embeddings (nomic-embed-text), graceful keyword/FTS5 fallback — Phase 2
- ✓ REST API for non-MCP integrations (loopback-guarded) — Phase 2
- ✓ Import from GSD STATE.md format (`agent-memory import --from gsd-state`), idempotent — Phase 2
- ✓ Pre-built binaries (macOS arm64/x86_64, Linux x86_64/aarch64 gnu) + Homebrew formula — Phase 2

### Active

(None — v1.0 milestone complete. v2 candidates live in REQUIREMENTS.md: SEARCH-04 hybrid RRF, MCP-06 update/relations, DIST-03 Windows, DIST-04 export.)

### Out of Scope

- Cloud/hosted backend or any required account — defeats the zero-dependency, local-first value
- Non-local embedding providers (OpenAI/Anthropic embeddings) — Ollama keeps it fully local; remote is a later opt-in at most
- JVM/Python runtime dependency — Rust chosen specifically for a low-footprint, dependency-free daemon
- A frontend/GUI — backend + CLI + MCP first (ecosystem rule: no frontend until backend is working and tested)

## Context

- **Current state (v1.0 milestone, shipped 2026-07-12):** v0.0.1 released publicly — GitHub Release with 4 checksummed target tarballs (macOS arm64/x86_64, Linux x86_64/aarch64 gnu) + Homebrew tap (`brew install unityinflow/tap/agent-memory`). ~5,400 LOC Rust across `agent-memory-core` + `agent-memory` crates; clippy `-D warnings` clean; >80% coverage CI gate; 25/25 STRIDE threats closed. Known v2 debt: musl binaries (sqlite-vec BSD typedefs), Windows (cfg(unix) refactor), REST boundary-value hardening, SEARCH-04 hybrid RRF, MCP-06 update/relations, DIST-04 export.
- **Ecosystem position:** Tool 10 of 20 in UnityInFlow. Phase 3. Its own `CLAUDE.md` marks it *"Planned — no strict blocking dependencies,"* so it can proceed in parallel with the active Phase 2 close-out (budget-breaker starter, kore v0.1.0).
- **Why it exists:** Every agent tool reinvents state storage (GSD has STATE.md, Superpowers has skill context, RTK has its own SQLite DB). Switching runtimes loses all project context. A standard MCP memory API works across tools and survives sessions.
- **Harness already set up:** RTK (global), Superpowers, GSD, and memtrace are wired; `.claude/` has Rust-adapted hooks (pre-bash safety, rustfmt-on-write, clippy+test on Stop); `.mcp.json` enables context7. The repo is its own git repo and indexed in memtrace.
- **Reusable infra:** Sibling Rust tools (injection-scanner, mcp-hub) established cross-platform binary CI on the `orangepi`/Hetzner self-hosted runners — reuse that release pipeline.
- **Reference docs:** `10-agent-memory.md` (feature spec, schema/decay/search notes, build todos), `CLAUDE.md` (constraints + acceptance criteria).

## Constraints

- **Tech stack**: Rust stable, edition 2021 — zero runtime dependency, low-memory long-running daemon
- **Storage**: SQLite via `rusqlite`, embedded — no DB server process
- **Embeddings**: Ollama (`nomic-embed-text`), fully local — no cloud embedding calls
- **Interfaces**: MCP server (primary) + REST API (secondary)
- **Code rules**: no `unwrap()` in production code (use `?`); exhaustive pattern matching; `clap` (derive) for CLI; `serde`/`serde_json`; `tokio` for async; `anyhow` (binary) / `thiserror` (library)
- **Quality**: >80% test coverage on core logic before release; `cargo clippy -- -D warnings` clean; `cargo fmt`
- **Security**: no secrets committed; everything local
- **License**: MIT
- **CI**: UnityInFlow self-hosted runners only (`arc-runner-unityinflow` X64 default, `orangepi` ARM64) — never `ubuntu-latest`
- **Distribution**: pre-built binaries (macOS/Linux/Windows) + Homebrew

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust (not JVM/Python) | Zero runtime dep, embedded SQLite, low footprint for a long-running daemon | ✓ Good — v0.0.1 shipped as a single static binary |
| SQLite via `rusqlite` as storage | Zero-config local experience, no server process | ✓ Good — WAL + single-writer lane held up across MCP+REST sharing one file |
| Local Ollama `nomic-embed-text` for embeddings | Keeps semantic search fully local — no cloud, no account | ✓ Good — with keyword/FTS5 fallback when Ollama is absent (SC2) |
| MCP server as the primary interface | Standard, cross-runtime; the whole point is tool-agnostic memory | ✓ Good — stdio purity held (MCP-05); brewed binary launches from .mcp.json |
| REST API as secondary interface | Non-MCP integrations need a path in | ✓ Good — shared SearchOutcome envelope keeps MCP/REST wire shapes aligned |
| Exponential decay scoring | Unused memories should fade so recall stays relevant | ✓ Good — recompute-on-read + hourly materialization sweep agree on the math |
| Make agent-memory its own git repo + `.planning/` | Matches sibling tools 02–09; lets GSD scope to the tool instead of the wrapper milestone | ✓ Good |
| Decay never deletes — TTL sweep and explicit forget are the only removal paths | Predictable data lifecycle; decay only down-ranks (STORE-03/04) | ✓ Good — locked by kill-tests |
| v0.0.1 ships gnu-only Linux binaries (no musl) | sqlite-vec.c uses BSD `u_int*_t` typedefs musl lacks; gnu covers Linux | Accepted — upstream fix or CFLAGS shim in v2 |
| chrono trimmed to `default-features=false, features=["now"]` (UTC-only) | Default clock feature pulls core-foundation-sys, un-linkable by zig darwin cross | ✓ Good — Local time must never be reintroduced |
| Two-tier error taxonomy at the store seam (InvalidQuery → 400/invalid_params; internal → 500/internal_error) | Bad client input must never read as a server fault, on either transport | ✓ Good — proven live at 4 layers (02-05) |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-07-12 after v1.0 milestone (v0.0.1 released)*
