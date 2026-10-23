//! STORE-05: shipped migrations are frozen; upgrades are backed up; newer DBs are refused cleanly
//!
//! Invariants proven here (D-11/D-12/D-13, RESEARCH Pattern 6, Pitfalls 6 + 8):
//!
//! - **Frozen SQL.** A committed binary fixture (`tests/fixtures/v0.0.1.db`,
//!   built ONCE from the v0.0.1-tagged 0001+0002 and never regenerated) migrated
//!   to latest has exactly the schema of a fresh-from-zero database. Any later
//!   edit of shipped 0001/0002 makes the two diverge and turns this suite red.
//! - **Backup before migrate.** Upgrading a non-empty older DB first writes ONE
//!   sibling `{db}.backup-pre-{NNNN}-{YYYYMMDD}` via the SQLite Online Backup
//!   API, and that backup still carries the PRE-migration schema version.
//! - **Newer-schema refusal.** A DB whose `user_version` is ahead of the binary
//!   is refused with a message naming both versions and the upgrade command,
//!   and the file is left byte-for-byte untouched.
//!
//! Every test runs on real on-disk tempfiles (an in-memory DB has no sibling
//! file to back up) and registers the sqlite-vec module first — migration 0002
//! creates a vec0 table (the `store/migrations.rs` test quirk).

use std::fs;
use std::path::{Path, PathBuf};

use agent_memory_core::store::migrations::{migrations, LATEST_SCHEMA_VERSION};
use agent_memory_core::store::sqlite::{register_vec_extension, SqliteStore};
use rusqlite::Connection;

/// The schema version v0.0.1 shipped (migrations 0001 + 0002).
const V0_0_1_SCHEMA_VERSION: usize = 2;

/// Rows planted in the fixture so a backup is provably a copy of real data.
const FIXTURE_ROWS: i64 = 2;

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0.0.1.db")
}

/// Copy the committed fixture to `dest` — tests NEVER open the fixture in place.
fn copy_fixture(dest: &Path) {
    register_vec_extension().expect("sqlite-vec registers");
    fs::copy(fixture_path(), dest).expect("copy committed v0.0.1 fixture");
}

fn user_version(path: &Path) -> i64 {
    let conn = Connection::open(path).expect("open db");
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read user_version")
}

/// The full schema as sorted, whitespace-normalized `type|name|tbl|sql` lines,
/// excluding SQLite's own `sqlite_*` internal objects.
fn normalized_schema(path: &Path) -> Vec<String> {
    let conn = Connection::open(path).expect("open db");
    let mut stmt = conn
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_master \
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .expect("prepare schema query");
    let rows = stmt
        .query_map([], |row| {
            let kind: String = row.get(0)?;
            let name: String = row.get(1)?;
            let tbl: String = row.get(2)?;
            let sql: Option<String> = row.get(3)?;
            let sql = sql
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            Ok(format!("{kind}|{name}|{tbl}|{sql}"))
        })
        .expect("query schema");
    let mut out: Vec<String> = rows.map(|r| r.expect("schema row")).collect();
    out.sort();
    out
}

/// Sibling backup files of `db` for the CURRENT schema version (the `-wal` /
/// `-shm` / `-journal` companions SQLite may create are not backups).
fn backups_for_latest(db: &Path) -> Vec<PathBuf> {
    let name = db
        .file_name()
        .and_then(|n| n.to_str())
        .expect("db file name");
    let prefix = format!("{name}.backup-pre-{LATEST_SCHEMA_VERSION:04}-");
    let mut out: Vec<PathBuf> = fs::read_dir(db.parent().expect("db dir"))
        .expect("read db dir")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            n.starts_with(&prefix)
                && !n.ends_with("-wal")
                && !n.ends_with("-shm")
                && !n.ends_with("-journal")
        })
        .collect();
    out.sort();
    out
}

