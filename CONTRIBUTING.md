# Contributing to agent-memory

Thanks for your interest in contributing! This document covers everything you
need to build, test, and submit changes.

## Prerequisites

- **Rust stable** (edition 2021) — install via [rustup](https://rustup.rs)
- No other toolchain is required: SQLite is bundled (compiled from source by
  `rusqlite`), and Ollama is an optional runtime dependency only.

## Build and test

```bash
cargo build --workspace            # full build
cargo test --workspace             # unit + integration tests (incl. stdout-purity)
```

## Lint gates (must pass before every commit)

```bash
cargo fmt                                            # format (run before committing)
cargo fmt --check                                    # CI-enforced
cargo clippy --workspace --all-targets -- -D warnings   # zero warnings tolerated
```

## Coverage gate

Core logic must stay above **80% line coverage**:

```bash
cargo install cargo-llvm-cov --locked
cargo llvm-cov --workspace --fail-under-lines 80
```

This gate is CI-enforced on every push/PR and re-verified in the release
pipeline.

## Commit messages

Use conventional prefixes:

```
feat: add semantic search fallback mode
fix: vec_memories orphan cleanup on TTL sweep
test: add idempotency cases for gsd-state import
docs: document --allow-remote semantics
chore: pin cargo-zigbuild 0.23.0
refactor: extract decay blend into helper
```

## Hard rules

- **No `unwrap()` in production code** — use `?` or handle the error
  explicitly. `unwrap`/`expect` are acceptable only inside `#[cfg(test)]`.
- **Stdout purity (MCP-05):** when `agent-memory serve` runs, stdout is a pure
  JSON-RPC channel. ALL logging goes to stderr (`tracing` subscriber). Never
  `println!` in any code path reachable from `serve` — the
  `stdio_purity` integration test enforces this.
- Pattern match exhaustively — avoid catch-all `_` arms unless truly needed.
- No secrets, API keys, or tokens committed — credentials via environment
  variables only.

## CI

CI (`.github/workflows/ci.yml`) runs on **GitHub-hosted `ubuntu-latest`**,
secretless, with a read-only token. This repo is public and the UnityInFlow
org runner group does not serve public repositories, so self-hosted runners
are not an option here. PRs, including fork PRs, run the gates listed above:
fmt check, clippy `-D warnings`, the full workspace test suite, and the >80%
llvm-cov line coverage gate.

## Releases (maintainers)

Releases are triggered by pushing an exact semver tag `vX.Y.Z`. Other
`v`-prefixed tags, such as GSD milestone tags (`vX.Y-milestone`),
deliberately do not trigger `.github/workflows/release.yml`.

The release workflow runs on GitHub-hosted runners. It uses only the built-in
`GITHUB_TOKEN`, with no org secret, and fires only on tag pushes, which forks
cannot make (issue #1, the same exception as injection-scanner #45). It
cross-compiles all supported triples via `cargo-zigbuild`, packages
per-triple tarballs, generates `SHA256SUMS.txt`, attests every tarball and
`SHA256SUMS.txt` with signed SLSA build provenance, and publishes the GitHub
Release. Verify a downloaded asset with:

```bash
gh attestation verify agent-memory-<triple>.tar.gz --repo UnityInFlow/agent-memory
```

The Homebrew formula in `UnityInFlow/homebrew-tap` is updated manually
afterwards. `.github/workflows/spike-cross-compile.yml` is a manually
dispatched Windows + musl feasibility spike (Phase 3), not a release gate.
