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
//! hybrid result set is a valid `Hybrid` answer).

use std::collections::HashMap;
use std::sync::Arc;

use crate::clock::Clock;
use crate::decay::{apply_decay, decay_score, DecayConfig, DecayEngine, RankWeights, SweepReport};
use crate::domain::{
    LinkKind, LinkOutcome, MemoryError, MemoryPatch, MemoryType, MemoryView, NewMemory,
    UpdatedMemory, MAX_EXPANDED_LINKS, MAX_LIMIT, MIN_LIMIT,
};
use crate::embed::Embedder;
use crate::store::sqlite::DEFAULT_SEARCH_LIMIT;
use crate::store::Store;

/// The only message a client ever sees for a storage-tier search failure
/// (T-04-19). The underlying error goes to the stderr log, never to a response
/// body, so no SQLite text (table/column names, engine detail) leaks out of
/// any search path. Both transports surface it on the internal tier (REST 500
/// / MCP `internal_error`).
pub const SEARCH_STORAGE_FAILURE: &str =
    "search failed due to a storage error (details are in the server log)";

/// Tier every search-path error for the wire — the ONE place this happens.
///
/// - Client tier (`InvalidType`, `InvalidQuery`, `InvalidArgument`) passes
///   through: its text echoes only client input and public bounds.
/// - Crate-authored internal text (`Internal`, `SchemaTooNew`) passes through.
/// - Storage-engine errors (`Sqlite`, `Pool`, `Join`, `Migration`) are logged
///   with full detail, then replaced by [`SEARCH_STORAGE_FAILURE`].
///
/// No catch-all arm: a new [`MemoryError`] variant must be tiered here at
/// compile time.
fn redact_search_error(e: MemoryError) -> MemoryError {
    match e {
        MemoryError::InvalidType(_)
        | MemoryError::InvalidQuery(_)
        | MemoryError::InvalidArgument(_) => e,
        MemoryError::Internal(_) | MemoryError::SchemaTooNew { .. } => e,
        MemoryError::Sqlite(_)
        | MemoryError::Pool(_)
        | MemoryError::Join(_)
        | MemoryError::Migration(_) => {
            tracing::error!(error = %e, "search failed with a storage error");
            MemoryError::Internal(SEARCH_STORAGE_FAILURE.to_string())
        }
    }
}

/// Filters for [`MemoryService::list`]. All fields are optional; a `None` field
/// disables that filter. `limit` caps the number of rows returned.
#[derive(Debug, Clone, Default)]
pub struct ListArgs {
    pub mem_type: Option<MemoryType>,
    pub tag: Option<String>,
    pub scope: Option<String>,
    pub limit: Option<i64>,
    /// Attach 1-hop relation summaries to every returned row (MCP-07 / D-04).
    /// `false` (the default) keeps the v1.0 response byte-identical. `true`
    /// requires `limit` (REVIEW WR-04, user decision option-a): without one,
    /// [`MemoryService::list`] rejects the call as
    /// [`MemoryError::InvalidArgument`].
    pub expand_links: bool,
}

/// Arguments for [`MemoryService::search`]. `query` is plain natural language or
/// FTS5 MATCH syntax: valid FTS5 runs exactly as written, anything FTS5 cannot
/// parse is retried once as its words OR-ed together, and a query with no words
/// contributes no keyword matches — it is never rejected for its syntax, in
/// either mode (T-04-15 revised). The remaining fields are optional filters and
/// a result cap.
#[derive(Debug, Clone, Default)]
pub struct SearchArgs {
    pub query: String,
    pub mem_type: Option<MemoryType>,
    pub tag: Option<String>,
    pub scope: Option<String>,
    pub limit: Option<i64>,
    /// Attach 1-hop relation summaries to every returned row (MCP-07 / D-04).
    /// Orthogonal to the search mode: applies on the hybrid and the keyword
    /// path alike. `false` (the default) keeps the v1.0 envelope byte-identical.
    pub expand_links: bool,
}

