//! The transport-agnostic memory service.
//!
//! [`MemoryService`] is the single business-logic seam every transport (the rmcp
//! tools, and a future REST layer) calls into. It owns the [`Store`] and the
//! injected [`Clock`], and wraps every blocking store call in `spawn_blocking` so
//! the async runtime is never blocked (RESEARCH Pitfall 2).
//!
//! `search` and `forget` land in Plan 02; the decay sweep is Plan 03.

use std::sync::Arc;

use crate::clock::Clock;
use crate::decay::{apply_decay, DecayConfig, DecayEngine, RankWeights, SweepReport};
use crate::domain::{MemoryError, MemoryType, MemoryView, NewMemory};
use crate::store::Store;

/// Filters for [`MemoryService::list`]. All fields are optional; a `None` field
/// disables that filter. `limit` caps the number of rows returned.
#[derive(Debug, Clone, Default)]
pub struct ListArgs {
    pub mem_type: Option<MemoryType>,
    pub tag: Option<String>,
    pub scope: Option<String>,
    pub limit: Option<i64>,
}

/// Arguments for [`MemoryService::search`]. `query` is the FTS5 MATCH string; the
/// remaining fields are optional filters and a result cap.
#[derive(Debug, Clone, Default)]
pub struct SearchArgs {
    pub query: String,
    pub mem_type: Option<MemoryType>,
    pub tag: Option<String>,
    pub scope: Option<String>,
    pub limit: Option<i64>,
}

/// The core memory service: validates input, stamps timestamps from the injected
/// clock, and delegates persistence to the [`Store`].
#[derive(Clone)]
pub struct MemoryService {
    store: Arc<dyn Store>,
    clock: Arc<dyn Clock>,
    decay_cfg: DecayConfig,
}

impl MemoryService {
    /// Construct a service over a store and clock with the given decay config.
    pub fn new(store: Arc<dyn Store>, clock: Arc<dyn Clock>, decay_cfg: DecayConfig) -> Self {
        MemoryService {
            store,
            clock,
            decay_cfg,
        }
    }

    /// Store a new memory, returning its assigned id (MCP-01 / D-05).
    ///
    /// The type is validated by the caller building a [`NewMemory`] with a typed
    /// [`MemoryType`]; an invalid wire string is rejected before this point via
    /// [`MemoryType::try_from`], so no bad type ever reaches SQL (D-07).
    pub async fn store(&self, new: NewMemory) -> Result<i64, MemoryError> {
        let store = self.store.clone();
        let now = self.clock.now();
        tokio::task::spawn_blocking(move || store.insert(new, now))
            .await
            .map_err(MemoryError::Join)?
    }

    /// List stored memories newest-first with optional filters (MCP-03 / D-06).
    pub async fn list(&self, args: ListArgs) -> Result<Vec<MemoryView>, MemoryError> {
        let store = self.store.clone();
        let now = self.clock.now();
        tokio::task::spawn_blocking(move || store.list(args, now))
            .await
            .map_err(MemoryError::Join)?
    }

    /// Keyword-search the corpus (MCP-02 / SEARCH-01 / D-04).
    ///
    /// Runs the blocking FTS5 + bm25×decay query on a `spawn_blocking` thread,
    /// recomputes each result's `decay_score` on read from `last_accessed`
    /// (STORE-03 surfaced), then fires a fire-and-forget recency bump on the
    /// returned ids through the writer lane (Open Question 2). A query that
    /// matches nothing returns `Ok(vec![])` — never an error. No network call is
    /// made on this path (keyword-only; Ollama is Phase 2).
    pub async fn search(&self, args: SearchArgs) -> Result<Vec<MemoryView>, MemoryError> {
        let store = self.store.clone();
        let now = self.clock.now();
        let cfg = self.decay_cfg;
        let weights = RankWeights::default();

        let mut views = tokio::task::spawn_blocking(move || store.search(args, now, weights, cfg))
            .await
            .map_err(MemoryError::Join)??;

        // Recompute decay on read so results carry an up-to-the-moment score even
        // between background sweeps (STORE-03).
        for view in &mut views {
            apply_decay(view, now, &cfg);
        }

        // Fire-and-forget recency bump: retrieving a memory bumps its
        // last_accessed/access_count so it surfaces higher next time. A failure
        // here must not fail the search.
        let ids: Vec<i64> = views.iter().map(|v| v.id).collect();
        if !ids.is_empty() {
            let store = self.store.clone();
            tokio::task::spawn_blocking(move || {
                let _ = store.bump_access(&ids, now);
            });
        }

        Ok(views)
    }

    /// Delete a memory by id (MCP-04 / D-06). Returns `true` if a row was deleted,
    /// `false` for an unknown id (clean not-found, never an error).
    pub async fn forget(&self, id: i64) -> Result<bool, MemoryError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.forget(id))
            .await
            .map_err(MemoryError::Join)?
    }

    /// Run one lifecycle sweep at `now` (STORE-04): delete TTL-expired rows, then
    /// materialize every survivor's `decay_score` so ranking stays cheap between
    /// reads. Decay never deletes — only TTL expiry (here) and `forget` remove.
    ///
    /// Driven by the injected clock so it is deterministic under test; exposed for
    /// both the background interval task and `tests/ttl.rs`. The blocking
    /// `DecayEngine::sweep` runs on a `spawn_blocking` thread (Pitfall 2).
    pub async fn sweep(&self, now: i64) -> Result<SweepReport, MemoryError> {
        let store = self.store.clone();
        let engine = DecayEngine::new(self.decay_cfg);
        tokio::task::spawn_blocking(move || engine.sweep(store.as_ref(), now))
            .await
            .map_err(MemoryError::Join)?
    }
}
