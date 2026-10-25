//! Spawned-binary realism tests for `agent-memory export` and
//! `agent-memory import --from jsonl` (Phase 5 D-01, D-02, D-11, D-12).
//!
//! These tests run the exact binary that ships, the way a user does: a fresh
//! `--db` under a tempdir, a hand-written seed file, real stdout/stderr and
//! real exit codes. They are **realism-only and contribute zero coverage by
//! design**, exactly like `tests/rest.rs`: a child process never flushes LLVM
//! profile data, so the coverage-bearing proofs of the same behaviour live in
//! `agent-memory-core` (`export::`, `import::jsonl`, `tests/export_import.rs`)
//! and in the in-process handler tests. What this file adds is the one thing
//! those cannot: `main.rs`'s argument parsing, file I/O, report strings and
//! exit paths, end to end over two databases.
//!
//! Every spawn gets `AGENT_MEMORY_OLLAMA_URL=http://127.0.0.1:9` (the discard
//! port — connection refused) so the outcome is deterministic on any machine;
//! neither subcommand contacts the embedder anyway (D-13).

use std::path::Path;
use std::process::{Command, Output, Stdio};

/// The first-line report after the seed (2 memories, 1 link) lands in an
/// empty database — and again after `out.jsonl` lands in a second one.
const FIRST_IMPORT: &str = "imported: 2, skipped duplicates: 0, skipped expired: 0, \
                            links imported: 1, links skipped duplicates: 0, links dropped: 0";

/// The first-line report when the same file is imported a second time: every
/// memory and the one edge dedup, nothing is expired, nothing is dropped.
const REIMPORT: &str = "imported: 0, skipped duplicates: 2, skipped expired: 0, \
                        links imported: 0, links skipped duplicates: 1, links dropped: 0";

fn binary_path() -> String {
    // CARGO_BIN_EXE_<name> is set by cargo for integration tests of a binary crate.
    env!("CARGO_BIN_EXE_agent-memory").to_string()
}

