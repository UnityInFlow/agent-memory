//! CR-03: the sweep's embedding backfill never writes a vector that disagrees
//! with the row's current content.
//!
//! The backfill reads `(id, content)` for pending rows, awaits ONE batch embed
//! outside any lock (seconds on a CPU Ollama), then writes the vectors. Before
//! the fix that write was unconditional, so anything landing between the read
//! and the write was silently overwritten:
//!
//! - **update (embed ok):** the fresh vector of the NEW text was replaced by a
//!   vector of the OLD text, and the row read as embedded (status 1), so no
//!   later sweep ever repaired it;
//! - **update (embed failed):** the pending row was "repaired" with the stale
//!   vector instead of being re-embedded with its new text;
//! - **forget:** a vector was re-inserted for a deleted id, ready to attach to
//!   a later memory that reuses the rowid;
//! - **a second sweep** (several processes share one DB file, each with its
//!   own sweep) wrote the same row twice.
//!
//! `Store::insert_embedding` now claims the row first, in the writer
//! transaction: `embedding_status = 0 AND content = <the embedded text>`. No
//! claim, no write. The store-level tests below drive each interleaving
//! directly through the public `Store` methods; the service-level tests pin
//! the real `MemoryService::sweep` mid-embed with a `GatedEmbedder` (tokio
//! `Notify` only, no sleeps decide ordering) and land an update in the gap.
//!
//! Real on-disk temp DBs and an injected `TestClock`, as in tests/hybrid.rs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{MemoryPatch, MemoryType, NewMemory};
use agent_memory_core::embed::{
    BoxFuture, EmbedError, Embedder, EmbedderHealth, FakeEmbedder, EMBEDDING_DIM,
};
use agent_memory_core::service::{MemoryService, SearchArgs};
use agent_memory_core::store::sqlite::SqliteStore;
use agent_memory_core::store::Store;
use tokio::sync::Notify;

const OLD_TEXT: &str = "old text";
const NEW_TEXT: &str = "new text";
const EPS: f64 = 1e-5;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A programmed [`FakeEmbedder`] behind an outage switch and a one-shot gate.
///
/// While `armed`, the first batch containing [`OLD_TEXT`] disarms the gate,
/// signals `entered`, and parks on `release` before delegating, which holds the
/// sweep exactly between its `pending_embeddings` read and its write. The
/// outage switch is checked on ENTRY, so a gated call that is already parked
/// is unaffected by flipping `fail` afterwards.
struct GatedEmbedder {
    inner: FakeEmbedder,
    fail: AtomicBool,
    armed: AtomicBool,
    entered: Notify,
    release: Notify,
}

impl GatedEmbedder {
    fn new(vectors: HashMap<String, Vec<f32>>) -> Self {
        GatedEmbedder {
            inner: FakeEmbedder::with_vectors(vectors),
            fail: AtomicBool::new(false),
            armed: AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
        }
    }

    fn set_fail(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }

    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
}

impl Embedder for GatedEmbedder {
    fn embed<'a>(
        &'a self,
        inputs: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, EmbedError>> {
        Box::pin(async move {
            if self.fail.load(Ordering::SeqCst) {
                return Err(EmbedError::InvalidResponse(
                    "GatedEmbedder is down".to_string(),
                ));
            }
            let gated = inputs.iter().any(|input| input == OLD_TEXT)
                && self.armed.swap(false, Ordering::SeqCst);
            if gated {
                // notify_one stores a permit if the test is not waiting yet.
                self.entered.notify_one();
                self.release.notified().await;
            }
            self.inner.embed(inputs).await
        })
    }

    fn health<'a>(&'a self) -> BoxFuture<'a, Result<EmbedderHealth, EmbedError>> {
        self.inner.health()
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    db_path: PathBuf,
    clock: Arc<TestClock>,
    store: Arc<SqliteStore>,
    embedder: Arc<GatedEmbedder>,
    service: MemoryService,
}

/// `OLD_TEXT → v_old`, `NEW_TEXT → v_new`: orthogonal unit vectors, so under
/// the schema's cosine metric the distance between them is exactly 1.0.
fn harness() -> Harness {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    let store = Arc::new(SqliteStore::open(&db_path).expect("open store"));
    let mut vectors = HashMap::new();
    vectors.insert(OLD_TEXT.to_string(), v_old());
    vectors.insert(NEW_TEXT.to_string(), v_new());
    let embedder = Arc::new(GatedEmbedder::new(vectors));
    let service = MemoryService::new(
        store.clone(),
        clock.clone() as Arc<dyn Clock>,
        embedder.clone() as Arc<dyn Embedder>,
        DecayConfig::default(),
    );
    Harness {
        _dir: dir,
        db_path,
        clock,
        store,
        embedder,
        service,
    }
}

/// A 768-dim unit vector along `axis`.
fn unit_axis(axis: usize) -> Vec<f32> {
    let mut v = vec![0.0_f32; EMBEDDING_DIM];
    v[axis] = 1.0;
    v
}