/// Which search path produced a result set. Serialized lowercase — the shared
/// wire form for MCP and REST (RESEARCH Open Question 3).
///
/// There is no `semantic` value any more: with the embedder up, every search
/// is fused (SEARCH-04), so a semantic-only variant would have no producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    /// The embedder was available: the KNN leg and the bm25 keyword leg were
    /// fused with Reciprocal Rank Fusion, then decay-blended (SEARCH-04/05).
    Hybrid,
    /// FTS5 keyword fallback — the embedder was unavailable (SEARCH-03).
    Keyword,
}

/// Reciprocal Rank Fusion smoothing constant: the `k` in `1 / (k + rank)`,
/// the Cormack & Clarke convention (SEARCH-04).
///
/// This is NOT the KNN oversample size or its cap (`MAX_KNN_K` in the SQLite
/// store, 800). The two are different quantities that happen to both be
/// called `k` in the literature, so neither is ever passed around as a bare
/// `k` (RESEARCH Pitfall 2). Changing this reshapes every hybrid ranking.
pub const RRF_K: f64 = 60.0;

/// Fuse ranked id lists with Reciprocal Rank Fusion (SEARCH-04).
///
/// Each leg is a list of ids, best first. An id at 1-based rank `r` in a leg
/// contributes `1 / (RRF_K + r)`, and an id that appears in several legs SUMS
/// its contributions, so agreement between legs is rewarded: rank 2 in both
/// legs (`2/62`) beats rank 1 in one leg (`1/61`). Only ranks matter, never
/// the legs' raw scores, which is why bm25 values and cosine distances never
/// need to be put on a common scale.
///
/// Output is sorted by fused score, descending (`total_cmp`), with ties
/// broken by ascending id, so the result is deterministic regardless of hash
/// order. Decay is NOT applied here: the caller blends it in once, after
/// fusion (SEARCH-05 / RESEARCH Pitfall 1), so the legs must be pure
/// relevance orderings.
pub fn rrf_fuse(legs: &[Vec<i64>]) -> Vec<(i64, f64)> {
    let mut scores: HashMap<i64, f64> = HashMap::new();
    for leg in legs {
        for (idx, id) in leg.iter().enumerate() {
            let rank = idx as f64 + 1.0;
            *scores.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + rank);
        }
    }
    let mut fused: Vec<(i64, f64)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    fused
}

/// The ONE shared search envelope every transport serializes (MCP now, REST in
/// plan 02-02): a top-level `search_mode` plus the ranked results.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchOutcome {
    /// Which path produced `results` — surfaces degraded state to agents/users.
    pub search_mode: SearchMode,
    /// Ranked result rows (may be empty; an empty hybrid set is still
    /// `Hybrid` — only an embed failure switches the mode).
    pub results: Vec<MemoryView>,
}