/// Run the binary with `args` and capture everything. Stdin is closed and
/// nothing but the environment is inherited from the test process.
fn run(args: &[&str]) -> Output {
    Command::new(binary_path())
        .args(args)
        // Deterministically dead Ollama URL: the same outcome on every
        // machine, with or without a local daemon.
        .env("AGENT_MEMORY_OLLAMA_URL", "http://127.0.0.1:9")
        .stdin(Stdio::null())
        .output()
        .expect("spawn agent-memory")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Tempdir paths are UTF-8 on every platform the suite runs on.
fn s(path: &Path) -> &str {
    path.to_str().expect("utf-8 path")
}

/// A format-v1 header declaring `rows` memories and `links` links. Built with
/// `serde_json::json!` — the parser does not care about key order (only the
/// exporter's output order is pinned, by the goldens in `export.rs`).
fn header(rows: u64, links: u64) -> serde_json::Value {
    serde_json::json!({
        "record": "header",
        "format": "agent-memory-export",
        "version": 1,
        "exported_at": 1_700_000_000,
        "embedding_model": "nomic-embed-text",
        "embedding_dim": 768,
        "schema_version": 3,
        "agent_memory_version": "0.0.1-seed",
        "exported_rows": rows,
        "exported_links": links,
    })
}

/// Two memories under keys 10 and 20 — deliberately not 1 and 2, so a key can
/// never coincide with the id the target database hands out — and one
/// `relates_to` edge between them.
fn seed_jsonl() -> String {
    let records = [
        header(2, 1),
        serde_json::json!({
            "record": "memory",
            "key": 10,
            "type": "DECISION",
            "content": "Use reciprocal-rank fusion for hybrid search",
            "tags": ["search", "ranking"],
            "source": "cli-realism",
            "scope": null,
            "created_at": 1_690_000_000,
            "last_accessed": 1_690_000_000,
            "access_count": 3,
            "expires_at": null,
        }),
        serde_json::json!({
            "record": "memory",
            "key": 20,
            "type": "PATTERN",
            "content": "Exports stream one JSON line per record",
            "tags": [],
            "source": null,
            "scope": "project-a",
            "created_at": 1_690_000_100,
            "last_accessed": 1_690_000_100,
            "access_count": 0,
            "expires_at": null,
        }),
        serde_json::json!({
            "record": "link",
            "from": 10,
            "to": 20,
            "kind": "relates_to",
            "created_at": 1_690_000_200,
        }),
    ];
    let mut text = String::new();
    for record in &records {
        text.push_str(&serde_json::to_string(record).expect("serialise seed record"));
        text.push('\n');
    }
    text
}

/// Names of entries in `dir` that look like an in-flight export temp file.
fn export_temp_entries(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .expect("read dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".export-tmp-"))
        .collect()
}

#[test]
fn export_import_round_trip_over_the_real_binary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let seed = dir.path().join("seed.jsonl");
    std::fs::write(&seed, seed_jsonl()).expect("write seed");
    let db_a = dir.path().join("a.db");
    let db_b = dir.path().join("b.db");
    let out = dir.path().join("out.jsonl");

    // 1. The seed lands in A with the documented six counts and pending line.
    let o = run(&["import", "--from", "jsonl", s(&seed), "--db", s(&db_a)]);
    assert!(o.status.success(), "import into A failed: {}", stderr(&o));
    let text = stdout(&o);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.first().copied(), Some(FIRST_IMPORT), "stdout: {text}");
    assert!(
        lines
            .get(1)
            .is_some_and(|l| l.starts_with("2 rows pending embedding")),
        "pending line: {text}"
    );

    // 2. `--output` writes a private, complete file beside no temp sibling.
    let o = run(&["export", "--db", s(&db_a), "--output", s(&out)]);
    assert!(o.status.success(), "export --output failed: {}", stderr(&o));
    assert!(
        stderr(&o).contains("exported 2 memories and 1 links"),
        "stderr report: {}",
        stderr(&o)
    );
    let file_text = std::fs::read_to_string(&out).expect("read out.jsonl");
    let file_lines: Vec<&str> = file_text.lines().collect();
    assert_eq!(
        file_lines.len(),
        4,
        "header + 2 memories + 1 link: {file_text}"
    );
    assert!(
        file_lines[0].starts_with("{\"record\":\"header\""),
        "line 1: {}",
        file_lines[0]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&out)
            .expect("out.jsonl metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the export file is owner-only (D-02)");
    }
    assert!(
        export_temp_entries(dir.path()).is_empty(),
        "no `.export-tmp-` sibling may survive a successful export"
    );

    // 3. The stdout export carries the same records (the header's
    //    `exported_at` may differ by wall clock).
    let o = run(&["export", "--db", s(&db_a)]);
    assert!(
        o.status.success(),
        "export to stdout failed: {}",
        stderr(&o)
    );
    let stdout_text = stdout(&o);
    let stdout_lines: Vec<&str> = stdout_text.lines().collect();
    assert_eq!(stdout_lines.len(), 4, "stdout export: {stdout_text}");
    assert_eq!(
        &stdout_lines[1..],
        &file_lines[1..],
        "records differ between sinks"
    );

    // 4. The exported file restores into an empty B ...
    let o = run(&["import", "--from", "jsonl", s(&out), "--db", s(&db_b)]);
    assert!(o.status.success(), "import into B failed: {}", stderr(&o));
    let text = stdout(&o);
    assert_eq!(text.lines().next(), Some(FIRST_IMPORT), "stdout: {text}");

    // 5. ... and importing it again is a no-op reported as duplicates (D-07).
    let o = run(&["import", "--from", "jsonl", s(&out), "--db", s(&db_b)]);
    assert!(
        o.status.success(),
        "re-import into B failed: {}",
        stderr(&o)
    );
    let text = stdout(&o);
    assert_eq!(text.lines().next(), Some(REIMPORT), "stdout: {text}");
    assert!(
        text.lines()
            .nth(1)
            .is_some_and(|l| l.starts_with("0 rows pending embedding")),
        "pending line after re-import: {text}"
    );
}