fn v_old() -> Vec<f32> {
    unit_axis(1)
}

fn v_new() -> Vec<f32> {
    unit_axis(2)
}

fn new_memory(content: &str) -> NewMemory {
    NewMemory {
        content: content.to_string(),
        mem_type: MemoryType::Pattern,
        tags: vec![],
        source: None,
        scope: None,
        ttl_secs: None,
    }
}

fn content_patch(content: &str) -> MemoryPatch {
    MemoryPatch {
        content: Some(Some(content.to_string())),
        ..Default::default()
    }
}

/// A row stored straight through the store with no vector: pending (status 0).
fn pending_row(h: &Harness, content: &str) -> i64 {
    h.store
        .insert(new_memory(content), None, h.clock.now())
        .expect("insert pending row")
}

fn raw_conn(db_path: &Path) -> rusqlite::Connection {
    agent_memory_core::store::sqlite::register_vec_extension().expect("register vec0");
    rusqlite::Connection::open(db_path).expect("open db for assertion")
}

fn vec_rows(h: &Harness, id: i64) -> i64 {
    raw_conn(&h.db_path)
        .query_row(
            "SELECT COUNT(*) FROM vec_memories WHERE memory_id = ?1",
            [id],
            |row| row.get(0),
        )
        .expect("count vector rows")
}

