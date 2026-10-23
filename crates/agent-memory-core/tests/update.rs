//! MCP-06: `memory_update` patches a memory IN PLACE — content, tags, scope and
//! TTL — with the FTS5 mirror re-synced and the vector invalidated/re-embedded
//! in the SAME writer transaction.
//!
//! Locked decisions proven here: D-08 (JSON-merge-patch: absent = unchanged,
//! explicit null = clear where legal, content never null/empty), D-09 (an
//! update is a freshness signal: `last_accessed = now`, `access_count + 1`),
//! D-10 (the response is the full post-update view plus `embedding_status`).
//!
//! Harness: a REAL on-disk temp DB, an injected `TestClock`, and a local
//! `SwitchEmbedder` that wraps the programmed `FakeEmbedder`, counts calls, and
//! can be flipped into an outage — so one service instance can store while
//! Ollama is "up", update while it is "down", and backfill once it recovers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{MemoryError, MemoryPatch, MemoryType, NewMemory, MAX_TTL_SECS};
use agent_memory_core::embed::{
    BoxFuture, EmbedError, Embedder, EmbedderHealth, FakeEmbedder, EMBEDDING_DIM,
};
use agent_memory_core::service::{ListArgs, MemoryService, SearchArgs, SearchMode};
use agent_memory_core::store::sqlite::{register_vec_extension, SqliteStore};

/// A programmed [`FakeEmbedder`] behind an outage switch and a call counter.
struct SwitchEmbedder {
    inner: FakeEmbedder,
    down: AtomicBool,
    calls: AtomicUsize,
}

impl SwitchEmbedder {
    fn new(vectors: HashMap<String, Vec<f32>>) -> Self {
        SwitchEmbedder {
            inner: FakeEmbedder::with_vectors(vectors),
            down: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
        }
    }

    fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Embedder for SwitchEmbedder {
    fn embed<'a>(
        &'a self,
        inputs: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, EmbedError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.down.load(Ordering::SeqCst) {
            return Box::pin(async {
                Err(EmbedError::InvalidResponse(
                    "SwitchEmbedder is down".to_string(),
                ))
            });
        }
        self.inner.embed(inputs)
    }

    fn health<'a>(&'a self) -> BoxFuture<'a, Result<EmbedderHealth, EmbedError>> {
        self.inner.health()
    }
}

struct Harness {
    service: MemoryService,
    clock: Arc<TestClock>,
    embedder: Arc<SwitchEmbedder>,
    db_path: PathBuf,
    _dir: tempfile::TempDir,
}

fn harness(vectors: HashMap<String, Vec<f32>>) -> Harness {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let store = SqliteStore::open(&db_path).expect("open store");
    let clock = Arc::new(TestClock::new(1_000));
    let embedder = Arc::new(SwitchEmbedder::new(vectors));
    let service = MemoryService::new(
        Arc::new(store),
        clock.clone() as Arc<dyn Clock>,
        embedder.clone() as Arc<dyn Embedder>,
        DecayConfig::default(),
    );
    Harness {
        service,
        clock,
        embedder,
        db_path,
        _dir: dir,
    }
}

fn new_memory(content: &str) -> NewMemory {
    NewMemory {
        content: content.to_string(),
        mem_type: MemoryType::Pattern,
        tags: vec!["original".to_string()],
        source: Some("update-test".to_string()),
        scope: Some("project-a".to_string()),
        ttl_secs: None,
    }
}

fn content_patch(content: &str) -> MemoryPatch {
    MemoryPatch {
        content: Some(Some(content.to_string())),
        ..Default::default()
    }
}

/// A 768-dim unit vector along the given axis.
fn unit_axis(axis: usize) -> Vec<f32> {
    let mut v = vec![0.0_f32; EMBEDDING_DIM];
    v[axis] = 1.0;
    v
}

/// `a * axis(i) + b * axis(j)` — a unit vector when `a² + b² = 1`.
fn mix(i: usize, a: f32, j: usize, b: f32) -> Vec<f32> {
    let mut v = vec![0.0_f32; EMBEDDING_DIM];
    v[i] = a;
    v[j] = b;
    v
}

