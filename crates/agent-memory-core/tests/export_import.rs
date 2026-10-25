//! DIST-04 / INTEROP-02 proof file: the portable export (format v1 JSONL) and
//! its import.
//!
//! Export decisions proven here: D-03 (line 1 is the header, read from the
//! `meta` pin and the injected clock, with `agent_memory_version` supplied by
//! the caller), D-04 (a memory record carries exactly the eleven documented
//! keys — no derived data, no vector), D-05 (links after all memories, as
//! stored), D-06 (every row, expired rows included, ascending id) and the
//! canonical-output rule (two exports of an unchanged database are
//! byte-identical).
//!
//! Import decisions proven here: D-07 (dedup on the unchanged `(source,
//! mem_type, content)` key; identical rows collapse; `source` preserved
//! verbatim), D-08 (two-pass validate-before-write; export-local keys are
//! remapped through a `key → id` map so a non-empty target with overlapping
//! rowids never gains a phantom edge — PITFALLS 18; links whose endpoint was
//! skipped are dropped and counted), D-10 (`created_at` / `access_count` /
//! `expires_at` preserved, `last_accessed` refreshed, `expires_at <= now`
//! skipped at the boundary — PITFALLS 19), D-11 (nothing is written when
//! validation fails), D-12 (the six-count report) and D-13 (rows land pending
//! and only the sweep re-embeds them, after which they are hybrid-searchable).
//!
//! Harness: a REAL on-disk temp DB, an injected `TestClock`, and a failing
//! `FakeEmbedder` by default (stores land pending; neither the exporter nor
//! the jsonl importer touches the embedder). The export is written through a
//! real file sink under the harness's temp dir and read back, so the
//! boxed-writer path is exercised end to end. Round trips use TWO harnesses —
//! a source and a target — each with its own database and clock. Stored
//! columns and edge rows are read through a raw side connection so the
//! assertions see the tables themselves.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{LinkKind, LinkOutcome, MemoryError, MemoryType, NewMemory};
use agent_memory_core::embed::{Embedder, FakeEmbedder};
use agent_memory_core::export::{Record, FORMAT_NAME, FORMAT_VERSION};
use agent_memory_core::import::jsonl::{parse_jsonl, JsonlFile};
use agent_memory_core::service::{
    ExportReport, ImportReport, ListArgs, MemoryService, SearchArgs, SearchMode,
};
use agent_memory_core::store::migrations::LATEST_SCHEMA_VERSION;
use agent_memory_core::store::sqlite::{register_vec_extension, SqliteStore};

/// The literal passed as `agent_memory_version` — the core crate has no say
/// in the binary's version (RESEARCH Pitfall 9), so the test pins what it
/// passed in.
const TEST_VERSION: &str = "test-0.0.0";

struct Harness {
    service: MemoryService,
    clock: Arc<TestClock>,
    db_path: PathBuf,
    dir: tempfile::TempDir,
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
        dir,
    }
}

fn new_memory(content: &str, mem_type: MemoryType) -> NewMemory {
    NewMemory {
        content: content.to_string(),
        mem_type,
        tags: Vec::new(),
        source: None,
        scope: None,
        ttl_secs: None,
    }
}

async fn store(h: &Harness, new: NewMemory) -> i64 {
    h.service.store(new).await.expect("store succeeds")
}

async fn link(h: &Harness, from: i64, to: i64, kind: LinkKind) {
    let outcome = h.service.link(from, to, kind).await.expect("link succeeds");
    assert_eq!(outcome, LinkOutcome::Linked { created: true });
}

/// Export through a real file sink under the harness dir and read it back.
async fn export_to_string(h: &Harness, name: &str) -> (String, ExportReport) {
    let path = h.dir.path().join(name);
    let file = std::fs::File::create(&path).expect("create export sink");
    let sink: Box<dyn std::io::Write + Send> = Box::new(std::io::BufWriter::new(file));
    let report = h
        .service
        .export(sink, TEST_VERSION.to_string())
        .await
        .expect("export succeeds");
    let text = std::fs::read_to_string(&path).expect("read export back");
    (text, report)
}

/// Parse every line of an export as a typed record.
fn lines(text: &str) -> Vec<Record> {
    text.lines()
        .enumerate()
        .map(|(i, line)| {
            serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("line {} is not a record: {e}\n{line}", i + 1))
        })
        .collect()
}

