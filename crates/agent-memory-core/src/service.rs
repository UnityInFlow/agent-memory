//! The transport-agnostic memory service.
//!
//! [`MemoryService`] is the single business-logic seam every transport (the rmcp
//! tools, and the REST layer) calls into. It owns the [`Store`], the injected
//! [`Clock`], and the injected [`Embedder`], and wraps every blocking store call
//! in `spawn_blocking` so the async runtime is never blocked (RESEARCH Pitfall
//! 2). Embed HTTP calls are async and happen OUTSIDE `spawn_blocking`
//! (Pitfall 5).
//!
//! **The SEARCH-03 degrade seam lives HERE**, never in transports: an embed
//! failure downgrades `store` to a pending row and routes `search` to the FTS5
//! keyword path — and ONLY an embed failure triggers the fallback (an empty
//! semantic result set is a valid `Semantic` answer).

use std::sync::Arc;

use crate::clock::Clock;
use crate::decay::{apply_decay, decay_score, DecayConfig, DecayEngine, RankWeights, SweepReport};
use crate::domain::{MemoryError, MemoryType, MemoryView, NewMemory};
use crate::embed::Embedder;
use crate::store::sqlite::DEFAULT_SEARCH_LIMIT;
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

/// Which search path produced a result set. Serialized lowercase — the shared
/// wire form for MCP and REST (RESEARCH Open Question 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    /// Ranked by Ollama embedding similarity blended with decay (SEARCH-02).
    Semantic,
    /// FTS5 keyword fallback — the embedder was unavailable (SEARCH-03).
    Keyword,
}

/// The ONE shared search envelope every transport serializes (MCP now, REST in
/// plan 02-02): a top-level `search_mode` plus the ranked results.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchOutcome {
    /// Which path produced `results` — surfaces degraded state to agents/users.
    pub search_mode: SearchMode,
    /// Ranked result rows (may be empty; an empty semantic set is still
    /// `Semantic` — only an embed failure switches the mode).
    pub results: Vec<MemoryView>,
}

/// The core memory service: validates input, stamps timestamps from the injected
/// clock, embeds best-effort via the injected [`Embedder`], and delegates
/// persistence to the [`Store`].
#[derive(Clone)]
pub struct MemoryService {
    store: Arc<dyn Store>,
    clock: Arc<dyn Clock>,
    embedder: Arc<dyn Embedder>,
    decay_cfg: DecayConfig,
}

impl MemoryService {
    /// Construct a service over a store, clock, and embedder with the given
    /// decay config. The embedder is injected exactly like the clock — the
    /// SEARCH-03 degrade seam (`Arc<dyn Embedder>`).
    pub fn new(
        store: Arc<dyn Store>,
        clock: Arc<dyn Clock>,
        embedder: Arc<dyn Embedder>,
        decay_cfg: DecayConfig,
    ) -> Self {
        MemoryService {
            store,
            clock,
            embedder,
            decay_cfg,
        }
    }

    /// Store a new memory, returning its assigned id (MCP-01 / D-05).
    ///
    /// The type is validated by the caller building a [`NewMemory`] with a typed
    /// [`MemoryType`]; an invalid wire string is rejected before this point via
    /// [`MemoryType::try_from`], so no bad type ever reaches SQL (D-07).
    ///
    /// The content is embedded best-effort (async, OUTSIDE `spawn_blocking` —
    /// Pitfall 5). Storing NEVER fails because Ollama is down: on embed failure
    /// the row lands with `embedding_status = 0` and exactly one loud warning is
    /// emitted (Pitfall 4 — loud, not silent; SEARCH-03).
    pub async fn store(&self, new: NewMemory) -> Result<i64, MemoryError> {
        let now = self.clock.now();
        let inputs = [new.content.clone()];
        let embedding = match self.embedder.embed(&inputs).await {
            Ok(mut vectors) if !vectors.is_empty() => Some(vectors.swap_remove(0)),
            Ok(_) => {
                tracing::warn!(
                    "embedding skipped (empty batch response); storing with embedding_status = 0"
                );
                None
            }
            Err(e) => {
                tracing::warn!("embedding failed ({e}); storing with embedding_status = 0");
                None
            }
        };
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.insert(new, embedding, now))
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

