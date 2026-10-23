//! MCP-07: flat typed relations between memories — `link` / `unlink` with the
//! closed three-kind vocabulary, cascade on every deletion path, and opt-in
//! 1-hop expansion on search/list.
//!
//! Locked decisions proven here: D-01 (closed `kind` enum: relates_to,
//! supersedes, caused_by — anything else is `InvalidArgument` naming the
//! allowed set), D-02 (duplicate link = idempotent no-op success, self-link =
//! `InvalidArgument`, missing endpoint / missing edge = clean not-found data,
//! never an error), D-03 (one directed edge row per link, read in both
//! directions with natural reverse labels), D-04 (`expand_links` is opt-in and
//! the flag-less envelope is byte-identical to v1.0), D-05 (trimmed neighbor
//! summaries), D-06 (expanded neighbors are never access-bumped) and D-07
//! (at most `MAX_EXPANDED_LINKS` neighbors, most-recent edges win).
//!
//! Harness: a REAL on-disk temp DB, an injected `TestClock`, and a failing
//! `FakeEmbedder` (stores land pending; search deterministically takes the
//! FTS5 keyword path). Edge rows are counted through a raw side connection so
//! the assertions see the table itself, not the service's view of it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{
    LinkDirection, LinkKind, LinkOutcome, MemoryError, MemoryType, MemoryView, NewMemory,
    RelatedLink, MAX_EXPANDED_LINKS,
};
use agent_memory_core::embed::{Embedder, FakeEmbedder};
use agent_memory_core::service::{ListArgs, MemoryService, SearchArgs, SearchMode};
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

// ---------------------------------------------------------------------------
// Task 2 — 1-hop expansion via expand_links
// ---------------------------------------------------------------------------

fn search_args(query: &str, expand_links: bool) -> SearchArgs {
    SearchArgs {
        query: query.to_string(),
        expand_links,
        ..Default::default()
    }
}

fn list_args(expand_links: bool) -> ListArgs {
    ListArgs {
        expand_links,
        ..Default::default()
    }
}

fn view_of(views: &[MemoryView], id: i64) -> &MemoryView {
    views
        .iter()
        .find(|v| v.id == id)
        .unwrap_or_else(|| panic!("id {id} missing from {views:?}"))
}

fn related_of(views: &[MemoryView], id: i64) -> &[RelatedLink] {
    view_of(views, id)
        .related
        .as_deref()
        .unwrap_or_else(|| panic!("id {id} carries no related list"))
}

#[tokio::test]
async fn expansion_follows_one_edge_both_ways_with_natural_labels() {
    let h = harness();
    let newer = store(&h, "orbit decision newer").await;
    let older = store(&h, "orbit decision older").await;
    let cause = store(&h, "orbit root cause").await;
    h.service
        .link(newer, older, LinkKind::Supersedes)
        .await
        .expect("newer supersedes older");
    h.service
        .link(newer, cause, LinkKind::CausedBy)
        .await
        .expect("newer caused_by cause");

    let outcome = h
        .service
        .search(search_args("orbit", true))
        .await
        .expect("search");
    assert_eq!(outcome.search_mode, SearchMode::Keyword);
    let results = &outcome.results;

    // The edge's own endpoint reads the stored kind, outgoing.
    let newer_related = related_of(results, newer);
    assert_eq!(newer_related.len(), 2);
    let to_older = newer_related
        .iter()
        .find(|r| r.id == older)
        .expect("older is a neighbor of newer");
    assert_eq!(to_older.kind, "supersedes");
    assert_eq!(to_older.direction, LinkDirection::Out);
    assert_eq!(to_older.mem_type, MemoryType::Decision);
    assert_eq!(to_older.snippet, "orbit decision older");
    assert_eq!(to_older.tags, vec!["rel".to_string()]);
    let to_cause = newer_related
        .iter()
        .find(|r| r.id == cause)
        .expect("cause is a neighbor of newer");
    assert_eq!(to_cause.kind, "caused_by");
    assert_eq!(to_cause.direction, LinkDirection::Out);

    // The SAME single edge row read from the far side: natural reverse labels.
    let older_related = related_of(results, older);
    assert_eq!(older_related.len(), 1);
    assert_eq!(older_related[0].id, newer);
    assert_eq!(older_related[0].kind, "superseded_by");
    assert_eq!(older_related[0].direction, LinkDirection::In);
    let cause_related = related_of(results, cause);
    assert_eq!(cause_related.len(), 1);
    assert_eq!(cause_related[0].id, newer);
    assert_eq!(cause_related[0].kind, "caused");
    assert_eq!(cause_related[0].direction, LinkDirection::In);

    assert_eq!(edge_count(&h.db_path), 2, "still one row per edge (D-03)");
}

