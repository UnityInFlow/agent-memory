//! SEARCH-02 integration proof: recall by MEANING, not keywords.
//!
//! Deterministic golden set via `FakeEmbedder` programmed vectors (the CI gate —
//! runners may lack Ollama, RESEARCH Open Question 4), plus one `#[ignore]`d
//! live-Ollama test for local/manual verification. Uses a REAL on-disk temp DB
//! (never `:memory:`) and an injected `TestClock`, copying the tests/store.rs
//! harness style.

use std::collections::HashMap;
use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::{DecayConfig, DEFAULT_HALF_LIFE_SECS};
use agent_memory_core::domain::{MemoryType, NewMemory};
use agent_memory_core::embed::ollama::OllamaClient;
use agent_memory_core::embed::{Embedder, FakeEmbedder, EMBEDDING_DIM};
use agent_memory_core::service::{MemoryService, SearchArgs, SearchMode};
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

/// A 768-dim unit vector along the given axis.
fn unit_axis(axis: usize) -> Vec<f32> {
    let mut v = vec![0.0_f32; EMBEDDING_DIM];
    v[axis] = 1.0;
    v
}

/// A unit vector VERY close to axis 0 but not identical (near-identical cosine
/// similarity to `unit_axis(0)`; ~orthogonal to every other axis vector).
fn near_axis0() -> Vec<f32> {
    let mut v = vec![0.0_f32; EMBEDDING_DIM];
    v[0] = 0.999;
    v[1] = (1.0_f32 - 0.999 * 0.999).sqrt();
    v
}

const RLS_CONTENT: &str = "Postgres row-level security policies for the tenants table";
const RLS_QUERY: &str = "database access control rules";