/// Raw row columns the wire view does not expose.
struct RawRow {
    embedding_status: i64,
    access_count: i64,
    last_accessed: i64,
    expires_at: Option<i64>,
    vec_rows: i64,
}

fn raw_row(db_path: &Path, id: i64) -> RawRow {
    register_vec_extension().expect("vec0 registers");
    let conn = rusqlite::Connection::open(db_path).expect("open raw connection");
    let (embedding_status, access_count, last_accessed, expires_at) = conn
        .query_row(
            "SELECT embedding_status, access_count, last_accessed, expires_at \
             FROM memories WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("row exists");
    let vec_rows = conn
        .query_row(
            "SELECT count(*) FROM vec_memories WHERE memory_id = ?1",
            [id],
            |row| row.get(0),
        )
        .expect("vec count");
    RawRow {
        embedding_status,
        access_count,
        last_accessed,
        expires_at,
        vec_rows,
    }
}

const OLD_CONTENT: &str = "Postgres is the database";
const NEW_CONTENT: &str = "SQLite is the database";
const OLD_QUERY: &str = "which relational server do we run";
const NEW_QUERY: &str = "which embedded file database do we use";

#[tokio::test]
async fn content_update_re_embeds_so_the_old_meaning_stops_matching() {
    // Phase success criterion 1 (the re-embed kill-test). Four distractors sit
    // near the OLD meaning, so with limit 1 (k = 4) a row that kept its stale
    // vector would still rank first for the old query; a re-embedded row is
    // pushed out of the candidate set entirely.
    let mut vectors = HashMap::new();
    vectors.insert(OLD_CONTENT.to_string(), unit_axis(0));
    vectors.insert(NEW_CONTENT.to_string(), unit_axis(1));
    vectors.insert(OLD_QUERY.to_string(), unit_axis(0));
    vectors.insert(NEW_QUERY.to_string(), unit_axis(1));
    for i in 0..4 {
        vectors.insert(format!("distractor {i}"), mix(0, 0.8, 10 + i, 0.6));
    }
    let h = harness(vectors);

    let id = h
        .service
        .store(new_memory(OLD_CONTENT))
        .await
        .expect("store target");
    for i in 0..4 {
        h.service
            .store(new_memory(&format!("distractor {i}")))
            .await
            .expect("store distractor");
    }

    let before = h
        .service
        .search(SearchArgs {
            query: OLD_QUERY.to_string(),
            limit: Some(1),
            ..Default::default()
        })
        .await
        .expect("search before update");
    assert_eq!(before.search_mode, SearchMode::Semantic);
    assert_eq!(
        before.results[0].id, id,
        "sanity: the old meaning matches the target before the update"
    );

    let updated = h
        .service
        .update(id, content_patch(NEW_CONTENT))
        .await
        .expect("update succeeds")
        .expect("id exists");
    assert_eq!(updated.view.content, NEW_CONTENT);
    assert_eq!(updated.embedding_status, 1, "re-embedded in the same tx");

    let old = h
        .service
        .search(SearchArgs {
            query: OLD_QUERY.to_string(),
            limit: Some(1),
            ..Default::default()
        })
        .await
        .expect("old-meaning search");
    assert_eq!(old.search_mode, SearchMode::Semantic);
    assert!(
        old.results.iter().all(|v| v.id != id),
        "the old meaning must no longer return the updated row: {:?}",
        old.results
    );

    let new = h
        .service
        .search(SearchArgs {
            query: NEW_QUERY.to_string(),
            limit: Some(1),
            ..Default::default()
        })
        .await
        .expect("new-meaning search");
    assert_eq!(new.search_mode, SearchMode::Semantic);
    assert_eq!(new.results[0].id, id, "the new meaning returns the row");

    // Exactly one vector row: the old one was replaced, not shadowed.
    assert_eq!(raw_row(&h.db_path, id).vec_rows, 1);

    // FTS5 was re-mirrored by the update trigger: keyword search (embedder
    // down) finds the new wording and no longer finds the old.
    h.embedder.set_down(true);
    let kw_old = h
        .service
        .search(SearchArgs {
            query: "Postgres".to_string(),
            ..Default::default()
        })
        .await
        .expect("keyword search old");
    assert_eq!(kw_old.search_mode, SearchMode::Keyword);
    assert!(kw_old.results.is_empty(), "old wording left the FTS mirror");
    let kw_new = h
        .service
        .search(SearchArgs {
            query: "SQLite".to_string(),
            ..Default::default()
        })
        .await
        .expect("keyword search new");
    assert_eq!(kw_new.results.len(), 1);
    assert_eq!(kw_new.results[0].id, id);
}

#[tokio::test]
async fn embed_outage_during_update_falls_back_to_pending_and_sweep_backfills() {
    let h = harness(HashMap::new());
    let id = h
        .service
        .store(new_memory(OLD_CONTENT))
        .await
        .expect("store");
    assert_eq!(raw_row(&h.db_path, id).embedding_status, 1);

    h.embedder.set_down(true);
    let updated = h
        .service
        .update(id, content_patch(NEW_CONTENT))
        .await
        .expect("an embed outage never fails the update")
        .expect("id exists");
    assert_eq!(updated.view.content, NEW_CONTENT);
    assert_eq!(
        updated.embedding_status, 0,
        "the response shows the pending re-embed"
    );
    let raw = raw_row(&h.db_path, id);
    assert_eq!(raw.embedding_status, 0);
    assert_eq!(raw.vec_rows, 0, "the stale vector was invalidated");

    h.embedder.set_down(false);
    h.service.sweep(h.clock.now()).await.expect("sweep");
    let raw = raw_row(&h.db_path, id);
    assert_eq!(raw.embedding_status, 1, "one sweep backfills the vector");
    assert_eq!(raw.vec_rows, 1);
}

#[tokio::test]
async fn update_bumps_freshness_by_exactly_one_access() {
    let h = harness(HashMap::new());
    let id = h
        .service
        .store(new_memory("fresh me"))
        .await
        .expect("store");
    let before = raw_row(&h.db_path, id);
    assert_eq!(before.last_accessed, 1_000);

    h.clock.advance(5_000);
    let updated = h
        .service
        .update(
            id,
            MemoryPatch {
                tags: Some(Some(vec!["touched".to_string()])),
                ..Default::default()
            },
        )
        .await
        .expect("update")
        .expect("id exists");

    let after = raw_row(&h.db_path, id);
    assert_eq!(after.last_accessed, 6_000, "last_accessed = patch-time now");
    assert_eq!(after.access_count, before.access_count + 1);
    assert_eq!(updated.view.last_accessed, 6_000);
    assert_eq!(updated.view.decay_score, 1.0, "just touched = fully fresh");
}

#[tokio::test]
async fn update_returns_the_full_post_update_view_with_embedding_status() {
    let h = harness(HashMap::new());
    let id = h
        .service
        .store(new_memory(OLD_CONTENT))
        .await
        .expect("store");

    let updated = h
        .service
        .update(
            id,
            MemoryPatch {
                content: Some(Some(NEW_CONTENT.to_string())),
                tags: Some(Some(vec!["a".to_string(), "b".to_string()])),
                scope: Some(Some("project-b".to_string())),
                ttl_secs: Some(Some(60)),
            },
        )
        .await
        .expect("update")
        .expect("id exists");

    assert_eq!(updated.view.id, id);
    assert_eq!(updated.view.content, NEW_CONTENT);
    assert_eq!(
        updated.view.mem_type,
        MemoryType::Pattern,
        "type is immutable"
    );
    assert_eq!(updated.view.tags, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(updated.view.scope.as_deref(), Some("project-b"));
    assert_eq!(updated.view.created_at, 1_000, "created_at is immutable");
    assert_eq!(updated.embedding_status, 1);
    assert_eq!(raw_row(&h.db_path, id).expires_at, Some(1_060));

    // Wire shape: the MemoryView fields flattened, plus embedding_status.
    let json = serde_json::to_value(&updated).expect("serializes");
    assert_eq!(json["id"], id);
    assert_eq!(json["type"], "PATTERN");
    assert_eq!(json["content"], NEW_CONTENT);
    assert_eq!(json["embedding_status"], 1);
    assert!(json.get("view").is_none(), "the view is flattened");

    // The list read path sees the same post-update row.
    let listed = h.service.list(ListArgs::default()).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0], updated.view);
}

