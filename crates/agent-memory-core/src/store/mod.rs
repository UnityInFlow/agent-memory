//! Persistence layer. The [`Store`] trait abstracts SQLite behind a swappable,
//! synchronous interface; [`sqlite::SqliteStore`] is the production implementation.
//!
//! `Store` methods are **blocking** (rusqlite is synchronous). The async
//! [`crate::service::MemoryService`] wraps every call in `spawn_blocking`, so the
//! tokio runtime is never blocked (RESEARCH Pitfall 2). The TTL/decay sweep is
//! added by Plan 03.

pub mod migrations;
pub mod sqlite;

use crate::decay::{DecayConfig, RankWeights};
use crate::domain::{
    LinkKind, LinkOutcome, MemoryError, MemoryPatch, MemoryType, MemoryView, NewMemory,
    RelatedLink, UpdatedMemory,
};
use crate::service::{ListArgs, SearchArgs};

/// Synchronous persistence interface, implemented by [`sqlite::SqliteStore`].
pub trait Store: Send + Sync {
    /// Insert a new memory, returning its assigned id. `now` is the injected
    /// timestamp used for `created_at`/`last_accessed`. When `embedding` is
    /// `Some`, the memories INSERT, the `vec_memories` vector INSERT, and
    /// `embedding_status = 1` all happen in ONE writer transaction; when `None`,
    /// the row lands with `embedding_status = 0` (pending — backfilled by the
    /// sweep once the embedder recovers, SEARCH-03).
    fn insert(
        &self,
        new: NewMemory,
        embedding: Option<Vec<f32>>,
        now: i64,
    ) -> Result<i64, MemoryError>;

    /// K-nearest-neighbor candidate selection over `vec_memories` (SEARCH-02).
    /// Returns `(view, cosine_distance)` pairs, nearest first, oversampled
    /// beyond `args.limit` so the caller can re-rank by the similarity×decay
    /// blend. Honors the optional type/scope/tag filters (`None` disables).
    /// `args.query` (the text) is ignored here — the caller embeds it first.
    fn knn_search(
        &self,
        query: Vec<f32>,
        args: SearchArgs,
    ) -> Result<Vec<(MemoryView, f64)>, MemoryError>;

    /// Attach an embedding to an existing memory (sweep backfill), but ONLY
    /// if the row is still the pending row whose content was embedded
    /// (REVIEW CR-03 / T-04-23, T-04-24).
    ///
    /// The backfill embeds outside any lock, so by the time the vector comes
    /// back the row may have changed. The write therefore starts with one
    /// guarded claim in the writer transaction:
    /// `UPDATE memories SET embedding_status = 1 WHERE id = ? AND
    /// embedding_status = 0 AND content = ?`. Only if that claim changes a row
    /// is the vector written (vec0 DELETE + INSERT, same transaction).
    ///
    /// - `Ok(true)`: the vector was written and `embedding_status` is now 1.
    /// - `Ok(false)`: nothing was written. The row was updated since it was
    ///   read (its current content is embedded by `update` itself or by a
    ///   later sweep), was already embedded, or was forgotten.
    ///
    /// The claim closes three races: an `update` landing mid-embed (a stale
    /// vector would otherwise overwrite the fresh one, permanently, since the
    /// row reads as embedded), a `forget` landing mid-embed (a vector would
    /// otherwise attach to a later memory reusing the rowid), and two sweeps
    /// embedding the same row. SQLite takes its write lock at the claim, so
    /// this holds across the several processes that share one DB file, not
    /// just across threads.
    fn insert_embedding(
        &self,
        memory_id: i64,
        expected_content: &str,
        embedding: Vec<f32>,
    ) -> Result<bool, MemoryError>;

    /// The ids + contents of up to `limit` rows still awaiting an embedding
    /// (`embedding_status = 0`), oldest first — the sweep backfill work queue.
    /// The returned content is exactly the text the caller must embed AND pass
    /// back to [`Store::insert_embedding`] as `expected_content`.
    fn pending_embeddings(&self, limit: usize) -> Result<Vec<(i64, String)>, MemoryError>;

    /// The INTEROP-01 idempotency probe: does a row with exactly this
    /// `(source, mem_type, content)` key already exist? `MemoryService::import`
    /// checks it before every insert so re-importing the same file is a no-op.
    /// `source` is `Option` and compared NULL-safely: a `None` source matches a
    /// stored NULL source, never an empty string (CR-01).
    fn exists(
        &self,
        source: Option<&str>,
        mem_type: MemoryType,
        content: &str,
    ) -> Result<bool, MemoryError>;