#[tokio::test]
async fn relates_to_reads_the_same_from_both_ends() {
    let h = harness();
    let a = store(&h, "peer one").await;
    let b = store(&h, "peer two").await;
    h.service
        .link(a, b, LinkKind::RelatesTo)
        .await
        .expect("link");

    let views = h.service.list(list_args(true)).await.expect("list");
    let from_a = &related_of(&views, a)[0];
    let from_b = &related_of(&views, b)[0];
    assert_eq!((from_a.id, from_a.kind.as_str()), (b, "relates_to"));
    assert_eq!(from_a.direction, LinkDirection::Out);
    assert_eq!((from_b.id, from_b.kind.as_str()), (a, "relates_to"));
    assert_eq!(from_b.direction, LinkDirection::In);
}

#[tokio::test]
async fn flag_less_envelopes_are_byte_identical_to_v1_and_never_mention_related() {
    let h = harness();
    let alpha = store(&h, "golden alpha").await;
    let partner = store(&h, "golden partner").await;
    h.service
        .link(alpha, partner, LinkKind::Supersedes)
        .await
        .expect("link");
    assert_eq!((alpha, partner), (1, 2), "fresh DB: ids are 1 and 2");

    // Golden v1.0 envelopes — captured from the pre-expansion wire shape.
    // Edges exist, yet without the flag nothing about them may appear.
    let list_golden = concat!(
        r#"[{"id":2,"content":"golden partner","type":"DECISION","tags":["rel"],"#,
        r#""scope":null,"decay_score":1.0,"created_at":1000,"last_accessed":1000},"#,
        r#"{"id":1,"content":"golden alpha","type":"DECISION","tags":["rel"],"#,
        r#""scope":null,"decay_score":1.0,"created_at":1000,"last_accessed":1000}]"#
    );
    let search_golden = concat!(
        r#"{"search_mode":"keyword","results":[{"id":1,"content":"golden alpha","#,
        r#""type":"DECISION","tags":["rel"],"scope":null,"decay_score":1.0,"#,
        r#""created_at":1000,"last_accessed":1000}]}"#
    );

    let listed = h.service.list(list_args(false)).await.expect("list");
    let list_json = serde_json::to_string(&listed).expect("serialize list");
    assert_eq!(list_json, list_golden);
    assert!(!list_json.contains("related"));

    let found = h
        .service
        .search(search_args("alpha", false))
        .await
        .expect("search");
    let search_json = serde_json::to_string(&found).expect("serialize search");
    assert_eq!(search_json, search_golden);
    assert!(!search_json.contains("related"));

    // With the flag the same query carries the trimmed summary, in D-05
    // field order, after the v1.0 fields.
    let expanded = h
        .service
        .search(search_args("alpha", true))
        .await
        .expect("expanded search");
    let expanded_json = serde_json::to_string(&expanded).expect("serialize");
    assert_eq!(
        expanded_json,
        concat!(
            r#"{"search_mode":"keyword","results":[{"id":1,"content":"golden alpha","#,
            r#""type":"DECISION","tags":["rel"],"scope":null,"decay_score":1.0,"#,
            r#""created_at":1000,"last_accessed":1000,"related":[{"id":2,"#,
            r#""kind":"supersedes","direction":"out","type":"DECISION","#,
            r#""snippet":"golden partner","tags":["rel"]}]}]}"#
        )
    );
}