fn status(h: &Harness, id: i64) -> i64 {
    raw_conn(&h.db_path)
        .query_row(
            "SELECT embedding_status FROM memories WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .expect("read embedding_status")
}

fn content(h: &Harness, id: i64) -> String {
    raw_conn(&h.db_path)
        .query_row("SELECT content FROM memories WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .expect("read content")
}

/// The KNN cosine distance of `id`'s stored vector to `query`.
fn knn_distance(h: &Harness, id: i64, query: Vec<f32>) -> f64 {
    let hits = h
        .store
        .knn_search(
            query,
            SearchArgs {
                limit: Some(50),
                ..Default::default()
            },
        )
        .expect("knn search");
    hits.into_iter()
        .find(|(view, _)| view.id == id)
        .map(|(_, distance)| distance)
        .expect("the id has a stored vector")
}

fn assert_stores_new_vector(h: &Harness, id: i64) {
    assert_eq!(status(h, id), 1, "the row reads as embedded");
    assert_eq!(vec_rows(h, id), 1, "exactly one vector row for the id");
    let to_new = knn_distance(h, id, v_new());
    let to_old = knn_distance(h, id, v_old());
    assert!(to_new.abs() < EPS, "vector is v_new (distance {to_new})");
    assert!(
        (to_old - 1.0).abs() < EPS,
        "vector is not v_old (distance {to_old})"
    );
}

// ---------------------------------------------------------------------------
// Store-level interleavings
// ---------------------------------------------------------------------------

#[test]
fn backfill_of_an_unchanged_pending_row_writes_its_vector() {
    let h = harness();
    let id = pending_row(&h, OLD_TEXT);
    assert_eq!(status(&h, id), 0);

    let pending = h.store.pending_embeddings(128).expect("pending");
    assert_eq!(pending, vec![(id, OLD_TEXT.to_string())]);

    let written = h
        .store
        .insert_embedding(id, OLD_TEXT, v_old())
        .expect("insert_embedding");
    assert!(written, "an unchanged pending row is claimed and written");
    assert_eq!(status(&h, id), 1);
    assert_eq!(vec_rows(&h, id), 1);
    assert!(knn_distance(&h, id, v_old()).abs() < EPS);
    assert!(h.store.pending_embeddings(128).expect("pending").is_empty());
}

#[test]
fn backfill_after_an_embedded_update_writes_nothing() {
    let h = harness();
    let id = pending_row(&h, OLD_TEXT);
    let pending = h.store.pending_embeddings(128).expect("pending");
    assert_eq!(pending, vec![(id, OLD_TEXT.to_string())]);

    // The update lands between the backfill's read and its write.
    h.store
        .update(id, content_patch(NEW_TEXT), Some(v_new()), h.clock.now())
        .expect("update")
        .expect("id exists");

    let written = h
        .store
        .insert_embedding(id, OLD_TEXT, v_old())
        .expect("insert_embedding");
    assert!(!written, "the stale backfill is skipped");
    assert_stores_new_vector(&h, id);
}

#[test]
fn backfill_after_an_unembedded_update_leaves_the_row_pending_for_its_new_text() {
    let h = harness();
    let id = pending_row(&h, OLD_TEXT);
    let _ = h.store.pending_embeddings(128).expect("pending");

    // The update's own embed failed: the row is pending with the new text.
    h.store
        .update(id, content_patch(NEW_TEXT), None, h.clock.now())
        .expect("update")
        .expect("id exists");

    let written = h
        .store
        .insert_embedding(id, OLD_TEXT, v_old())
        .expect("insert_embedding");
    assert!(!written, "the stale text is never embedded into the row");
    assert_eq!(vec_rows(&h, id), 0, "no vector for the row");
    assert_eq!(status(&h, id), 0, "still pending");

    // The next sweep sees the CURRENT content and embeds that.
    let pending = h.store.pending_embeddings(128).expect("pending");
    assert_eq!(pending, vec![(id, NEW_TEXT.to_string())]);
    let written = h
        .store
        .insert_embedding(id, NEW_TEXT, v_new())
        .expect("insert_embedding");
    assert!(written);
    assert_stores_new_vector(&h, id);
}

#[test]
fn backfill_after_a_forget_never_resurrects_a_vector() {
    let h = harness();
    let id = pending_row(&h, OLD_TEXT);
    let _ = h.store.pending_embeddings(128).expect("pending");

    assert!(h.store.forget(id).expect("forget"));

    let written = h
        .store
        .insert_embedding(id, OLD_TEXT, v_old())
        .expect("insert_embedding on a forgotten id is not an error");
    assert!(!written, "a forgotten row is never claimed");
    assert_eq!(vec_rows(&h, id), 0, "no vector for the forgotten id");

    // INTEGER PRIMARY KEY without AUTOINCREMENT: the next insert into the now
    // empty table reuses the rowid. The stale backfill must not attach to it.
    let reused = pending_row(&h, "a different memory");
    assert_eq!(reused, id, "the rowid was recycled");
    let written = h
        .store
        .insert_embedding(id, OLD_TEXT, v_old())
        .expect("insert_embedding");
    assert!(
        !written,
        "a reused rowid with other content is never claimed"
    );
    assert_eq!(vec_rows(&h, reused), 0);
    assert_eq!(status(&h, reused), 0);
}

#[test]
fn a_second_backfill_of_the_same_row_writes_nothing() {
    let h = harness();
    let id = pending_row(&h, OLD_TEXT);
    let _ = h.store.pending_embeddings(128).expect("pending");

    // Two sweeps (e.g. two processes on one DB file) both read the row.
    assert!(h
        .store
        .insert_embedding(id, OLD_TEXT, v_old())
        .expect("first backfill"));
    assert!(
        !h.store
            .insert_embedding(id, OLD_TEXT, v_old())
            .expect("second backfill"),
        "the row is no longer pending, so the second write is skipped"
    );
    assert_eq!(vec_rows(&h, id), 1, "exactly one vector row");
    assert_eq!(status(&h, id), 1);
}

// ---------------------------------------------------------------------------
// Service-level: the real sweep, pinned mid-embed
// ---------------------------------------------------------------------------

/// Store `OLD_TEXT` while the embedder is down, so it lands pending.
async fn store_pending_via_service(h: &Harness) -> i64 {
    h.embedder.set_fail(true);
    let id = h
        .service
        .store(new_memory(OLD_TEXT))
        .await
        .expect("store during an outage still succeeds");
    assert_eq!(status(h, id), 0, "stored pending");
    h.embedder.set_fail(false);
    id
}

#[tokio::test]
async fn an_update_landing_mid_backfill_keeps_its_fresh_vector() {
    let h = harness();
    let id = store_pending_via_service(&h).await;

    h.embedder.arm();
    let sweeper = h.service.clone();
    let now = h.clock.now();
    let sweep = tokio::spawn(async move { sweeper.sweep(now).await });

    // The sweep has read (id, OLD_TEXT) and is parked inside its embed call.
    h.embedder.entered.notified().await;

    // The update's own embed is not gated: it succeeds and stores v_new.
    let updated = h
        .service
        .update(id, content_patch(NEW_TEXT))
        .await
        .expect("update")
        .expect("id exists");
    assert_eq!(updated.embedding_status, 1);

    h.embedder.release.notify_one();
    sweep
        .await
        .expect("sweep task joins")
        .expect("sweep succeeds");

    assert_eq!(content(&h, id), NEW_TEXT);
    assert_stores_new_vector(&h, id);
}

#[tokio::test]
async fn a_failed_update_landing_mid_backfill_is_embedded_with_its_new_text_next_sweep() {
    let h = harness();
    let id = store_pending_via_service(&h).await;

    h.embedder.arm();
    let sweeper = h.service.clone();
    let now = h.clock.now();
    let sweep = tokio::spawn(async move { sweeper.sweep(now).await });
    h.embedder.entered.notified().await;

    // Ollama goes down only around the update: it lands pending.
    h.embedder.set_fail(true);
    let updated = h
        .service
        .update(id, content_patch(NEW_TEXT))
        .await
        .expect("an embed outage never fails the update")
        .expect("id exists");
    assert_eq!(updated.embedding_status, 0);
    h.embedder.set_fail(false);

    h.embedder.release.notify_one();
    sweep
        .await
        .expect("sweep task joins")
        .expect("sweep succeeds");

    assert_eq!(content(&h, id), NEW_TEXT);
    assert_eq!(status(&h, id), 0, "the stale vector was not written");
    assert_eq!(vec_rows(&h, id), 0);

    // An ordinary sweep now embeds the CURRENT content.
    h.service.sweep(h.clock.now()).await.expect("second sweep");
    assert_stores_new_vector(&h, id);
}
