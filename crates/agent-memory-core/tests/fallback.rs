//! SEARCH-03 kill-test (RESEARCH Pitfall 2): with the embedder DEAD, store still
//! succeeds and search returns keyword/FTS5 results with `search_mode: keyword`
//! — never an error and never empty-when-FTS-matches.
//!
//! `FakeEmbedder::failing()` is the deterministic stand-in for "Ollama is down";
//! the degrade decision lives at the `Embedder` seam in the service, so these
//! tests prove the product behavior on a machine with no Ollama at all.

use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{MemoryError, MemoryType, NewMemory};
use agent_memory_core::embed::{Embedder, FakeEmbedder};
use agent_memory_core::service::{ListArgs, MemoryService, SearchArgs, SearchMode};
use agent_memory_core::store::sqlite::SqliteStore;

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

fn service_for(
    path: &std::path::Path,
    clock: Arc<dyn Clock>,
    embedder: Arc<dyn Embedder>,
) -> MemoryService {
    let store = SqliteStore::open(path).expect("open store");
    MemoryService::new(Arc::new(store), clock, embedder, DecayConfig::default())
}

#[tokio::test]
async fn store_succeeds_with_failing_embedder_and_row_is_listable() {
    // memory_store NEVER fails because Ollama is down: the row lands with
    // embedding_status = 0 and stays fully usable (SEARCH-03).
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    let service = service_for(&db_path, clock.clone(), Arc::new(FakeEmbedder::failing()));

    let id = service
        .store(new_memory(
            "the database connection pool configuration",
            MemoryType::Pattern,
        ))
        .await
        .expect("store MUST succeed while the embedder is down");
    assert!(id > 0);

    let listed = service.list(ListArgs::default()).await.expect("list");
    assert!(
        listed.iter().any(|v| v.id == id),
        "the stored row must be listable despite the embed outage"
    );

    // The row landed pending: embedding_status = 0, no fake/zero vector inserted.
    agent_memory_core::store::sqlite::register_vec_extension().expect("register vec0");
    let conn = rusqlite::Connection::open(&db_path).expect("open db for assertion");
    let status: i64 = conn
        .query_row(
            "SELECT embedding_status FROM memories WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .expect("row exists");
    assert_eq!(status, 0, "embed outage must leave embedding_status = 0");
    let vec_rows: i64 = conn
        .query_row("SELECT count(*) FROM vec_memories", [], |row| row.get(0))
        .expect("count vec rows");
    assert_eq!(
        vec_rows, 0,
        "no vector row may be inserted on embed failure"
    );
}

#[tokio::test]
async fn keyword_fallback_honors_tag_filter() {
    // Gap 1 / CR-01 regression: the degraded keyword mode must honor the SAME
    // tag filter the semantic path does — a tag-filtered search with a dead
    // embedder may never return the unfiltered superset (SEARCH-03 / SC2).
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    let service = service_for(&db_path, clock.clone(), Arc::new(FakeEmbedder::failing()));

    let mut alpha = new_memory("the deploy pipeline for staging", MemoryType::Pattern);
    alpha.tags = vec!["alpha".to_string()];
    let alpha_id = service
        .store(alpha)
        .await
        .expect("store alpha row while embedder is down");

    let mut beta = new_memory("the release pipeline for production", MemoryType::Pattern);
    beta.tags = vec!["beta".to_string()];
    service
        .store(beta)
        .await
        .expect("store beta row while embedder is down");

    let outcome = service
        .search(SearchArgs {
            query: "pipeline".to_string(),
            tag: Some("alpha".to_string()),
            ..Default::default()
        })
        .await
        .expect("tag-filtered search MUST NOT error when the embedder is down");

    assert_eq!(
        outcome.search_mode,
        SearchMode::Keyword,
        "a dead embedder must surface search_mode 'keyword'"
    );
    assert_eq!(
        outcome.results.len(),
        1,
        "the tag filter must exclude the beta-tagged row in keyword mode, got: {:?}",
        outcome.results
    );
    assert_eq!(
        outcome.results[0].id, alpha_id,
        "the single result must be the alpha-tagged row"
    );
}

#[tokio::test]
async fn keyword_fallback_tag_filter_is_exact() {
    // API-03 / D-07..D-09 regression, keyword-search site: extends the 02-05
    // keyword_fallback_honors_tag_filter shape with the substring/case locks —
    // tag=rust must not match "rustling", and "Rust" must not match "rust".
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    let service = service_for(&db_path, clock.clone(), Arc::new(FakeEmbedder::failing()));

    let mut rust_row = new_memory("the deploy pipeline for staging", MemoryType::Pattern);
    rust_row.tags = vec!["rust".to_string()];
    let rust_id = service
        .store(rust_row)
        .await
        .expect("store rust row while embedder is down");

    let mut rustling_row = new_memory("the release pipeline for production", MemoryType::Pattern);
    rustling_row.tags = vec!["rustling".to_string()];
    service
        .store(rustling_row)
        .await
        .expect("store rustling row while embedder is down");

    let search = |tag: Option<&str>| SearchArgs {
        query: "pipeline".to_string(),
        tag: tag.map(str::to_string),
        ..Default::default()
    };

    let outcome = service
        .search(search(Some("rust")))
        .await
        .expect("tag-filtered keyword search");
    assert_eq!(outcome.search_mode, SearchMode::Keyword);
    assert_eq!(
        outcome.results.len(),
        1,
        "tag=rust must match ONLY the exactly-rust-tagged row in keyword mode, got: {:?}",
        outcome.results
    );
    assert_eq!(outcome.results[0].id, rust_id);

    let outcome = service
        .search(search(Some("Rust")))
        .await
        .expect("case-mismatched tag keyword search");
    assert!(
        outcome.results.is_empty(),
        "tag matching must be case-sensitive: 'Rust' must not match 'rust' (D-08), got: {:?}",
        outcome.results
    );

    let outcome = service
        .search(search(None))
        .await
        .expect("unfiltered keyword search");
    assert_eq!(
        outcome.results.len(),
        2,
        "tag=None must disable the filter and return both rows"
    );
}

#[tokio::test]
async fn malformed_fts5_query_maps_to_invalid_query_in_keyword_mode() {
    // Gap 2 / WR-05 regression: a query FTS5 cannot parse (a lone double-quote)
    // is CLIENT input, not an internal failure — the store seam must surface it
    // as MemoryError::InvalidQuery, never MemoryError::Sqlite (API-01).
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    let service = service_for(&db_path, clock.clone(), Arc::new(FakeEmbedder::failing()));

    service
        .store(new_memory(
            "any row so the table is non-empty",
            MemoryType::Pattern,
        ))
        .await
        .expect("store succeeds while embedder is down");

    let err = service
        .search(SearchArgs {
            query: "\"".to_string(),
            ..Default::default()
        })
        .await
        .expect_err("a lone double-quote is not a valid FTS5 match expression");

    assert!(
        matches!(err, MemoryError::InvalidQuery(_)),
        "malformed FTS5 input must map to InvalidQuery, got: {err:?}"
    );
}

#[tokio::test]
async fn search_with_failing_embedder_returns_keyword_results_never_error_or_empty() {
    // The Pitfall 2 kill-test: a dead embedder must route to the FTS5 path and
    // return the keyword matches — NOT an error, NOT an empty list.
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    let service = service_for(&db_path, clock.clone(), Arc::new(FakeEmbedder::failing()));

    let id = service
        .store(new_memory(
            "use sqlite for the embedded database",
            MemoryType::Decision,
        ))
        .await
        .expect("store succeeds while embedder is down");

    let outcome = service
        .search(SearchArgs {
            query: "database".to_string(),
            ..Default::default()
        })
        .await
        .expect("search MUST NOT error when the embedder is down");

    assert_eq!(
        outcome.search_mode,
        SearchMode::Keyword,
        "a dead embedder must surface search_mode 'keyword'"
    );
    assert!(
        !outcome.results.is_empty(),
        "FTS5 matches must be returned — degraded search may never be empty-because-of-outage"
    );
    assert!(
        outcome.results.iter().any(|v| v.id == id),
        "the keyword match must be the stored row"
    );
}
