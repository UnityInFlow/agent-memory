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
use agent_memory_core::domain::{ImportedMemory, MemoryType, NewMemory};
use agent_memory_core::embed::FakeEmbedder;
use agent_memory_core::service::{ListArgs, MemoryService, SearchArgs};
use agent_memory_core::store::sqlite::{register_vec_extension, SqliteStore};
use agent_memory_core::store::Store;

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
async fn list_tag_filter_is_exact_and_case_sensitive() {
    // API-03 / D-07..D-09 regression, list site: tag=rust must match ONLY the
    // exactly-rust-tagged row — never "rustling" (substring) and never via
    // "Rust" (case). tag=None keeps the NULL-disables-filter convention.
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(4_000));
    let service = service_for(&db_path, clock.clone());

    clock.advance(1);
    let mut rust_row = new_memory(MemoryType::Pattern, "the deploy pipeline for staging");
    rust_row.tags = vec!["rust".to_string()];
    let rust_id = service.store(rust_row).await.expect("store rust row");

    clock.advance(1);
    let mut rustling_row = new_memory(MemoryType::Pattern, "the release pipeline for production");
    rustling_row.tags = vec!["rustling".to_string()];
    service
        .store(rustling_row)
        .await
        .expect("store rustling row");

    let list_by_tag = |tag: Option<&str>| ListArgs {
        tag: tag.map(str::to_string),
        ..ListArgs::default()
    };

    let rows = service
        .list(list_by_tag(Some("rust")))
        .await
        .expect("tag-filtered list");
    assert_eq!(
        rows.len(),
        1,
        "tag=rust must match ONLY the exactly-rust-tagged row on the list path, got: {rows:?}"
    );
    assert_eq!(rows[0].id, rust_id);

    let rows = service
        .list(list_by_tag(Some("Rust")))
        .await
        .expect("case-mismatched tag list");
    assert!(
        rows.is_empty(),
        "tag matching must be case-sensitive: 'Rust' must not match 'rust' (D-08), got: {rows:?}"
    );

    let rows = service
        .list(list_by_tag(None))
        .await
        .expect("unfiltered list");
    assert_eq!(
        rows.len(),
        2,
        "tag=None must disable the filter and return both rows"
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

// ---------------------------------------------------------------------------
// Phase 5 store contract (D-07): the dedup probe returns the matching row id
// ---------------------------------------------------------------------------

#[test]
fn find_by_key_returns_the_matching_id_null_safely() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SqliteStore::open(&dir.path().join("memory.db")).expect("open store");

    // Row A has no source (stored NULL); row B has source "s".
    let id_a = store
        .insert(new_memory(MemoryType::Decision, "alpha"), None, 1_000)
        .expect("insert a");
    let mut b = new_memory(MemoryType::Decision, "beta");
    b.source = Some("s".to_string());
    let id_b = store.insert(b, None, 1_000).expect("insert b");

    assert_eq!(
        store
            .find_by_key(None, MemoryType::Decision, "alpha", 1_000)
            .expect("probe a"),
        Some(id_a),
        "a None source matches the stored NULL (CR-01)"
    );
    assert_eq!(
        store
            .find_by_key(Some("s"), MemoryType::Decision, "beta", 1_000)
            .expect("probe b"),
        Some(id_b)
    );
    assert_eq!(
        store
            .find_by_key(Some(""), MemoryType::Decision, "alpha", 1_000)
            .expect("probe empty source"),
        None,
        "NULL is not the empty string (CR-01)"
    );
    assert_eq!(
        store
            .find_by_key(None, MemoryType::Pattern, "alpha", 1_000)
            .expect("probe other type"),
        None,
        "mem_type is part of the key"
    );
    assert_eq!(
        store
            .find_by_key(None, MemoryType::Decision, "gamma", 1_000)
            .expect("probe other content"),
        None,
        "content is part of the key"
    );

    // 05-REVIEW WR-03: only LIVE rows match. A row whose TTL lapsed with no
    // sweep run is still in the table but must be invisible to the probe, on
    // the importer's own `expires_at <= now` boundary.
    let mut stale = new_memory(MemoryType::Decision, "delta");
    stale.ttl_secs = Some(500); // expires_at = 1_500
    let id_stale = store.insert(stale, None, 1_000).expect("insert stale");
    assert_eq!(
        store
            .find_by_key(None, MemoryType::Decision, "delta", 1_499)
            .expect("probe before expiry"),
        Some(id_stale),
        "live until expires_at"
    );
    assert_eq!(
        store
            .find_by_key(None, MemoryType::Decision, "delta", 1_500)
            .expect("probe at expiry"),
        None,
        "expires_at == now is already expired (the importer's `<=`)"
    );
    assert_eq!(
        store
            .find_by_key(None, MemoryType::Decision, "delta", 9_000)
            .expect("probe long after expiry, unswept"),
        None,
        "an expired-but-unswept row is never a dedup hit"
    );

    // 05-REVIEW IN-01: with duplicate live rows the LOWEST id wins, by
    // contract, not by scan order.
    let id_a2 = store
        .insert(new_memory(MemoryType::Decision, "alpha"), None, 1_000)
        .expect("insert second alpha");
    assert!(id_a2 > id_a);
    assert_eq!(
        store
            .find_by_key(None, MemoryType::Decision, "alpha", 1_000)
            .expect("probe duplicated key"),
        Some(id_a),
        "the lowest matching live id is returned"
    );
}