#[tokio::test]
async fn export_of_an_empty_database_is_exactly_one_header_line() {
    let h = harness();

    let (text, report) = export_to_string(&h, "empty.jsonl").await;

    assert_eq!(text.lines().count(), 1, "exactly one line:\n{text}");
    assert!(
        text.ends_with('\n'),
        "the header line is newline-terminated"
    );
    let records = lines(&text);
    match &records[0] {
        Record::Header {
            format,
            version,
            exported_at,
            embedding_model,
            embedding_dim,
            schema_version,
            agent_memory_version,
            exported_rows,
            exported_links,
        } => {
            assert_eq!(format, FORMAT_NAME);
            assert_eq!(format, "agent-memory-export");
            assert_eq!(*version, FORMAT_VERSION);
            assert_eq!(*version, 1);
            assert_eq!(*exported_at, 1_000, "exported_at comes from the clock");
            assert_eq!(embedding_model, "nomic-embed-text", "read from meta");
            assert_eq!(*embedding_dim, 768, "read from meta");
            assert_eq!(*schema_version, LATEST_SCHEMA_VERSION);
            assert_eq!(*schema_version, 3);
            assert_eq!(agent_memory_version, TEST_VERSION);
            assert_eq!(*exported_rows, 0);
            assert_eq!(*exported_links, 0);
        }
        other => panic!("line 1 must be the header, got {other:?}"),
    }
    assert_eq!(
        report,
        ExportReport {
            memories: 0,
            links: 0
        }
    );
}

#[tokio::test]
async fn export_writes_every_memory_ascending_then_every_link_as_stored() {
    let h = harness();

    // Three memories at three clock values: one fully decorated with a
    // far-future TTL, one of a different type, one bare.
    let a = store(
        &h,
        NewMemory {
            content: "decided to export JSONL".to_string(),
            mem_type: MemoryType::Decision,
            tags: vec!["export".to_string(), "format".to_string()],
            source: Some("unit-test".to_string()),
            scope: Some("phase-5".to_string()),
            ttl_secs: Some(1_000_000),
        },
    )
    .await;
    h.clock.advance(10);
    let b = store(
        &h,
        new_memory("stream one line at a time", MemoryType::Pattern),
    )
    .await;
    h.clock.advance(10);
    let c = store(&h, new_memory("no tags here", MemoryType::Todo)).await;
    assert!(a < b && b < c, "ids ascend with insertion order");

    h.clock.advance(5);
    let supersedes_at = h.clock.now();
    link(&h, a, b, LinkKind::Supersedes).await;
    h.clock.advance(5);
    let relates_at = h.clock.now();
    // Linked c -> b: stored canonically as (min, max) = (b, c).
    link(&h, c, b, LinkKind::RelatesTo).await;

    let (text, report) = export_to_string(&h, "three.jsonl").await;
    let records = lines(&text);
    assert_eq!(records.len(), 6, "header + 3 memories + 2 links:\n{text}");

    match &records[0] {
        Record::Header {
            exported_rows,
            exported_links,
            exported_at,
            ..
        } => {
            assert_eq!(*exported_rows, 3);
            assert_eq!(*exported_links, 2);
            assert_eq!(*exported_at, h.clock.now());
        }
        other => panic!("line 1 must be the header, got {other:?}"),
    }

    // The rows as the service itself reads them, oldest first to match
    // ascending id.
    let mut views = h
        .service
        .list(ListArgs::default())
        .await
        .expect("list succeeds");
    views.sort_by_key(|v| v.id);
    assert_eq!(views.len(), 3);

    let expected_sources = [Some("unit-test".to_string()), None, None];
    let expected_expiry = [Some(1_000 + 1_000_000), None, None];
    for (i, rec) in records[1..4].iter().enumerate() {
        let view = &views[i];
        match rec {
            Record::Memory {
                key,
                mem_type,
                content,
                tags,
                source,
                scope,
                created_at,
                last_accessed,
                access_count,
                expires_at,
            } => {
                assert_eq!(*key, view.id, "key is the stored id");
                assert_eq!(mem_type, view.mem_type.as_wire_str());
                assert_eq!(content, &view.content);
                assert_eq!(tags, &view.tags);
                assert_eq!(source, &expected_sources[i]);
                assert_eq!(scope, &view.scope);
                assert_eq!(*created_at, view.created_at);
                assert_eq!(*last_accessed, view.last_accessed);
                assert_eq!(*access_count, 0, "fresh rows have never been accessed");
                assert_eq!(*expires_at, expected_expiry[i]);
            }
            other => panic!("line {} must be a memory, got {other:?}", i + 2),
        }
    }
    // Ascending key order, strictly.
    let keys: Vec<i64> = records[1..4]
        .iter()
        .map(|r| match r {
            Record::Memory { key, .. } => *key,
            other => panic!("expected memory, got {other:?}"),
        })
        .collect();
    assert_eq!(keys, vec![a, b, c]);

    // Links after all memories, ordered by (from, to, kind), endpoints as
    // stored: supersedes keeps request order, relates_to is (min, max).
    assert_eq!(
        records[4],
        Record::Link {
            from: a,
            to: b,
            kind: "supersedes".to_string(),
            created_at: supersedes_at,
        }
    );
    assert_eq!(
        records[5],
        Record::Link {
            from: b.min(c),
            to: b.max(c),
            kind: "relates_to".to_string(),
            created_at: relates_at,
        }
    );

    assert_eq!(
        report,
        ExportReport {
            memories: 3,
            links: 2
        }
    );
}