#[tokio::test]
async fn flag_on_without_edges_is_an_explicit_empty_related_list() {
    let h = harness();
    let lone = store(&h, "unlinked memory").await;

    let views = h.service.list(list_args(true)).await.expect("list");
    assert_eq!(view_of(&views, lone).related, Some(vec![]));
    let json = serde_json::to_string(&views).expect("serialize");
    assert!(json.contains(r#""related":[]"#), "{json}");
}

#[tokio::test]
async fn expansion_is_capped_and_the_most_recent_edges_win() {
    let h = harness();
    let anchor = store(&h, "hub memory").await;
    let mut neighbors = Vec::new();
    for i in 0..(MAX_EXPANDED_LINKS + 2) {
        let n = store(&h, &format!("spoke {i}")).await;
        // Stagger created_at so "most recent" is unambiguous; alternate the
        // direction so the cap orders both directions together.
        h.clock.advance(1);
        if i % 2 == 0 {
            h.service
                .link(anchor, n, LinkKind::RelatesTo)
                .await
                .expect("out link");
        } else {
            h.service
                .link(n, anchor, LinkKind::CausedBy)
                .await
                .expect("in link");
        }
        neighbors.push(n);
    }
    assert_eq!(MAX_EXPANDED_LINKS, 10, "D-07 cap value");

    let views = h.service.list(list_args(true)).await.expect("list");
    let related = related_of(&views, anchor);
    assert_eq!(related.len(), MAX_EXPANDED_LINKS);

    // The two OLDEST edges are the ones dropped; the rest come newest first.
    let expected: Vec<i64> = neighbors
        .iter()
        .rev()
        .take(MAX_EXPANDED_LINKS)
        .copied()
        .collect();
    let got: Vec<i64> = related.iter().map(|r| r.id).collect();
    assert_eq!(got, expected);
    assert!(!got.contains(&neighbors[0]) && !got.contains(&neighbors[1]));
}

#[test]
fn snippets_cut_at_160_chars_on_a_char_boundary_with_an_ellipsis() {
    use agent_memory_core::domain::{snippet, RELATED_SNIPPET_CHARS};
    assert_eq!(RELATED_SNIPPET_CHARS, 160);

    let short = "fits easily";
    assert_eq!(snippet(short), short, "no ellipsis when nothing is cut");

    let exact: String = "x".repeat(160);
    assert_eq!(snippet(&exact), exact, "exactly 160 chars is not cut");

    // Multi-byte content: a byte-index cut would split a codepoint.
    let long: String = "é".repeat(200);
    let cut = snippet(&long);
    assert!(cut.ends_with('…'));
    assert_eq!(cut.chars().count(), 161, "160 chars plus the ellipsis");
    assert_eq!(cut.trim_end_matches('…'), "é".repeat(160));
}

#[tokio::test]
async fn expanded_snippets_are_truncated_for_long_neighbor_content() {
    let h = harness();
    let anchor = store(&h, "short anchor").await;
    let long_content = format!("{} tail", "ünïcode ".repeat(30));
    let long = store(&h, &long_content).await;
    h.service
        .link(anchor, long, LinkKind::RelatesTo)
        .await
        .expect("link");

    let views = h.service.list(list_args(true)).await.expect("list");
    let entry = &related_of(&views, anchor)[0];
    assert!(entry.snippet.ends_with('…'), "{:?}", entry.snippet);
    assert_eq!(entry.snippet.chars().count(), 161);
    assert!(long_content.starts_with(entry.snippet.trim_end_matches('…')));
}

/// Raw `(access_count, last_accessed)` for one row.
fn access_of(db_path: &Path, id: i64) -> (i64, i64) {
    raw_conn(db_path)
        .query_row(
            "SELECT access_count, last_accessed FROM memories WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("row exists")
}

#[tokio::test]
async fn expanded_neighbors_are_never_access_bumped() {
    let h = harness();
    let anchor = store(&h, "needle anchor").await;
    let mut neighbors = Vec::new();
    for word in ["one", "two", "three"] {
        let n = store(&h, &format!("haystack {word}")).await;
        h.service
            .link(anchor, n, LinkKind::RelatesTo)
            .await
            .expect("link");
        neighbors.push(n);
    }

    h.clock.advance(500);
    let found = h
        .service
        .search(search_args("needle", true))
        .await
        .expect("search");
    assert_eq!(found.results.len(), 1, "only the anchor matches");
    assert_eq!(found.results[0].related.as_ref().map(Vec::len), Some(3));

    // The returned result's bump is fire-and-forget: wait for it to land
    // (bounded), so the neighbor assertions below are not vacuous.
    let mut bumped = false;
    for _ in 0..200 {
        if access_of(&h.db_path, anchor) == (1, 1_500) {
            bumped = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(bumped, "the returned result is still bumped as before");

    for n in neighbors {
        assert_eq!(
            access_of(&h.db_path, n),
            (0, 1_000),
            "neighbor {n} must not be bumped by expansion (D-06)"
        );
    }
}

#[tokio::test]
async fn expansion_also_applies_on_the_semantic_path() {
    // A succeeding hash-vector embedder: search takes the semantic leg.
    let h = harness_with(Arc::new(FakeEmbedder::with_vectors(
        std::collections::HashMap::new(),
    )));
    let a = store(&h, "vector anchor").await;
    let b = store(&h, "vector neighbor").await;
    h.service
        .link(b, a, LinkKind::Supersedes)
        .await
        .expect("link");

    let found = h
        .service
        .search(search_args("vector anchor", true))
        .await
        .expect("search");
    assert_eq!(found.search_mode, SearchMode::Hybrid);
    let entry = &related_of(&found.results, a)[0];
    assert_eq!((entry.id, entry.kind.as_str()), (b, "superseded_by"));
    assert_eq!(entry.direction, LinkDirection::In);

    let plain = h
        .service
        .search(search_args("vector anchor", false))
        .await
        .expect("search");
    assert!(plain.results.iter().all(|v| v.related.is_none()));
}