    /// List stored memories newest-first, honoring optional type/tag/scope filters
    /// and an optional limit. `now` is reserved for on-read decay recomputation in
    /// later plans.
    fn list(&self, args: ListArgs, now: i64) -> Result<Vec<MemoryView>, MemoryError>;

    /// Keyword-search the corpus via the FTS5 mirror, ranking by a blend of
    /// (negated) bm25 relevance and the decay score recomputed inline from each
    /// row's `last_accessed` at `now` (D-04, recompute-on-read — Open Question 3).
    /// A query that matches nothing returns an empty vec, never an error (MCP-02).
    /// `weights` tunes the relevance/decay blend; `cfg` supplies the half-life.
    ///
    /// A natural-language query never fails this method (user decision
    /// option-a, T-04-15 revised): the raw MATCH string runs first, so valid
    /// FTS5 syntax keeps its meaning; input FTS5 cannot parse (punctuation, a
    /// hyphen or colon read as a column filter, an unbalanced quote, …) gets
    /// ONE retry as its word tokens quoted and OR-joined; a query with no word
    /// tokens is an empty vec. This method never returns
    /// [`MemoryError::InvalidQuery`] — only server faults are errors.
    fn search(
        &self,
        args: SearchArgs,
        now: i64,
        weights: RankWeights,
        cfg: DecayConfig,
    ) -> Result<Vec<MemoryView>, MemoryError>;

    /// The keyword leg of hybrid search (SEARCH-04): FTS5 candidates in
    /// **pure relevance** order (bm25 best first, ties by ascending id), with
    /// no decay term. The service applies decay once, after fusion (SEARCH-05),
    /// so a decay-blended leg here would count recency twice.
    ///
    /// Oversampled exactly like [`Store::knn_search`]: `limit * 4` rows (the
    /// limit defaults to 50), capped at the same KNN cap, so a hit ranked just
    /// past the limit still reaches fusion. Same type/scope/tag filters as
    /// [`Store::search`] (`None` disables). The query is handled exactly as on
    /// [`Store::search`], through the same helper, so classification does not
    /// depend on the mode: raw MATCH first, one sanitized retry (word tokens
    /// quoted and OR-joined) for input FTS5 cannot parse, and an empty vec for
    /// a tokenless query. Never [`MemoryError::InvalidQuery`]; only server
    /// faults are errors. A valid query that matches nothing is an empty vec.
    ///
    /// [`Store::search`] itself is unchanged: it is the keyword-only degrade
    /// path used when the embedder is down.
    fn keyword_candidates(&self, args: SearchArgs) -> Result<Vec<MemoryView>, MemoryError>;

    /// Delete a memory by id. Returns `true` if a row was deleted, `false` if no
    /// row with that id existed (clean not-found, never an error — MCP-04). The
    /// FTS5 mirror is kept in sync by the delete trigger; the `vec_memories`
    /// row is deleted EXPLICITLY (vec0 virtual tables ignore triggers —
    /// RESEARCH Pitfall 3 / T-02-05). Every `memory_links` edge touching the
    /// row goes with it: the FK `ON DELETE CASCADE` is the primary mechanism,
    /// and an explicit orphan-edge delete in the same transaction covers any
    /// connection that skipped the `foreign_keys` pragma (MCP-07 / T-04-12).
    fn forget(&self, id: i64) -> Result<bool, MemoryError>;

    /// Patch a memory IN PLACE (MCP-06 / D-08), returning the post-update row,
    /// or `Ok(None)` if no row with that id exists (clean not-found, never an
    /// error — the [`Store::forget`] contract).
    ///
    /// One writer transaction: the `memories` UPDATE (the FTS5 update trigger
    /// re-mirrors content), the freshness bump folded into the same statement
    /// (`last_accessed = now`, `access_count + 1` — D-09), and, when the patch
    /// carries new content, the explicit `vec_memories` invalidation (vec0
    /// ignores triggers). With new content, a `Some` `embedding` is written and
    /// `embedding_status` becomes 1; `None` leaves the row pending (0) for the
    /// sweep backfill. The id never changes: an update is never a DELETE+INSERT.
    /// `patch` must already be validated by the service.
    ///
    /// A row whose TTL has passed but which the sweep has not removed yet is
    /// still a row (REVIEW IN-05): the update succeeds, and a `ttl_secs` patch
    /// can extend or clear its expiry. This is consistent with list and search,
    /// which also show such rows until the sweep runs. Documented behaviour,
    /// deliberately unchanged.
    fn update(
        &self,
        id: i64,
        patch: MemoryPatch,
        embedding: Option<Vec<f32>>,
        now: i64,
    ) -> Result<Option<UpdatedMemory>, MemoryError>;

