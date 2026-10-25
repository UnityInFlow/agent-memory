//! D-14 proof: a storage fault on EVERY non-search `MemoryService` path is
//! redacted at the service seam — the T-04-19 pattern generalised from
//! `search` to `store`, `list`, `import`, `forget`, `link`, `unlink`, `update`,
//! `sweep`, and the Phase 5 `export` / `import_jsonl` paths (04-REVIEW IN-01
//! closed; 05-REVIEW IN-02).
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
use agent_memory_core::import::jsonl::parse_jsonl;
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

// ---------------------------------------------------------------------------
// Phase 5 paths (05-REVIEW IN-02): `export` and `import_jsonl` get the same
// take_offline proof as the eight paths above, so a refactor that unwraps
// either one turns red here instead of leaking engine text to the CLI.
// ---------------------------------------------------------------------------

/// A valid v1 header declaring `rows` memories and `links` links.
fn jsonl_header(rows: u64, links: u64) -> String {
    format!(
        r#"{{"record":"header","format":"agent-memory-export","version":1,"exported_at":1000,"embedding_model":"nomic-embed-text","embedding_dim":768,"schema_version":3,"agent_memory_version":"0.0.1","exported_rows":{rows},"exported_links":{links}}}"#
    )
}

const JSONL_MEM1: &str = r#"{"record":"memory","key":1,"type":"DECISION","content":"redaction first","tags":[],"source":null,"scope":null,"created_at":1,"last_accessed":1,"access_count":0,"expires_at":null}"#;
const JSONL_MEM2: &str = r#"{"record":"memory","key":2,"type":"PATTERN","content":"redaction second","tags":[],"source":null,"scope":null,"created_at":2,"last_accessed":2,"access_count":0,"expires_at":null}"#;
const JSONL_LINK12: &str =
    r#"{"record":"link","from":1,"to":2,"kind":"relates_to","created_at":5}"#;

#[tokio::test]
async fn export_storage_failure_is_redacted() {
    let h = harness();
    store(&h, "exported memory").await;
    take_offline(&h.db_path, "memories");

    let sink: Box<dyn std::io::Write + Send> = Box::new(Vec::new());
    let err = h
        .service
        .export(sink, "test-0.0.0".to_string())
        .await
        .expect_err("export must fail with the table offline");
    assert_redacted(&err, "export");
}

#[tokio::test]
async fn import_jsonl_storage_failure_is_redacted() {
    let h = harness();
    // `memories` stays online so pass 2 inserts both rows; pass 3's
    // `store.link` is the first statement to touch the renamed table.
    take_offline(&h.db_path, "memory_links");

    let text = format!(
        "{}\n{JSONL_MEM1}\n{JSONL_MEM2}\n{JSONL_LINK12}\n",
        jsonl_header(2, 1)
    );
    let file = parse_jsonl(&text).expect("the file itself is valid");
    let err = h
        .service
        .import_jsonl(file)
        .await
        .expect_err("import_jsonl must fail with the links table offline");
    assert_redacted(&err, "import");
}