#[tokio::test]
async fn updating_an_unknown_id_is_a_clean_not_found() {
    let h = harness(HashMap::new());
    let outcome = h
        .service
        .update(424_242, content_patch("nobody home"))
        .await
        .expect("an unknown id is data, never an error");
    assert!(outcome.is_none());
}

#[tokio::test]
async fn a_tags_only_patch_never_calls_the_embedder() {
    let h = harness(HashMap::new());
    let id = h
        .service
        .store(new_memory(OLD_CONTENT))
        .await
        .expect("store");
    let calls_after_store = h.embedder.calls();
    let before = raw_row(&h.db_path, id);

    let updated = h
        .service
        .update(
            id,
            MemoryPatch {
                tags: Some(Some(vec!["retagged".to_string()])),
                scope: Some(None),
                ttl_secs: Some(Some(30)),
                ..Default::default()
            },
        )
        .await
        .expect("update")
        .expect("id exists");

    assert_eq!(
        h.embedder.calls(),
        calls_after_store,
        "no content in the patch → zero embed calls"
    );
    assert_eq!(updated.view.content, OLD_CONTENT, "content unchanged");
    assert_eq!(updated.embedding_status, before.embedding_status);
    let after = raw_row(&h.db_path, id);
    assert_eq!(after.embedding_status, before.embedding_status);
    assert_eq!(after.vec_rows, 1, "the vector row is untouched");
}

