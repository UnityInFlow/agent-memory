# Pitfalls Research

**Domain:** v1.1 Hardening & Interop milestone — adding hybrid RRF search, memory_update/relations, schema migrations, Windows/musl distribution, and portable export to a **shipped** Rust+SQLite+MCP tool (agent-memory v0.0.1)
**Researched:** 2026-07-12
**Confidence:** HIGH overall (every pitfall grounded in the actual v1.0 source at `crates/agent-memory-core/` / `crates/agent-memory/`, plus verified upstream state: sqlite-vec musl PR #199 still open, cargo-zigbuild Windows unsupported, rusqlite_migration too-new-DB behavior). MEDIUM on Windows ACL specifics.

> Scope note: this file covers mistakes specific to ADDING v1.1 features to the existing shipped codebase. The v1.0 domain pitfalls (stdout purity, spawn_blocking, vec0-ignores-triggers, etc.) are already encoded in the code and are referenced here only where a new feature can silently break them. Generic Rust advice lives in CLAUDE.md.

**v1.0 ground truths every v1.1 change must preserve (verified in source):**
- `service.rs` — degrade seam lives in `MemoryService`, never in transports; only an embed FAILURE triggers keyword fallback (empty semantic set is a valid `Semantic` answer); `bump_access` fires only on RETURNED ids.
- `sqlite.rs` — one writer `Mutex<Connection>` + r2d2 read pool; vec0 rows deleted explicitly (triggers don't fire on virtual tables); DELETE+INSERT instead of `INSERT OR REPLACE` on `vec_memories`; `map_fts_query_error` → `InvalidQuery` → 400/invalid_params at both transports.
- `0002_embeddings.sql` — `meta` table pins `embedding_model`/`embedding_dim`; drift must degrade, never mix vector spaces.
- `Cargo.toml` — chrono is `default-features=false, features=["now"]` (UTC-only; `Local` breaks zig darwin cross); rusqlite 0.39 / rusqlite_migration 2.5 / libsqlite3-sys 0.37 pin set.
- Coverage gate — tests must exercise code **in-process**; spawned child binaries flush no LLVM profile data.

## Critical Pitfalls

### Pitfall 1: RRF implemented as score fusion instead of rank fusion

**What goes wrong:**
The two existing legs produce scores on incomparable scales: the semantic path blends `relevance × (1 − cosine_distance) + decay × d` (bounded ~[0, 1.5]) and the keyword path blends `(−bm25) × w + decay × w` (bm25 is unbounded and corpus-dependent). A "hybrid" that sums or averages these numbers is not RRF — it silently over-weights whichever leg's scale is larger that day, and ranking quality regressions are invisible without a golden-query test set. RRF uses **positions only**: `score(doc) = Σ_legs 1/(k + rank_in_leg)`.

**Why it happens:**
Both legs already return "a score", so summing them feels like fusion. The bm25-sign trap compounds it (bm25 is smaller-is-better; the v1.0 code already negates it inside `ORDER BY` — a hybrid layer reading that ordering as a score re-inverts semantics easily).

**How to avoid:**
- Fuse on rank position per leg, never on the raw blended scores. Convert each leg's ordered result list to `(id, rank)` before fusion.
- Decide ONCE where decay lives and document it in the fusion function: either (a) each leg keeps its existing decay-blended internal ordering and RRF fuses those ranks (decay counted once per leg — acceptable), or (b) legs rank on pure relevance and decay multiplies the fused RRF score. Do **not** do both — adding a post-fusion decay term on top of decay-blended legs double-counts recency and buries pinned-type memories' relevance.
- Build a small golden-query fixture corpus (10–20 memories, hand-ranked expectations) as a unit test before wiring the fusion into transports.

**Warning signs:**
Hybrid results are ~identical to the semantic-only results (semantic scale dominating); a memory that is the #1 keyword hit for an exact-phrase query never appears in hybrid output.

**Phase to address:** Hybrid search phase — fusion function is pure Rust over two candidate vectors; TDD it before touching `MemoryService::search`.

---

### Pitfall 2: The two unrelated `k` constants collide (RRF k vs KNN oversample k)

**What goes wrong:**
`knn_search` already has a `k` — the oversample size `(limit × 4).min(MAX_KNN_K = 200)` bound into `WHERE embedding MATCH ?1 AND k = ?2`. RRF introduces a second, unrelated `k` — the smoothing constant in `1/(k + rank)`, conventionally **60** (Cormack & Clarke). Confusing them produces two distinct bugs: using `limit` or the KNN k as the RRF constant makes fusion behave arbitrarily (large k flattens all ranks to near-equal; k=1 makes rank 1 dominate everything), and using 60 as the KNN fetch size silently caps semantic recall.

**Why it happens:**
Same letter, same function, no type distinction. The literature always calls both "k".

**How to avoid:**
- Name them: `RRF_K: f64 = 60.0` (a tuning constant with a doc comment citing the convention) and keep `MAX_KNN_K`/oversample as-is. Never pass either as a bare `i64` argument named `k`.
- Unit-test the fusion math against hand-computed values: two legs of 3 ranks each, k=60, assert exact `1/(60+1) + 1/(60+2)`-style sums.

**Warning signs:**
An `RRF k` that varies with `limit`; fusion tests that pass only for the default limit.

**Phase to address:** Hybrid search phase.

---

### Pitfall 3: Duplicate rows across the FTS and vector legs (dedup vs sum)

**What goes wrong:**
A memory matching both legs (common — the best matches match both) appears twice in the concatenated candidate set. Two failure modes: (a) no dedup → the same memory id appears twice in the response, a client-visible correctness bug on both MCP and REST; (b) dedup by dropping the second occurrence → the memory loses its second leg's RRF contribution, which is precisely the signal hybrid search exists to capture (both-legs agreement should BOOST, not be discarded).

**Why it happens:**
`Vec::dedup`-style thinking. The naive merge is written as "concat, sort, truncate" and passes every test whose fixtures have disjoint leg results.

**How to avoid:**
- Fuse into a `HashMap<i64, f64>` keyed by memory id, **summing** `1/(k + rank)` contributions across legs, then sort the map's entries. Dedup is then structural, not a post-pass.
- Kill-test: a memory ranked #2 in both legs must outrank a memory ranked #1 in exactly one leg (with k=60: 2/62 > 1/61). Also assert response ids are unique.

**Warning signs:**
Test fixtures where FTS and KNN results never overlap (the dangerous case is untested); duplicate ids in a live `/api/search` response.

**Phase to address:** Hybrid search phase.

---

### Pitfall 4: Degraded-mode and error-taxonomy semantics break when "hybrid" is the third mode

**What goes wrong:**
Three regressions hide here, all against locked v1.0 contracts:
1. **Wire shape:** `SearchMode` serializes lowercase into the shared `SearchOutcome` envelope on both transports (`search_mode: "semantic" | "keyword"`). Adding `"hybrid"` is a wire change — existing clients/tests matching the two known strings break, and the REST tests assert `body["search_mode"]` exactly.
2. **Degrade seam:** with the embedder down, "hybrid" must degrade to keyword-only **at the `MemoryService` seam** and report `search_mode: "keyword"` honestly — not report `"hybrid"` while silently serving one leg. The v1.0 rule (only embed FAILURE triggers fallback; an empty semantic leg is still a valid semantic answer) must extend: an empty KNN leg with a working embedder is still `"hybrid"`.
3. **Error taxonomy:** a malformed FTS5 MATCH string (`"`, dangling `NEAR`) surfaces from the keyword leg as `MemoryError::InvalidQuery` → 400/invalid_params (WR-05, mapped in `map_fts_query_error`). In hybrid, the tempting move is "keyword leg errored, just return the semantic leg" — which makes the same bad input a 400 in keyword mode but a silent success in hybrid mode. Inconsistent taxonomy across modes is exactly what the two-tier error work (02-05) exists to prevent.

**Why it happens:**
Hybrid is naturally written as "try both, use what worked", which conflates *infrastructure failure* (Ollama down → degrade loudly) with *client error* (bad MATCH string → 400 always).

**How to avoid:**
- Extend the `SearchMode` enum with `Hybrid` and treat it as a versioned wire change: update both transport test suites in the same plan, and grep docs/README for the mode strings.
- Encode the decision table in service tests: embed OK + both legs OK → `hybrid`; embed FAIL → loud warn + `keyword` (unchanged v1.0 path); `InvalidQuery` from the FTS leg → propagate the 400 regardless of mode; internal SQLite error from either leg → 500.
- Keep the decision in `MemoryService` — transports must stay dumb (v1.0 architecture rule).

**Warning signs:**
A REST test asserting `search_mode == "hybrid"` while the test harness has no Ollama (should be `keyword`); a malformed-query test that passes in keyword mode but returns 200 in hybrid mode.

**Phase to address:** Hybrid search phase (decision table first, in `discuss-phase`); API hardening phase re-verifies the taxonomy end-to-end.

---

### Pitfall 5: Limit semantics after fusion (leg under-fetch, and bump_access over-fire)

**What goes wrong:**
Two subtle limit bugs: (a) each leg fetching exactly `limit` rows starves fusion — a memory ranked `limit+1` in one leg but top-3 in the other never enters the candidate pool, so hybrid can rank *worse* than single-leg for border results; (b) the fire-and-forget recency bump currently fires only on **returned** ids — if the hybrid path bumps every *candidate* (up to 200 KNN + N FTS rows), each search inflates `last_accessed`/`access_count` corpus-wide, which corrupts decay ordering permanently and invisibly.

**Why it happens:**
The v1.0 semantic path already oversamples (limit×4 capped at `MAX_KNN_K`) but the FTS path fetches exactly `limit` (its `LIMIT ?9`); reusing `Store::search` as the keyword leg quietly imports the under-fetch. The bump bug happens because the candidate set is the natural thing to have in hand when fusion completes.

**How to avoid:**
- Both legs oversample symmetrically (limit×4, capped — reuse `DEFAULT_SEARCH_LIMIT`/`MAX_KNN_K` conventions), fuse in Rust, then `take(limit)`. The FTS leg needs either a raised internal limit parameter or a dedicated leg query — do not silently change the public `Store::search` behavior that keyword-only mode still depends on.
- Bump only `results.iter().map(|v| v.id)` after truncation — same as both v1.0 paths. Kill-test: search a 20-row corpus with limit=5 in hybrid mode; assert exactly 5 rows had `access_count` incremented.
- Re-apply the existing extreme-limit guards (T-02-03/T-02-04): `limit=0`, huge limits, `None` → default 50; this overlaps the WR-01/WR-02 REST hardening work.

**Warning signs:**
`access_count` climbing on memories never returned to a client; hybrid quality tests that only pass with limit ≥ corpus size.

**Phase to address:** Hybrid search phase; boundary values re-checked in API hardening phase.

---

### Pitfall 6: `memory_update` leaves a stale vector — semantic search serves the OLD content

**What goes wrong:**
Updating `memories.content` fires the FTS5 `AFTER UPDATE` trigger (keyword mirror stays correct) but **vec0 ignores triggers** (v1.0 Pitfall 3, encoded in `0002_embeddings.sql`), so the old embedding keeps serving. Result: keyword search finds the new text, semantic search finds the memory via its *old* meaning — the two hybrid legs permanently disagree about one row, and a "forgotten" meaning resurfaces semantically. This is the update-shaped twin of the forget bug v1.0 already fixed.

**Why it happens:**
The FTS trigger gives false confidence that "SQLite keeps the mirrors in sync". Only one of the two mirrors is trigger-maintained — deliberately.

**How to avoid:**
- In `Store::update`, inside ONE writer transaction: `UPDATE memories …`, `DELETE FROM vec_memories WHERE memory_id = ?`, `SET embedding_status = 0`. Then re-embed best-effort at the service layer (same pattern as `store()`: async embed OUTSIDE `spawn_blocking`, `insert_embedding` on success, pending row + sweep backfill on failure). The existing `insert_embedding` DELETE+INSERT is the write-back path — reuse it; do not add `INSERT OR REPLACE` (conflict clauses are unreliable on virtual tables — v1.0 decision).
- If only tags/scope/ttl change (not content), skip the vector invalidation — but make the "did content change" check explicit and tested, not inferred from parameter presence.
- Kill-test: store "Postgres is the database", update to "SQLite is the database", with a stub embedder assert the vec row is gone (or re-embedded) and a semantic query for the old meaning no longer returns the row.

**Warning signs:**
`embedding_status = 1` on a row whose `vec_memories` vector predates its content; hybrid results where the same id ranks high in FTS for the new text and high in KNN for unrelated queries.

**Phase to address:** MCP update/relations phase — the transaction shape is the first thing to design.

---

### Pitfall 7: Implementing update as DELETE+INSERT (new rowid) instead of UPDATE in place

**What goes wrong:**
Reusing the existing `insert` path for update ("delete the row, insert the new version") changes the memory's rowid. Everything keyed on id breaks: relation edges (new feature) point at a dead id, MCP clients holding the id from `memory_store` get NotFound on the next call, the FTS external-content mirror and vec sidecar must be manually re-pointed, and `created_at`/`access_count` history is lost. With `AUTOINCREMENT` absent (plain `INTEGER PRIMARY KEY`), the freed rowid can even be **reused** by a later insert — old relation edges then silently attach to an unrelated new memory (data corruption, not an error).

**Why it happens:**
`insert` already handles tags-serialization, base_weight, embedding — wrapping it looks DRY. The DELETE+INSERT idiom is also fresh in mind because it IS correct for `vec_memories` (no-rowid semantics there differ).

**How to avoid:**
- `UPDATE memories SET … WHERE id = ?` in place; ids are immutable for the lifetime of a memory. State this as an invariant in `domain.rs` docs.
- Kill-test: update a memory that has an inbound relation; assert the edge still resolves and the id is unchanged.

**Warning signs:**
`last_insert_rowid()` called anywhere in the update path; ids changing across an update in integration tests.

**Phase to address:** MCP update/relations phase.

---

### Pitfall 8: `updated_at` vs `last_accessed` — editing a memory silently rewrites its decay standing

**What goes wrong:**
Decay ranking recomputes from `last_accessed` at read time (`exp(-ln2 × (now − last_accessed) / half_life)` — same formula inline in `Store::search`, `materialize_decay`, and `decay_score()`). If `memory_update` bumps `last_accessed`, every edit is a full recency resurrection (a typo fix un-decays a year-old memory); if it doesn't, an edited-and-clearly-cared-about memory stays buried. Either can be right — what goes wrong is deciding *implicitly* via whichever column the UPDATE happens to touch, and adding an `updated_at` column that one of the three decay-formula sites reads while the others don't (the v1.0 STORE-03 invariant is that recompute-on-read and the materialization sweep agree EXACTLY).

**Why it happens:**
The column names invite conflation, and the decay math lives in three places that must stay in lockstep — a new timestamp column is a fourth place to get wrong.

**How to avoid:**
- Decide explicitly in discuss-phase and record it: recommended default — update bumps `last_accessed` (an edit is the strongest access signal there is) and adds `updated_at` as pure metadata that NO ranking formula reads.
- Add `updated_at` via a plain `ALTER TABLE ADD COLUMN` migration (nullable or default-0 — SQLite ALTER cannot add a non-constant default).
- Kill-test both directions: post-update decay score reflects the chosen policy; the sweep's materialized `decay_score` equals the on-read recompute for an updated row (extend the existing STORE-03 agreement test).

**Warning signs:**
Any SQL referencing `updated_at` inside an `ORDER BY` or the sweep UPDATE; disagreement between `tests/decay.rs` recompute and materialized values after an update.

**Phase to address:** MCP update/relations phase (decision), verified alongside decay tests.

---

### Pitfall 9: Relation edges orphan after forget/TTL — and FK enforcement is per-connection

**What goes wrong:**
A `memory_relations(from_id, to_id, kind)` table adds the third sidecar that must never drift from `memories` (after `memories_fts` and `vec_memories` — each already needed its own sync mechanism). Rows can orphan via **two** delete paths: `forget()` and the TTL `sweep_expired()`. Declared `ON DELETE CASCADE` only fires on connections where `PRAGMA foreign_keys = ON` — which in this codebase is applied per-connection in `prepare_connection()`, not in the schema; any future connection that skips the pragma (a one-off maintenance script, a test opening its own `Connection`) silently orphans edges with no error. Orphaned edges then make a `memory_graph`/`memory_related` tool return phantom neighbors or NotFound — and if rowids are recycled (Pitfall 7), edges attach to the wrong memory.

**Why it happens:**
SQLite FKs are opt-in per connection (legacy default OFF) — unusual vs every server RDBMS. And the sweep is easy to forget because it was written before relations existed.

**How to avoid:**
- Belt and braces: declare `FOREIGN KEY … ON DELETE CASCADE` in the migration **and** delete edges explicitly in the same writer transaction in both `forget()` and `sweep_expired()` (mirror the existing explicit `vec_memories` deletes — the codebase already follows "explicit in Rust, don't trust the engine's optional mechanism" for vec0; extend the idiom).
- Constrain the table: `CHECK (from_id != to_id)`, `UNIQUE (from_id, to_id, kind)`, and validate `kind` against a closed enum at the domain layer (same pattern as `MemoryType::try_from` — no bad kind reaches SQL, D-07).
- Kill-tests: forget a related memory → zero edges referencing it; TTL-expire a related memory via `sweep(now)` → same; storing a duplicate or self-referential edge → clean client error, not a 500.

**Warning signs:**
`SELECT count(*) FROM memory_relations WHERE from_id NOT IN (SELECT id FROM memories)` > 0 in any test; relation tests that only exercise `forget` and never the sweep.

**Phase to address:** MCP update/relations phase; the sweep interaction re-verified whenever TTL tests run.

---

### Pitfall 10: Editing shipped migration files — rusqlite_migration has no checksums

**What goes wrong:**
`rusqlite_migration` tracks progress with SQLite's `user_version` integer ONLY — there is no per-migration hash. v0.0.1 databases in the wild have `user_version = 2` (0001 + 0002 applied). If v1.1 "cleans up" or amends `0001_init.sql`/`0002_embeddings.sql` (tempting when adding relations: "just put the FK in 0001"), fresh installs and upgraded installs end up with **silently different schemas** — same version number, different tables/triggers/constraints. Bugs then reproduce only on one population and every schema assumption in tests (which always run the fresh path) is unverified against real user databases.

**Why it happens:**
In pre-release development, editing migration files was harmless — every dev DB was rebuilt. v0.0.1 shipping flipped that permanently, and nothing in the toolchain errors when a shipped file changes.

**How to avoid:**
- Freeze `sql/0001_*.sql` and `sql/0002_*.sql` forever; all v1.1 schema work is `0003+` appended `M::up` entries.
- Add the divergence test this milestone: commit a v0.0.1-schema fixture DB (or build one by applying only migrations 1–2), migrate it `to_latest()`, and assert its full `sqlite_master` SQL matches a fresh-from-zero database's. This single test converts the invisible failure into a red CI.
- Note the existing quirk: `Migrations::validate()` (and any migration test) needs `register_vec_extension()` first because 0002 creates a vec0 table — every new migration touching vec0/FTS keeps this requirement; keep the registration line in new tests.

**Warning signs:**
Any git diff touching `sql/0001` or `sql/0002`; a schema bug report reproducible only on an upgraded-from-v0.0.1 database.

**Phase to address:** Whichever phase lands the FIRST new migration (likely MCP update/relations) — the freeze rule and divergence test go in before migration 0003.

---

### Pitfall 11: Newer-DB-meets-older-binary, and no pre-migration backup

**What goes wrong:**
Once v0.1.0 raises `user_version` past 2, any older binary opening that DB gets an error from `Migrations::to_latest` (rusqlite_migration returns an error when the database version exceeds the known migrations — verified in docs). Real triggers: Homebrew rollback, `AGENT_MEMORY_DB` pointing at a shared/synced file used from two machines on different versions, or a user testing a pre-release. The failure surfaces as an opaque startup error inside an MCP client (stderr often invisible), reading as "agent-memory is broken". Separately: migrations 0003+ run unattended on first launch after upgrade against the user's only copy of their memory corpus — a buggy data-shuffling migration (see Pitfall 12's rebuild pattern) can be destructive with no way back.

**Why it happens:**
Pre-1.0 tools rarely think about version skew because there was only ever one version; and SQLite makes "just open the file" so easy that open-time is the only hook.

**How to avoid:**
- Map the too-new error at `SqliteStore::open` to a purposeful message naming both versions and the fix ("database was created by a newer agent-memory; upgrade this binary or point --db elsewhere") — and make sure it reaches the MCP client as a proper error, not a stdout panic (stdout purity).
- Before applying pending migrations to an on-disk DB (detect: `user_version` < latest and file exists), copy `memory.db` to `memory.db.pre-v{N}.bak` (WAL note: checkpoint or use `VACUUM INTO`/rusqlite backup API rather than a raw file copy of a live WAL DB). Local memory corpora are small; the insurance is nearly free.
- Do not write `.down()` migrations to "support rollback" — a data-destroying down-migration is worse than a clean refusal.

**Warning signs:**
A support report of "worked yesterday, upgrade broke it"; startup errors mentioning `user_version` verbatim.

**Phase to address:** Same phase as the first new migration; the backup step is one function in `SqliteStore::open`.

---

### Pitfall 12: vec0 and FTS5 tables can't ALTER — schema changes need rebuild patterns, and vec0's is special

**What goes wrong:**
`ALTER TABLE` on virtual tables supports rename only. Two v1.1-relevant cases: (a) changing the embedding dimension or distance metric (e.g., supporting a different Ollama model) cannot modify `vec_memories FLOAT[768] distance_metric=cosine` — the table must be dropped and recreated; (b) any change to FTS5 tokenization or indexed columns requires recreating `memories_fts` AND its three triggers, then `INSERT INTO memories_fts(memories_fts) VALUES('rebuild')` to repopulate from external content — forgetting the rebuild leaves keyword search running on an empty index that returns no rows and no errors. Also, copying `vec_memories` blobs across a rebuild is wasted effort with a trap: vectors are derived data.

**Why it happens:**
Migration muscle memory is "ALTER TABLE ADD COLUMN"; virtual tables look like tables until the migration fails at a user's machine (worse: FTS 'rebuild' omission doesn't fail at all).

