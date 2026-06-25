//! STORE-03 / MCP-02 / SEARCH-01: deterministic decay curve, per-type pinning,
//! and the bm25×decay search ranking surfaced through `MemoryService::search`.
//!
//! All time is driven by an injected `TestClock` (no `SystemTime`), so the decay
//! curve and the recency-bump ranking are fully deterministic.

use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::{decay_score, DecayConfig, DEFAULT_HALF_LIFE_SECS};
use agent_memory_core::domain::{MemoryType, NewMemory};
use agent_memory_core::service::{MemoryService, SearchArgs};
use agent_memory_core::store::sqlite::SqliteStore;

fn service_on_temp_db(clock: Arc<dyn Clock>) -> (MemoryService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let store = SqliteStore::open(&db_path).expect("open store");
    let service = MemoryService::new(Arc::new(store), clock, DecayConfig::default());
    (service, dir)
}

fn new_memory(content: &str, mem_type: MemoryType) -> NewMemory {
    NewMemory {
        content: content.to_string(),
        mem_type,
        tags: vec![],
        source: None,
        scope: None,
        ttl_secs: None,
    }
}

#[test]
fn decay_score_is_strictly_decreasing_and_half_at_one_half_life() {
    let hl = DEFAULT_HALF_LIFE_SECS;
    // Strictly decreasing in elapsed time (deterministic, explicit timestamps).
    let s0 = decay_score(0, 0, hl, false);
    let s1 = decay_score(hl as i64 / 4, 0, hl, false);
    let s2 = decay_score(hl as i64 / 2, 0, hl, false);
    let s3 = decay_score(hl as i64, 0, hl, false);
    assert!(
        s0 > s1 && s1 > s2 && s2 > s3,
        "decay must strictly decrease"
    );
    // ~0.5 at exactly one half-life.
    assert!(
        (s3 - 0.5).abs() < 1e-6,
        "score halves at one half-life, got {s3}"
    );
}

#[test]
fn pinned_type_has_higher_decay_score_than_unpinned_at_equal_age() {
    let hl = DEFAULT_HALF_LIFE_SECS;
    let age = hl as i64; // one (unpinned) half-life of elapsed time
    let pinned = decay_score(age, 0, hl, true);
    let unpinned = decay_score(age, 0, hl, false);
    assert!(
        pinned > unpinned,
        "pinned ({pinned}) must out-rank unpinned ({unpinned}) at equal age (D-08)"
    );
}

#[tokio::test]
async fn just_accessed_ranks_above_un_accessed_with_surfaced_decay() {
    // Both memories share the same single matching keyword so bm25 relevance is
    // comparable; ranking is then decided by the decay score.
    let clock = Arc::new(TestClock::new(1_000));
    let (service, _dir) = service_on_temp_db(clock.clone());

    let id_old = service
        .store(new_memory("alpha widget one", MemoryType::Pattern))
        .await
        .expect("store old");
    let id_new = service
        .store(new_memory("alpha widget two", MemoryType::Pattern))
        .await
        .expect("store new");
    assert!(id_old > 0 && id_new > 0);

    // Advance well past a half-life so both have decayed from their stored time.
    clock.advance(DEFAULT_HALF_LIFE_SECS as i64);

    // Touch only the "new" one via a search that matches just it (recency bump).
    let bump = service
        .search(SearchArgs {
            query: "two".to_string(),
            ..Default::default()
        })
        .await
        .expect("bump search");
    assert_eq!(bump.len(), 1, "only the 'two' memory matches");
    assert_eq!(bump[0].id, id_new);

    // The recency bump is fire-and-forget on a blocking thread; give it a moment.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Now both match "alpha"; the just-accessed one must rank first and its
    // surfaced decay_score must reflect the recomputed on-read value (STORE-03).
    let results = service
        .search(SearchArgs {
            query: "alpha".to_string(),
            ..Default::default()
        })
        .await
        .expect("ranked search");
    assert_eq!(results.len(), 2, "both memories match 'alpha'");
    assert_eq!(
        results[0].id, id_new,
        "the just-accessed memory must rank above the un-accessed one"
    );
    assert!(
        results[0].decay_score > results[1].decay_score,
        "just-accessed decay_score ({}) must exceed un-accessed ({})",
        results[0].decay_score,
        results[1].decay_score
    );
    // The surfaced score is the recomputed on-read value: the just-accessed one
    // was touched at now, so its score is ~1.0.
    assert!(
        results[0].decay_score > 0.99,
        "just-accessed surfaced decay_score should be ~1.0, got {}",
        results[0].decay_score
    );
}

#[tokio::test]
async fn no_match_returns_empty_not_error() {
    let clock = Arc::new(TestClock::new(1_000));
    let (service, _dir) = service_on_temp_db(clock);

    service
        .store(new_memory("the quick brown fox", MemoryType::Todo))
        .await
        .expect("store");

    let results = service
        .search(SearchArgs {
            query: "nonexistentkeyword".to_string(),
            ..Default::default()
        })
        .await
        .expect("no-match search must be Ok, not Err");
    assert!(results.is_empty(), "no match must return an empty list");
}
