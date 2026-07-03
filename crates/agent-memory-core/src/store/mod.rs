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
use crate::domain::{MemoryError, MemoryType, MemoryView, NewMemory};
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

    /// Attach an embedding to an existing memory (sweep backfill): writes the
    /// vector row and flips `embedding_status` to 1 on the writer lane.
    fn insert_embedding(&self, memory_id: i64, embedding: Vec<f32>) -> Result<(), MemoryError>;

    /// The ids + contents of up to `limit` rows still awaiting an embedding
    /// (`embedding_status = 0`), oldest first — the sweep backfill work queue.
    fn pending_embeddings(&self, limit: usize) -> Result<Vec<(i64, String)>, MemoryError>;

    /// The INTEROP-01 idempotency probe: does a row with exactly this
    /// `(source, mem_type, content)` key already exist? `MemoryService::import`
    /// checks it before every insert so re-importing the same file is a no-op.
    fn exists(
        &self,
        source: &str,
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
    fn search(
        &self,
        args: SearchArgs,
        now: i64,
        weights: RankWeights,
        cfg: DecayConfig,
    ) -> Result<Vec<MemoryView>, MemoryError>;

    /// Delete a memory by id. Returns `true` if a row was deleted, `false` if no
    /// row with that id existed (clean not-found, never an error — MCP-04). The
    /// FTS5 mirror is kept in sync by the delete trigger; the `vec_memories`
    /// row is deleted EXPLICITLY (vec0 virtual tables ignore triggers —
    /// RESEARCH Pitfall 3 / T-02-05).
    fn forget(&self, id: i64) -> Result<bool, MemoryError>;

    /// Bump the recency of the given ids: set `last_accessed = now` and increment
    /// `access_count`. Runs on the writer lane. A no-op for an empty id slice.
    fn bump_access(&self, ids: &[i64], now: i64) -> Result<(), MemoryError>;

    /// Delete every TTL-expired row (`expires_at IS NOT NULL AND expires_at < now`)
    /// on the writer lane, returning the number of rows removed. Rows with a NULL
    /// `expires_at` are never touched. This is the ONLY delete the sweep performs —
    /// decay materialization is structurally forbidden from deleting (STORE-04 /
    /// Pitfall 7). The FTS5 delete trigger keeps `memories_fts` in sync; orphaned
    /// `vec_memories` rows are cleared explicitly in the same transaction (vec0
    /// ignores triggers — Pitfall 3 / T-02-05).
    fn sweep_expired(&self, now: i64) -> Result<usize, MemoryError>;

    /// Recompute and persist each surviving row's `decay_score` column at `now`,
    /// honoring the per-type pinned half-life (D-08), returning the number of rows
    /// updated. This is an UPDATE-only operation — it never deletes — so ranking
    /// stays cheap between reads while STORE-04 holds (decay never removes).
    fn materialize_decay(&self, now: i64, cfg: &DecayConfig) -> Result<usize, MemoryError>;
}
