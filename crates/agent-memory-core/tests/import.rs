//! INTEROP-01 integration proof: a GSD STATE.md fixture imports as typed,
//! keyword-searchable memories, and re-importing the same file is a no-op
//! (idempotent on the `(source, mem_type, content)` key).
//!
//! Uses the store.rs harness shape: a real tempdir DB plus a FakeEmbedder — a
//! FAILING one here, so searchability is proven through the FTS5 keyword path
//! (the fallback that must work on a fresh machine with no Ollama, SEARCH-03).

use std::sync::Arc;

use agent_memory_core::clock::TestClock;
use agent_memory_core::decay::DecayConfig;
use agent_memory_core::domain::{MemoryType, NewMemory};
use agent_memory_core::embed::FakeEmbedder;
use agent_memory_core::import::gsd_state::parse_gsd_state;
use agent_memory_core::service::{ListArgs, MemoryService, SearchArgs, SearchMode};
use agent_memory_core::store::sqlite::SqliteStore;

const FIXTURE: &str = include_str!("fixtures/STATE.md");

fn service_for(path: &std::path::Path) -> MemoryService {
    let store = SqliteStore::open(path).expect("open store");
    // Failing embedder: import must succeed and searchability must hold on the
    // keyword path even with Ollama dead (best-effort embed, SEARCH-03).
    let embedder = Arc::new(FakeEmbedder::failing());
    MemoryService::new(
        Arc::new(store),
        Arc::new(TestClock::new(1_000)),
        embedder,
        DecayConfig::default(),
    )
}

#[tokio::test]
async fn import_loads_typed_searchable_memories_idempotently() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service_for(&dir.path().join("memory.db"));

    // Parse: 4 decisions + 2 blockers + 1 real todo + 2 deferred rows = 9
    // drafts; exactly the lone malformed dash is skip-counted.
    let parsed = parse_gsd_state(FIXTURE, Some("fixture".to_string()));
    assert_eq!(parsed.drafts.len(), 9, "fixture yields 9 typed drafts");
    assert_eq!(parsed.skipped, 1, "the lone malformed bullet is counted");

    // First import: everything lands, nothing deduped.
    let first = service.import(parsed.drafts).await.expect("first import");
    assert_eq!(first.imported, 9);
    assert_eq!(first.skipped_duplicates, 0);
    // The gsd-state path has no expiry and no links: the four jsonl-only
    // counts are always zero here (Phase 5 D-12).
    assert_eq!(first.skipped_expired, 0);
    assert_eq!(first.links_imported, 0);
    assert_eq!(first.links_skipped_duplicates, 0);
    assert_eq!(first.links_dropped, 0);

    // Per-type counts through the normal list path.
    let count = |mem_type: Option<MemoryType>, tag: Option<&str>| {
        let service = service.clone();
        let tag = tag.map(str::to_string);
        async move {
            service
                .list(ListArgs {
                    mem_type,
                    tag,
                    ..ListArgs::default()
                })
                .await
                .expect("list")
                .len()
        }
    };
    assert_eq!(count(Some(MemoryType::Decision), None).await, 4);
    assert_eq!(count(Some(MemoryType::Constraint), None).await, 2);
    assert_eq!(count(Some(MemoryType::Todo), None).await, 3);
    assert_eq!(count(None, Some("deferred")).await, 2);
    assert_eq!(count(None, Some("gsd")).await, 9);

    // Imported content is findable via keyword search (embedder is dead, so
    // the outcome must be the FTS5 fallback — proving fresh-machine recall).
    let outcome = service
        .search(SearchArgs {
            query: "zigbuild".to_string(),
            ..SearchArgs::default()
        })
        .await
        .expect("search");
    assert_eq!(outcome.search_mode, SearchMode::Keyword);
    assert!(
        outcome
            .results
            .iter()
            .any(|m| m.content.contains("zigbuild release matrix")),
        "imported decision must be keyword-searchable"
    );

    // Idempotency (phase success criterion 4): a re-parse + re-import of the
    // SAME file imports nothing and reports every draft as a duplicate.
    let reparsed = parse_gsd_state(FIXTURE, Some("fixture".to_string()));
    let second = service
        .import(reparsed.drafts)
        .await
        .expect("second import");
    assert_eq!(second.imported, 0, "re-run must import nothing");
    assert_eq!(second.skipped_duplicates, first.imported);
}

/// CR-01 regression (03-REVIEW): a draft with `source: None` persists its
/// source as SQL NULL, so the dedup probe must use NULL-safe equality — before
/// the fix the probe coerced `None` to `""`, `NULL = ''` never matched, and
/// every re-import inserted a duplicate row.
#[tokio::test]
async fn reimporting_a_sourceless_draft_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = service_for(&dir.path().join("memory.db"));

    let draft = || NewMemory {
        content: "sourceless decision survives re-import exactly once".to_string(),
        mem_type: MemoryType::Decision,
        tags: vec![],
        source: None,
        scope: None,
        ttl_secs: None,
    };

    let first = service.import(vec![draft()]).await.expect("first import");
    assert_eq!(first.imported, 1);
    assert_eq!(first.skipped_duplicates, 0);

    let second = service.import(vec![draft()]).await.expect("second import");
    assert_eq!(second.imported, 0, "re-run must import nothing");
    assert_eq!(
        second.skipped_duplicates, 1,
        "the source:None draft must be recognised as a duplicate"
    );

    let rows = service.list(ListArgs::default()).await.expect("list").len();
    assert_eq!(rows, 1, "exactly one row after two imports");
}