**How to avoid:**
- For vec0 changes: migration drops and recreates `vec_memories` and sets `UPDATE memories SET embedding_status = 0` — the existing sweep backfill re-embeds the corpus incrementally (BACKFILL_BATCH=128/tick) with zero data loss. Update the `meta` table's `embedding_model`/`embedding_dim` in the same migration. This makes semantic search degraded-but-honest until backfill completes (search_mode reports keyword/hybrid accordingly) — document the transient in release notes.
- For FTS changes: recreate table + all three triggers + `'rebuild'` in one migration; kill-test by migrating a fixture DB with rows and asserting a keyword query still matches.
- Remember migration SQL runs inside rusqlite_migration's transaction — statements that can't run in a transaction (VACUUM) can't live in a migration.

**Warning signs:**
A migration containing `ALTER TABLE vec_memories` or `ALTER TABLE memories_fts` (will error at upgrade time); keyword search returning zero rows on an upgraded DB while `SELECT count(*) FROM memories` is non-zero.

**Phase to address:** Only if a phase actually changes vec/FTS shape — but the divergence-fixture test (Pitfall 10) should land regardless so this class of bug is caught.

---

### Pitfall 13: Assuming the proven release pipeline extends to Windows — cargo-zigbuild has NO Windows target support

**What goes wrong:**
The ecosystem's standard release path (OPS-01: serial builds on the ARM64 `orangepi` runner, cross-compiling all targets with cargo-zigbuild) **cannot produce Windows binaries**: cargo-zigbuild officially supports only Linux and macOS targets. Planning DIST-03 as "add x86_64-pc-windows-msvc to the zigbuild matrix" fails at the first CI run — after the phase was scoped assuming a solved pipeline. The sibling precedent is worse than neutral: mcp-hub's Windows build failed on `cfg(unix)`-gated deps used unguarded and Windows was deferred to v2 there too — nobody in the ecosystem has shipped a Windows binary yet.

