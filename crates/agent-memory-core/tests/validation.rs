//! API-02 boundary matrix at the `MemoryService` seam (D-01..D-06).
//!
//! Out-of-bounds `limit`/`ttl_secs` must reject with
//! `MemoryError::InvalidArgument` BEFORE any embed or store hop; valid
//! boundaries and omitted values keep the exact v1.0 behavior. The dead
//! `FakeEmbedder::failing()` harness (copied from tests/fallback.rs) exercises
//! the real seam with no Ollama and no network.
//!
//! Per RESEARCH Pitfall 2 the matrix includes the VALID boundaries (1, 200 /
//! 1, 3_155_760_000), not just the invalid values — an off-by-one in the range
//! check must fail these tests, not slip through.

use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{MemoryError, MemoryType, NewMemory};
use agent_memory_core::embed::{Embedder, FakeEmbedder};
use agent_memory_core::service::{ListArgs, MemoryService, SearchArgs};
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

fn dead_embedder_service(dir: &tempfile::TempDir) -> MemoryService {
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    service_for(&db_path, clock, Arc::new(FakeEmbedder::failing()))
}

const INVALID_LIMITS: [i64; 5] = [0, -1, 201, i64::MAX, i64::MIN];
const INVALID_TTLS: [i64; 5] = [0, -1, 3_155_760_001, i64::MAX, i64::MIN];

#[tokio::test]
async fn out_of_bounds_limit_rejected_at_service_seam() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = dead_embedder_service(&dir);

    for bad in INVALID_LIMITS {
        let err = service
            .search(SearchArgs {
                query: "anything".to_string(),
                limit: Some(bad),
                ..Default::default()
            })
            .await
            .expect_err("out-of-bounds limit must reject on search");
        assert!(
            matches!(err, MemoryError::InvalidArgument(_)),
            "search limit {bad} must map to InvalidArgument, got: {err:?}"
        );
        assert!(
            err.to_string().contains("limit must be between"),
            "message must carry the range, got: {err}"
        );

        let err = service
            .list(ListArgs {
                limit: Some(bad),
                ..Default::default()
            })
            .await
            .expect_err("out-of-bounds limit must reject on list");
        assert!(
            matches!(err, MemoryError::InvalidArgument(_)),
            "list limit {bad} must map to InvalidArgument, got: {err:?}"
        );
        assert!(
            err.to_string().contains("limit must be between"),
            "message must carry the range, got: {err}"
        );
    }
}

#[tokio::test]
async fn valid_limit_boundaries_succeed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = dead_embedder_service(&dir);

    service
        .store(new_memory("a searchable pipeline row", MemoryType::Pattern))
        .await
        .expect("store succeeds under a dead embedder");

    for good in [Some(1), Some(200), None] {
        service
            .search(SearchArgs {
                query: "pipeline".to_string(),
                limit: good,
                ..Default::default()
            })
            .await
            .unwrap_or_else(|e| panic!("search with limit {good:?} must succeed, got: {e}"));

        service
            .list(ListArgs {
                limit: good,
                ..Default::default()
            })
            .await
            .unwrap_or_else(|e| panic!("list with limit {good:?} must succeed, got: {e}"));
    }
}

#[tokio::test]
async fn out_of_bounds_ttl_rejected_on_store_and_import() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = dead_embedder_service(&dir);

    for bad in INVALID_TTLS {
        let mut draft = new_memory("ttl store probe", MemoryType::Todo);
        draft.ttl_secs = Some(bad);
        let err = service
            .store(draft)
            .await
            .expect_err("out-of-bounds ttl_secs must reject on store");
        assert!(
            matches!(err, MemoryError::InvalidArgument(_)),
            "store ttl {bad} must map to InvalidArgument, got: {err:?}"
        );
        assert!(
            err.to_string().contains("ttl_secs must be between"),
            "message must carry the range, got: {err}"
        );

        // The import bypass is closed (RESEARCH Pitfall 3): import() builds
        // rows via store.insert, skipping store() — it must validate per draft
        // through the same shared helper.
        let good = new_memory("import good draft", MemoryType::Todo);
        let mut bad_draft = new_memory("import bad draft", MemoryType::Todo);
        bad_draft.ttl_secs = Some(bad);
        let err = service
            .import(vec![good, bad_draft])
            .await
            .expect_err("a bad-ttl draft must reject the import batch");
        assert!(
            matches!(err, MemoryError::InvalidArgument(_)),
            "import ttl {bad} must map to InvalidArgument, got: {err:?}"
        );
        assert!(
            err.to_string().contains("ttl_secs must be between"),
            "message must carry the range, got: {err}"
        );
    }
}

#[tokio::test]
async fn valid_ttl_boundaries_succeed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = dead_embedder_service(&dir);

    for (i, good) in [Some(1), Some(3_155_760_000), None].iter().enumerate() {
        let mut draft = new_memory(&format!("ttl boundary row {i}"), MemoryType::Todo);
        draft.ttl_secs = *good;
        // Dead embedder → the row lands with embedding_status 0, still Ok
        // (v1.0 SEARCH-03 behavior, unchanged).
        let id = service
            .store(draft)
            .await
            .unwrap_or_else(|e| panic!("store with ttl {good:?} must succeed, got: {e}"));
        assert!(id > 0);
    }
}