#[tokio::test]
async fn semantic_search_finds_meaning_with_no_shared_keywords() {
    // SEARCH-02 success criterion 1: the query shares NO exact keyword with the
    // content, yet the semantically-programmed memory ranks first.
    let mut vectors = HashMap::new();
    vectors.insert(RLS_CONTENT.to_string(), unit_axis(0));
    vectors.insert(RLS_QUERY.to_string(), near_axis0());
    vectors.insert(
        "weekly standup happens every monday morning".to_string(),
        unit_axis(5),
    );
    vectors.insert(
        "the frontend bundle uses tailwind for styling".to_string(),
        unit_axis(9),
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    let embedder = Arc::new(FakeEmbedder::with_vectors(vectors));
    let service = service_for(&db_path, clock.clone(), embedder);

    let rls_id = service
        .store(new_memory(RLS_CONTENT, MemoryType::Decision))
        .await
        .expect("store rls memory");
    service
        .store(new_memory(
            "weekly standup happens every monday morning",
            MemoryType::Pattern,
        ))
        .await
        .expect("store unrelated 1");
    service
        .store(new_memory(
            "the frontend bundle uses tailwind for styling",
            MemoryType::Pattern,
        ))
        .await
        .expect("store unrelated 2");

    let outcome = service
        .search(SearchArgs {
            query: RLS_QUERY.to_string(),
            ..Default::default()
        })
        .await
        .expect("semantic search");

    assert_eq!(
        outcome.search_mode,
        SearchMode::Semantic,
        "with a working embedder the search mode must be semantic"
    );
    assert!(
        !outcome.results.is_empty(),
        "semantic search must find the programmed-similar memory"
    );
    assert_eq!(
        outcome.results[0].id, rls_id,
        "the RLS memory must rank FIRST despite zero keyword overlap with the query"
    );
}

#[tokio::test]
async fn at_equal_vector_distance_recent_memory_outranks_aged() {
    // Blend respects decay: both memories embed to the SAME vector as the query
    // (equal distance), so ranking is decided purely by the decay term.
    let shared = unit_axis(0);
    let mut vectors = HashMap::new();
    vectors.insert(
        "old note about the caching layer".to_string(),
        shared.clone(),
    );
    vectors.insert(
        "new note about the caching layer".to_string(),
        shared.clone(),
    );
    vectors.insert("caching approach".to_string(), shared);

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    let embedder = Arc::new(FakeEmbedder::with_vectors(vectors));
    let service = service_for(&db_path, clock.clone(), embedder);

    let old_id = service
        .store(new_memory(
            "old note about the caching layer",
            MemoryType::Pattern,
        ))
        .await
        .expect("store old");

    // Age the first memory far past its half-life, then store the second (its
    // last_accessed is stamped `now`, so it is the recently-accessed one).
    clock.advance(DEFAULT_HALF_LIFE_SECS as i64 * 10);
    let new_id = service
        .store(new_memory(
            "new note about the caching layer",
            MemoryType::Pattern,
        ))
        .await
        .expect("store new");

    let outcome = service
        .search(SearchArgs {
            query: "caching approach".to_string(),
            ..Default::default()
        })
        .await
        .expect("semantic search");

    assert_eq!(outcome.search_mode, SearchMode::Semantic);
    assert_eq!(outcome.results.len(), 2, "both memories are candidates");
    assert_eq!(
        outcome.results[0].id, new_id,
        "at equal vector distance the recently-accessed memory must outrank the aged one"
    );
    assert_eq!(outcome.results[1].id, old_id);
}

#[tokio::test]
async fn forgotten_memory_never_resurfaces_semantically() {
    // Vec sync kill-test (RESEARCH Pitfall 3): forget must remove the vector,
    // so a deleted memory's id never comes back from a semantic search.
    let mut vectors = HashMap::new();
    vectors.insert("the secret launch plan alpha".to_string(), unit_axis(0));
    vectors.insert("what is the launch plan".to_string(), near_axis0());

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    let embedder = Arc::new(FakeEmbedder::with_vectors(vectors));
    let service = service_for(&db_path, clock.clone(), embedder);

    let id = service
        .store(new_memory(
            "the secret launch plan alpha",
            MemoryType::Decision,
        ))
        .await
        .expect("store");

    let before = service
        .search(SearchArgs {
            query: "what is the launch plan".to_string(),
            ..Default::default()
        })
        .await
        .expect("search before forget");
    assert!(
        before.results.iter().any(|v| v.id == id),
        "the memory must be semantically findable before forget"
    );

    assert!(service.forget(id).await.expect("forget"), "row deleted");

    let after = service
        .search(SearchArgs {
            query: "what is the launch plan".to_string(),
            ..Default::default()
        })
        .await
        .expect("search after forget");
    assert!(
        after.results.iter().all(|v| v.id != id),
        "a forgotten memory's id must NEVER resurface from semantic search"
    );

    // Belt-and-braces: the vector row itself is gone (not merely masked by the
    // JOIN) — vec_memories can never drift from memories (T-02-05).
    agent_memory_core::store::sqlite::register_vec_extension().expect("register vec0");
    let conn = rusqlite::Connection::open(&db_path).expect("open db for assertion");
    let vec_rows: i64 = conn
        .query_row(
            "SELECT count(*) FROM vec_memories WHERE memory_id = ?1",
            [id],
            |row| row.get(0),
        )
        .expect("count vec rows");
    assert_eq!(vec_rows, 0, "forget must delete the vec_memories row");
}

/// Live end-to-end proof against a REAL local Ollama (Open Question 4 policy:
/// CI never runs this — `cargo test -p agent-memory-core --test semantic -- --ignored`).
#[tokio::test]
#[ignore = "requires a local Ollama daemon with nomic-embed-text pulled"]
async fn live_ollama_semantic_recall_without_shared_keywords() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    let embedder = Arc::new(OllamaClient::new("http://localhost:11434"));
    let service = service_for(&db_path, clock.clone(), embedder);

    let rls_id = service
        .store(new_memory(RLS_CONTENT, MemoryType::Decision))
        .await
        .expect("store rls memory");
    service
        .store(new_memory(
            "weekly standup happens every monday morning",
            MemoryType::Pattern,
        ))
        .await
        .expect("store unrelated 1");
    service
        .store(new_memory(
            "the frontend bundle uses tailwind for styling",
            MemoryType::Pattern,
        ))
        .await
        .expect("store unrelated 2");

    let outcome = service
        .search(SearchArgs {
            query: RLS_QUERY.to_string(),
            ..Default::default()
        })
        .await
        .expect("live semantic search");

    assert_eq!(outcome.search_mode, SearchMode::Semantic);
    assert_eq!(
        outcome.results.first().map(|v| v.id),
        Some(rls_id),
        "live Ollama must rank the RLS memory first for a no-keyword-overlap query"
    );
}
