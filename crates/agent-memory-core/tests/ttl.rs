//! STORE-04: the background sweep removes TTL-expired rows and materializes each
//! survivor's `decay_score`, while NEVER deleting on decay alone.
//!
//! Core invariant proven here (RESEARCH Anti-Pattern "Decay that deletes",
//! Pitfall 7): the ONLY removal paths are TTL expiry and `memory_forget`. A memory
//! whose decay score has fallen near zero but which carries no TTL stays fully
//! retrievable. All time is driven by an injected `TestClock` so the sweep is
//! deterministic.

use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::{decay_score, DecayConfig, DEFAULT_HALF_LIFE_SECS};
use agent_memory_core::domain::{MemoryType, NewMemory};
use agent_memory_core::embed::FakeEmbedder;
use agent_memory_core::service::{ListArgs, MemoryService, SearchArgs};
use agent_memory_core::store::sqlite::SqliteStore;

fn service_on_temp_db(clock: Arc<dyn Clock>) -> (MemoryService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let store = SqliteStore::open(&db_path).expect("open store");
    // Succeeding hash-vector FakeEmbedder: searches here run the SEMANTIC path,
    // which also exercises the vec_memories sync on sweep/forget (Pitfall 3).
    let embedder = Arc::new(FakeEmbedder::with_vectors(std::collections::HashMap::new()));
    let service = MemoryService::new(Arc::new(store), clock, embedder, DecayConfig::default());
    (service, dir)
}

fn new_memory(content: &str, mem_type: MemoryType, ttl_secs: Option<i64>) -> NewMemory {
    NewMemory {
        content: content.to_string(),
        mem_type,
        tags: vec![],
        source: None,
        scope: None,
        ttl_secs,
    }
}

#[tokio::test]
async fn sweep_removes_expired_rows_from_list_and_search() {
    let clock = Arc::new(TestClock::new(1_000));
    let (service, _dir) = service_on_temp_db(clock.clone());

    // A memory with a 10-second TTL: expires_at = 1_010.
    let expiring = service
        .store(new_memory(
            "ephemeral alpha note",
            MemoryType::Todo,
            Some(10),
        ))
        .await
        .expect("store expiring");
    // A memory with no TTL: must survive forever.
    let durable = service
        .store(new_memory("durable alpha note", MemoryType::Todo, None))
        .await
        .expect("store durable");
    assert!(expiring > 0 && durable > 0);

    // Move the clock past the TTL and sweep.
    clock.advance(100); // now = 1_100 > 1_010
    let report = service.sweep(clock.now()).await.expect("sweep");
    assert_eq!(report.expired, 1, "exactly one row was TTL-expired");

    // The expired row is gone from both list and search; the durable one remains.
    let listed = service.list(ListArgs::default()).await.expect("list");
    let ids: Vec<i64> = listed.iter().map(|v| v.id).collect();
    assert!(
        !ids.contains(&expiring),
        "expired row must be gone from list"
    );
    assert!(ids.contains(&durable), "durable row must remain in list");

    let found = service
        .search(SearchArgs {
            query: "alpha".to_string(),
            ..Default::default()
        })
        .await
        .expect("search");
    let found_ids: Vec<i64> = found.results.iter().map(|v| v.id).collect();
    assert!(
        !found_ids.contains(&expiring),
        "expired row must be gone from search (FTS trigger + explicit vec delete synced)"
    );
    assert!(
        found_ids.contains(&durable),
        "durable row must still be searchable"
    );
}

