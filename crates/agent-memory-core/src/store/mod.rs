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
use crate::domain::{MemoryError, MemoryView, NewMemory};
use crate::service::{ListArgs, SearchArgs};

/// Synchronous persistence interface, implemented by [`sqlite::SqliteStore`].
pub trait Store: Send + Sync {
    /// Insert a new memory, returning its assigned id. `now` is the injected
    /// timestamp used for `created_at`/`last_accessed`.
    fn insert(&self, new: NewMemory, now: i64) -> Result<i64, MemoryError>;

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
    /// FTS5 mirror is kept in sync by the delete trigger.
    fn forget(&self, id: i64) -> Result<bool, MemoryError>;

    /// Bump the recency of the given ids: set `last_accessed = now` and increment
    /// `access_count`. Runs on the writer lane. A no-op for an empty id slice.
    fn bump_access(&self, ids: &[i64], now: i64) -> Result<(), MemoryError>;

    /// Delete every TTL-expired row (`expires_at IS NOT NULL AND expires_at < now`)
    /// on the writer lane, returning the number of rows removed. Rows with a NULL
    /// `expires_at` are never touched. This is the ONLY delete the sweep performs —
    /// decay materialization is structurally forbidden from deleting (STORE-04 /
    /// Pitfall 7). The FTS5 delete trigger keeps `memories_fts` in sync.
    fn sweep_expired(&self, now: i64) -> Result<usize, MemoryError>;

    /// Recompute and persist each surviving row's `decay_score` column at `now`,
    /// honoring the per-type pinned half-life (D-08), returning the number of rows
    /// updated. This is an UPDATE-only operation — it never deletes — so ranking
    /// stays cheap between reads while STORE-04 holds (decay never removes).
    fn materialize_decay(&self, now: i64, cfg: &DecayConfig) -> Result<usize, MemoryError>;
}
