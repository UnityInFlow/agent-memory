//! DIST-04 / INTEROP-02 proof file: the portable export (format v1 JSONL) and,
//! in plan 05-03, its import.
//!
//! Export decisions proven here: D-03 (line 1 is the header, read from the
//! `meta` pin and the injected clock, with `agent_memory_version` supplied by
//! the caller), D-04 (a memory record carries exactly the eleven documented
//! keys — no derived data, no vector), D-05 (links after all memories, as
//! stored), D-06 (every row, expired rows included, ascending id) and the
//! canonical-output rule (two exports of an unchanged database are
//! byte-identical).
//!
//! Harness: a REAL on-disk temp DB, an injected `TestClock`, and a failing
//! `FakeEmbedder` (stores land pending; the exporter never touches the
//! embedder). The export is written through a real file sink under the
//! harness's temp dir and read back, so the boxed-writer path is exercised
//! end to end.

use std::path::PathBuf;
use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{LinkKind, LinkOutcome, MemoryType, NewMemory};
use agent_memory_core::embed::{Embedder, FakeEmbedder};
use agent_memory_core::export::{Record, FORMAT_NAME, FORMAT_VERSION};
use agent_memory_core::service::{ExportReport, ListArgs, MemoryService};
use agent_memory_core::store::migrations::LATEST_SCHEMA_VERSION;
use agent_memory_core::store::sqlite::SqliteStore;

/// The literal passed as `agent_memory_version` — the core crate has no say
/// in the binary's version (RESEARCH Pitfall 9), so the test pins what it
/// passed in.
const TEST_VERSION: &str = "test-0.0.0";

struct Harness {
    service: MemoryService,
    clock: Arc<TestClock>,
    _db_path: PathBuf,
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
        _db_path: db_path,
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
