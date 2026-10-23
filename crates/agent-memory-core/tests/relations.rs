//! MCP-07: flat typed relations between memories — `link` / `unlink` with the
//! closed three-kind vocabulary, cascade on every deletion path, and opt-in
//! 1-hop expansion on search/list.
//!
//! Locked decisions proven here: D-01 (closed `kind` enum: relates_to,
//! supersedes, caused_by — anything else is `InvalidArgument` naming the
//! allowed set), D-02 (duplicate link = idempotent no-op success, self-link =
//! `InvalidArgument`, missing endpoint / missing edge = clean not-found data,
//! never an error), D-03 (one directed edge row per link).
//!
//! Harness: a REAL on-disk temp DB, an injected `TestClock`, and a failing
//! `FakeEmbedder` (stores land pending; search deterministically takes the
//! FTS5 keyword path). Edge rows are counted through a raw side connection so
//! the assertions see the table itself, not the service's view of it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{LinkKind, LinkOutcome, MemoryError, MemoryType, NewMemory};
use agent_memory_core::embed::{Embedder, FakeEmbedder};
use agent_memory_core::service::MemoryService;
use agent_memory_core::store::sqlite::{register_vec_extension, SqliteStore};

struct Harness {
    service: MemoryService,
    clock: Arc<TestClock>,
    db_path: PathBuf,
    _dir: tempfile::TempDir,
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let store = SqliteStore::open(&db_path).expect("open store");
    let clock = Arc::new(TestClock::new(1_000));
    let embedder: Arc<dyn Embedder> = Arc::new(FakeEmbedder::failing());
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

fn new_memory(content: &str, ttl_secs: Option<i64>) -> NewMemory {
    NewMemory {
        content: content.to_string(),
        mem_type: MemoryType::Decision,
        tags: vec!["rel".to_string()],
        source: Some("relations-test".to_string()),
        scope: None,
        ttl_secs,
    }
}

async fn store(h: &Harness, content: &str) -> i64 {
    h.service
        .store(new_memory(content, None))
        .await
        .expect("store succeeds")
}

fn raw_conn(db_path: &Path) -> rusqlite::Connection {
    register_vec_extension().expect("vec0 registers");
    rusqlite::Connection::open(db_path).expect("open raw connection")
}

/// Total number of edge rows in `memory_links`.
fn edge_count(db_path: &Path) -> i64 {
    raw_conn(db_path)
        .query_row("SELECT count(*) FROM memory_links", [], |row| row.get(0))
        .expect("count edges")
}

/// Edge rows touching `id` in either direction.
fn edges_touching(db_path: &Path, id: i64) -> i64 {
    raw_conn(db_path)
        .query_row(
            "SELECT count(*) FROM memory_links WHERE from_id = ?1 OR to_id = ?1",
            [id],
            |row| row.get(0),
        )
        .expect("count edges touching id")
}

/// Edge rows whose either endpoint no longer exists in `memories`.
fn orphan_edges(db_path: &Path) -> i64 {
    raw_conn(db_path)
        .query_row(
            "SELECT count(*) FROM memory_links \
             WHERE from_id NOT IN (SELECT id FROM memories) \
                OR to_id NOT IN (SELECT id FROM memories)",
            [],
            |row| row.get(0),
        )
        .expect("count orphan edges")
}

fn invalid_argument_message(err: MemoryError) -> String {
    match err {
        MemoryError::InvalidArgument(msg) => msg,
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Task 1 — link / unlink / cascade
// ---------------------------------------------------------------------------

#[tokio::test]
async fn link_succeeds_and_a_duplicate_link_is_an_idempotent_no_op() {
    let h = harness();
    let a = store(&h, "memory a").await;
    let b = store(&h, "memory b").await;

    let first = h
        .service
        .link(a, b, LinkKind::RelatesTo)
        .await
        .expect("link succeeds");
    assert_eq!(first, LinkOutcome::Linked);
    assert_eq!(edge_count(&h.db_path), 1);

    // D-02: the identical (from, to, kind) again is a success, not an error,
    // and still exactly one row.
    let again = h
        .service
        .link(a, b, LinkKind::RelatesTo)
        .await
        .expect("duplicate link is not an error");
    assert_eq!(again, LinkOutcome::Linked);
    assert_eq!(edge_count(&h.db_path), 1, "duplicate link adds no row");

    // A different kind between the same pair is a distinct edge.
    h.service
        .link(a, b, LinkKind::Supersedes)
        .await
        .expect("second kind links");
    assert_eq!(edge_count(&h.db_path), 2);
}

#[tokio::test]
async fn a_self_link_is_invalid_argument_naming_the_id() {
    let h = harness();
    let a = store(&h, "lonely").await;

    let err = h
        .service
        .link(a, a, LinkKind::RelatesTo)
        .await
        .expect_err("self-link is rejected");
    assert_eq!(
        invalid_argument_message(err),
        format!("from_id and to_id must differ (got {a})")
    );
    assert_eq!(edge_count(&h.db_path), 0);

    // The seam check runs before any store hop: a self-link on an id that
    // does not even exist is still the client error, not a not-found.
    let err = h
        .service
        .link(424_242, 424_242, LinkKind::CausedBy)
        .await
        .expect_err("self-link is rejected before the existence check");
    assert_eq!(
        invalid_argument_message(err),
        "from_id and to_id must differ (got 424242)"
    );
}

#[test]
fn an_unknown_kind_is_invalid_argument_listing_the_allowed_values() {
    let err = LinkKind::try_from("blocks").expect_err("unknown kind is rejected");
    assert_eq!(
        err.to_string(),
        "invalid argument: kind must be one of relates_to, supersedes, caused_by (got 'blocks')"
    );
    assert_eq!(
        invalid_argument_message(err),
        "kind must be one of relates_to, supersedes, caused_by (got 'blocks')"
    );
    // The wire form is exact: case and spelling variants are not aliases.
    assert!(LinkKind::try_from("Supersedes").is_err());
    assert!(LinkKind::try_from("superseded_by").is_err());
    assert!(LinkKind::try_from("").is_err());
}

#[test]
fn every_kind_round_trips_its_wire_form() {
    for (wire, kind) in [
        ("relates_to", LinkKind::RelatesTo),
        ("supersedes", LinkKind::Supersedes),
        ("caused_by", LinkKind::CausedBy),
    ] {
        assert_eq!(LinkKind::try_from(wire).expect("known kind"), kind);
        assert_eq!(kind.as_wire_str(), wire);
        assert_eq!(
            serde_json::to_value(kind).expect("serializes"),
            serde_json::json!(wire)
        );
    }
}

#[tokio::test]
async fn linking_to_a_missing_memory_is_a_clean_not_found_naming_the_id() {
    let h = harness();
    let a = store(&h, "present").await;
    let missing = a + 1_000;

    let outcome = h
        .service
        .link(a, missing, LinkKind::CausedBy)
        .await
        .expect("a missing endpoint is data, never an FK-violation error");
    assert_eq!(
        outcome,
        LinkOutcome::NotFound {
            missing_id: missing
        }
    );

    // A missing FROM id is named too (the first missing id is reported).
    let outcome = h
        .service
        .link(missing, a, LinkKind::CausedBy)
        .await
        .expect("missing from_id is data");
    assert_eq!(
        outcome,
        LinkOutcome::NotFound {
            missing_id: missing
        }
    );
    assert_eq!(edge_count(&h.db_path), 0, "nothing was written");
}

#[tokio::test]
async fn unlink_removes_an_edge_and_a_missing_edge_is_a_clean_not_found() {
    let h = harness();
    let a = store(&h, "from").await;
    let b = store(&h, "to").await;
    h.service
        .link(a, b, LinkKind::Supersedes)
        .await
        .expect("link");
    assert_eq!(edge_count(&h.db_path), 1);

    // Wrong direction / wrong kind do not match the directed edge.
    assert!(!h
        .service
        .unlink(b, a, LinkKind::Supersedes)
        .await
        .expect("reverse direction is not-found data"));
    assert!(!h
        .service
        .unlink(a, b, LinkKind::RelatesTo)
        .await
        .expect("other kind is not-found data"));
    assert_eq!(edge_count(&h.db_path), 1);

    assert!(h
        .service
        .unlink(a, b, LinkKind::Supersedes)
        .await
        .expect("unlink succeeds"));
    assert_eq!(edge_count(&h.db_path), 0);

    // Unlinking it again is the clean not-found shape, never an error.
    assert!(!h
        .service
        .unlink(a, b, LinkKind::Supersedes)
        .await
        .expect("missing edge is data"));
}

#[tokio::test]
async fn forget_cascades_every_edge_touching_the_memory() {
    let h = harness();
    let a = store(&h, "doomed").await;
    let b = store(&h, "survivor b").await;
    let c = store(&h, "survivor c").await;

    // Both directions, mixed kinds, plus one edge not touching `a`.
    h.service
        .link(a, b, LinkKind::Supersedes)
        .await
        .expect("a>b");
    h.service.link(b, a, LinkKind::CausedBy).await.expect("b>a");
    h.service
        .link(c, a, LinkKind::RelatesTo)
        .await
        .expect("c>a");
    h.service
        .link(b, c, LinkKind::RelatesTo)
        .await
        .expect("b>c");
    assert_eq!(edges_touching(&h.db_path, a), 3);

    assert!(h.service.forget(a).await.expect("forget"));

    assert_eq!(edges_touching(&h.db_path, a), 0, "no edge references a");
    assert_eq!(edge_count(&h.db_path), 1, "the unrelated edge b>c survives");
    assert_eq!(orphan_edges(&h.db_path), 0);
}

#[tokio::test]
async fn a_ttl_sweep_cascades_the_expired_memorys_edges() {
    let h = harness();
    let expiring = h
        .service
        .store(new_memory("short-lived", Some(10)))
        .await
        .expect("store expiring");
    let durable = store(&h, "long-lived").await;
    h.service
        .link(expiring, durable, LinkKind::CausedBy)
        .await
        .expect("out edge");
    h.service
        .link(durable, expiring, LinkKind::Supersedes)
        .await
        .expect("in edge");
    assert_eq!(edge_count(&h.db_path), 2);

    h.clock.advance(100); // past expires_at = 1_010
    let report = h.service.sweep(h.clock.now()).await.expect("sweep");
    assert_eq!(report.expired, 1);

    assert_eq!(edges_touching(&h.db_path, expiring), 0);
    assert_eq!(edge_count(&h.db_path), 0);
    assert_eq!(orphan_edges(&h.db_path), 0);
}

/// Simulate a connection that skipped `PRAGMA foreign_keys = ON` (milestone
/// Pitfall 9): delete a memory row with FK enforcement off, which leaves its
/// edges orphaned exactly as such a connection would.
fn orphan_by_raw_delete(db_path: &Path, id: i64) {
    let conn = raw_conn(db_path);
    conn.pragma_update(None, "foreign_keys", "OFF")
        .expect("fk off");
    conn.execute("DELETE FROM memories WHERE id = ?1", [id])
        .expect("raw delete");
}

#[tokio::test]
async fn forget_also_clears_orphans_left_by_a_connection_without_the_fk_pragma() {
    let h = harness();
    let a = store(&h, "orphaned by a pragma-less connection").await;
    let b = store(&h, "partner").await;
    let c = store(&h, "forgotten normally").await;
    h.service
        .link(a, b, LinkKind::RelatesTo)
        .await
        .expect("a>b");
    orphan_by_raw_delete(&h.db_path, a);
    assert_eq!(orphan_edges(&h.db_path), 1, "precondition: one orphan edge");

    // Forgetting an UNRELATED memory runs the belt-and-braces delete.
    assert!(h.service.forget(c).await.expect("forget"));
    assert_eq!(orphan_edges(&h.db_path), 0);
}

#[tokio::test]
async fn sweep_also_clears_orphans_left_by_a_connection_without_the_fk_pragma() {
    let h = harness();
    let a = store(&h, "orphaned by a pragma-less connection").await;
    let b = store(&h, "partner").await;
    h.service.link(b, a, LinkKind::CausedBy).await.expect("b>a");
    orphan_by_raw_delete(&h.db_path, a);
    assert_eq!(orphan_edges(&h.db_path), 1, "precondition: one orphan edge");

    // A sweep with nothing to expire still runs the explicit cleanup.
    let report = h.service.sweep(h.clock.now()).await.expect("sweep");
    assert_eq!(report.expired, 0);
    assert_eq!(orphan_edges(&h.db_path), 0);
}