/// Outcome of one [`MemoryService::import`] batch (INTEROP-01): how many drafts
/// landed as new rows and how many were dropped as exact duplicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ImportReport {
    /// Drafts inserted as new memories.
    pub imported: usize,
    /// Drafts skipped because an identical `(source, mem_type, content)` row
    /// already exists — the idempotency guarantee.
    pub skipped_duplicates: usize,
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
        // API-02 / D-02: reject a bad TTL FIRST — before the embed call, so a
        // bad request never costs an Ollama round-trip (D-06 seam).
        crate::domain::validate_ttl(new.ttl_secs)?;
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
    ///
    /// `expand_links` requires a `limit` (REVIEW WR-04, user decision
    /// option-a): an expanded list with no limit is rejected as
    /// [`MemoryError::InvalidArgument`] before any store hop, with a message
    /// telling the caller to pass a limit. The rule is enforced here, at the
    /// shared seam, so MCP and REST cannot drift, and the
    /// `200 × MAX_EXPANDED_LINKS` bound therefore covers every expanded
    /// response. A list without `expand_links` still returns every row when
    /// `limit` is omitted.
    pub async fn list(&self, args: ListArgs) -> Result<Vec<MemoryView>, MemoryError> {
        // API-02 / D-01: a present limit must be 1..=200. An omitted limit
        // stays unlimited (v1.0 behavior, RESEARCH Pitfall 6) only without
        // `expand_links`; range validation runs first.
        crate::domain::validate_limit(args.limit)?;
        if args.expand_links && args.limit.is_none() {
            return Err(MemoryError::InvalidArgument(format!(
                "limit is required when expand_links is true; \
                 pass a limit between {MIN_LIMIT} and {MAX_LIMIT}"
            )));
        }
        let expand_links = args.expand_links;
        let store = self.store.clone();
        let now = self.clock.now();
        let views = tokio::task::spawn_blocking(move || store.list(args, now))
            .await
            .map_err(MemoryError::Join)??;
        if expand_links {
            return self.expand_related(views).await;
        }
        Ok(views)
    }

    /// Search the corpus (MCP-02 / SEARCH-03 / SEARCH-04 / SEARCH-05 / D-04).
    ///
    /// Hybrid-first with graceful keyword fallback — **the degrade decision
    /// lives HERE at the embedder seam, never in transports**:
    ///
    /// - Embed OK → [`Self::hybrid_candidates`]: the KNN leg and the bm25
    ///   keyword leg run concurrently, both oversampled and both in pure
    ///   relevance order. They are fused with [`rrf_fuse`], each fused score is
    ///   multiplied by the pinned-aware decay score once, and the result is
    ///   truncated to the limit and returned as [`SearchMode::Hybrid`]. An
    ///   empty result is a valid hybrid answer (Pitfall 2: emptiness never
    ///   means "fall back"). A leg's internal error is never swallowed, so
    ///   hybrid never silently serves one leg's rows as a fused answer.
    /// - Embed Err → one loud warning, then the UNCHANGED Phase-1 FTS5
    ///   bm25×decay path, returned as [`SearchMode::Keyword`]. Never an error
    ///   and never empty-because-of-outage (SEARCH-03).
    ///
    /// **Query handling is identical in both modes** (user decision option-a,
    /// T-04-15 revised): both keyword paths share one store helper that runs
    /// the raw FTS5 MATCH first, retries FTS5-unparseable input once as its
    /// word tokens OR-ed together, and treats a tokenless query as no keyword
    /// matches. Hybrid then answers from the semantic leg alone; keyword mode
    /// returns an empty set. Search never returns [`MemoryError::InvalidQuery`].
    ///
    /// Both paths fire the fire-and-forget recency bump on the returned ids
    /// only — never on fusion candidates or on expanded neighbors — and then
    /// attach the 1-hop expansion when `expand_links` is set.
    ///
    /// Every error, from either arm and from the expansion, leaves through
    /// [`redact_search_error`]: a storage failure reaches the client only as
    /// [`MemoryError::Internal`] carrying [`SEARCH_STORAGE_FAILURE`], with the
    /// detail in the stderr log (T-04-19).
    pub async fn search(&self, args: SearchArgs) -> Result<SearchOutcome, MemoryError> {
        self.search_unredacted(args)
            .await
            .map_err(redact_search_error)
    }

    /// The body of [`Self::search`], before its errors are tiered for the wire
    /// by [`redact_search_error`].
    async fn search_unredacted(&self, args: SearchArgs) -> Result<SearchOutcome, MemoryError> {
        // API-02 / D-01: validate before any embed hop — an out-of-bounds
        // limit rejects with MemoryError::InvalidArgument; the old silent
        // zero-clamp is gone, so bad input never truncates quietly.
        crate::domain::validate_limit(args.limit)?;
        let now = self.clock.now();
        let cfg = self.decay_cfg;
        let weights = RankWeights::default();
        let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT) as usize;
        let expand_links = args.expand_links;

        let query_input = [args.query.clone()];
        match self.embedder.embed(&query_input).await {
            Ok(mut vectors) if !vectors.is_empty() => {
                let query_vec = vectors.swap_remove(0);
                // `args` moves into the legs here; `limit` and `expand_links`
                // were captured above.
                let fused = self.hybrid_candidates(query_vec, args, now).await?;
                let results: Vec<MemoryView> = fused.into_iter().take(limit).collect();

                // Bump ONLY the returned ids — after truncation, so fusion
                // candidates are never bumped (Pitfall 3 / T-04-17), and before
                // expansion, so expanded neighbors are never bumped (D-06).
                self.spawn_bump(results.iter().map(|v| v.id).collect(), now);
                let results = if expand_links {
                    self.expand_related(results).await?
                } else {
                    results
                };
                Ok(SearchOutcome {
                    search_mode: SearchMode::Hybrid,
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

                // Same order as the hybrid path: bump the returned ids, then
                // expand — neighbors are never bumped (D-06).
                self.spawn_bump(views.iter().map(|v| v.id).collect(), now);
                let views = if expand_links {
                    self.expand_related(views).await?
                } else {
                    views
                };
                Ok(SearchOutcome {
                    search_mode: SearchMode::Keyword,
                    results: views,
                })
            }
        }
    }

    /// Import a batch of drafts idempotently (INTEROP-01).
    ///
    /// 1. **Dedup** — every draft is checked against the store on the exact
    ///    `(source, mem_type, content)` key via [`Store::exists`], in ONE
    ///    `spawn_blocking` hop over the whole batch. An edited bullet imports
    ///    as a NEW memory by design: the key is exact content equality — the
    ///    idempotency contract (phase success criterion 4) requires only that
    ///    an unchanged re-import creates no duplicates.
    /// 2. **Batch embed** — all new contents go to the embedder in ONE call
    ///    (`/api/embed` accepts an array). On failure, one warning is logged
    ///    and every draft inserts with a pending embedding — an import NEVER
    ///    fails because Ollama is down (SEARCH-03; the sweep backfills later).
    /// 3. **Insert** — new drafts land via the standard [`Store::insert`]
    ///    path, all stamped with one clock reading.
    pub async fn import(&self, drafts: Vec<NewMemory>) -> Result<ImportReport, MemoryError> {
        // API-02 / D-02 + RESEARCH Pitfall 3: import() builds rows via
        // store.insert, bypassing store() — validate every draft through the
        // SAME shared helper here, before the dedup hop, so future import
        // surfaces (Phase 5 JSONL) inherit the seam structurally.
        for draft in &drafts {
            crate::domain::validate_ttl(draft.ttl_secs)?;
        }
        let now = self.clock.now();

        // (1) Partition new vs duplicate in one blocking hop (read pool).
        let store = self.store.clone();
        let (fresh, skipped_duplicates) =
            tokio::task::spawn_blocking(move || -> Result<(Vec<NewMemory>, usize), MemoryError> {
                let mut fresh = Vec::new();
                let mut duplicates = 0usize;
                for draft in drafts {
                    // Pass the source through as-is: a `None` must probe for
                    // NULL, exactly what the insert stores (CR-01).
                    if store.exists(draft.source.as_deref(), draft.mem_type, &draft.content)? {
                        duplicates += 1;
                    } else {
                        fresh.push(draft);
                    }
                }
                Ok((fresh, duplicates))
            })
            .await
            .map_err(MemoryError::Join)??;

        if fresh.is_empty() {
            return Ok(ImportReport {
                imported: 0,
                skipped_duplicates,
            });
        }

        // (2) One best-effort batch embed call for every new draft.
        let contents: Vec<String> = fresh.iter().map(|d| d.content.clone()).collect();
        let embeddings = match self.embedder.embed(&contents).await {
            Ok(vectors) if vectors.len() == fresh.len() => Some(vectors),
            Ok(_) => {
                tracing::warn!("import embedding skipped (batch size mismatch); rows land pending");
                None
            }
            Err(e) => {
                tracing::warn!("import embedding failed ({e}); rows land pending");
                None
            }
        };

        // (3) Insert every new draft through the standard store path.
        let store = self.store.clone();
        let imported = tokio::task::spawn_blocking(move || -> Result<usize, MemoryError> {
            let mut imported = 0usize;
            for (i, draft) in fresh.into_iter().enumerate() {
                let embedding = embeddings.as_ref().map(|v| v[i].clone());
                store.insert(draft, embedding, now)?;
                imported += 1;
            }
            Ok(imported)
        })
        .await
        .map_err(MemoryError::Join)??;

        Ok(ImportReport {
            imported,
            skipped_duplicates,
        })
    }

    /// The hybrid ranking (SEARCH-04/05), before truncation: every fusion
    /// candidate, best first.
    ///
    /// 1. Both legs run concurrently on blocking threads: [`Store::knn_search`]
    ///    (cosine-distance order) and [`Store::keyword_candidates`] (bm25
    ///    order). Both are pure relevance and both oversample `limit * 4`.
    /// 2. Errors propagate, the keyword leg's first, so which internal error
    ///    surfaces is deterministic when both legs fail. The keyword leg never
    ///    fails on query syntax: it shares the keyword-mode helper (raw MATCH
    ///    first, one sanitized OR-of-words retry, tokenless → no candidates),
    ///    so classification is identical in both modes (T-04-15 revised). A
    ///    tokenless query fuses the KNN leg alone.
    /// 3. The two id lists are fused with [`rrf_fuse`].
    /// 4. Decay is applied exactly ONCE, post-fusion and multiplicatively:
    ///    `final = rrf_score × decay_score(now, last_accessed, half_life,
    ///    pinned)`. RRF scores are tiny (at most 2/61 ≈ 0.033) while decay is
    ///    in (0, 1], so an additive term would drown relevance. Multiplying
    ///    keeps "equal fused rank → the fresher memory wins" exact, and the
    ///    pinned half-life keeps its advantage (Pitfall 1). Each view carries
    ///    that same decay value on the wire.
    /// 5. Sorted by the final score descending, ties by ascending id.
    ///
    /// Fusion is O(candidates) in memory over lists bounded by the KNN cap
    /// (T-04-16). Nothing here bumps access stats.
    async fn hybrid_candidates(
        &self,
        query_vec: Vec<f32>,
        args: SearchArgs,
        now: i64,
    ) -> Result<Vec<MemoryView>, MemoryError> {
        let cfg = self.decay_cfg;
        let knn_store = self.store.clone();
        let knn_args = args.clone();
        let keyword_store = self.store.clone();
        let keyword_args = args;
        let (knn, keyword) = tokio::join!(
            tokio::task::spawn_blocking(move || knn_store.knn_search(query_vec, knn_args)),
            tokio::task::spawn_blocking(move || keyword_store.keyword_candidates(keyword_args)),
        );
        let keyword = keyword.map_err(MemoryError::Join)??;
        let knn = knn.map_err(MemoryError::Join)??;

        let knn_ids: Vec<i64> = knn.iter().map(|(view, _)| view.id).collect();
        let keyword_ids: Vec<i64> = keyword.iter().map(|view| view.id).collect();
        let mut views: HashMap<i64, MemoryView> = HashMap::new();
        for view in knn.into_iter().map(|(view, _)| view).chain(keyword) {
            views.entry(view.id).or_insert(view);
        }

        let mut scored: Vec<(MemoryView, f64)> = rrf_fuse(&[knn_ids, keyword_ids])
            .into_iter()
            .filter_map(|(id, rrf_score)| {
                // Every fused id came from one of the two legs, so the lookup
                // always hits; filter_map just avoids an unwrap.
                views.remove(&id).map(|mut view| {
                    let d = decay_score(
                        now,
                        view.last_accessed,
                        cfg.half_life_secs,
                        view.mem_type.is_pinned(),
                    );
                    view.decay_score = d;
                    (view, rrf_score * d)
                })
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.id.cmp(&b.0.id)));
        Ok(scored.into_iter().map(|(view, _)| view).collect())
    }

    /// Attach 1-hop relation summaries to already-final result rows (MCP-07):
    /// one `spawn_blocking` hop that reads each row's neighbors via
    /// [`Store::related`], capped at [`MAX_EXPANDED_LINKS`] most-recent edges
    /// (D-07). Runs AFTER truncation and AFTER the recency bump, and is a pure
    /// read — neighbor ids never reach [`Self::spawn_bump`] (D-06). Every row
    /// gets `Some(..)`, an explicit `[]` when it has no edges, so a flag-on
    /// response always carries the member. Strictly 1-hop: neighbors are not
    /// themselves expanded (T-04-11).
    async fn expand_related(
        &self,
        mut views: Vec<MemoryView>,
    ) -> Result<Vec<MemoryView>, MemoryError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            for view in &mut views {
                view.related = Some(store.related(view.id, MAX_EXPANDED_LINKS)?);
            }
            Ok(views)
        })
        .await
        .map_err(MemoryError::Join)?
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

    /// Link two memories with a typed, directed relation (MCP-07):
    /// `from_id -[kind]-> to_id`.
    ///
    /// The self-link check runs FIRST, at this seam, before any store hop
    /// (D-02): `from_id == to_id` is [`MemoryError::InvalidArgument`] even if
    /// the id does not exist. The kind is already a closed [`LinkKind`] — the
    /// transports parse the wire string with [`LinkKind::try_from`] (D-01).
    ///
    /// A missing endpoint is [`LinkOutcome::NotFound`] naming the id (clean
    /// not-found data, never an error); linking an edge that already exists is
    /// an idempotent [`LinkOutcome::Linked`] (D-02). The edge is stamped with
    /// the injected clock, which orders 1-hop expansion (D-07).
    pub async fn link(
        &self,
        from_id: i64,
        to_id: i64,
        kind: LinkKind,
    ) -> Result<LinkOutcome, MemoryError> {
        crate::domain::validate_link_endpoints(from_id, to_id)?;
        let now = self.clock.now();
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.link(from_id, to_id, kind, now))
            .await
            .map_err(MemoryError::Join)?
    }

    /// Remove the directed edge `from_id -[kind]-> to_id` (MCP-07). Returns
    /// `true` if it was removed, `false` if no such edge existed — the clean
    /// not-found of [`MemoryService::forget`], never an error (D-02). A
    /// self-pair simply never matches an edge, so it is a not-found too.
    pub async fn unlink(
        &self,
        from_id: i64,
        to_id: i64,
        kind: LinkKind,
    ) -> Result<bool, MemoryError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.unlink(from_id, to_id, kind))
            .await
            .map_err(MemoryError::Join)?
    }

    /// Patch a memory in place (MCP-06), returning the full post-update view
    /// plus `embedding_status` (D-10), or `Ok(None)` for an unknown id (clean
    /// not-found, never an error — the [`MemoryService::forget`] contract).
    ///
    /// Order mirrors [`MemoryService::store`]: validate the patch FIRST (so a
    /// bad patch never costs an embed round-trip — D-06 seam), stamp the clock,
    /// then embed the new content best-effort ONLY when the patch carries
    /// content (tags/scope/TTL-only patches never touch the embedder). An embed
    /// failure never fails the update: the row lands pending
    /// (`embedding_status = 0`) and the sweep backfills it (SEARCH-03). The
    /// store applies the patch, the vector swap and the freshness bump (D-09)
    /// in one writer transaction.
    pub async fn update(
        &self,
        id: i64,
        patch: MemoryPatch,
    ) -> Result<Option<UpdatedMemory>, MemoryError> {
        crate::domain::validate_patch(&patch)?;
        let now = self.clock.now();
        let embedding = match &patch.content {
            Some(Some(content)) => {
                let inputs = [content.clone()];
                match self.embedder.embed(&inputs).await {
                    Ok(mut vectors) if !vectors.is_empty() => Some(vectors.swap_remove(0)),
                    Ok(_) => {
                        tracing::warn!(
                            "embedding skipped (empty batch response); updating with embedding_status = 0"
                        );
                        None
                    }
                    Err(e) => {
                        tracing::warn!(
                            "embedding failed ({e}); updating with embedding_status = 0"
                        );
                        None
                    }
                }
            }
            Some(None) | None => None,
        };
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.update(id, patch, embedding, now))
            .await
            .map_err(MemoryError::Join)?
    }

    /// Run one lifecycle sweep at `now` (STORE-04): delete TTL-expired rows, then
    /// materialize every survivor's `decay_score` so ranking stays cheap between
    /// reads. Decay never deletes — only TTL expiry (here) and `forget` remove.
    ///
    /// After the decay pass, pending embeddings (`embedding_status = 0`) are
    /// backfilled best-effort in ONE batch embed call (RESEARCH Open Question 1)
    /// — this completes the "install Ollama later and old memories become
    /// semantically searchable" story. A backfill never overwrites a row that
    /// changed or vanished while it was being embedded: each vector is written
    /// only if the row is still pending with exactly the embedded content
    /// ([`Store::insert_embedding`], REVIEW CR-03). A row edited mid-embed is
    /// either already embedded by `update` itself, or still pending, in which
    /// case its CURRENT content is embedded on a later sweep. Backfill counts
    /// (`backfilled`, `skipped_stale`) are tracing-only:
    /// [`SweepReport`]'s shape is unchanged, so the TTL contract stays intact.
    ///
    /// Driven by the injected clock so it is deterministic under test; exposed for
    /// both the background interval task and `tests/ttl.rs`. The blocking
    /// `DecayEngine::sweep` runs on a `spawn_blocking` thread (Pitfall 2); the
    /// embed call is async, OUTSIDE `spawn_blocking` (Pitfall 5).
    pub async fn sweep(&self, now: i64) -> Result<SweepReport, MemoryError> {
        let store = self.store.clone();
        let engine = DecayEngine::new(self.decay_cfg);
        let report = tokio::task::spawn_blocking(move || engine.sweep(store.as_ref(), now))
            .await
            .map_err(MemoryError::Join)??;

        // Embedding backfill: fetch a bounded batch of pending rows, embed them
        // in one call, and write the vectors back. An embed failure logs one
        // warning and skips — the next tick retries (SEARCH-03: never fatal).
        let store = self.store.clone();
        let pending = tokio::task::spawn_blocking(move || store.pending_embeddings(BACKFILL_BATCH))
            .await
            .map_err(MemoryError::Join)??;
        if !pending.is_empty() {
            let contents: Vec<String> =
                pending.iter().map(|(_, content)| content.clone()).collect();
            match self.embedder.embed(&contents).await {
                Ok(vectors) if vectors.len() == pending.len() => {
                    let store = self.store.clone();
                    let (written, skipped) = tokio::task::spawn_blocking(
                        move || -> Result<(usize, usize), MemoryError> {
                            let mut written = 0usize;
                            let mut skipped = 0usize;
                            // Each vector is written only if its row is still
                            // pending with exactly the content embedded above
                            // (CR-03). A row that changed or vanished while the
                            // embed was in flight is skipped, never overwritten.
                            for ((id, content), vector) in pending.into_iter().zip(vectors) {
                                if store.insert_embedding(id, &content, vector)? {
                                    written += 1;
                                } else {
                                    skipped += 1;
                                }
                            }
                            Ok((written, skipped))
                        },
                    )
                    .await
                    .map_err(MemoryError::Join)??;
                    tracing::info!(
                        backfilled = written,
                        skipped_stale = skipped,
                        "embedded pending memories on sweep"
                    );
                }
                Ok(_) => tracing::warn!(
                    "embedding backfill skipped (batch size mismatch); retrying next sweep"
                ),
                Err(e) => {
                    tracing::warn!("embedding backfill skipped ({e}); retrying next sweep");
                }
            }
        }

        Ok(report)
    }
}

/// How many pending rows one sweep tick backfills (bounded so a huge corpus
/// embeds incrementally instead of one giant request — T-02-03).
const BACKFILL_BATCH: usize = 128;