// ---------------------------------------------------------------------------
// Phase 5 store contract (D-10 / D-13): a restored row keeps its provenance,
// starts decay-fresh, and always lands pending for the sweep backfill
// ---------------------------------------------------------------------------

/// Every stored column of one `memories` row, read through a raw connection
/// so the assertions see the table itself, not a view of it.
#[derive(Debug)]
struct RawRow {
    mem_type: String,
    content: String,
    tags: String,
    source: Option<String>,
    scope: Option<String>,
    base_weight: f64,
    decay_score: f64,
    access_count: i64,
    created_at: i64,
    last_accessed: i64,
    expires_at: Option<i64>,
    embedding_status: i64,
}

fn raw_row(conn: &rusqlite::Connection, id: i64) -> RawRow {
    conn.query_row(
        "SELECT mem_type, content, tags, source, scope, base_weight, decay_score, \
         access_count, created_at, last_accessed, expires_at, embedding_status \
         FROM memories WHERE id = ?1",
        [id],
        |r| {
            Ok(RawRow {
                mem_type: r.get(0)?,
                content: r.get(1)?,
                tags: r.get(2)?,
                source: r.get(3)?,
                scope: r.get(4)?,
                base_weight: r.get(5)?,
                decay_score: r.get(6)?,
                access_count: r.get(7)?,
                created_at: r.get(8)?,
                last_accessed: r.get(9)?,
                expires_at: r.get(10)?,
                embedding_status: r.get(11)?,
            })
        },
    )
    .expect("read raw row")
}

fn vec_rows(conn: &rusqlite::Connection, id: i64) -> i64 {
    conn.query_row(
        "SELECT count(*) FROM vec_memories WHERE memory_id = ?1",
        [id],
        |r| r.get(0),
    )
    .expect("count vector rows")
}