    /// Create the relation `from_id -[kind]-> to_id` (MCP-07), stamped
    /// `created_at = now`. One writer transaction: both endpoints are checked
    /// first, in REQUEST order, and a missing one returns
    /// [`LinkOutcome::NotFound`] naming it (`from_id` first) — a clean
    /// not-found, never an FK-violation error (D-02 / T-04-13).
    ///
    /// The row is stored under [`LinkKind::canonical_endpoints`]: `relates_to`
    /// as (smaller id, larger id), so linking b→a after a→b is the same
    /// symmetric relation (REVIEW WR-03); `supersedes` / `caused_by` as given
    /// (D-03 directed identity). A duplicate of the stored row is an idempotent
    /// no-op that still returns [`LinkOutcome::Linked`]. Canonicalisation is
    /// storage-only: callers echo the request's ids on the wire, and
    /// [`LinkOutcome`] carries no stored ids. The self-link check belongs to
    /// the service seam and has already run.
    fn link(
        &self,
        from_id: i64,
        to_id: i64,
        kind: LinkKind,
        now: i64,
    ) -> Result<LinkOutcome, MemoryError>;

    /// Remove the relation `from_id -[kind]-> to_id` (MCP-07). Returns `true`
    /// if an edge was removed, `false` if no such edge existed — the clean
    /// not-found of [`Store::forget`], never an error (D-02).
    ///
    /// For `supersedes` and `caused_by`, direction and kind are part of the
    /// edge's identity: only the requested orientation is removed. For a
    /// symmetric kind ([`LinkKind::is_symmetric`], i.e. `relates_to`) BOTH
    /// orientations match, so the relation can be removed from either end
    /// (REVIEW WR-03). That also removes any `relates_to` row a pre-fix dev
    /// build stored in request order; no migration is involved.
    fn unlink(&self, from_id: i64, to_id: i64, kind: LinkKind) -> Result<bool, MemoryError>;

    /// The 1-hop neighbors of memory `id`, following its edges in BOTH
    /// directions (MCP-07 / D-03), as trimmed summaries (D-05). At most `cap`
    /// entries, most-recent edges first (`created_at` descending — D-07).
    /// A pure read on the read pool: it never bumps anything (D-06). An id
    /// with no edges (or no row) yields an empty vec.
    fn related(&self, id: i64, cap: usize) -> Result<Vec<RelatedLink>, MemoryError>;

    /// Bump the recency of the given ids: set `last_accessed = now` and increment
    /// `access_count`. Runs on the writer lane. A no-op for an empty id slice.
    fn bump_access(&self, ids: &[i64], now: i64) -> Result<(), MemoryError>;

    /// Delete every TTL-expired row (`expires_at IS NOT NULL AND expires_at < now`)
    /// on the writer lane, returning the number of rows removed. Rows with a NULL
    /// `expires_at` are never touched. This is the ONLY delete the sweep performs —
    /// decay materialization is structurally forbidden from deleting (STORE-04 /
    /// Pitfall 7). The FTS5 delete trigger keeps `memories_fts` in sync; orphaned
    /// `vec_memories` rows are cleared explicitly in the same transaction (vec0
    /// ignores triggers — Pitfall 3 / T-02-05). Edges of expired rows cascade
    /// exactly as for [`Store::forget`], with the same explicit orphan-edge
    /// delete as the belt-and-braces backstop (MCP-07 / T-04-12).
    fn sweep_expired(&self, now: i64) -> Result<usize, MemoryError>;

    /// Recompute and persist each surviving row's `decay_score` column at `now`,
    /// honoring the per-type pinned half-life (D-08), returning the number of rows
    /// updated. This is an UPDATE-only operation — it never deletes — so ranking
    /// stays cheap between reads while STORE-04 holds (decay never removes).
    fn materialize_decay(&self, now: i64, cfg: &DecayConfig) -> Result<usize, MemoryError>;
}