**Why it happens:**
zigbuild made darwin-from-Linux work so smoothly (injection-scanner shipped all 6 triples from orangepi) that "zig cross-compiles everything" became an assumed capability. Zig itself CAN target windows-gnu (bundled MinGW-w64), but cargo-zigbuild doesn't wire it.

**How to avoid:**
- Treat the Windows **build path** as a research spike with explicit fallback before committing the phase: options are (a) `x86_64-pc-windows-gnu` via a mingw-w64 cross toolchain or manual zig-as-linker config on orangepi (must verify an aarch64-hosted mingw exists on that box), (b) `cargo-xwin` for the msvc target (needs clang; verify on ARM64 host), (c) a GitHub-hosted `windows-latest` job — which needs an explicit policy exception like spec-ci-plugin's sanctioned D-02 split, and note the repo is now public, where self-hosted jobs can't run at all under `allows_public_repositories: false`.
- Whatever the toolchain: `rusqlite` bundled sqlite3.c AND sqlite-vec's `cc`-built C must both compile for the Windows target — smoke-test the C build first, it's the long pole (msvc vs gnu ABI differences in the cc crate).
- Budget for the fact that a cross-compiled Windows exe can't be executed on the Linux runner — plan at least a manual smoke test on a real Windows machine (or Wine as a weak proxy) before publishing; mirror the host-arch-aware smoke-test lesson from mcp-hub 03-04.
- Packaging: `.exe` suffix and `.zip` (not `.tar.gz`) for the Windows artifact; checksum file covers it.