/// No in-flight backup temp file (`{db}.backup-tmp-*`) may be left beside the
/// DB after an open, successful or not (REVIEW WR-01 / IN-03).
fn assert_no_backup_temp(dir: &Path) {
    let leftovers: Vec<String> = fs::read_dir(dir)
        .expect("read db dir")
        .map(|e| {
            e.expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|n| n.contains(".backup-tmp-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no backup temp file may remain: {leftovers:?}"
    );
}

/// One-shot generator for the committed fixture. It applies ONLY migrations
/// 0001+0002 (the schema v0.0.1 shipped — verified byte-identical to the
/// `v0.0.1` tag before generation) and plants two rows. It is a no-op when the
/// fixture already exists: the fixture must NEVER be regenerated from SQL that
/// may since have been edited (Pitfall 6), or the freeze test proves nothing.
///
/// Run once: `cargo test -p agent-memory-core --test migration_hygiene -- --ignored`
#[test]
#[ignore = "one-shot fixture generator; never regenerate the committed fixture"]
fn generate_v0_0_1_fixture() {
    let path = fixture_path();
    if path.exists() {
        return;
    }
    register_vec_extension().expect("sqlite-vec registers");
    let mut conn = Connection::open(&path).expect("create fixture");
    // v0.0.1 store files are WAL databases (apply_pragmas); mirror that.
    conn.pragma_update(None, "journal_mode", "WAL")
        .expect("WAL mode");
    migrations()
        .to_version(&mut conn, V0_0_1_SCHEMA_VERSION)
        .expect("apply 0001 + 0002 only");
    conn.execute_batch(
        "INSERT INTO memories \
           (mem_type, content, tags, source, scope, base_weight, decay_score, \
            access_count, created_at, last_accessed, expires_at, embedding_status) \
         VALUES \
           ('DECISION', 'fixture decision from a v0.0.1 store', '[\"fixture\"]', \
            'fixture', NULL, 2.0, 1.0, 0, 1000, 1000, NULL, 0), \
           ('TODO', 'fixture todo without a source', '[]', \
            NULL, 'proj', 1.0, 1.0, 0, 1001, 1001, NULL, 0);",
    )
    .expect("plant fixture rows");
    // Fold the WAL into the main file so the committed artifact is one file.
    conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
        .expect("checkpoint");
    drop(conn);
    assert!(
        !PathBuf::from(format!("{}-wal", path.display())).exists()
            || fs::metadata(format!("{}-wal", path.display()))
                .map(|m| m.len() == 0)
                .unwrap_or(true),
        "fixture WAL must be fully checkpointed"
    );
}

#[test]
fn committed_fixture_is_the_v0_0_1_schema() {
    // Guard against a missing/regenerated fixture silently weakening the freeze.
    assert!(
        fixture_path().exists(),
        "tests/fixtures/v0.0.1.db is committed"
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("fixture-copy.db");
    copy_fixture(&db);
    assert_eq!(user_version(&db), V0_0_1_SCHEMA_VERSION as i64);
}

#[test]
fn fixture_migrated_to_latest_matches_a_fresh_database() {
    let dir = tempfile::tempdir().expect("tempdir");
    let upgraded = dir.path().join("upgraded.db");
    let fresh = dir.path().join("fresh.db");
    copy_fixture(&upgraded);

    drop(SqliteStore::open(&upgraded).expect("upgrade v0.0.1 fixture"));
    drop(SqliteStore::open(&fresh).expect("open fresh db"));

    assert_eq!(user_version(&upgraded), LATEST_SCHEMA_VERSION);
    assert_eq!(user_version(&fresh), LATEST_SCHEMA_VERSION);
    let upgraded_schema = normalized_schema(&upgraded);
    assert!(
        upgraded_schema
            .iter()
            .any(|l| l.starts_with("table|memory_links|")),
        "migration 0003 must create memory_links"
    );
    assert_eq!(
        upgraded_schema,
        normalized_schema(&fresh),
        "an upgraded v0.0.1 DB must have exactly the fresh-from-zero schema — \
         shipped 0001/0002 SQL is frozen; append a new migration instead"
    );
}

#[test]
fn upgrading_writes_one_pre_migration_backup_with_the_old_schema() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("memory.db");
    copy_fixture(&db);

    drop(SqliteStore::open(&db).expect("upgrade v0.0.1 fixture"));

    let backups = backups_for_latest(&db);
    assert_eq!(backups.len(), 1, "exactly one backup, got {backups:?}");
    let backup = &backups[0];
    let backup_name = backup
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let stamp = backup_name.rsplit('-').next().unwrap_or_default();
    assert!(
        stamp.len() == 8 && stamp.chars().all(|c| c.is_ascii_digit()),
        "backup name ends in a YYYYMMDD stamp, got {backup_name}"
    );

    // Pitfall 8 proof: the backup was taken BEFORE migrating, so it still
    // carries the v0.0.1 schema version and none of 0003's objects.
    assert_eq!(user_version(backup), V0_0_1_SCHEMA_VERSION as i64);
    assert!(
        !normalized_schema(backup)
            .iter()
            .any(|l| l.contains("memory_links")),
        "backup must predate migration 0003"
    );
    let conn = Connection::open(backup).expect("backup opens as a valid database");
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM memories", [], |row| row.get(0))
        .expect("count backup rows");
    assert_eq!(rows, FIXTURE_ROWS, "backup holds the pre-upgrade data");

    // The live DB itself was migrated.
    assert_eq!(user_version(&db), LATEST_SCHEMA_VERSION);
    assert_no_backup_temp(dir.path());
}

#[test]
fn re_running_the_upgrade_keeps_one_backup_per_schema_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("memory.db");
    let name = "memory.db";
    // A stale backup for the SAME target version must be replaced; a backup
    // for a DIFFERENT version must never be touched (D-11).
    let stale_same = dir.path().join(format!(
        "{name}.backup-pre-{LATEST_SCHEMA_VERSION:04}-20000101"
    ));
    let other_version = dir.path().join(format!("{name}.backup-pre-0002-20000101"));
    fs::write(&stale_same, b"stale").expect("plant stale backup");
    fs::write(&other_version, b"other").expect("plant other-version backup");

    for _ in 0..2 {
        // Reset to a pristine v0.0.1 DB so each open has a migration pending.
        for suffix in ["", "-wal", "-shm"] {
            let p = PathBuf::from(format!("{}{suffix}", db.display()));
            if p.exists() {
                fs::remove_file(&p).expect("reset db");
            }
        }
        copy_fixture(&db);
        drop(SqliteStore::open(&db).expect("upgrade v0.0.1 fixture"));
    }

    let backups = backups_for_latest(&db);
    assert_eq!(
        backups.len(),
        1,
        "one backup per schema version, got {backups:?}"
    );
    assert_ne!(
        backups[0], stale_same,
        "the stale same-version backup is replaced"
    );
    assert_eq!(user_version(&backups[0]), V0_0_1_SCHEMA_VERSION as i64);
    assert_eq!(
        fs::read(&other_version).expect("other-version backup survives"),
        b"other",
        "another schema version's backup is never touched"
    );
    assert_no_backup_temp(dir.path());
}

#[test]
fn opening_a_fresh_database_creates_no_backup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("memory.db");

    drop(SqliteStore::open(&db).expect("open fresh db"));

    let any_backup = fs::read_dir(dir.path())
        .expect("read dir")
        .map(|e| e.expect("dir entry").file_name())
        .any(|n| n.to_string_lossy().contains(".backup-pre-"));
    assert!(!any_backup, "a brand-new DB has nothing to back up (D-12)");
    assert_eq!(user_version(&db), LATEST_SCHEMA_VERSION);
}

