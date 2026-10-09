---
phase: quick-261009-hc1
plan: 01
status: complete
completed: 2026-10-09
requirements: [ISSUE-1]
commits: [ff9ab48, 043f444, 9810e31]
---

# Quick 261009-hc1: hosted release pipeline with SLSA provenance (closes #1)

**`release.yml` now runs on GitHub-hosted runners, so a v0.1.0 tag can actually be scheduled.**
The repo is public and the org runner group enforces `allows_public_repositories: false`, so the
self-hosted jobs could never run. The workflow now also carries signed build provenance, gives
write and OIDC scopes to the release job only, and pins every action by SHA.

## Commits

| Commit | Change |
|---|---|
| `ff9ab48` | release.yml: all jobs on `ubuntu-latest`, `contents: read` default, the release job alone gets contents/id-token/attestations write, `actions/attest-build-provenance` over every tarball and SHA256SUMS.txt before the Release, all actions SHA-pinned, semver-only trigger, corrected header and release notes |
| `043f444` | ci.yml and spike-cross-compile.yml pinned to the same SHAs, with `toolchain: stable` passed explicitly |
| `9810e31` | CONTRIBUTING.md and README.md describe the real runner policy and the `gh attestation verify` command |

## Decisions

- **The release-side runner decision Phase 3 D-12 deferred to Phase 6 is resolved: hosted runners, issue #1.**
- Planner additions beyond the original request:
  - **(a) Trigger narrowed from `v*` to `v[0-9]+.[0-9]+.[0-9]+`.** Run 29185695331 fired on the GSD tag
    `v1.0-milestone`. On a hosted runner it would have published that tag as a real Release.
    Side effect: pre-release tags such as `v0.1.0-rc.1` don't trigger either. Widen this in Phase 6
    if a pre-release dry run needs it.
  - **(b) ci.yml and the spike use the same pins.** ci.yml runs on this PR, so it is the only
    pre-merge proof that the checkout v7.0.1 and rust-toolchain pins work in this repo.
  - **(c) CONTRIBUTING.md and README.md corrected.** CLAUDE.md is untracked, so the public copy of
    the policy has to live in tracked docs.
- **CLAUDE.md was edited locally only.** The file is untracked (never committed, absent from
  origin/main), so its corrected "CI / Self-Hosted Runners" section isn't part of the PR.
- Upgrades: checkout v5 → v7.0.1; upload-artifact v5 → v7.0.1 and download-artifact v5 → v8.0.1
  move together, as the matched pair proven in injection-scanner's releases.

## Verification

- Task 1 structural check: `release.yml structure OK`. This checks the trigger shape and
  semver-only filter, the read-only default, the three-scope release permissions, all six matrix
  legs unchanged, the step order SHA256SUMS → attest → release, both subject paths, the verify
  line in the release body, a 40-hex pin on every `uses:`, and `toolchain: stable` on every
  rust-toolchain step.
- `actionlint .github/workflows/*.yml`: clean. Before the change it reported three unknown
  runner-label errors.
- Exactly three `runs-on: ubuntu-latest` lines; no self-hosted label in any workflow; no
  `secrets.` expression; all six pin strings present.
- Task 2: every `uses:` across all three workflows is pinned with a version comment; 4 of 4
  rust-toolchain steps pass `toolchain: stable`.
- Task 3: all doc gates green, CLAUDE.md still untracked, and the branch touches only the five
  intended tracked files.
- **One planned gate deliberately not met, and the reason.** The check "the diff removes no line
  containing `tar.gz`" flags exactly one line: the old header comment "...adapted to tar.gz
  packaging: ONE self-hosted Linux host". The plan itself orders that comment rewritten, and
  another of its gates forbids "ONE self-hosted". No asset-contract, checksum, upload, `files:`
  or release-table line changed.
- The executor step ran inline in the orchestrator session. The Agent isolation guard resolves
  isolation against the orchestrating session's project (injection-scanner), not this repo, which
  sets `use_worktrees: false`.

## Not verifiable before merge (by design)

`release.yml` runs only when a `vX.Y.Z` tag is pushed, and that tag-only trigger is the safety
property that justifies the exception. The PR's own ci.yml run is the only pre-merge evidence, and
it covers only the checkout and rust-toolchain pins. **The first hosted tag run is the Phase 6
release dry run**, which must confirm:

- the darwin zigbuild cross-link from an x86_64 host;
- the attest step;
- the native x86_64 `--version` smoke test;
- `gh attestation verify` on a downloaded tarball.

## Follow-ups

- Bump the `dtolnay/rust-toolchain` pin (4360b525, from 2026-08-05) to a commit after upstream
  fix #187, in agent-memory and injection-scanner together. The risk is accepted for now because
  every caller passes static inputs.
- Consider porting injection-scanner's gate that requires the tag to match the Cargo.toml version.