**Warning signs:**
A release workflow diff that just adds a windows triple to the existing zigbuild matrix; phase estimates for DIST-03 similar to "add another Linux triple".

**Phase to address:** Windows distribution phase — spike the toolchain FIRST (needs-research flag), before any `cfg` refactor work.

---

### Pitfall 14: Windows data directory — `dirs::data_dir()` is Roaming AppData, and SQLite WAL must not live on a synced/network profile

**What goes wrong:**
`config.rs` resolves the default DB path via `dirs::data_dir()`. On Windows that maps to `%APPDATA%` (**Roaming** AppData) — on domain-joined machines this directory syncs over the network with the user profile. A live SQLite database in WAL mode on a roaming/SMB-backed path is a documented corruption scenario (WAL requires shared-memory-coherent locking that network filesystems don't provide), and profile sync can copy a `.db` mid-write or strand the `-wal`/`-shm` files. Additionally, the Unix `mode 0700` protection (threat T-01-03: other local users must not read the memory corpus) has no effect — the current `cfg(not(unix))` fallback is a bare `create_dir_all`.

**Why it happens:**
`data_dir()` is correct on Linux/macOS, so the same call "works" in a Windows build and every local test — the corruption needs an enterprise roaming profile to manifest, which no CI has.

**How to avoid:**
- On Windows use `dirs::data_local_dir()` (`%LOCALAPPDATA%`) for the default DB path. The config module was deliberately built as the ONE platform-path seam ("single-file change, not a scattered cfg(unix) hunt" — its own doc comment); keep it that way: one `#[cfg(windows)]` branch in `resolve_db_path`.
- Accept default `%LOCALAPPDATA%` ACLs as the T-01-03 answer (per-user by default) and record that in the threat model rather than attempting Win32 ACL programming in v1.1.
- Document loudly (README + startup warning if detectable) that `AGENT_MEMORY_DB` must not point at a network share — this applies to Unix NFS users too, and export/import (DIST-04) is the supported way to move a corpus between machines.

**Warning signs:**
`data_dir()` still referenced in config.rs on the windows path at review time; a Windows user reporting `database disk image is malformed` after a laptop roamed networks.

**Phase to address:** Windows distribution phase — same plan as the `cfg(unix)` refactor.

---

### Pitfall 15: Windows runtime semantics — signals, open-file locking, CRLF, and the in-process-coverage rule

**What goes wrong:**
A basket of "works on Unix" behaviors that each break the daemon or its CI on Windows:
- **Signals:** there is no SIGTERM. `tokio::signal::ctrl_c()` is cross-platform, but any `#[cfg(unix)] signal(SignalKind::terminate())` shutdown arm needs a windows counterpart (`ctrl_close` etc.) or the process dies without flushing the WAL checkpoint on console close. (v1.0 shuts down on stdin EOF for MCP — that path is portable; the REST-only daemon mode is the exposure.)
- **Open-file semantics:** Windows can't delete or rename-over a file another handle has open. Export writing `tmp` + `rename` over an existing export file fails if the target is open in an editor; likewise cleanup of `-wal`/`-shm` and the pre-migration `.bak` copy of a live DB behave differently. `tempfile::persist` across volumes (temp dir on C:, target elsewhere) also fails where Unix `rename` would.
- **CRLF/BOM:** rmcp's stdio framing tolerates `\r\n` (and `\r` is JSON whitespace), but CI plumbing is the trap — PowerShell `echo`/redirection produces UTF-16/BOM output, and `git autocrlf` can mangle test fixtures; stdio smoke tests must pipe raw bytes.
- **Coverage:** the >80% gate relies on in-process tests (spawned binaries flush no LLVM profile data — v1.0 ground truth). New Windows-specific paths (config branch, shutdown arm) need in-process unit tests, not "spawn the exe on a Windows box" tests, or coverage silently drops below the gate.

**Why it happens:**
Each is individually small; collectively they're why "the binary compiles for Windows" ≠ "Windows is supported".

**How to avoid:**
- Grep gate in CI: no bare `#[cfg(unix)]` without a sibling `#[cfg(windows)]`/`#[cfg(not(unix))]` implementation or an explicit comment (mcp-hub's failure mode was exactly unguarded unix-only code).
- Keep all Windows divergence in the two existing seams (config.rs paths, main.rs shutdown) — resist scattering.
- Export/backup file writes: write-to-temp-in-same-directory + rename, and treat rename failure as a retriable/reportable error, not an unwrap.
- Run `cargo test --target x86_64-pc-windows-gnu` under CI at least via build (compile-check) even if execution needs a Windows host.

**Warning signs:**
`cfg(unix)` count in the diff rising outside config.rs/main.rs; a Windows CI job that only runs `cargo build` and no test of the new branches.

**Phase to address:** Windows distribution phase.

---

### Pitfall 16: musl fix — the upstream patch is NOT released; the CFLAGS shim must be target-scoped and must also serve `cargo install` users

**What goes wrong:**
sqlite-vec's `sqlite-vec.c` still carries BSD `u_int8_t`/`u_int16_t`/`u_int32_t`/`u_int64_t` typedef fallbacks that musl lacks; the upstream fix (PR #199) remains **open and unmerged** as of 2026-07-12, and the pinned crate `sqlite-vec = "0.1.9"` ships the broken source (0.1.10 is alpha-only — v1.0 already rejected it). Three ways teams get this wrong: (a) waiting on upstream (unbounded); (b) forking/patching the vendored C (permanent maintenance debt, and `[patch]`-ing a `cc`-built crate means owning its build.rs too); (c) fixing it ONLY via CI env vars — which ships working release binaries but leaves `cargo install agent-memory --target …-musl` broken for source-install users with a baffling C compiler error.

**Why it happens:**
The macro shim (`-Du_int8_t=uint8_t …`) is trivially easy to apply in one CI yaml line, so it never graduates to a durable fix.

**How to avoid:**
- Use the target-scoped env form so gnu builds are untouched: `CFLAGS_x86_64_unknown_linux_musl` / `CFLAGS_aarch64_unknown_linux_musl` = `-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int32_t=uint32_t -Du_int64_t=uint64_t` (the `cc` crate reads target-suffixed CFLAGS and appends them even when `CC` is zig's wrapper under zigbuild). Macro `-D` substitution stays harmless if upstream later removes the typedefs — safe to leave in place across a future crate bump.
- Verify the shim actually reached the C compile: zigbuild changes the compiler front-end; assert in CI that the musl build fails WITHOUT the flags (a canary job or a one-time recorded check) so a silent zig-side fix doesn't leave dead config.
- For source installs: document the required env in README's musl section, and (cheap, durable) file/ping the upstream PR — if a fixed crate version releases, bump the pin and delete the shim in the same commit. Note the June 2026 upstream comment claims newer sqlite-vec no longer needs the patch — verify against whatever crate version is actually on crates.io before bumping, and re-run the whole vec0 test suite (the crate pin interacts with the rusqlite 0.39/libsqlite3-sys 0.37 graph).
- Smoke-run the musl binary (it's static — runs anywhere on same-arch Linux): `--version` plus one real `store`+`search` against a temp DB, honoring the host-arch-aware lesson (don't exec x86_64 musl output on the ARM64 runner).

**Warning signs:**
`unknown type name 'u_int8_t'` in a release log; a musl tarball published without any functional smoke test; CFLAGS set globally (unsuffixed) and macOS/gnu builds warning about redefined macros.

**Phase to address:** musl distribution phase (can ride the same release-pipeline plan as Windows or ship earlier — it's one CI env line + tests).

---

### Pitfall 17: Export/import — embedding vectors are model-pinned; a portable file that carries them can poison the target corpus

**What goes wrong:**
If DIST-04 exports embedding vectors and the importer inserts them blindly, importing onto a machine whose `meta` table pins a different `embedding_model` or `embedding_dim` produces a mixed-space `vec_memories` — cosine distances between vectors from different models are meaningless, so semantic ranking silently degrades corpus-wide (no error, ever). The dim=768 case at least fails the vec0 column type; a same-dim different-model case (many models are 768) corrupts invisibly. This is exactly the drift the v1.0 `meta` pin exists to prevent ("must degrade, never mix" — 0002_embeddings.sql comment).

**Why it happens:**
Exporting vectors feels like completeness ("round-trip everything") and saves re-embedding time on import.

**How to avoid:**
- Recommended: **don't export vectors at all.** Export rows with `embedding_status` conceptually reset; the importer inserts them pending and the existing sweep backfill (128/tick) re-embeds against the LOCAL model. Zero mixing risk, smaller files, and it reuses proven machinery. Accept the transient where fresh imports are keyword-only until backfill completes.
- If vectors are exported (offline-import use case): the file header MUST carry `embedding_model`+`embedding_dim` (and a format/schema version), and the importer compares against `meta` — on mismatch, drop vectors and fall back to pending+re-embed; never insert, never hard-fail the whole import.
- Either way: header versioning from day one (`{"format": "agent-memory-export", "version": 1, …}`) — a shipped tool's export format is a compatibility contract the moment one user writes a file.

**Warning signs:**
An export file containing raw float arrays with no model metadata; semantic search quality complaints that started after an import.

**Phase to address:** Export/import phase — the "no vectors in v1" decision should be made in discuss-phase.

---

### Pitfall 18: Export/import — rowid collisions and relation-edge remapping

**What goes wrong:**
Memory ids are SQLite rowids — meaningful only inside one database file. A naive import that preserves ids either collides with existing rows (constraint error at best, `INSERT OR REPLACE` clobbering a stranger's memory at worst) or — with relations landing in the same milestone — inserts edges `(from_id, to_id)` whose numbers now point at whatever unrelated memories occupy those rowids in the target DB. That is silent graph corruption. The interplay with dedup makes it worse: when a draft is skipped as a duplicate (existing `(source, mem_type, content)` idempotency key), its edges must attach to the **existing** row's id, not to a fresh insert.

**Why it happens:**
Round-tripping into an EMPTY database works perfectly with preserved ids, and that's the only case tests cover by default.

**How to avoid:**
- The export format uses export-local keys (the original id is fine AS a key, or an ordinal), and edges reference those keys. The importer builds a `export_key → new_or_existing_rowid` map as it inserts/dedups rows, then inserts edges through the map. Edges whose endpoints didn't survive (endpoint row skipped as expired, say) are dropped with a count in the report.
- Reuse the existing `ImportReport`-style summary: `imported / skipped_duplicates / edges_imported / edges_dropped`.
- Kill-tests: import into a NON-empty DB whose rowids overlap the export's key range and assert no existing memory gained a phantom edge; export→import→export and diff the two files modulo ids/timestamps (the true round-trip test).
- Idempotency: importing the same file twice must not duplicate rows (the exists() dedup gives this for memories — extend it to edges via the UNIQUE constraint + INSERT OR IGNORE... note OR IGNORE is fine here, `memory_relations` is a plain table, not virtual).

**Warning signs:**
Import code containing `INSERT INTO memories (id, …)` with an explicit id; round-trip tests that only ever target a fresh temp DB.

**Phase to address:** Export/import phase — format design (keys + edges) before any code; sequence AFTER the relations phase so the format includes edges from day one.

---

### Pitfall 19: Export/import — absolute TTL and decay timestamps travel badly across machines and time

**What goes wrong:**
All lifecycle state is absolute UTC epoch seconds: `expires_at`, `last_accessed`, `created_at`. Three surprises on import: (a) rows whose `expires_at` already passed import "successfully", then the hourly sweep deletes them — to the user, the import ate their memories; (b) preserved `last_accessed` from months ago means imported memories arrive fully decayed (score ≈ 0 after a few half-lives) and never surface in ranked search — "import worked but recall can't find anything"; (c) a source machine with a skewed clock exports `expires_at` values that misbehave on the target. None of these error — they're all policy gaps that read as data loss or broken search.

**Why it happens:**
Serializing the columns verbatim is the obvious implementation, and it IS correct for backup/restore on the same machine — the pitfall is that "portable export" and "backup" are different features with different timestamp semantics.

**How to avoid:**
- Decide per column, in discuss-phase, and write it into the format spec: `created_at` preserve (provenance); `expires_at` preserve but have the importer skip already-expired rows AT import time and count them in the report (`skipped_expired`) — explicit beats a delayed sweep deletion; `last_accessed` — either refresh to import-time `now` (imported memories start decay-fresh; simple, recommended) or preserve with a documented "aged import" flag. Whatever is chosen, the decay-never-deletes invariant must hold: an imported ancient memory ranks low but MUST still be retrievable by direct filter/list (that's the v1.0 STORE-03/04 contract).
- Stamp the export with its creation time so the importer can detect gross clock skew (export "from the future") and warn.
- Kill-tests: import a fixture with one expired row (reported, not silently swept later), one ancient row (retrievable via list, low decay), one fresh row.

**Warning signs:**
An import report that says N imported while `memory_list` shows fewer than N an hour later; imported memories absent from every search but present in `list`.

**Phase to address:** Export/import phase.

---

## Technical Debt Patterns

Shortcuts that seem reasonable this milestone but create long-term problems.

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Score-sum "hybrid" instead of true RRF | No rank bookkeeping; reuses existing blends | Unfixable-by-tuning ranking bias; can't cite/verify behavior | Never — RRF is ~30 lines of Rust |
| Editing shipped SQL migration files | Cleaner-looking migration history | Silent schema divergence across the installed base (no checksums) | Never after v0.0.1 shipped |
| CI-only CFLAGS shim for musl, undocumented | One yaml line; release binaries work | Source installs (`cargo install --target musl`) fail cryptically | OK for v1.1 IF README documents the env vars and the upstream pin-bump path is tracked |
| Skipping the pre-migration DB backup | Less startup code | A buggy 0003+ migration destroys a user's only memory corpus | Never once migrations mutate data (pure ADD COLUMN-only releases: tolerable) |
| Exporting embedding vectors in v1 of the format | Faster imports (no re-embed wait) | Model-mismatch corruption risk; bigger format surface frozen forever | Only with enforced model/dim header check; simpler to defer vectors entirely |
| `memory_update` reusing delete+insert | Reuses tested insert path | Rowid churn → broken relations, reused ids, lost history | Never |
| Windows support = "it compiles for the target" | Ships DIST-03 checkbox | Roaming-profile WAL corruption, unkillable daemon, no smoke test | Never — an untested binary on a new OS is a liability, not a feature |
| Relations cleanup via FK CASCADE only (no explicit deletes) | Less SQL in forget/sweep | Any pragma-less connection silently orphans edges | Acceptable only with a CI test opening a raw pragma-less connection to prove triggers/cascades aren't load-bearing |

## Integration Gotchas

Common mistakes at this milestone's integration seams.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| RRF ↔ existing degrade seam | Deciding hybrid-vs-keyword in the transport layer | Decision stays in `MemoryService`; transports serialize `SearchOutcome` unchanged |
| RRF ↔ FTS5 error taxonomy | Swallowing `InvalidQuery` from the keyword leg when the semantic leg succeeded | Malformed MATCH is a client 400 in every mode (WR-05 consistency) |
| memory_update ↔ vec0 sidecar | Trusting triggers to sync `vec_memories` (they never fire on virtual tables) | Explicit DELETE + status reset in the same writer tx; re-embed via existing `insert_embedding` |
| memory_update ↔ FTS5 mirror | Bypassing the `AFTER UPDATE` trigger by rewriting rows via delete+insert | Plain in-place `UPDATE memories` so the external-content trigger handles the mirror |
| Relations ↔ TTL sweep | Cleaning edges only in `forget()` | Both delete paths (`forget` AND `sweep_expired`) clear edges in-transaction |
| New migrations ↔ vec0 registration | `Migrations::validate()`/tests without `register_vec_extension()` first | Register process-globally before any Connection::open in every migration test (existing pattern) |
| Windows build ↔ release pipeline | Adding a windows triple to the zigbuild matrix | zigbuild has no Windows support — spike mingw-w64/xwin/hosted-runner first |
| musl CFLAGS ↔ zigbuild | Global `CFLAGS` bleeding `-Du_int8_t=…` into darwin/gnu compiles | Target-suffixed `CFLAGS_<triple>` env vars only |
| Export ↔ Ollama | Failing import when Ollama is down (re-embed path) | Import NEVER fails on embed failure — rows land pending, sweep backfills (v1.0 SEARCH-03 contract extends to import) |
| Import ↔ dedup | Inserting relation edges before knowing whether endpoints deduped to existing rows | Two-pass import: rows first (building key→id map incl. dedup hits), then edges through the map |
| REST hardening ↔ new endpoints | Hardening `limit`/`ttl_secs` only on the v1.0 endpoints | New update/relations/export endpoints get the same boundary-value + two-tier-error treatment in the same phase |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Hybrid = 2 full leg queries + embed call, serially | Search latency ≈ embed + KNN + FTS stacked | Run KNN and FTS legs concurrently (`tokio::join!` over two `spawn_blocking` calls — read pool supports it); embed is already first | Noticeable at any corpus size; matters most with cold Ollama (~100ms embed) |
| Bumping access on all fusion candidates | `access_count` inflation corpus-wide; decay ordering flattens | Bump only post-truncation returned ids (existing contract) | Immediately — corrupts ranking state permanently |
| FTS leg oversampling via `LIKE '%tag%'` on big corpora | Hybrid slower than either leg alone | Tag filtering stays a bound parameter post-MATCH (current shape); revisit tag exact-match (WR-07) before adding more scans | ~50k+ rows |
| Re-embedding the whole corpus synchronously after a vec0 rebuild migration | Startup hangs minutes on upgrade | Reset `embedding_status=0` and let the bounded sweep backfill (128/tick) do it incrementally | ~1k+ memories |
| Export loading the entire corpus into memory as one JSON document | OOM/slow on large corpora; giant string on stdout | Stream JSONL row-per-line to a file handle; never route export data through MCP stdout | ~100MB corpora; stdout purity risk at any size |
| Relation graph queries with unbounded traversal depth | `memory_related` latency explodes on dense graphs | v1.1 ships depth-1 neighbors only (bounded query); defer traversal | Dense graphs (many edges/node) |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Export written world-readable | Memory corpus (decisions, errors, possibly sensitive content) readable by other local users — T-01-03's file-copy twin | Create export files `0600` on Unix; document Windows ACL inheritance; never default the export path to a shared/tmp dir |
| Import path traversal / arbitrary file read via REST | A REST-triggered import reading any file the daemon can | Keep import file-path-based ONLY on the CLI; REST import (if any) takes the payload in-body, loopback-guarded as today |
| Relation `kind` string reaching SQL unvalidated | Injection surface + junk taxonomy | Closed enum with `try_from` validation at the domain layer (MemoryType pattern, D-07) |
| Update endpoint accepting id + attacker-controlled fields without the two-tier taxonomy | Bad input reads as server fault; probing via 500s | Same `InvalidQuery`/invalid_params vs internal split as search (02-05), applied to update/relations from day one |
| Pre-migration backup file left with looser perms than the DB | Corpus copy readable where the original wasn't | Backup inherits the 0700-dir/0600-file discipline of the live DB |
| Windows binaries published unsigned/unverified with no checksum discipline | Tampered-binary risk higher on Windows (SmartScreen also flags) | Same SHA256SUMS discipline as v0.0.1 tarballs; document SmartScreen expectations in README |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| `search_mode: "hybrid"` reported while Ollama is down | Agent trusts degraded results as full-quality | Mode reports what actually ran (`keyword` when degraded) — extend, don't dilute, the honest-mode contract |
| Import silently "losing" expired/deduped rows | "Import ate my memories" | Rich import report: imported / skipped_duplicates / skipped_expired / edges_dropped, printed and returned |
| Imported memories invisible in search (decayed last_accessed) | "Import broke search" | Refresh `last_accessed` on import (or document aged-import explicitly) |
| Old binary + new DB = opaque startup failure inside an MCP client | Tool "randomly broken" after a rollback | Friendly version-skew error naming both versions and the fix, delivered via proper error channel (never stdout) |
| Update semantics unclear (does editing re-pin/re-decay/re-embed?) | Agents misuse update; surprising ranking shifts | Document update side-effects in the MCP tool description itself (agents read tool descriptions, not READMEs) |
| Windows install with no smoke-tested happy path | First Windows users hit path/AV/SmartScreen issues cold | Windows section in README: install, expected data dir (`%LOCALAPPDATA%`), SmartScreen note, `.mcp.json` example with `.exe` path |

## "Looks Done But Isn't" Checklist

- [ ] **Hybrid search:** fusion unit-tested — verify duplicate-id summing (both-legs #2 beats single-leg #1), k=60 math by hand, and a golden-query fixture; verify `search_mode` honesty when Ollama is stopped mid-test
- [ ] **Hybrid search:** malformed FTS5 query returns 400/invalid_params in hybrid mode too — verify at REST AND MCP layers (4-layer proof like 02-05)
- [ ] **memory_update:** semantic search no longer matches the OLD content after a content update — verify with stub embedder; verify tags-only update does NOT invalidate the vector
- [ ] **memory_update:** id unchanged, relations intact, sweep-materialized decay equals on-read recompute after update
- [ ] **Relations:** edges cleared by BOTH `forget` and TTL sweep — verify the sweep case, everyone tests only forget
- [ ] **Migrations:** fixture DB at v0.0.1 schema migrates to a byte-identical `sqlite_master` vs a fresh DB; too-new DB produces the friendly error; backup file appears before 0003 applies
- [ ] **Windows:** binary tested on real Windows (or documented Wine proxy) — store/search/forget round-trip against `%LOCALAPPDATA%`; console-close doesn't corrupt WAL; `.zip` + `.exe` + checksums published
- [ ] **musl:** functional smoke (store+search, not just `--version`) on the musl binary, host-arch-aware; README documents source-install CFLAGS
- [ ] **Export/import:** round-trip into a NON-empty overlapping-rowid DB; double-import idempotent (rows AND edges); expired-at-import rows reported not silently swept
- [ ] **All new endpoints:** limit/ttl_secs extremes (0, negative, i64::MAX, absent) return 400s per the taxonomy — the WR-01/WR-02 hardening covers new surface, not just old
- [ ] **Coverage:** every new branch (Windows config path, fusion, update tx, import remap) exercised in-process — spawned-binary tests contribute zero coverage

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Score-fusion shipped instead of RRF | LOW | Pure-function swap + golden tests; no schema/wire impact if `search_mode` was right |
| Stale vectors from update bug in the wild | LOW | One-shot maintenance: `UPDATE memories SET embedding_status=0 WHERE …` + `DELETE FROM vec_memories …`; sweep re-embeds |
| Access-count inflation from candidate-bumping | MEDIUM | Counts unrecoverable; reset `last_accessed` via one-time normalization migration; ranking self-heals over half-lives |
| Shipped-migration edit divergence | HIGH | Write a reconciliation migration that detects both schema variants and converges them; add the fixture test that should have existed |
| Orphaned relation edges | LOW | Cleanup migration `DELETE FROM memory_relations WHERE from_id/to_id NOT IN (SELECT id FROM memories)` + fix both delete paths |
| Mixed-model vectors after bad import | MEDIUM | Nuke and re-embed: clear `vec_memories`, reset all `embedding_status=0`, let sweep rebuild; detectable via meta pin comparison |
| Wrong-id relation edges after naive import | HIGH | No reliable detection (edges are plausible) — prevention only; if caught, restore from pre-import backup |
| Windows WAL corruption on roaming profile | HIGH | `sqlite3 .recover` best-effort; ship the `%LOCALAPPDATA%` fix + startup network-path warning; point users at export/import for migration |
| Broken musl release published | LOW | Yank/replace the tarball; binaries are per-target so other platforms unaffected |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| 1–3, 5 (RRF math, k's, dedup, limits) | Hybrid search phase | Fusion unit tests incl. hand-computed RRF values, duplicate-summing kill-test, bump-only-returned-ids test |
| 4 (mode honesty + taxonomy) | Hybrid search phase (+ API hardening re-check) | Decision-table service tests; malformed-query 400 in all three modes at both transports |
| 6–8 (stale vectors, rowid churn, timestamps) | MCP update/relations phase | Old-meaning-gone semantic test; id-stability test; STORE-03 agreement test extended to updated rows |
| 9 (relation orphans, per-connection FK) | MCP update/relations phase | Orphan-count-zero assertions after forget AND sweep; raw pragma-less connection test |
| 10–12 (shipped migrations, version skew, virtual-table limits) | First phase adding migration 0003 (likely update/relations) | v0.0.1 fixture divergence test; too-new error message test; backup-file-exists test; no ALTER on virtual tables in any migration |
| 13–15 (Windows pipeline, data dir, runtime) | Windows distribution phase — **needs a toolchain research spike first** | Real-Windows smoke run; `data_local_dir` in config.rs; cfg-audit grep gate; compile-check CI for the windows target |
| 16 (musl shim) | musl distribution phase (can land early — small) | musl release job green; functional smoke on musl binary; README source-install section |
| 17–19 (export vectors, id remap, timestamps) | Export/import phase — **sequence after relations** so edges are in format v1 | Non-empty-DB round-trip test; double-import idempotency; expired/aged fixture import report test |

**Suggested ordering consequence:** update/relations before export (format must carry edges); the migration-hygiene tests (Pitfall 10–11) land with the FIRST new migration regardless of which phase that is; the Windows toolchain spike should start early because it carries the only unresolved feasibility question in the milestone.

## Sources

- Codebase ground truth (read 2026-07-12): `crates/agent-memory-core/src/store/sqlite.rs` (writer lane, DELETE+INSERT on vec0, `map_fts_query_error`, explicit vec deletes in forget/sweep), `src/service.rs` (degrade seam, bump-on-returned-ids, backfill batch), `sql/0001_init.sql`/`0002_embeddings.sql` (FTS triggers, vec0-no-triggers divergence note, meta pin), `src/config.rs` (`dirs::data_dir`, single-seam cfg(unix)), root `Cargo.toml` (chrono/rusqlite/rusqlite_migration/sqlite-vec pins and rationale comments)
- [sqlite-vec PR #199 — musl typedef fix](https://github.com/asg017/sqlite-vec/pull/199) — verified still open/unmerged 2026-07-12; community-confirmed fix; June 2026 comment claims newer versions don't need it (unverified against released crates)
- [cargo-zigbuild README](https://github.com/rust-cross/cargo-zigbuild) — "currently only Linux and macOS targets are supported"; Windows-gnu possible via zig directly but not wired into zigbuild
- [rusqlite_migration docs — Migrations](https://docs.rs/rusqlite_migration/latest/rusqlite_migration/struct.Migrations.html) and [SchemaVersionError](https://docs.rs/rusqlite_migration/latest/rusqlite_migration/enum.SchemaVersionError.html) — `to_latest` errors when the DB version exceeds known migrations; user_version-only tracking (no checksums)
- SQLite official docs: FTS5 external-content tables + `'rebuild'` command; virtual tables ALTER limitations; WAL "does not work over a network filesystem"; per-connection `PRAGMA foreign_keys`
- Cormack & Clarke, "Reciprocal Rank Fusion outperforms Condorcet…" (SIGIR 2009) — the k≈60 convention; sqlite-vec hybrid-search examples use the same rank-based formulation
- Ecosystem precedents (wrapper CLAUDE.md Decisions Log): mcp-hub Windows cross-build failure on unguarded cfg(unix) deps (Phase 3 / 03-04); host-arch-aware smoke tests; orangepi/zigbuild as the standard release path (OPS-01); `allows_public_repositories: false` — no self-hosted jobs on public repos (OPS-02)
- v1.0 milestone artifacts: 02-REVIEW WR-01/WR-02/WR-05/WR-07 (REST boundary values, error taxonomy, tag semantics); prior `.planning/research/PITFALLS.md` (2026-06-24) for the v1.0 pitfall numbering referenced in code comments

---
*Pitfalls research for: agent-memory v1.1 Hardening & Interop milestone*
*Researched: 2026-07-12*
