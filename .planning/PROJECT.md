# agent-memory

## What This Is

A persistent, structured memory layer for AI agents, exposed over a standard MCP server interface. Agents store and recall typed memories (decisions, patterns, errors, todos, architecture, constraints) that survive session boundaries and work across runtimes (Claude Code, Cursor, etc.). Everything runs locally — embedded SQLite, local Ollama embeddings — with zero cloud dependency and zero account required.

Tool 10 in the [UnityInFlow](https://github.com/UnityInFlow) ecosystem (Phase 3).

## Core Value

An agent can persist a structured memory and retrieve the right one later — across sessions and across tools — through a standard MCP interface, with no cloud. If everything else fails, store → search → recall must work.

## Requirements

### Validated

(None yet — ship to validate)

### Active

<!-- v0.0.1 acceptance criteria from CLAUDE.md. Hypotheses until shipped. -->

- [ ] SQLite backend — zero cloud, zero account required
- [ ] MCP server interface — `memory_store`, `memory_search`, `memory_list`, `memory_forget`
- [ ] Memory types — DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT
- [ ] Decay scoring — unused memories fade over time (exponential decay)
- [ ] Semantic search via local Ollama embeddings (nomic-embed-text)
- [ ] Import from GSD STATE.md format (`agent-memory import --from gsd-state .planning/STATE.md`)
- [ ] TTL support — memories expire after a configurable period
- [ ] REST API for non-MCP integrations
- [ ] Pre-built binaries (macOS arm64/x86_64, Linux x86_64/aarch64, Windows) + Homebrew formula

### Out of Scope

- Cloud/hosted backend or any required account — defeats the zero-dependency, local-first value
- Non-local embedding providers (OpenAI/Anthropic embeddings) — Ollama keeps it fully local; remote is a later opt-in at most
- JVM/Python runtime dependency — Rust chosen specifically for a low-footprint, dependency-free daemon
- A frontend/GUI — backend + CLI + MCP first (ecosystem rule: no frontend until backend is working and tested)

## Context

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
| Rust (not JVM/Python) | Zero runtime dep, embedded SQLite, low footprint for a long-running daemon | — Pending |
| SQLite via `rusqlite` as storage | Zero-config local experience, no server process | — Pending |
| Local Ollama `nomic-embed-text` for embeddings | Keeps semantic search fully local — no cloud, no account | — Pending |
| MCP server as the primary interface | Standard, cross-runtime; the whole point is tool-agnostic memory | — Pending |
| REST API as secondary interface | Non-MCP integrations need a path in | — Pending |
| Exponential decay scoring | Unused memories should fade so recall stays relevant | — Pending |
| Make agent-memory its own git repo + `.planning/` | Matches sibling tools 02–09; lets GSD scope to the tool instead of the wrapper milestone | ✓ Good |

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
*Last updated: 2026-06-24 after initialization*