    /// Search the corpus (MCP-02 / SEARCH-02 / SEARCH-03 / D-04).
    ///
    /// Semantic-first with graceful keyword fallback — **the degrade decision
    /// lives HERE at the embedder seam, never in transports**:
    ///
    /// - Embed OK → vec0 KNN candidates (`spawn_blocking`), then in Rust blend
    ///   `relevance × (1 − cosine_distance) + decay × decay_score` over the
    ///   oversampled set, truncate to the limit, and return
    ///   [`SearchMode::Semantic`]. An empty candidate set is a valid semantic
    ///   answer (Pitfall 2: emptiness never means "fall back").
    /// - Embed Err → one loud warning, then the UNCHANGED Phase-1 FTS5
    ///   bm25×decay path, returned as [`SearchMode::Keyword`]. A malformed FTS5
    ///   query error on this path maps exactly as before. Never an error and
    ///   never empty-because-of-outage (SEARCH-03).
    ///
    /// Both paths fire the fire-and-forget recency bump on returned ids.
    pub async fn search(&self, args: SearchArgs) -> Result<SearchOutcome, MemoryError> {
        let now = self.clock.now();
        let cfg = self.decay_cfg;
        let weights = RankWeights::default();
        let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).max(0) as usize;

        let query_input = [args.query.clone()];
        match self.embedder.embed(&query_input).await {
            Ok(mut vectors) if !vectors.is_empty() => {
                let query_vec = vectors.swap_remove(0);
                let store = self.store.clone();
                let knn_args = args;
                let candidates =
                    tokio::task::spawn_blocking(move || store.knn_search(query_vec, knn_args))
                        .await
                        .map_err(MemoryError::Join)??;

                // Similarity×decay blend in Rust over the small candidate set:
                // cosine distance 0 = identical, so (1 - distance) grows with a
                // better match — additive with the decay term like the FTS5
                // blend (decay recomputed from last_accessed at `now`, STORE-03).
                let mut scored: Vec<(MemoryView, f64)> = candidates
                    .into_iter()
                    .map(|(mut view, distance)| {
                        let d = decay_score(
                            now,
                            view.last_accessed,
                            cfg.half_life_secs,
                            view.mem_type.is_pinned(),
                        );
                        view.decay_score = d;
                        let blended = weights.relevance * (1.0 - distance) + weights.decay * d;
                        (view, blended)
                    })
                    .collect();
                scored.sort_by(|a, b| b.1.total_cmp(&a.1));
                let results: Vec<MemoryView> = scored
                    .into_iter()
                    .take(limit)
                    .map(|(view, _)| view)
                    .collect();

                self.spawn_bump(results.iter().map(|v| v.id).collect(), now);
                Ok(SearchOutcome {
                    search_mode: SearchMode::Semantic,
                    results,
                })
            }
            other => {
                // Embed failure (or an invalid empty batch): degrade LOUDLY to
                // the existing FTS5 keyword path (SEARCH-03, Pitfall 2/4).
                match other {
                    Err(e) => tracing::warn!(
                        "semantic search unavailable ({e}); falling back to keyword search"
                    ),
                    _ => tracing::warn!(
                        "semantic search unavailable (empty embed batch); falling back to keyword search"
                    ),
                }
                let store = self.store.clone();
                let mut views =
                    tokio::task::spawn_blocking(move || store.search(args, now, weights, cfg))
                        .await
                        .map_err(MemoryError::Join)??;

                // Recompute decay on read so results carry an up-to-the-moment
                // score even between background sweeps (STORE-03).
                for view in &mut views {
                    apply_decay(view, now, &cfg);
                }

                self.spawn_bump(views.iter().map(|v| v.id).collect(), now);
                Ok(SearchOutcome {
                    search_mode: SearchMode::Keyword,
                    results: views,
                })
            }
        }
    }

    /// Fire-and-forget recency bump: retrieving a memory bumps its
    /// last_accessed/access_count so it surfaces higher next time (Open
    /// Question 2). A failure here must not fail the search.
    fn spawn_bump(&self, ids: Vec<i64>, now: i64) {
        if ids.is_empty() {
            return;
        }
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let _ = store.bump_access(&ids, now);
        });
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