#[tokio::test]
async fn memory_records_carry_exactly_the_documented_keys() {
    let h = harness();
    store(
        &h,
        NewMemory {
            content: "fully decorated".to_string(),
            mem_type: MemoryType::Architecture,
            tags: vec!["t".to_string()],
            source: Some("s".to_string()),
            scope: Some("sc".to_string()),
            ttl_secs: Some(500),
        },
    )
    .await;
    store(&h, new_memory("bare", MemoryType::Error)).await;

    let (text, _) = export_to_string(&h, "keys.jsonl").await;
    let memory_lines: Vec<&str> = text.lines().skip(1).collect();
    assert_eq!(memory_lines.len(), 2);

    let documented = [
        "record",
        "key",
        "type",
        "content",
        "tags",
        "source",
        "scope",
        "created_at",
        "last_accessed",
        "access_count",
        "expires_at",
    ];
    for line in memory_lines {
        let value: serde_json::Value = serde_json::from_str(line).expect("memory line is JSON");
        let object = value.as_object().expect("memory line is an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected = documented.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected, "exactly the 11 documented keys:\n{line}");
        for derived in [
            "decay_score",
            "base_weight",
            "embedding_status",
            "embedding",
        ] {
            assert!(
                !object.contains_key(derived),
                "{derived} is derived data and must never travel:\n{line}"
            );
        }
        // Optional fields are written explicitly, never omitted.
        assert!(object.contains_key("source") && object.contains_key("expires_at"));
    }
}

#[tokio::test]
async fn expired_rows_are_still_exported() {
    let h = harness();
    let id = store(
        &h,
        NewMemory {
            content: "short lived".to_string(),
            mem_type: MemoryType::Todo,
            tags: Vec::new(),
            source: None,
            scope: None,
            ttl_secs: Some(10),
        },
    )
    .await;
    // Well past expiry, and NO sweep has run: the row is still in the table,
    // and export writes what is in the table (D-06 — expiry is the importer's
    // policy, not the exporter's).
    h.clock.advance(100);

    let (text, report) = export_to_string(&h, "expired.jsonl").await;
    let records = lines(&text);
    assert_eq!(records.len(), 2, "header + the expired row:\n{text}");
    match &records[1] {
        Record::Memory {
            key, expires_at, ..
        } => {
            assert_eq!(*key, id);
            assert_eq!(*expires_at, Some(1_000 + 10));
        }
        other => panic!("expected the expired memory, got {other:?}"),
    }
    assert_eq!(report.memories, 1);
}

#[tokio::test]
async fn two_exports_of_an_unchanged_database_are_byte_identical() {
    let h = harness();
    let a = store(&h, new_memory("first", MemoryType::Decision)).await;
    let b = store(&h, new_memory("second", MemoryType::Pattern)).await;
    link(&h, a, b, LinkKind::CausedBy).await;

    let (first, first_report) = export_to_string(&h, "one.jsonl").await;
    let (second, second_report) = export_to_string(&h, "two.jsonl").await;

    assert_eq!(first, second);
    assert_eq!(first_report, second_report);
    assert_eq!(first.lines().count(), 4);
}

// ---------------------------------------------------------------------------
// Import half (plan 05-03) — helpers
// ---------------------------------------------------------------------------

fn raw_conn(db_path: &Path) -> rusqlite::Connection {
    register_vec_extension().expect("vec0 registers");
    rusqlite::Connection::open(db_path).expect("open raw connection")
}

/// Total rows in `memories`.
fn row_count(db_path: &Path) -> i64 {
    raw_conn(db_path)
        .query_row("SELECT count(*) FROM memories", [], |row| row.get(0))
        .expect("count memories")
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

/// `embedding_status` of one row.
fn status(db_path: &Path, id: i64) -> i64 {
    raw_conn(db_path)
        .query_row(
            "SELECT embedding_status FROM memories WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .expect("read embedding_status")
}

/// Vector rows for one memory id.
fn vec_rows(db_path: &Path, id: i64) -> i64 {
    raw_conn(db_path)
        .query_row(
            "SELECT count(*) FROM vec_memories WHERE memory_id = ?1",
            [id],
            |row| row.get(0),
        )
        .expect("count vector rows")
}

/// The id of the (single) row with exactly this content.
fn id_by_content(db_path: &Path, content: &str) -> i64 {
    raw_conn(db_path)
        .query_row(
            "SELECT id FROM memories WHERE content = ?1",
            [content],
            |row| row.get(0),
        )
        .unwrap_or_else(|e| panic!("exactly one row with content {content:?}: {e}"))
}

/// The provenance columns the importer must preserve or refresh (D-10), read
/// straight from the table.
#[derive(Debug, PartialEq, Eq)]
struct Provenance {
    source: Option<String>,
    created_at: i64,
    last_accessed: i64,
    access_count: i64,
    expires_at: Option<i64>,
}

fn provenance(db_path: &Path, id: i64) -> Provenance {
    raw_conn(db_path)
        .query_row(
            "SELECT source, created_at, last_accessed, access_count, expires_at \
             FROM memories WHERE id = ?1",
            [id],
            |row| {
                Ok(Provenance {
                    source: row.get(0)?,
                    created_at: row.get(1)?,
                    last_accessed: row.get(2)?,
                    access_count: row.get(3)?,
                    expires_at: row.get(4)?,
                })
            },
        )
        .expect("read provenance columns")
}

/// `(from_id, to_id, kind, created_at)` of every edge, in storage order.
fn edges(db_path: &Path) -> Vec<(i64, i64, String, i64)> {
    let conn = raw_conn(db_path);
    let mut stmt = conn
        .prepare(
            "SELECT from_id, to_id, kind, created_at FROM memory_links \
             ORDER BY from_id, to_id, kind",
        )
        .expect("prepare edges");
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("query edges");
    rows.map(|r| r.expect("edge row")).collect()
}

/// Parse a file the strict way and import it into `h`.
async fn import(h: &Harness, text: &str) -> ImportReport {
    let file: JsonlFile = parse_jsonl(text).expect("export parses as format v1");
    h.service
        .import_jsonl(file)
        .await
        .expect("import_jsonl succeeds")
}

/// Every line as a JSON value with the fields that legitimately differ between
/// a source export and a re-export of its import nulled out: `key` and link
/// `from`/`to` (the target assigns its own ids), `last_accessed` (refreshed on
/// import, D-10) and the header's `exported_at` / `agent_memory_version`.
fn normalised(text: &str) -> Vec<serde_json::Value> {
    text.lines()
        .map(|line| {
            let mut value: serde_json::Value = serde_json::from_str(line).expect("json line");
            if let Some(object) = value.as_object_mut() {
                for field in [
                    "key",
                    "last_accessed",
                    "from",
                    "to",
                    "exported_at",
                    "agent_memory_version",
                ] {
                    if object.contains_key(field) {
                        object.insert(field.to_string(), serde_json::Value::Null);
                    }
                }
            }
            value
        })
        .collect()
}

fn six(
    imported: usize,
    skipped_duplicates: usize,
    skipped_expired: usize,
    links_imported: usize,
    links_skipped_duplicates: usize,
    links_dropped: usize,
) -> ImportReport {
    ImportReport {
        imported,
        skipped_duplicates,
        skipped_expired,
        links_imported,
        links_skipped_duplicates,
        links_dropped,
    }
}

// ---------------------------------------------------------------------------
// Import half — Task 1 (tracer): round trip + idempotency
// ---------------------------------------------------------------------------

/// Source fixture shared by the round-trip proofs: three decorated memories
/// stored at distinct clock values, two links of two kinds. Returns the
/// source ids `(a, b, c)` and the two link times `(supersedes_at, relates_at)`.
async fn seed_round_trip_source(src: &Harness) -> ((i64, i64, i64), (i64, i64)) {
    let a = store(
        src,
        NewMemory {
            content: "alpha decided the export format".to_string(),
            mem_type: MemoryType::Decision,
            tags: vec!["x".to_string(), "y".to_string()],
            source: Some("gsd-state".to_string()),
            scope: Some("proj".to_string()),
            ttl_secs: Some(1_000_000),
        },
    )
    .await;
    src.clock.advance(10);
    let b = store(
        src,
        new_memory("beta streams one line at a time", MemoryType::Pattern),
    )
    .await;
    src.clock.advance(10);
    let c = store(
        src,
        NewMemory {
            content: "gamma supersedes nothing yet".to_string(),
            mem_type: MemoryType::Decision,
            tags: Vec::new(),
            source: Some("manual".to_string()),
            scope: None,
            ttl_secs: None,
        },
    )
    .await;
    src.clock.advance(5);
    let supersedes_at = src.clock.now();
    link(src, a, b, LinkKind::Supersedes).await;
    src.clock.advance(5);
    let relates_at = src.clock.now();
    link(src, c, b, LinkKind::RelatesTo).await;
    ((a, b, c), (supersedes_at, relates_at))
}

#[tokio::test]
async fn round_trip_into_an_empty_database_recreates_memories_and_links() {
    let src = harness();
    let ((a, b, c), (supersedes_at, relates_at)) = seed_round_trip_source(&src).await;
    let (text, export_report) = export_to_string(&src, "source.jsonl").await;
    assert_eq!(
        export_report,
        ExportReport {
            memories: 3,
            links: 2
        }
    );

    // The target is another machine: its own empty database and a clock far
    // from the source's, so the D-10 refresh is observable.
    let dst = harness();
    dst.clock.set(5_000);
    let report = import(&dst, &text).await;
    assert_eq!(report, six(3, 0, 0, 2, 0, 0));

    // Every row is back with content, type, tags and scope intact.
    let mut src_views = src
        .service
        .list(ListArgs::default())
        .await
        .expect("source list");
    let mut dst_views = dst
        .service
        .list(ListArgs::default())
        .await
        .expect("target list");
    src_views.sort_by_key(|v| v.created_at);
    dst_views.sort_by_key(|v| v.created_at);
    assert_eq!(dst_views.len(), 3);
    for (s, d) in src_views.iter().zip(&dst_views) {
        assert_eq!(d.content, s.content);
        assert_eq!(d.mem_type, s.mem_type);
        assert_eq!(d.tags, s.tags);
        assert_eq!(d.scope, s.scope);
    }

    // Provenance columns: preserved, except last_accessed, which is refreshed
    // to the TARGET clock; rows land pending with no vector (D-10, D-13).
    let target_ids: Vec<i64> = [a, b, c]
        .iter()
        .map(|id| id_by_content(&dst.db_path, &provenance_content(&src.db_path, *id)))
        .collect();
    for (src_id, dst_id) in [a, b, c].iter().zip(&target_ids) {
        let s = provenance(&src.db_path, *src_id);
        let d = provenance(&dst.db_path, *dst_id);
        assert_eq!(d.source, s.source, "source is preserved verbatim (D-07)");
        assert_eq!(d.created_at, s.created_at, "created_at preserved");
        assert_eq!(d.access_count, s.access_count, "access_count preserved");
        assert_eq!(d.expires_at, s.expires_at, "expires_at preserved");
        assert_eq!(
            d.last_accessed, 5_000,
            "last_accessed refreshed to import time"
        );
        assert_eq!(status(&dst.db_path, *dst_id), 0, "lands pending");
        assert_eq!(vec_rows(&dst.db_path, *dst_id), 0, "no vector travelled");
    }
    assert_eq!(
        provenance(&dst.db_path, target_ids[0]).expires_at,
        Some(1_000 + 1_000_000)
    );
    assert_eq!(provenance(&dst.db_path, target_ids[1]).expires_at, None);
    assert_eq!(provenance(&dst.db_path, target_ids[2]).expires_at, None);

    // Edges attach to the TARGET ids with their original times (D-05, D-08).
    let (ta, tb, tc) = (target_ids[0], target_ids[1], target_ids[2]);
    let mut expected = vec![
        (ta, tb, "supersedes".to_string(), supersedes_at),
        (tb.min(tc), tb.max(tc), "relates_to".to_string(), relates_at),
    ];
    expected.sort();
    assert_eq!(edges(&dst.db_path), expected);
    assert_eq!(edges_touching(&dst.db_path, tb), 2);
    assert_eq!(orphan_edges(&dst.db_path), 0);

    // A re-export of the target equals the source export modulo the fields
    // that legitimately differ (ids, refreshed last_accessed, header clock).
    let (again, _) = export_to_string(&dst, "target.jsonl").await;
    assert_eq!(normalised(&again), normalised(&text));
}

/// Content of one source row, to find its restored twin in the target.
fn provenance_content(db_path: &Path, id: i64) -> String {
    raw_conn(db_path)
        .query_row("SELECT content FROM memories WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .expect("read content")
}

#[tokio::test]
async fn re_importing_the_same_file_is_idempotent() {
    let src = harness();
    seed_round_trip_source(&src).await;
    let (text, _) = export_to_string(&src, "source.jsonl").await;

    let dst = harness();
    dst.clock.set(5_000);
    let first = import(&dst, &text).await;
    assert_eq!(first, six(3, 0, 0, 2, 0, 0));
    let rows_after_first = row_count(&dst.db_path);
    let edges_after_first = edge_count(&dst.db_path);
    assert_eq!((rows_after_first, edges_after_first), (3, 2));

    dst.clock.advance(100);
    let second = import(&dst, &text).await;
    assert_eq!(second, six(0, 3, 0, 0, 2, 0));
    assert_eq!(row_count(&dst.db_path), rows_after_first);
    assert_eq!(edge_count(&dst.db_path), edges_after_first);
}

// ---------------------------------------------------------------------------
// Import half — Task 2: kill-tests and edge probes
// ---------------------------------------------------------------------------

/// A valid v1 header declaring `rows` memories and `links` links, for
/// hand-built files.
fn header(rows: u64, links: u64) -> String {
    format!(
        r#"{{"record":"header","format":"agent-memory-export","version":1,"exported_at":1000,"embedding_model":"nomic-embed-text","embedding_dim":768,"schema_version":3,"agent_memory_version":"0.0.1","exported_rows":{rows},"exported_links":{links}}}"#
    )
}

const MEM1: &str = r#"{"record":"memory","key":1,"type":"DECISION","content":"hand-built first","tags":[],"source":null,"scope":null,"created_at":1,"last_accessed":1,"access_count":0,"expires_at":null}"#;
const MEM2: &str = r#"{"record":"memory","key":2,"type":"PATTERN","content":"hand-built second","tags":["t"],"source":"s","scope":"sc","created_at":2,"last_accessed":2,"access_count":3,"expires_at":null}"#;
const LINK12: &str = r#"{"record":"link","from":1,"to":2,"kind":"relates_to","created_at":5}"#;

/// Join lines into a newline-terminated file.
fn file_from(lines: &[&str]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[tokio::test]
async fn importing_into_a_non_empty_db_with_overlapping_rowids_never_attaches_phantom_edges() {
    // Source: three memories whose ids are exactly 1..=3 (a fresh database),
    // and one edge 1 -> 2.
    let src = harness();
    let a = store(&src, new_memory("source one", MemoryType::Decision)).await;
    let b = store(&src, new_memory("source two", MemoryType::Pattern)).await;
    let c = store(&src, new_memory("source three", MemoryType::Todo)).await;
    assert_eq!((a, b, c), (1, 2, 3), "the file's keys are 1..=3");
    link(&src, a, b, LinkKind::RelatesTo).await;
    let (text, _) = export_to_string(&src, "overlap.jsonl").await;

    // Target: three UNRELATED rows already hold ids 1..=3 — exactly the keys
    // the file's link record names (PITFALLS 18).
    let dst = harness();
    let pre: Vec<i64> = vec![
        store(&dst, new_memory("target x", MemoryType::Error)).await,
        store(&dst, new_memory("target y", MemoryType::Architecture)).await,
        store(&dst, new_memory("target z", MemoryType::Constraint)).await,
    ];
    assert_eq!(
        pre,
        vec![1, 2, 3],
        "the target's rowids overlap the file's keys"
    );

    let report = import(&dst, &text).await;
    assert_eq!(report, six(3, 0, 0, 1, 0, 0));

    for id in &pre {
        assert_eq!(
            edges_touching(&dst.db_path, *id),
            0,
            "pre-existing row {id} gained a phantom edge"
        );
    }
    let new_a = id_by_content(&dst.db_path, "source one");
    let new_b = id_by_content(&dst.db_path, "source two");
    assert!(new_a > 3 && new_b > 3, "restored rows get fresh target ids");
    assert_eq!(edges_touching(&dst.db_path, new_a), 1);
    assert_eq!(edges_touching(&dst.db_path, new_b), 1);
    assert_eq!(orphan_edges(&dst.db_path), 0);
    assert_eq!(row_count(&dst.db_path), 6);
    assert_eq!(edge_count(&dst.db_path), 1);

    // Importing the same file again on top of the now-populated target maps
    // every key onto the restored rows and changes nothing.
    let again = import(&dst, &text).await;
    assert_eq!(again, six(0, 3, 0, 0, 1, 0));
    assert_eq!(row_count(&dst.db_path), 6);
    assert_eq!(edge_count(&dst.db_path), 1);
    for id in &pre {
        assert_eq!(edges_touching(&dst.db_path, *id), 0);
    }
}

#[tokio::test]
async fn link_endpoints_that_dedup_to_existing_rows_attach_to_those_rows() {
    let shared = || NewMemory {
        content: "shared decision already on the target".to_string(),
        mem_type: MemoryType::Decision,
        tags: vec!["any".to_string()],
        source: Some("shared-src".to_string()),
        scope: Some("other-scope".to_string()),
        ttl_secs: None,
    };

    // Source: A (key 1) and B (key 2), A -> B.
    let src = harness();
    let a = store(&src, shared()).await;
    let b = store(&src, new_memory("only in the file", MemoryType::Pattern)).await;
    link(&src, a, b, LinkKind::RelatesTo).await;
    let (text, _) = export_to_string(&src, "dedup.jsonl").await;

    // Target: a filler row takes id 1, then a row identical to A on the
    // (source, mem_type, content) key takes id 2 — so A's key (1) and the
    // existing row's id (2) differ.
    let dst = harness();
    let filler = store(&dst, new_memory("unrelated filler row", MemoryType::Error)).await;
    let existing_a = store(&dst, shared()).await;
    assert_eq!((filler, existing_a), (1, 2));
    assert_ne!(existing_a, a, "the existing row's id differs from A's key");

    let report = import(&dst, &text).await;
    assert_eq!(report.imported, 1, "only B is new");
    assert_eq!(report.skipped_duplicates, 1, "A dedups to the existing row");
    assert_eq!(report.links_imported, 1);
    assert_eq!(report, six(1, 1, 0, 1, 0, 0));

    assert_eq!(
        edges_touching(&dst.db_path, existing_a),
        1,
        "the edge attaches to the EXISTING row A dedups to"
    );
    assert_eq!(
        edges_touching(&dst.db_path, filler),
        0,
        "never to the row at A's key"
    );
    assert_eq!(
        row_count(&dst.db_path),
        3,
        "grew by exactly one (no second A)"
    );
    assert_eq!(
        id_by_content(&dst.db_path, "shared decision already on the target"),
        existing_a
    );
}

#[tokio::test]
async fn identical_rows_collapse_to_one_on_import_and_count_a_duplicate() {
    let twin = || new_memory("the same row stored twice", MemoryType::Architecture);
    let src = harness();
    let first = store(&src, twin()).await;
    let second = store(&src, twin()).await;
    assert_ne!(first, second, "no uniqueness constraint: two rows exist");

    let (text, report) = export_to_string(&src, "twins.jsonl").await;
    assert_eq!(report.memories, 2);
    match &lines(&text)[0] {
        Record::Header { exported_rows, .. } => assert_eq!(*exported_rows, 2),
        other => panic!("expected header, got {other:?}"),
    }
    assert_eq!(text.lines().count(), 3, "header + two memory records");

    let dst = harness();
    assert_eq!(import(&dst, &text).await, six(1, 1, 0, 0, 0, 0));
    assert_eq!(row_count(&dst.db_path), 1, "identical rows collapse (D-07)");
}

#[tokio::test]
async fn a_link_between_two_rows_that_collapse_on_import_is_dropped_not_miscounted() {
    // 05-REVIEW WR-01: the parser only rejects self-links on FILE keys. Two
    // distinct keys that dedup to the same target id would reach the store as
    // `link(N, N)`, which `INSERT OR IGNORE` swallows under the 0003 CHECK as
    // a phantom "duplicate". The importer must count it as dropped instead.
    let twin = || new_memory("the same row stored twice and linked", MemoryType::Decision);
    let src = harness();
    let first = store(&src, twin()).await;
    let second = store(&src, twin()).await;
    link(&src, first, second, LinkKind::RelatesTo).await;

    let (text, report) = export_to_string(&src, "linked-twins.jsonl").await;
    assert_eq!((report.memories, report.links), (2, 1));
    assert_eq!(text.lines().count(), 4, "header + two memories + one link");

    let dst = harness();
    assert_eq!(
        import(&dst, &text).await,
        six(1, 1, 0, 0, 0, 1),
        "the collapsed edge is dropped, never a 'skipped duplicate'"
    );
    assert_eq!(row_count(&dst.db_path), 1);
    assert_eq!(
        edge_count(&dst.db_path),
        0,
        "no self-link ever reaches the store"
    );

    // Re-importing is still idempotent and still reports the drop.
    assert_eq!(import(&dst, &text).await, six(0, 2, 0, 0, 0, 1));
    assert_eq!(edge_count(&dst.db_path), 0);
}

#[tokio::test]
async fn expired_rows_are_skipped_and_counted_and_their_links_dropped() {
    // Source at 1_000: A expires at 1_050, B at 11_000, C never.
    let src = harness();
    let a = store(
        &src,
        NewMemory {
            ttl_secs: Some(50),
            ..new_memory("alpha expires soon", MemoryType::Todo)
        },
    )
    .await;
    let b = store(
        &src,
        NewMemory {
            ttl_secs: Some(10_000),
            ..new_memory("beta lives long", MemoryType::Decision)
        },
    )
    .await;
    let c = store(&src, new_memory("gamma is permanent", MemoryType::Pattern)).await;
    link(&src, a, c, LinkKind::CausedBy).await;
    link(&src, b, c, LinkKind::Supersedes).await;
    let (text, _) = export_to_string(&src, "expiry.jsonl").await;

    // Target clock EXACTLY at A's expiry: `expires_at <= now` skips (D-10),
    // and the edge that needed A is dropped and counted (PITFALLS 19).
    let dst = harness();
    dst.clock.set(1_050);
    let report = import(&dst, &text).await;
    assert_eq!(report, six(2, 0, 1, 1, 0, 1));
    let contents: Vec<String> = dst
        .service
        .list(ListArgs::default())
        .await
        .expect("list")
        .into_iter()
        .map(|v| v.content)
        .collect();
    assert_eq!(contents.len(), 2);
    assert!(contents.iter().any(|c| c == "beta lives long"));
    assert!(contents.iter().any(|c| c == "gamma is permanent"));
    assert!(
        !contents.iter().any(|c| c == "alpha expires soon"),
        "the expired row was never inserted"
    );
    assert_eq!(row_count(&dst.db_path), 2);
    assert_eq!(edge_count(&dst.db_path), 1);
    assert_eq!(orphan_edges(&dst.db_path), 0);

    // One second earlier the same file restores everything: the boundary is
    // `<=`, not `<`.
    let earlier = harness();
    earlier.clock.set(1_049);
    assert_eq!(import(&earlier, &text).await, six(3, 0, 0, 2, 0, 0));

    // An ancient row — last touched 400 days before its export — is imported
    // decay-fresh: last_accessed is refreshed to the target clock and the row
    // is listed (decay never deletes; PITFALLS 19).
    let old = harness();
    let ancient = store(
        &old,
        new_memory("ancient wisdom about caches", MemoryType::Pattern),
    )
    .await;
    old.clock.advance(400 * 86_400);
    let (old_text, _) = export_to_string(&old, "ancient.jsonl").await;
    assert_eq!(provenance(&old.db_path, ancient).last_accessed, 1_000);

    let fresh = harness();
    fresh.clock.set(7_777);
    // Note: the header's exported_at (400 days ahead of 7_777) also exercises
    // the clock-skew warning path — the import must continue regardless.
    assert_eq!(import(&fresh, &old_text).await, six(1, 0, 0, 0, 0, 0));
    let restored = id_by_content(&fresh.db_path, "ancient wisdom about caches");
    assert_eq!(provenance(&fresh.db_path, restored).last_accessed, 7_777);
    assert_eq!(
        provenance(&fresh.db_path, restored).created_at,
        1_000,
        "created_at preserved"
    );
    let listed = fresh.service.list(ListArgs::default()).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].content, "ancient wisdom about caches");
}

#[tokio::test]
async fn imported_rows_land_pending_and_the_sweep_backfills_them() {
    let src = harness();
    store(
        &src,
        new_memory("orchid pollination schedule", MemoryType::Pattern),
    )
    .await;
    store(
        &src,
        new_memory("granite countertop sealing", MemoryType::Todo),
    )
    .await;
    let (text, _) = export_to_string(&src, "pending.jsonl").await;

    // The target's embedder is UP — and the importer still never calls it.
    let dst = harness_with(Arc::new(FakeEmbedder::with_vectors(HashMap::new())));
    assert_eq!(import(&dst, &text).await, six(2, 0, 0, 0, 0, 0));
    let orchid = id_by_content(&dst.db_path, "orchid pollination schedule");
    let granite = id_by_content(&dst.db_path, "granite countertop sealing");
    for id in [orchid, granite] {
        assert_eq!(
            status(&dst.db_path, id),
            0,
            "imported rows land pending (D-13)"
        );
        assert_eq!(
            vec_rows(&dst.db_path, id),
            0,
            "no vector was written inline"
        );
    }

    // Exactly one re-embed mechanism: the sweep backfill.
    dst.service
        .sweep(dst.clock.now())
        .await
        .expect("sweep succeeds");
    for id in [orchid, granite] {
        assert_eq!(status(&dst.db_path, id), 1, "the sweep embedded the row");
        assert_eq!(vec_rows(&dst.db_path, id), 1);
    }

    let outcome = dst
        .service
        .search(SearchArgs {
            query: "orchid".to_string(),
            ..SearchArgs::default()
        })
        .await
        .expect("search");
    assert_eq!(outcome.search_mode, SearchMode::Hybrid);
    assert!(
        outcome.results.iter().any(|v| v.id == orchid),
        "the backfilled row is hybrid-searchable"
    );
}

#[tokio::test]
async fn a_header_only_file_imports_nothing_and_reports_zeros() {
    let src = harness();
    let (text, _) = export_to_string(&src, "empty.jsonl").await;
    assert_eq!(text.lines().count(), 1);

    let dst = harness();
    store(&dst, new_memory("already here", MemoryType::Decision)).await;
    let before = row_count(&dst.db_path);
    assert_eq!(import(&dst, &text).await, ImportReport::default());
    assert_eq!(import(&dst, &text).await, six(0, 0, 0, 0, 0, 0));
    assert_eq!(row_count(&dst.db_path), before);
    assert_eq!(edge_count(&dst.db_path), 0);
}

#[test]
fn a_zero_byte_file_is_rejected_at_line_one() {
    for text in ["", "\n"] {
        let err = parse_jsonl(text).expect_err("no header");
        assert!(matches!(err, MemoryError::InvalidArgument(_)), "{err:?}");
        assert!(
            err.to_string().starts_with("invalid argument: line 1:"),
            "{err}"
        );
    }
}

#[tokio::test]
async fn links_are_applied_after_all_memories_regardless_of_file_order() {
    let link_first = file_from(&[&header(2, 1), LINK12, MEM1, MEM2]);
    let canonical = file_from(&[&header(2, 1), MEM1, MEM2, LINK12]);

    let shuffled = harness();
    let report = import(&shuffled, &link_first).await;
    assert_eq!(report, six(2, 0, 0, 1, 0, 0));
    let first = id_by_content(&shuffled.db_path, "hand-built first");
    let second = id_by_content(&shuffled.db_path, "hand-built second");
    assert_eq!(
        edges(&shuffled.db_path),
        vec![(
            first.min(second),
            first.max(second),
            "relates_to".to_string(),
            5
        )],
        "the edge carries the record's own created_at"
    );

    let ordered = harness();
    assert_eq!(import(&ordered, &canonical).await, report);
    assert_eq!(row_count(&ordered.db_path), row_count(&shuffled.db_path));
    assert_eq!(edge_count(&ordered.db_path), edge_count(&shuffled.db_path));
}

#[tokio::test]
async fn nothing_is_written_when_validation_fails() {
    let dst = harness();
    store(&dst, new_memory("pre-existing row", MemoryType::Decision)).await;
    let before = row_count(&dst.db_path);

    // Line 2 is a perfectly valid memory; line 3 is not. The whole file is
    // refused, so `import_jsonl` is never reached — the only entry point to
    // the importer's writes takes a fully parsed JsonlFile.
    let text = file_from(&[&header(2, 0), MEM1, r#"{"record":"memo"}"#]);
    let err = parse_jsonl(&text).expect_err("line 3 is malformed");
    assert!(matches!(err, MemoryError::InvalidArgument(_)), "{err:?}");
    assert!(err.to_string().contains("line 3:"), "{err}");

    assert_eq!(row_count(&dst.db_path), before, "nothing was written");
    assert_eq!(edge_count(&dst.db_path), 0);
    assert!(
        dst.service
            .list(ListArgs::default())
            .await
            .expect("list")
            .iter()
            .all(|v| v.content == "pre-existing row"),
        "the valid line 2 record was NOT imported"
    );
}