#[tokio::test]
async fn an_invalid_patch_is_rejected_before_the_embedder_is_called() {
    let h = harness(HashMap::new());
    let id = h
        .service
        .store(new_memory(OLD_CONTENT))
        .await
        .expect("store");
    let calls_after_store = h.embedder.calls();

    let cases: Vec<(MemoryPatch, &str)> = vec![
        (
            MemoryPatch::default(),
            "at least one of content, tags, scope, ttl_secs must be provided",
        ),
        (
            MemoryPatch {
                content: Some(None),
                ..Default::default()
            },
            "content must be a non-empty string",
        ),
        (content_patch(""), "content must be a non-empty string"),
        (
            content_patch("  \n\t "),
            "content must be a non-empty string",
        ),
        (
            MemoryPatch {
                content: Some(Some("valid".to_string())),
                ttl_secs: Some(Some(0)),
                ..Default::default()
            },
            "ttl_secs must be between",
        ),
        (
            MemoryPatch {
                ttl_secs: Some(Some(MAX_TTL_SECS + 1)),
                ..Default::default()
            },
            "ttl_secs must be between",
        ),
    ];

    for (patch, expected) in cases {
        let err = h
            .service
            .update(id, patch.clone())
            .await
            .expect_err("invalid patch is rejected");
        match err {
            MemoryError::InvalidArgument(msg) => assert!(
                msg.contains(expected),
                "patch {patch:?}: expected message containing {expected:?}, got {msg:?}"
            ),
            other => panic!("patch {patch:?}: expected InvalidArgument, got {other:?}"),
        }
    }
    assert_eq!(
        h.embedder.calls(),
        calls_after_store,
        "validation runs before any embed hop"
    );

    // Nothing was written: the row is exactly as stored.
    let listed = h.service.list(ListArgs::default()).await.expect("list");
    assert_eq!(listed[0].content, OLD_CONTENT);
    assert_eq!(raw_row(&h.db_path, id).access_count, 0);
}
