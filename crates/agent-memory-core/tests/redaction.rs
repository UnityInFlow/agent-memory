//! D-14 proof: a storage fault on EVERY non-search `MemoryService` path is
//! redacted at the service seam — the T-04-19 pattern generalised from
//! `search` to `store`, `list`, `import`, `forget`, `link`, `unlink`, `update`
//! and `sweep` (04-REVIEW IN-01 closed).
//!
//! Each test takes the table the operation writes or reads offline (an
//! `ALTER TABLE … RENAME` on a raw side connection, exactly the hybrid.rs
//! recipe), calls the operation, and asserts the caller sees ONLY
//! `MemoryError::Internal("<op> failed due to a storage error (details are
//! in the server log)")` — never the engine's `sqlite error: …` text with its
//! table and column names. The detail goes to the `tracing::error!` log.
//!
//! Harness: a REAL on-disk temp DB, an injected `TestClock`, and a failing
//! `FakeEmbedder` (stores land pending; nothing here depends on Ollama).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{LinkKind, MemoryError, MemoryPatch, MemoryType, NewMemory};
use agent_memory_core::embed::{Embedder, FakeEmbedder};
use agent_memory_core::service::{ListArgs, MemoryService};
use agent_memory_core::store::sqlite::{register_vec_extension, SqliteStore};

struct Harness {
    service: MemoryService,
    clock: Arc<TestClock>,
    db_path: PathBuf,
    _dir: tempfile::TempDir,
}

fn harness() -> Harness {
    harness_with(Arc::new(FakeEmbedder::failing()))
}

fn harness_with(embedder: Arc<dyn Embedder>) -> Harness {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let store = SqliteStore::open(&db_path).expect("open store");
    let clock = Arc::new(TestClock::new(1_000));
    let service = MemoryService::new(
        Arc::new(store),
        clock.clone() as Arc<dyn Clock>,
        embedder,
        DecayConfig::default(),
    );
    Harness {
        service,
        clock,
        db_path,
        _dir: dir,
    }
}

fn new_memory(content: &str) -> NewMemory {
    NewMemory {
        content: content.to_string(),
        mem_type: MemoryType::Decision,
        tags: vec!["redaction".to_string()],
        source: Some("redaction-test".to_string()),
        scope: None,
        ttl_secs: None,
    }
}

async fn store(h: &Harness, content: &str) -> i64 {
    h.service
        .store(new_memory(content))
        .await
        .expect("store succeeds while the table is online")
}

fn raw_conn(db_path: &Path) -> rusqlite::Connection {
    register_vec_extension().expect("vec0 registers");
    rusqlite::Connection::open(db_path).expect("open raw connection")
}

/// Take `table` offline underneath the live store by renaming it away. The
/// store's next statement against it fails at prepare time with the engine's
/// `no such table` error — the storage-tier fault the seam must redact.
fn take_offline(db_path: &Path, table: &str) {
    raw_conn(db_path)
        .execute_batch(&format!("ALTER TABLE {table} RENAME TO {table}_offline;"))
        .expect("rename table offline");
}

/// Both D-14 assertions: the exact redacted template for `op`, and no
/// storage-engine text anywhere in the rendered error.
fn assert_redacted(err: &MemoryError, op: &str) {
    let expected = format!("{op} failed due to a storage error (details are in the server log)");
    assert!(
        matches!(err, MemoryError::Internal(ref m) if m == &expected),
        "{op}: expected Internal({expected:?}), got {err:?}"
    );
    assert!(
        !err.to_string().to_lowercase().contains("sqlite"),
        "{op}: no SQLite text may reach the client: {err}"
    );
}

#[tokio::test]
async fn store_storage_failure_is_redacted() {
    let h = harness();
    take_offline(&h.db_path, "memories");

    let err = h
        .service
        .store(new_memory("will not land"))
        .await
        .expect_err("store must fail with the table offline");
    assert_redacted(&err, "store");
}

#[tokio::test]
async fn list_storage_failure_is_redacted() {
    let h = harness();
    store(&h, "listed memory").await;
    take_offline(&h.db_path, "memories");

    let err = h
        .service
        .list(ListArgs::default())
        .await
        .expect_err("list must fail with the table offline");
    assert_redacted(&err, "list");
}

#[tokio::test]
async fn import_storage_failure_is_redacted() {
    let h = harness();
    take_offline(&h.db_path, "memories");

    let err = h
        .service
        .import(vec![new_memory("imported draft")])
        .await
        .expect_err("import must fail with the table offline");
    assert_redacted(&err, "import");
}

#[tokio::test]
async fn forget_storage_failure_is_redacted() {
    let h = harness();
    let id = store(&h, "to be forgotten").await;
    take_offline(&h.db_path, "memories");

    let err = h
        .service
        .forget(id)
        .await
        .expect_err("forget must fail with the table offline");
    assert_redacted(&err, "forget");
}

#[tokio::test]
async fn link_storage_failure_is_redacted() {
    let h = harness();
    let a = store(&h, "link endpoint a").await;
    let b = store(&h, "link endpoint b").await;
    // The endpoint pre-check reads `memories` and must still pass; it is the
    // edge INSERT into `memory_links` that fails.
    take_offline(&h.db_path, "memory_links");

    let err = h
        .service
        .link(a, b, LinkKind::RelatesTo)
        .await
        .expect_err("link must fail with the edge table offline");
    assert_redacted(&err, "link");
}

#[tokio::test]
async fn unlink_storage_failure_is_redacted() {
    let h = harness();
    let a = store(&h, "unlink endpoint a").await;
    let b = store(&h, "unlink endpoint b").await;
    take_offline(&h.db_path, "memory_links");

    let err = h
        .service
        .unlink(a, b, LinkKind::RelatesTo)
        .await
        .expect_err("unlink must fail with the edge table offline");
    assert_redacted(&err, "unlink");
}

#[tokio::test]
async fn update_storage_failure_is_redacted() {
    let h = harness();
    let id = store(&h, "original wording").await;
    take_offline(&h.db_path, "memories");

    let patch = MemoryPatch {
        content: Some(Some("new wording".to_string())),
        ..MemoryPatch::default()
    };
    let err = h
        .service
        .update(id, patch)
        .await
        .expect_err("update must fail with the table offline");
    assert_redacted(&err, "update");
}

#[tokio::test]
async fn sweep_storage_failure_is_redacted() {
    let h = harness();
    store(&h, "swept memory").await;
    take_offline(&h.db_path, "memories");

    let err = h
        .service
        .sweep(h.clock.now())
        .await
        .expect_err("sweep must fail with the table offline");
    assert_redacted(&err, "sweep");
}