#[tokio::test]
async fn decay_never_deletes_low_score_no_ttl_memory_stays_retrievable() {
    // STORE-04 core invariant: a memory with NO TTL whose decay score has fallen
    // near zero is STILL retrievable after a sweep. Decay re-ranks; it never deletes.
    let clock = Arc::new(TestClock::new(1_000));
    let (service, _dir) = service_on_temp_db(clock.clone());

    let id = service
        .store(new_memory(
            "ancient unique keyword zephyr",
            MemoryType::Todo,
            None,
        ))
        .await
        .expect("store");

    // Advance the clock by 20 half-lives: decay_score is ~2^-20 ≈ 1e-6, near zero.
    clock.advance(DEFAULT_HALF_LIFE_SECS as i64 * 20);
    let report = service.sweep(clock.now()).await.expect("sweep");
    assert_eq!(report.expired, 0, "no TTL row was expired by the sweep");
    assert_eq!(report.rescored, 1, "the survivor was re-scored");

    // Still in the list.
    let listed = service.list(ListArgs::default()).await.expect("list");
    assert!(
        listed.iter().any(|v| v.id == id),
        "near-zero-decay no-TTL memory must STILL be in the list (decay != delete)"
    );

    // Still searchable.
    let found = service
        .search(SearchArgs {
            query: "zephyr".to_string(),
            ..Default::default()
        })
        .await
        .expect("search");
    assert!(
        found.results.iter().any(|v| v.id == id),
        "near-zero-decay no-TTL memory must STILL be searchable (decay != delete)"
    );
}

#[tokio::test]
async fn materialized_decay_matches_on_read_recompute_and_pinned_outranks() {
    let clock = Arc::new(TestClock::new(1_000));
    let (service, _dir) = service_on_temp_db(clock.clone());

    // Two memories of EQUAL age: one pinned (DECISION), one unpinned (TODO).
    let pinned = service
        .store(new_memory(
            "pinned survivor row",
            MemoryType::Decision,
            None,
        ))
        .await
        .expect("store pinned");
    let unpinned = service
        .store(new_memory("unpinned survivor row", MemoryType::Todo, None))
        .await
        .expect("store unpinned");

    // Age both by one (unpinned) half-life, then sweep to materialize decay_score.
    let now = clock.now() + DEFAULT_HALF_LIFE_SECS as i64;
    clock.set(now);
    let report = service.sweep(now).await.expect("sweep");
    assert_eq!(report.rescored, 2, "both survivors were re-scored");

    // Read the materialized decay_score straight from the store (list surfaces it).
    let listed = service.list(ListArgs::default()).await.expect("list");
    let mat = |id: i64| -> f64 {
        listed
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.decay_score)
            .expect("row present")
    };
    let cfg = DecayConfig::default();

    // The materialized value matches the pure on-read recompute for the same `now`.
    let expected_pinned = decay_score(now, 1_000, cfg.half_life_secs, true);
    let expected_unpinned = decay_score(now, 1_000, cfg.half_life_secs, false);
    assert!(
        (mat(pinned) - expected_pinned).abs() < 1e-9,
        "materialized pinned decay {} must match recompute {}",
        mat(pinned),
        expected_pinned
    );
    assert!(
        (mat(unpinned) - expected_unpinned).abs() < 1e-9,
        "materialized unpinned decay {} must match recompute {}",
        mat(unpinned),
        expected_unpinned
    );

    // A pinned survivor materializes a HIGHER score than an unpinned one of equal age.
    assert!(
        mat(pinned) > mat(unpinned),
        "pinned materialized decay ({}) must exceed unpinned ({}) at equal age (D-08)",
        mat(pinned),
        mat(unpinned)
    );
}

#[tokio::test]
async fn sweep_never_touches_null_expires_rows() {
    let clock = Arc::new(TestClock::new(1_000));
    let (service, _dir) = service_on_temp_db(clock.clone());

    // Three no-TTL rows + one expiring row.
    for i in 0..3 {
        service
            .store(new_memory(
                &format!("persistent row {i}"),
                MemoryType::Pattern,
                None,
            ))
            .await
            .expect("store persistent");
    }
    service
        .store(new_memory("doomed row", MemoryType::Pattern, Some(5)))
        .await
        .expect("store doomed");

    clock.advance(1_000); // well past the 5s TTL
    let report = service.sweep(clock.now()).await.expect("sweep");

    // Only the one expires_at<now row is deleted; the three NULL-expires rows remain.
    assert_eq!(report.expired, 1, "only the TTL row is deleted");
    let listed = service.list(ListArgs::default()).await.expect("list");
    assert_eq!(
        listed.len(),
        3,
        "all three NULL-expires rows must be untouched by the sweep"
    );
}