#[tokio::test]
async fn insert_imported_preserves_provenance_and_lands_pending() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let store = Arc::new(SqliteStore::open(&db_path).expect("open store"));
    let now = 1_000;

    let id = store
        .insert_imported(
            ImportedMemory {
                content: "restored decision".to_string(),
                mem_type: MemoryType::Decision,
                tags: vec!["a".to_string(), "b".to_string()],
                source: Some("gsd-state".to_string()),
                scope: Some("proj".to_string()),
                created_at: 500,
                access_count: 7,
                expires_at: Some(9_000),
            },
            now,
        )
        .expect("insert imported row");

    register_vec_extension().expect("vec0 registers");
    let conn = rusqlite::Connection::open(&db_path).expect("open raw connection");
    let row = raw_row(&conn, id);
    assert_eq!(row.created_at, 500, "created_at is preserved (D-10)");
    assert_eq!(
        row.last_accessed, now,
        "last_accessed is refreshed to now (D-10)"
    );
    assert_eq!(row.access_count, 7, "access_count is preserved (D-10)");
    assert_eq!(
        row.expires_at,
        Some(9_000),
        "expires_at is preserved (D-10)"
    );
    assert_eq!(
        row.embedding_status, 0,
        "always pending for the sweep (D-13)"
    );
    assert_eq!(row.decay_score, 1.0);
    assert_eq!(row.base_weight, 2.0, "DECISION is a pinned type");
    assert_eq!(row.tags, "[\"a\",\"b\"]");
    assert_eq!(row.source.as_deref(), Some("gsd-state"));
    assert_eq!(row.scope.as_deref(), Some("proj"));
    assert_eq!(row.mem_type, "DECISION");
    assert_eq!(row.content, "restored decision");
    assert_eq!(
        vec_rows(&conn, id),
        0,
        "no vector row is ever written (D-13)"
    );

    // An unpinned type with every optional column absent.
    let id2 = store
        .insert_imported(
            ImportedMemory {
                content: "restored pattern".to_string(),
                mem_type: MemoryType::Pattern,
                tags: vec![],
                source: None,
                scope: None,
                created_at: 600,
                access_count: 0,
                expires_at: None,
            },
            now,
        )
        .expect("insert second imported row");
    let row2 = raw_row(&conn, id2);
    assert_eq!(row2.base_weight, 1.0, "PATTERN is not pinned");
    assert_eq!(row2.source, None);
    assert_eq!(row2.scope, None);
    assert_eq!(row2.expires_at, None);
    assert_eq!(row2.embedding_status, 0);

    // Visible through the normal read path: the FTS trigger fired on the
    // shared INSERT, so a keyword search finds the restored row at once.
    let clock = Arc::new(TestClock::new(now));
    let service = MemoryService::new(
        store,
        clock,
        Arc::new(FakeEmbedder::failing()),
        DecayConfig::default(),
    );
    let listed = service.list(ListArgs::default()).await.expect("list");
    assert!(
        listed
            .iter()
            .any(|v| v.id == id && v.content == "restored decision"),
        "the restored row is listed: {listed:?}"
    );
    let found = service
        .search(SearchArgs {
            query: "restored".to_string(),
            ..SearchArgs::default()
        })
        .await
        .expect("keyword search");
    assert!(
        found.results.iter().any(|v| v.id == id),
        "the restored row is keyword-searchable: {:?}",
        found.results
    );
}

#[test]
fn bump_access_saturates_at_i64_max_and_keeps_the_column_an_integer() {
    // 05-REVIEW WR-02: an imported row carries its access_count verbatim
    // (D-10), so i64::MAX is a legal stored value. SQLite's `+` would overflow
    // it to REAL (no error), and every later export_rows read of the column
    // would then fail with InvalidColumnType. The bump must saturate instead.
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let store = SqliteStore::open(&db_path).expect("open store");

    let maxed = store
        .insert_imported(
            ImportedMemory {
                content: "counted to the brim".to_string(),
                mem_type: MemoryType::Pattern,
                tags: vec![],
                source: None,
                scope: None,
                created_at: 500,
                access_count: i64::MAX,
                expires_at: None,
            },
            1_000,
        )
        .expect("insert maxed row");
    let ordinary = store
        .insert(
            new_memory(MemoryType::Pattern, "counted normally"),
            None,
            1_000,
        )
        .expect("insert ordinary row");

    store
        .bump_access(&[maxed, ordinary], 2_000)
        .expect("bump both rows");

    register_vec_extension().expect("vec0 registers");
    let conn = rusqlite::Connection::open(&db_path).expect("open raw connection");
    let (maxed_count, maxed_type): (i64, String) = conn
        .query_row(
            "SELECT access_count, typeof(access_count) FROM memories WHERE id = ?1",
            [maxed],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("read maxed row");
    assert_eq!(maxed_count, i64::MAX, "the bump saturates, never wraps");
    assert_eq!(maxed_type, "integer", "the column never degrades to REAL");
    assert_eq!(
        raw_row(&conn, maxed).last_accessed,
        2_000,
        "recency still bumps"
    );
    assert_eq!(
        raw_row(&conn, ordinary).access_count,
        1,
        "an ordinary row still increments"
    );
}
