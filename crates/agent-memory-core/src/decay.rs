//! Exponential decay scoring.
//!
//! Decay only ever *down-ranks* a memory — it never deletes (deletion is TTL or
//! `memory_forget` exclusively, STORE-04). Pinned types (D-08) use a longer
//! half-life so high-value memories fade much more slowly.
//!
//! Phase 1 lands only the pure scoring function and its config. The sweep engine
//! that materialises `decay_score` and the search-time surfacing are Plans 02/03.

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