#[test]
fn a_newer_schema_database_is_refused_and_left_untouched() {
    register_vec_extension().expect("sqlite-vec registers");
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("memory.db");
    {
        let conn = Connection::open(&db).expect("create db");
        conn.execute_batch("CREATE TABLE from_the_future (x INTEGER);")
            .expect("create table");
        conn.pragma_update(None, "user_version", 99)
            .expect("set user_version");
    }
    let before = fs::read(&db).expect("read db bytes");

    let message = match SqliteStore::open(&db) {
        Ok(_) => panic!("a schema-99 database must be refused"),
        Err(e) => e.to_string(),
    };

    assert!(message.contains("99"), "names the DB version: {message}");
    assert!(
        message.contains(&LATEST_SCHEMA_VERSION.to_string()),
        "names the binary version: {message}"
    );
    assert!(
        message.contains("3"),
        "binary supports version 3: {message}"
    );
    assert!(
        message.contains("brew upgrade unityinflow/tap/agent-memory"),
        "names the upgrade command: {message}"
    );
    assert_eq!(
        fs::read(&db).expect("re-read db bytes"),
        before,
        "the refused DB file must be byte-for-byte untouched (D-13)"
    );
    assert!(
        backups_for_latest(&db).is_empty(),
        "a refused DB is never backed up"
    );
}