#[test]
fn a_malformed_file_exits_non_zero_naming_the_line_and_creates_no_database() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bad = dir.path().join("bad.jsonl");
    let header_line = serde_json::to_string(&header(0, 0)).expect("serialise header");
    std::fs::write(&bad, format!("{header_line}\n{{\"record\":\"memo\"}}\n")).expect("write bad");
    let never = dir.path().join("never.db");

    let o = run(&["import", "--from", "jsonl", s(&bad), "--db", s(&never)]);

    assert!(!o.status.success(), "a malformed file must not import");
    let err = stderr(&o);
    assert!(
        err.contains("line 2:"),
        "stderr names the offending line: {err}"
    );
    assert!(
        !never.exists(),
        "validation runs before the database is opened (D-08 pass 1 / D-11)"
    );
}

#[test]
fn an_unsupported_from_value_still_bails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let seed = dir.path().join("seed.jsonl");
    std::fs::write(&seed, seed_jsonl()).expect("write seed");
    let db = dir.path().join("never.db");

    let o = run(&["import", "--from", "bogus", s(&seed), "--db", s(&db)]);

    assert!(!o.status.success(), "an unknown --from must fail");
    let err = stderr(&o);
    assert!(
        err.contains("supported formats: gsd-state, jsonl"),
        "stderr lists the supported formats: {err}"
    );
    assert!(
        !db.exists(),
        "the database is never created for a bad --from"
    );
}

#[test]
fn scope_is_rejected_for_jsonl() {
    let dir = tempfile::tempdir().expect("tempdir");
    let seed = dir.path().join("seed.jsonl");
    std::fs::write(&seed, seed_jsonl()).expect("write seed");
    let db = dir.path().join("never.db");

    let o = run(&[
        "import",
        "--from",
        "jsonl",
        s(&seed),
        "--scope",
        "x",
        "--db",
        s(&db),
    ]);

    assert!(!o.status.success(), "--scope with jsonl must fail");
    let err = stderr(&o);
    assert!(
        err.contains("--scope applies only to --from gsd-state"),
        "stderr explains the rejection: {err}"
    );
    assert!(
        !db.exists(),
        "the database is never created when --scope is rejected"
    );
}

#[test]
fn an_unwritable_output_path_fails_before_the_database_is_created() {
    // 05-REVIEW IN-03: `--output` is validated BEFORE the database is opened,
    // the way `import --from jsonl` validates the file first. Before the fix
    // this command resolved, created and migrated the database and only then
    // failed on `create_new` in the missing directory.
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("nonexistent").join("out.jsonl");
    let never = dir.path().join("never.db");

    let o = run(&["export", "--db", s(&never), "--output", s(&out)]);

    assert!(!o.status.success(), "a missing output directory must fail");
    let err = stderr(&o);
    assert!(
        err.contains("creating export file beside"),
        "stderr names the output step: {err}"
    );
    assert!(
        !never.exists(),
        "the database is never created when --output is unwritable"
    );
    assert!(!out.exists());
    assert!(
        export_temp_entries(dir.path()).is_empty(),
        "no temp is left anywhere under the tempdir"
    );
}

#[test]
fn invalid_utf8_is_rejected_cleanly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let garbage = dir.path().join("garbage.jsonl");
    std::fs::write(&garbage, [0xffu8, 0xfe, 0x0a]).expect("write garbage");
    let db = dir.path().join("never.db");

    let o = run(&["import", "--from", "jsonl", s(&garbage), "--db", s(&db)]);

    assert!(!o.status.success(), "invalid UTF-8 must fail");
    let err = stderr(&o);
    assert!(err.contains("reading"), "the read step names itself: {err}");
    assert!(
        !err.contains("PANIC"),
        "garbage must never panic the binary: {err}"
    );
    assert!(
        !db.exists(),
        "the database is never created for unreadable input"
    );
}
