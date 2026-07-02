//! Integration tests for the SQLite store + MemoryService (STORE-01/STORE-02).
//!
//! These use a REAL on-disk temp-file DB (never `:memory:`) so the restart-
//! durability behavior is genuinely exercised: store → drop → reopen the same path.
//!
//! Offline note: since Phase 2 the core carries `reqwest` for the Ollama
//! embedder, but these tests inject a deterministic `FakeEmbedder`, so the
//! store/list path under test never makes a network call.

use std::convert::TryFrom;
use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{MemoryType, NewMemory};
use agent_memory_core::embed::FakeEmbedder;
use agent_memory_core::service::{ListArgs, MemoryService};
use agent_memory_core::store::sqlite::SqliteStore;

const ALL_TYPES: [&str; 6] = [
    "DECISION",
    "PATTERN",
    "ERROR",
    "TODO",
    "ARCHITECTURE",
    "CONSTRAINT",
];

fn new_memory(mem_type: MemoryType, content: &str) -> NewMemory {
    NewMemory {
        content: content.to_string(),
        mem_type,
        tags: vec![],
        source: None,
        scope: None,
        ttl_secs: None,
    }
}

fn service_for(path: &std::path::Path, clock: Arc<dyn Clock>) -> MemoryService {
    let store = SqliteStore::open(path).expect("open store");
    // Succeeding hash-vector FakeEmbedder: store/list behavior under test is
    // embedding-agnostic; deterministic vectors keep the semantic path harmless.
    let embedder = Arc::new(FakeEmbedder::with_vectors(std::collections::HashMap::new()));
    MemoryService::new(Arc::new(store), clock, embedder, DecayConfig::default())
}

#[tokio::test]
async fn six_typed_rows_survive_store_drop_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));

    // Phase 1: store one of each type, then drop the store (end of scope).
    {
        let service = service_for(&db_path, clock.clone());
        for (i, ty) in ALL_TYPES.iter().enumerate() {
            let mem_type = MemoryType::try_from(*ty).expect("valid type");
            clock.advance(1); // distinct created_at per row for deterministic ordering
            let id = service
                .store(new_memory(mem_type, &format!("memory {i} of type {ty}")))
                .await
                .expect("store should succeed");
            assert!(id > 0, "store should return a positive id");
        }
    } // service (and its SqliteStore) dropped here

    // Phase 2: reopen the SAME path; all six rows must still be there.
    let service = service_for(&db_path, clock.clone());
    let rows = service
        .list(ListArgs::default())
        .await
        .expect("list should succeed");
    assert_eq!(rows.len(), 6, "all 6 typed rows must survive a reopen");
}

#[tokio::test]
async fn invalid_type_never_reaches_sql() {
    // The service only accepts a typed MemoryType, so an invalid wire string is
    // rejected by TryFrom before any SQL runs — a clean Err, never a panic.
    let err = MemoryType::try_from("NOT_A_TYPE").expect_err("should reject unknown type");
    match err {
        agent_memory_core::domain::MemoryError::InvalidType(got) => assert_eq!(got, "NOT_A_TYPE"),
        other => panic!("expected InvalidType, got {other:?}"),
    }
}

#[tokio::test]
async fn omitted_scope_persists_null_and_filters_apply() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(2_000));
    let service = service_for(&db_path, clock.clone());

    // Row with NO scope (NULL).
    clock.advance(1);
    service
        .store(new_memory(MemoryType::Todo, "global todo"))
        .await
        .expect("store global");

    // Two rows scoped to "projectA".
    for content in ["scoped one", "scoped two"] {
        clock.advance(1);
        service
            .store(NewMemory {
                scope: Some("projectA".to_string()),
                ..new_memory(MemoryType::Decision, content)
            })
            .await
            .expect("store scoped");
    }

    // Filtering by scope returns only the two scoped rows.
    let scoped = service
        .list(ListArgs {
            scope: Some("projectA".to_string()),
            ..ListArgs::default()
        })
        .await
        .expect("list scoped");
    assert_eq!(
        scoped.len(),
        2,
        "scope filter should match exactly two rows"
    );
    assert!(scoped
        .iter()
        .all(|m| m.scope.as_deref() == Some("projectA")));

    // The global row has a NULL scope.
    let all = service.list(ListArgs::default()).await.expect("list all");
    assert_eq!(all.len(), 3);
    assert!(
        all.iter().any(|m| m.scope.is_none()),
        "the global row must persist a NULL scope"
    );
}

#[tokio::test]
async fn limit_caps_count_and_order_is_newest_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(3_000));
    let service = service_for(&db_path, clock.clone());

    for i in 0..5 {
        clock.advance(10);
        service
            .store(new_memory(MemoryType::Pattern, &format!("row {i}")))
            .await
            .expect("store");
    }

    // Newest-first: the last inserted ("row 4") must come first.
    let all = service.list(ListArgs::default()).await.expect("list all");
    assert_eq!(all.len(), 5);
    assert_eq!(all.first().expect("first row").content, "row 4");
    assert_eq!(all.last().expect("last row").content, "row 0");

    // Limit caps the count.
    let limited = service
        .list(ListArgs {
            limit: Some(2),
            ..ListArgs::default()
        })
        .await
        .expect("list limited");
    assert_eq!(limited.len(), 2, "limit should cap returned rows");
    assert_eq!(limited[0].content, "row 4");
    assert_eq!(limited[1].content, "row 3");
}