#[test]
fn a_failed_backup_aborts_the_open_before_migrating() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("memory.db");
    copy_fixture(&db);
    // A non-removable entry occupying the backup slot: the retention step
    // cannot clear it, so the backup cannot be written — open must abort
    // rather than run an unbacked migration (backup failure = abort).
    let blocker = dir.path().join(format!(
        "memory.db.backup-pre-{LATEST_SCHEMA_VERSION:04}-blocker"
    ));
    fs::create_dir(&blocker).expect("create blocker dir");
    fs::write(blocker.join("keep"), b"x").expect("populate blocker dir");

    let message = match SqliteStore::open(&db) {
        Ok(_) => panic!("open must abort when the backup cannot be written"),
        Err(e) => e.to_string(),
    };

    assert!(
        message.contains("backup"),
        "names the backup failure: {message}"
    );
    assert_eq!(
        user_version(&db),
        V0_0_1_SCHEMA_VERSION as i64,
        "no migration may run without a backup"
    );
}

/// REVIEW WR-01 smoke test: an MCP server and the REST daemon starting at the
/// same moment after `brew upgrade`. Two openers race on a fresh copy of the
/// v0.0.1 fixture; both must start, the DB ends at the latest schema, and the
/// ONE surviving backup is a genuine pre-migration copy. The deterministic
/// proof lives in the seam tests in `store/sqlite.rs`; this exercises the real
/// race window, repeated so a real interleaving is likely to be hit.
#[test]
fn two_concurrent_first_opens_of_a_v0_0_1_database_both_succeed_with_one_genuine_backup() {
    for round in 0..10 {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("memory.db");
        copy_fixture(&db);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let openers: Vec<_> = (0..2)
            .map(|_| {
                let barrier = std::sync::Arc::clone(&barrier);
                let db = db.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    SqliteStore::open(&db).map(drop).map_err(|e| e.to_string())
                })
            })
            .collect();
        for (n, opener) in openers.into_iter().enumerate() {
            let outcome = match opener.join() {
                Ok(outcome) => outcome,
                Err(payload) => {
                    let message = payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                        .unwrap_or_else(|| "non-string panic payload".to_string());
                    panic!("round {round}: opener {n} panicked: {message}");
                }
            };
            if let Err(message) = outcome {
                panic!("round {round}: opener {n} failed to open: {message}");
            }
        }

        assert_eq!(
            user_version(&db),
            LATEST_SCHEMA_VERSION,
            "round {round}: the DB was migrated"
        );
        let backups = backups_for_latest(&db);
        assert_eq!(
            backups.len(),
            1,
            "round {round}: exactly one backup, got {backups:?}"
        );
        assert_eq!(
            user_version(&backups[0]),
            V0_0_1_SCHEMA_VERSION as i64,
            "round {round}: the surviving backup is a genuine pre-migration copy"
        );
        assert_no_backup_temp(dir.path());
    }
}
