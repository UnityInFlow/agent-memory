//! Exponential decay scoring.
//!
//! Decay only ever *down-ranks* a memory — it never deletes (deletion is TTL or
//! `memory_forget` exclusively, STORE-04). Pinned types (D-08) use a longer
//! half-life so high-value memories fade much more slowly.
//!
//! Phase 1 lands the pure scoring function and its config (Plan 01), plus the
//! on-read decay surfacing and ranking weights used by `memory_search` (Plan 02).
//! The background sweep that materialises `decay_score` is Plan 03.

use crate::domain::{MemoryError, MemoryView};
use crate::store::Store;

/// How long, in seconds, until a non-pinned memory's score halves (D-09).
/// Default: 30 days.
pub const DEFAULT_HALF_LIFE_SECS: f64 = 2_592_000.0;

/// Pinned types decay this many times slower than the configured half-life (D-08).
/// Tunable; the contract is only "pinned fades much more slowly than unpinned".
pub const PINNED_HALF_LIFE_MULTIPLIER: f64 = 6.0;

/// Decay configuration. Defaults are good out of the box (D-09) and overridable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecayConfig {
    /// Half-life in seconds for non-pinned memories.
    pub half_life_secs: f64,
}

impl Default for DecayConfig {
    fn default() -> Self {
        DecayConfig {
            half_life_secs: DEFAULT_HALF_LIFE_SECS,
        }
    }
}

/// Compute a decay score in `(0, 1]` from elapsed time since last access.
///
/// The score halves every `half_life_secs` (longer for pinned types). A
/// just-accessed memory scores ~1.0; an old one trends toward 0 but never reaches
/// it — decay re-ranks, it does not delete.
pub fn decay_score(now: i64, last_accessed: i64, half_life_secs: f64, pinned: bool) -> f64 {
    let elapsed = (now - last_accessed).max(0) as f64;
    let half_life = if pinned {
        half_life_secs * PINNED_HALF_LIFE_MULTIPLIER
    } else {
        half_life_secs
    };
    // exp(-ln2 * elapsed / half_life): score halves every half_life.
    (-std::f64::consts::LN_2 * elapsed / half_life).exp()
}

/// Default weight given to FTS5/bm25 relevance when blending with decay (D-04).
pub const DEFAULT_RELEVANCE_WEIGHT: f64 = 1.0;
/// Default weight given to the decay score when blending with relevance (D-04).
pub const DEFAULT_DECAY_WEIGHT: f64 = 1.0;

/// Weights for the `memory_search` ranking blend (D-04).
///
/// The blended score is `(-bm25) * relevance + decay_score * decay` — bm25 is
/// negated (smaller = better) so both terms grow with a *better* result. The
/// defaults are good out of the box and overridable by configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankWeights {
    /// Weight applied to the (negated) bm25 relevance term.
    pub relevance: f64,
    /// Weight applied to the decay-score term.
    pub decay: f64,
}

impl Default for RankWeights {
    fn default() -> Self {
        RankWeights {
            relevance: DEFAULT_RELEVANCE_WEIGHT,
            decay: DEFAULT_DECAY_WEIGHT,
        }
    }
}

/// Recompute a [`MemoryView`]'s `decay_score` on read, from its `last_accessed`
/// timestamp and the configured half-life, honoring per-type pinning (D-08).
///
/// This overwrites whatever materialised value the row carried so search results
/// always surface a correct, up-to-the-moment decay score (STORE-03) even between
/// background sweeps.
pub fn apply_decay(view: &mut MemoryView, now: i64, cfg: &DecayConfig) {
    let pinned = view.mem_type.is_pinned();
    view.decay_score = decay_score(now, view.last_accessed, cfg.half_life_secs, pinned);
}

/// Outcome of a single [`DecayEngine::sweep`] pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SweepReport {
    /// Rows removed because their TTL had expired (`expires_at < now`).
    pub expired: usize,
    /// Surviving rows whose materialized `decay_score` column was re-computed.
    pub rescored: usize,
}

/// The background lifecycle engine: TTL expiry followed by decay materialization.
///
/// `sweep` runs in a fixed, deliberate order — **TTL delete FIRST, then re-score
/// the survivors**. The two phases are distinct [`Store`] calls: [`Store::sweep_expired`]
/// is the only one that deletes, and [`Store::materialize_decay`] is UPDATE-only.
/// This structure is what enforces the STORE-04 invariant (RESEARCH Pitfall 7):
/// decay materialization physically cannot remove a row, so a near-zero-decay
/// memory without a TTL is never deleted — only TTL and `memory_forget` remove.
#[derive(Debug, Clone, Copy)]
pub struct DecayEngine {
    cfg: DecayConfig,
}

impl DecayEngine {
    /// Construct an engine with the given decay configuration.
    pub fn new(cfg: DecayConfig) -> Self {
        DecayEngine { cfg }
    }

    /// Run one sweep against `store` at `now`: delete TTL-expired rows, then
    /// materialize the decay score of every survivor. Returns counts for logging.
    pub fn sweep(&self, store: &dyn Store, now: i64) -> Result<SweepReport, MemoryError> {
        // TTL delete FIRST so we never waste work re-scoring rows about to vanish,
        // and so the survivor set is final before materialization.
        let expired = store.sweep_expired(now)?;
        let rescored = store.materialize_decay(now, &self.cfg)?;
        Ok(SweepReport { expired, rescored })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshly_accessed_scores_near_one() {
        let score = decay_score(1_000, 1_000, DEFAULT_HALF_LIFE_SECS, false);
        assert!((score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn halves_after_one_half_life() {
        let hl = DEFAULT_HALF_LIFE_SECS;
        let score = decay_score(hl as i64, 0, hl, false);
        assert!((score - 0.5).abs() < 1e-6);
    }

    #[test]
    fn pinned_decays_slower_than_unpinned_at_equal_age() {
        let hl = DEFAULT_HALF_LIFE_SECS;
        let age = hl as i64;
        let pinned = decay_score(age, 0, hl, true);
        let unpinned = decay_score(age, 0, hl, false);
        assert!(pinned > unpinned);
    }

    #[test]
    fn future_last_accessed_is_clamped_to_score_one() {
        // now < last_accessed should not produce a score above 1.0.
        let score = decay_score(0, 1_000, DEFAULT_HALF_LIFE_SECS, false);
        assert!((score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn default_config_is_thirty_days() {
        assert_eq!(DecayConfig::default().half_life_secs, 2_592_000.0);
    }
}
