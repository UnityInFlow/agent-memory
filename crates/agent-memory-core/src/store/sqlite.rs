//! SQLite-backed [`Store`] implementation.
//!
//! Concurrency model (RESEARCH Pattern 5): one owned writer `Connection` behind a
//! `Mutex` serializes all writes; an `r2d2` read pool serves concurrent reads. WAL
//! mode plus a `busy_timeout` keep readers and the writer from colliding.
//! Connection-level PRAGMAs are applied to every connection as it is created
//! (writer init and the pool customizer); they are not part of a migration.
//!
//! All SQL is parameterized; user input is never formatted into a query string
//! (threat T-01-01).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::backup::Backup;
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::Migrations;

use crate::decay::{DecayConfig, RankWeights, PINNED_HALF_LIFE_MULTIPLIER};
use crate::domain::{
    snippet, ImportedMemory, LinkDirection, LinkKind, LinkOutcome, MemoryError, MemoryPatch,
    MemoryType, MemoryView, NewMemory, RelatedLink, UpdatedMemory,
};
use crate::export::{ExportEvent, ExportSnapshot, Record};
use crate::service::{ListArgs, SearchArgs};
use crate::store::migrations::{migrations, LATEST_SCHEMA_VERSION};
use crate::store::Store;

/// Default cap on returned search rows when the caller omits `limit` (T-02-04).
/// `pub` because it is part of the documented API contract (D-03: the README
/// bounds tables cite it) and the service shares the same cap when truncating
/// the semantic candidate set.
pub const DEFAULT_SEARCH_LIMIT: i64 = 50;

/// Hard cap on the KNN/keyword candidate oversample size `k` (T-02-03: a huge
/// `limit` must not turn into an unbounded vector scan). Defined as 4x the D-01
/// validation bound so the `limit * 4` oversample holds at EVERY valid limit,
/// including `MAX_LIMIT` itself (03-REVIEW WR-01: at `MAX_LIMIT` the old cap
/// collapsed the oversample factor to 1). Still derived from the single source
/// of truth (D-03). This is a candidate-count cap only — it is unrelated to the
/// RRF smoothing constant (`k = 60`) used by hybrid fusion.
const MAX_KNN_K: i64 = crate::domain::MAX_LIMIT * 4;

/// Register the sqlite-vec `vec0` extension process-globally. Idempotent: a
/// `OnceLock` captures the FIRST result (success or error) so repeated calls
/// are cheap and the original error stays visible.
///
/// MUST run BEFORE any `Connection::open` — auto-extensions apply only to
/// connections opened after registration. Calling it at the top of
/// [`SqliteStore::open`] therefore covers the writer, the migration run, and
/// every r2d2 pool connection (the pool opens lazily, after registration).
pub fn register_vec_extension() -> Result<(), rusqlite::Error> {
    static VEC_REGISTRATION: OnceLock<Result<(), String>> = OnceLock::new();
    let result = VEC_REGISTRATION.get_or_init(|| {
        // SAFETY: sqlite3_vec_init is a valid SQLite extension entry point
        // compiled in via the sqlite-vec crate's cc build; the transmute adapts
        // its C signature to rusqlite's RawAutoExtension type. This is the
        // documented rusqlite-0.34+ pattern (sqlite-vec issue #206) — do NOT
        // copy the stale pre-0.34 snippet from the sqlite-vec docs site.
        unsafe {
            let raw: unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> std::os::raw::c_int =
                std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
            rusqlite::auto_extension::register_auto_extension(raw).map_err(|e| e.to_string())
        }
    });
    result.clone().map_err(|msg| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some(format!(
                "sqlite-vec auto-extension registration failed: {msg}"
            )),
        )
    })
}

/// Apply the per-connection PRAGMAs that every connection (writer + pool) needs.
fn apply_pragmas(conn: &Connection) -> Result<(), rusqlite::Error> {
    // busy_timeout FIRST (REVIEW WR-01): several agent-memory processes open
    // one DB at once by design (an MCP server per editor plus the REST
    // daemon), and `journal_mode` and every later pragma can contend for a
    // lock. With the timeout already set they wait out a concurrent opener
    // instead of failing startup with SQLITE_BUSY.
    conn.pragma_update(None, "busy_timeout", 5000)?;
    // journal_mode returns a row; the others do not. Use pragma_update where we can.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// Register an `exp(x)` scalar function on the connection.
///
/// The bundled SQLite is not guaranteed to be compiled with
/// `SQLITE_ENABLE_MATH_FUNCTIONS`, so the decay blend in `search` (which needs
/// `exp`) cannot rely on a built-in. We register a deterministic, side-effect-free
/// `exp` so the recompute-on-read ranking math runs inside the `ORDER BY`.
fn register_functions(conn: &Connection) -> Result<(), rusqlite::Error> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "exp",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let x: f64 = ctx.get(0)?;
            Ok(x.exp())
        },
    )
}

/// Apply both the PRAGMAs and the registered scalar functions to a connection.
fn prepare_connection(conn: &Connection) -> Result<(), rusqlite::Error> {
    apply_pragmas(conn)?;
    register_functions(conn)
}

/// Today's UTC date as `YYYYMMDD`, for the backup file name. UTC only — `Local`
/// time must never be reintroduced (chrono is trimmed to the `now` feature for
/// the darwin cross-link; `Datelike` needs no extra feature).
fn utc_yyyymmdd() -> String {
    use chrono::Datelike;
    let today = chrono::Utc::now().date_naive();
    format!("{:04}{:02}{:02}", today.year(), today.month(), today.day())
}

/// Serializes [`pre_migration_backup`] within one process. The temp file is
/// pid-qualified, which keeps separate processes apart; two openers in the
/// SAME process (threads) would otherwise share one temp path.
static BACKUP_LOCK: Mutex<()> = Mutex::new(());

/// The in-flight temp file for this process's backup of `file_name`:
/// `{db}.backup-tmp-{NNNN}-{pid}`. Deliberately OUTSIDE the
/// `{db}.backup-pre-{NNNN}-` retention prefix, so no opener ever prunes
/// another's in-flight copy.
fn backup_temp_path(dir: &Path, file_name: &str) -> PathBuf {
    dir.join(format!(
        "{file_name}.backup-tmp-{LATEST_SCHEMA_VERSION:04}-{}",
        std::process::id()
    ))
}

/// Best-effort removal of a backup temp file and any SQLite sidecar it left.
/// Never fails: a leftover temp is harmless (T-04-32) and must not mask the
/// error that led here.
fn discard_backup_temp(temp: &Path) {
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let path = PathBuf::from(format!("{}{suffix}", temp.display()));
        if let Err(e) = std::fs::remove_file(&path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(temp = %path.display(), error = %e, "could not remove backup temp file");
            }
        }
    }
}

/// Copy `src` into a NEW database at `temp` with the SQLite Online Backup API
/// and return the copy's `user_version`. The destination connection is
/// closed before returning, so the file can be renamed.
fn copy_into_temp(src: &Connection, temp: &Path) -> Result<i64, String> {
    let mut dst =
        Connection::open(temp).map_err(|e| format!("cannot create {}: {e}", temp.display()))?;
    Backup::new(src, &mut dst)
        .and_then(|backup| backup.run_to_completion(64, Duration::from_millis(50), None))
        .map_err(|e| format!("copy to {} failed: {e}", temp.display()))?;
    let copied_version: i64 = dst
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| format!("cannot read the schema version of {}: {e}", temp.display()))?;
    dst.close()
        .map_err(|(_, e)| format!("cannot close {}: {e}", temp.display()))?;
    Ok(copied_version)
}

/// Remove every `{prefix}*` entry in `dir` except `keep`. An entry that is
/// already gone (a concurrent opener pruned it first) is not an error.
fn prune_other_backups(dir: &Path, prefix: &str, keep: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read directory {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read directory entry: {e}"))?;
        let matches = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(prefix));
        let stale = entry.path();
        if !matches || stale == keep {
            continue;
        }
        match std::fs::remove_file(&stale) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(format!(
                    "cannot remove previous backup {}: {e}",
                    stale.display()
                ))
            }
        }
    }
    Ok(())
}

/// Write the pre-migration backup of `src` (the open DB at `db_path`, probed
/// at schema `expected_version`) to the sibling file
/// `{db}.backup-pre-{NNNN}-{YYYYMMDD}`, where `NNNN` is the zero-padded schema
/// version about to be migrated to (D-11). Generic over the target version,
/// so every future migration inherits it (D-12).
///
/// Race-safe and replace-then-prune (REVIEW WR-01, IN-03):
///
/// 1. The copy goes to a temp file outside the retention prefix
///    ([`backup_temp_path`]), via the SQLite Online Backup API, which is
///    WAL-correct by construction (a raw file copy of a live WAL database is
///    not).
/// 2. The copy's `user_version` must still equal `expected_version`. If it
///    does not, another process migrated the DB between our probe and our
///    copy: the copy is a POST-migration database, so it is discarded and
///    `Ok(None)` is returned. Existing backups are never touched.
/// 3. The temp file is renamed into place, an atomic replace of a same-day
///    stale backup. Only THEN are the other `{db}.backup-pre-{NNNN}-*`
///    entries pruned, so a failed write never costs the previous valid
///    backup. Other versions' backups are never touched.
///
/// The backup sits beside the DB, in the same directory and trust domain —
/// never a shared temp dir (T-04-03).
///
/// Any failure to copy, rename or prune is returned as an error and ABORTS
/// the open: a machine that cannot write a backup must not run an unattended
/// migration (T-04-02). The temp file is removed (best-effort) on every
/// failure path.
fn pre_migration_backup(
    src: &Connection,
    db_path: &Path,
    expected_version: i64,
) -> Result<Option<PathBuf>, MemoryError> {
    let fail = |what: String| {
        MemoryError::Internal(format!(
            "pre-migration backup failed ({what}); refusing to migrate {}",
            db_path.display()
        ))
    };

    let file_name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| fail("database path has no UTF-8 file name".to_string()))?;
    let dir = match db_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let prefix = format!("{file_name}.backup-pre-{LATEST_SCHEMA_VERSION:04}-");
    let temp_path = backup_temp_path(&dir, file_name);
    let final_path = dir.join(format!("{prefix}{}", utc_yyyymmdd()));

    // A poisoned lock only means another thread panicked mid-backup; the
    // guard protects no data, so carry on.
    let _serialized = BACKUP_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let copied_version = match copy_into_temp(src, &temp_path) {
        Ok(version) => version,
        Err(what) => {
            discard_backup_temp(&temp_path);
            return Err(fail(what));
        }
    };

    if copied_version != expected_version {
        discard_backup_temp(&temp_path);
        tracing::info!(
            db = %db_path.display(),
            expected_version,
            copied_version,
            "another process migrated the database first; discarding the post-migration copy"
        );
        return Ok(None);
    }

    if let Err(e) = std::fs::rename(&temp_path, &final_path) {
        discard_backup_temp(&temp_path);
        return Err(fail(format!(
            "cannot move {} into place as {}: {e}",
            temp_path.display(),
            final_path.display()
        )));
    }

    prune_other_backups(&dir, &prefix, &final_path).map_err(fail)?;

    tracing::info!(
        backup = %final_path.display(),
        target_schema_version = LATEST_SCHEMA_VERSION,
        "pre-migration backup written"
    );
    Ok(Some(final_path))
}

/// Run `migrate` and absorb exactly one failure: another process completing
/// the same upgrade first (REVIEW WR-01).
///
/// rusqlite_migration reads `user_version` OUTSIDE its own deferred
/// transaction, so a second opener that probed the old version can attempt
/// 0003's `CREATE TABLE` after the first one committed it, and fail with
/// `table memory_links already exists` or `SQLITE_BUSY_SNAPSHOT`. Its
/// transaction rolls back. On any failure the version is re-read on the same
/// connection: if it is now [`LATEST_SCHEMA_VERSION`], the schema this binary
/// needs is in place, so the open proceeds. Anything else returns the
/// original error. The FTS5 and vec0 smoke checks in [`SqliteStore::open`]
/// still run afterwards, so a half-built schema cannot slip through.
fn migrate_tolerating_concurrent_upgrade(
    conn: &mut Connection,
    migrate: impl FnOnce(&mut Connection) -> Result<(), MemoryError>,
) -> Result<(), MemoryError> {
    let err = match migrate(conn) {
        Ok(()) => return Ok(()),
        Err(err) => err,
    };
    match conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)) {
        Ok(version) if version == LATEST_SCHEMA_VERSION => {
            tracing::info!(
                schema_version = version,
                error = %err,
                "another process completed the schema upgrade first"
            );
            Ok(())
        }
        Ok(_) | Err(_) => Err(err),
    }
}

/// A pool customizer that runs [`apply_pragmas`] on each pooled read connection.
#[derive(Debug)]
struct PragmaCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for PragmaCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        prepare_connection(conn)
    }
}

/// SQLite store: a serialized writer lane plus an r2d2 read pool.
pub struct SqliteStore {
    writer: Mutex<Connection>,
    reads: Pool<SqliteConnectionManager>,
}

impl SqliteStore {
    /// Open (creating if absent) the database at `path`, run migrations on the
    /// writer, and build the read pool. Confirms FTS5 is compiled in.
    ///
    /// Migration hygiene (STORE-05), in this order — the probe and the backup
    /// MUST precede `to_latest` (RESEARCH Pitfall 8):
    ///
    /// 1. Register vec0, open the writer, and set its `busy_timeout`, so every
    ///    later step waits out a concurrent opener (REVIEW WR-01). The timeout
    ///    is connection-level state; it writes nothing to the file.
    /// 2. Probe `PRAGMA user_version`. A DB AHEAD of [`LATEST_SCHEMA_VERSION`]
    ///    is refused with [`MemoryError::SchemaTooNew`] (D-13). The probe runs
    ///    before the other per-connection PRAGMAs, because switching a non-WAL
    ///    file to WAL rewrites its header — a refused DB stays byte-for-byte
    ///    untouched.
    /// 3. Apply the per-connection PRAGMAs and functions.
    /// 4. If migrations are pending on a non-empty DB (`0 < user_version <
    ///    LATEST`, D-12), write a sibling online backup first (D-11), checked
    ///    against the probed version so a copy taken after a concurrent
    ///    migration is discarded, never kept under the pre-migration name. A
    ///    backup failure aborts the open; no migration runs. A brand-new file
    ///    (`user_version == 0`) has nothing to back up.
    /// 5. Migrate to latest, tolerating a concurrent process that completed
    ///    the same upgrade first ([`migrate_tolerating_concurrent_upgrade`]).
    /// 6. Smoke-check FTS5 and vec0, then build the read pool.
    pub fn open(path: &Path) -> Result<Self, MemoryError> {
        // vec0 registration MUST precede Connection::open: auto-extensions only
        // apply to connections opened after registration (this also covers every
        // r2d2 pool connection created below).
        register_vec_extension()?;

        let mut writer = Connection::open(path)?;
        // Connection-level state, set before anything that can contend for the
        // DB (WR-01). It touches nothing on disk, so the D-13 probe below still
        // leaves a refused file byte-for-byte untouched.
        writer.busy_timeout(Duration::from_millis(5000))?;

        // (2) D-13 newer-schema refusal — read-only, before any write.
        let db_version: i64 = writer.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if db_version > LATEST_SCHEMA_VERSION {
            return Err(MemoryError::SchemaTooNew {
                db_version,
                binary_version: LATEST_SCHEMA_VERSION,
            });
        }

        // (3) Per-connection PRAGMAs (busy_timeout first) and functions.
        prepare_connection(&writer)?;

        // (4) D-11/D-12 pre-migration backup. open() runs before serving, so
        // within this process the writer is the only connection and can be
        // the backup source; another process may still be opening the same
        // file, which the version check inside pre_migration_backup handles.
        if db_version > 0 && db_version < LATEST_SCHEMA_VERSION {
            // Ok(None) = another process migrated in between, so our copy was
            // post-migration and was discarded (logged inside); the genuine
            // backup is the one that process wrote. Either way, migrate on.
            pre_migration_backup(&writer, path, db_version)?;
        }

        // (5) Migrate. rusqlite_migration's own DatabaseTooFarAhead check stays
        // as a backstop should the probe above ever be bypassed.
        let migrations: Migrations<'static> = migrations();
        migrate_tolerating_concurrent_upgrade(&mut writer, |conn| {
            migrations.to_latest(conn).map_err(MemoryError::from)
        })?;

        // Smoke-check that FTS5 is available (bundled SQLite compiles it in).
        writer.query_row("SELECT count(*) FROM memories_fts", [], |row| {
            row.get::<_, i64>(0)
        })?;

        // Mirror smoke-check for vec0: fail fast at open if the sqlite-vec
        // extension is missing rather than erroring on the first KNN query.
        writer.query_row("SELECT count(*) FROM vec_memories", [], |row| {
            row.get::<_, i64>(0)
        })?;

        let manager = SqliteConnectionManager::file(path);
        let reads = Pool::builder()
            .connection_customizer(Box::new(PragmaCustomizer))
            .build(manager)
            .map_err(MemoryError::Pool)?;

        Ok(SqliteStore {
            writer: Mutex::new(writer),
            reads,
        })
    }
}

/// Map a `memories` row to a [`MemoryView`]. Column order must match the SELECT.
fn row_to_view(row: &rusqlite::Row<'_>) -> Result<MemoryView, rusqlite::Error> {
    let mem_type_str: String = row.get("mem_type")?;
    let tags_json: String = row.get("tags")?;
    let mem_type = MemoryType::try_from(mem_type_str.as_str()).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid mem_type in row: {mem_type_str}"),
            )),
        )
    })?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(MemoryView {
        id: row.get("id")?,
        content: row.get("content")?,
        mem_type,
        tags,
        scope: row.get("scope")?,
        decay_score: row.get("decay_score")?,
        created_at: row.get("created_at")?,
        last_accessed: row.get("last_accessed")?,
        // Expansion is attached by the service only when asked for (D-04).
        related: None,
    })
}

/// A stored text value that fails its closed-enum parse is corrupt data, not
/// client input: report it as a row conversion failure (the `row_to_view`
/// mem_type precedent), which surfaces on the internal tier.
fn corrupt_column(what: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid {what} in row: {value}"),
        )),
    )
}

/// FTS5 query-parse markers, matched against the SQLite error message.
///
/// - `fts5: ` covers `fts5: syntax error near …` (bare punctuation, a dangling
///   `AND`, an empty query) and `fts5: parser stack overflow` (deep nesting).
/// - `unterminated string`: an unbalanced `"`.
/// - `no such column`: a hyphen or colon makes FTS5 read a column filter
///   (`sqlite-vec` → column `vec`), which fails on the one-column table.
/// - `unknown special query`: a leading `*`.
/// - `expected integer`: a non-numeric `NEAR(…, x)` distance.
/// - `fts5 expression tree is too large`: the query exceeds
///   `SQLITE_FTS5_MAX_EXPR_DEPTH` (256), e.g. 257+ chained `NOT`s, which FTS5
///   never flattens. The message has no `fts5: ` prefix. The sanitized retry
///   cannot hit it: quoted tokens joined by `OR` form one flat node.
///
/// Only FTS5 query-parse messages belong here, never a generic SQLite message
/// (T-04-22): a marker match is what licenses the sanitized retry.
const FTS5_QUERY_PARSE_MARKERS: [&str; 6] = [
    "fts5: ",
    "fts5 expression tree is too large",
    "unterminated string",
    "no such column",
    "unknown special query",
    "expected integer",
];

/// Is `e` FTS5 rejecting the bound MATCH string as client input?
///
/// True only when the error is a `SqliteFailure` whose primary result code is
/// `SQLITE_ERROR` AND whose message carries one of
/// [`FTS5_QUERY_PARSE_MARKERS`]. The message is read from the failure's detail
/// string, falling back to the error's `Display`.
///
/// Invariant: this is only ever handed errors raised AFTER a static statement
/// prepared successfully (statement step / row iteration), so every table and
/// column the SQL names exists. A `no such column` here can therefore only
/// come from FTS5 parsing a column filter out of the MATCH string on the
/// single-column `memories_fts(content)` table. Prepare-time failures (a
/// missing table, a missing fts5 module) never reach this function.
fn is_match_input_error(e: &rusqlite::Error) -> bool {
    let rusqlite::Error::SqliteFailure(code, detail) = e else {
        return false;
    };
    if code.extended_code & 0xff != rusqlite::ffi::SQLITE_ERROR {
        return false;
    }
    let msg = detail.clone().unwrap_or_else(|| e.to_string());
    FTS5_QUERY_PARSE_MARKERS
        .iter()
        .any(|marker| msg.contains(marker))
}

/// Rewrite `raw` as an FTS5 expression that always parses: its word tokens,
/// each quoted as an FTS5 string, joined with ` OR ` (order preserved, no
/// dedupe, no cap).
///
/// A token is a maximal run of `char::is_alphanumeric` or `_` characters;
/// every other character splits. Tokens therefore cannot contain `"`, but any
/// embedded quote is doubled anyway, as FTS5 string syntax requires. Returns
/// `None` when no token remains (empty, punctuation-only, `*`): the keyword
/// leg then has nothing to match.
fn sanitize_fts_query(raw: &str) -> Option<String> {
    let terms: Vec<String> = raw
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|piece| !piece.is_empty())
        .map(|piece| format!("\"{}\"", piece.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

/// Run a keyword MATCH raw first, with ONE sanitized retry (user decision
/// option-a, 2026-10-23; T-04-15 revised).
///
/// - `run(raw)` succeeds → its rows. Valid structured FTS5 syntax (`AND`,
///   quoted phrases, `prefix*`) therefore keeps its exact meaning.
/// - `run(raw)` fails with an FTS5 query-parse error ([`is_match_input_error`])
///   → the query is retried once as [`sanitize_fts_query`]. A tokenless query
///   is an empty answer, never an error. Any failure of the retry is a server
///   fault ([`MemoryError::Sqlite`]): the sanitized form parses by
///   construction.
/// - `run(raw)` fails any other way → [`MemoryError::Sqlite`].
///
/// Both [`Store::search`] (the keyword-degrade path) and
/// [`Store::keyword_candidates`] (the hybrid keyword leg) go through here, so a
/// query classifies identically with the embedder up or down.
fn match_with_sanitized_retry(
    raw: &str,
    mut run: impl FnMut(&str) -> Result<Vec<MemoryView>, rusqlite::Error>,
) -> Result<Vec<MemoryView>, MemoryError> {
    match run(raw) {
        Ok(rows) => Ok(rows),
        Err(e) if is_match_input_error(&e) => {
            // Log the size only: the query text is user content.
            tracing::debug!(
                query_chars = raw.chars().count(),
                "FTS5 could not parse the search query; retrying with its words OR-ed"
            );
            match sanitize_fts_query(raw) {
                None => Ok(Vec::new()),
                Some(fallback) => run(&fallback).map_err(MemoryError::Sqlite),
            }
        }
        Err(e) => Err(MemoryError::Sqlite(e)),
    }
}

/// The column values of one `memories` row about to be inserted, as bound by
/// [`insert_row`]. A struct rather than positional arguments so the two
/// callers cannot transpose a column and the helper stays under clippy's
/// argument limit.
struct MemoryRow<'a> {
    mem_type: MemoryType,
    content: &'a str,
    /// The tags array already serialised with `serde_json`.
    tags_json: &'a str,
    source: Option<&'a str>,
    scope: Option<&'a str>,
    access_count: i64,
    created_at: i64,
    last_accessed: i64,
    expires_at: Option<i64>,
    embedding_status: i64,
}

/// THE one `memories` INSERT, shared by [`Store::insert`] (fresh rows: counts
/// zeroed, both timestamps `now`) and [`Store::insert_imported`] (restored
/// rows: provenance preserved, `last_accessed` refreshed, always pending).
/// Keeping a single statement is what stops the two paths drifting in column
/// order or defaults (Phase 5 T-05-15). `base_weight` follows the type pin and
/// `decay_score` starts at 1.0 for every row. Runs inside the caller's writer
/// transaction and returns the new rowid; the `memories_ai` trigger mirrors
/// the content into FTS5 on the same statement.
fn insert_row(tx: &rusqlite::Transaction<'_>, row: &MemoryRow<'_>) -> Result<i64, rusqlite::Error> {
    let base_weight = if row.mem_type.is_pinned() { 2.0 } else { 1.0 };
    tx.execute(
        "INSERT INTO memories \
         (mem_type, content, tags, source, scope, base_weight, decay_score, \
          access_count, created_at, last_accessed, expires_at, embedding_status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1.0, ?7, ?8, ?9, ?10, ?11)",
        params![
            row.mem_type.as_wire_str(),
            row.content,
            row.tags_json,
            row.source,
            row.scope,
            base_weight,
            row.access_count,
            row.created_at,
            row.last_accessed,
            row.expires_at,
            row.embedding_status,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

impl Store for SqliteStore {
    fn insert(
        &self,
        new: NewMemory,
        embedding: Option<Vec<f32>>,
        now: i64,
    ) -> Result<i64, MemoryError> {
        let tags_json = serde_json::to_string(&new.tags).unwrap_or_else(|_| "[]".to_string());
        let expires_at = new.ttl_secs.map(|ttl| now + ttl);
        let embedding_status: i64 = i64::from(embedding.is_some());

        // ONE writer transaction covers the memories row, the vector row, and
        // the status flag, so `vec_memories` can never hold a vector for a row
        // that failed to insert (T-02-05).
        let mut conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        let tx = conn.transaction()?;
        let id = insert_row(
            &tx,
            &MemoryRow {
                mem_type: new.mem_type,
                content: &new.content,
                tags_json: &tags_json,
                source: new.source.as_deref(),
                scope: new.scope.as_deref(),
                access_count: 0,
                created_at: now,
                last_accessed: now,
                expires_at,
                embedding_status,
            },
        )?;
        if let Some(vector) = embedding {
            // Vector blob bound via bytemuck (safe zero-copy f32→u8 cast),
            // never formatted into the SQL (T-02-01).
            let blob: &[u8] = bytemuck::cast_slice(&vector);
            tx.execute(
                "INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)",
                params![id, blob],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    fn insert_imported(&self, m: ImportedMemory, now: i64) -> Result<i64, MemoryError> {
        let tags_json = serde_json::to_string(&m.tags).unwrap_or_else(|_| "[]".to_string());

        // Same writer lane and single transaction as `insert`, through the same
        // shared statement. The file's provenance columns are bound as-is;
        // `last_accessed` is the import-time clock (D-10). There is no vector
        // write and `embedding_status` is hard-coded pending: restored rows are
        // re-embedded only by the sweep backfill against the LOCAL model pin
        // (D-13, PITFALLS 17), so a file written beside another model can never
        // mix vectors into this corpus.
        let mut conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        let tx = conn.transaction()?;
        let id = insert_row(
            &tx,
            &MemoryRow {
                mem_type: m.mem_type,
                content: &m.content,
                tags_json: &tags_json,
                source: m.source.as_deref(),
                scope: m.scope.as_deref(),
                access_count: m.access_count,
                created_at: m.created_at,
                last_accessed: now,
                expires_at: m.expires_at,
                embedding_status: 0,
            },
        )?;
        tx.commit()?;
        Ok(id)
    }

    fn knn_search(
        &self,
        query: Vec<f32>,
        args: SearchArgs,
    ) -> Result<Vec<(MemoryView, f64)>, MemoryError> {
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        let mem_type_filter: Option<String> = args.mem_type.map(|t| t.as_wire_str().to_string());
        // Oversample beyond the final limit (RESEARCH A6): the service re-ranks
        // candidates by the similarity×decay blend, so fetch limit*4, capped.
        let k = (args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).max(1) * 4).min(MAX_KNN_K);
        // The query vector binds as a BLOB via bytemuck — never string-formatted
        // into the SQL (T-02-01). `k` and every filter are bound too. The tag
        // filter is an exact, case-sensitive json_each equality over the JSON
        // tags array (API-03 / D-07) — same bound parameter as before.
        let query_blob: &[u8] = bytemuck::cast_slice(&query);

        let mut stmt = conn.prepare(
            "WITH knn AS (SELECT memory_id, distance FROM vec_memories \
                          WHERE embedding MATCH ?1 AND k = ?2) \
             SELECT m.id, m.mem_type, m.content, m.tags, m.source, m.scope, \
                    m.base_weight, m.decay_score, m.access_count, m.created_at, \
                    m.last_accessed, m.expires_at, knn.distance \
             FROM knn JOIN memories m ON m.id = knn.memory_id \
             WHERE (?3 IS NULL OR m.mem_type = ?3) \
               AND (?4 IS NULL OR m.scope = ?4) \
               AND (?5 IS NULL OR EXISTS (SELECT 1 FROM json_each(m.tags) \
                                          WHERE json_each.value = ?5)) \
             ORDER BY knn.distance ASC",
        )?;

        let rows = stmt.query_map(
            params![query_blob, k, mem_type_filter, args.scope, args.tag],
            |row| {
                let view = row_to_view(row)?;
                let distance: f64 = row.get("distance")?;
                Ok((view, distance))
            },
        )?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn insert_embedding(
        &self,
        memory_id: i64,
        expected_content: &str,
        embedding: Vec<f32>,
    ) -> Result<bool, MemoryError> {
        let mut conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        let tx = conn.transaction()?;
        // CR-03: claim the row FIRST. It must still be pending AND still hold
        // the exact text that was embedded. An update (content changed, or
        // status already 1), a forget (row gone, or a reused rowid with other
        // content), or a racing sweep (status already 1) all match nothing,
        // and then nothing is written: the tx rolls back on drop. SQLite takes
        // the write lock at this statement, so the claim is atomic across
        // processes sharing the DB file, not only across this process's
        // threads (the writer mutex is per process).
        let claimed = tx.execute(
            "UPDATE memories SET embedding_status = 1 \
             WHERE id = ?1 AND embedding_status = 0 AND content = ?2",
            params![memory_id, expected_content],
        )?;
        if claimed == 0 {
            return Ok(false);
        }
        let blob: &[u8] = bytemuck::cast_slice(&embedding);
        // DELETE + INSERT instead of INSERT OR REPLACE: conflict-resolution
        // clauses are not reliably supported on virtual tables, and the two
        // statements share one writer transaction anyway.
        tx.execute(
            "DELETE FROM vec_memories WHERE memory_id = ?1",
            params![memory_id],
        )?;
        tx.execute(
            "INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)",
            params![memory_id, blob],
        )?;
        tx.commit()?;
        Ok(true)
    }

    fn pending_embeddings(&self, limit: usize) -> Result<Vec<(i64, String)>, MemoryError> {
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        let mut stmt = conn.prepare(
            "SELECT id, content FROM memories \
             WHERE embedding_status = 0 ORDER BY id LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn export_rows(
        &self,
        visit: &mut dyn FnMut(ExportEvent) -> Result<(), MemoryError>,
    ) -> Result<(), MemoryError> {
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        // ONE read transaction on ONE pooled connection spans the pin, both
        // counts, and both row scans, so everything below is one WAL snapshot:
        // a link committed concurrently can never reference a memory this
        // export did not also see (T-05-19 / PITFALLS 5). `unchecked` only
        // because the pooled handle is shared (`&Connection`); nothing else
        // runs on it while we hold it.
        let tx = conn.unchecked_transaction()?;

        let embedding_model: String = tx.query_row(
            "SELECT value FROM meta WHERE key = 'embedding_model'",
            [],
            |row| row.get(0),
        )?;
        let dim_text: String = tx.query_row(
            "SELECT value FROM meta WHERE key = 'embedding_dim'",
            [],
            |row| row.get(0),
        )?;
        // `meta.value` is TEXT; a non-integer dim is corrupt data (internal
        // tier), never something a client did.
        let embedding_dim: u32 = dim_text.trim().parse().map_err(|_| {
            MemoryError::Internal(format!(
                "meta.embedding_dim is not an integer ({dim_text:?})"
            ))
        })?;
        let memories: i64 = tx.query_row("SELECT count(*) FROM memories", [], |row| row.get(0))?;
        let links: i64 = tx.query_row("SELECT count(*) FROM memory_links", [], |row| row.get(0))?;
        visit(ExportEvent::Snapshot(ExportSnapshot {
            embedding_model,
            embedding_dim,
            memories: u64::try_from(memories).unwrap_or(0),
            links: u64::try_from(links).unwrap_or(0),
        }))?;

        {
            // The D-04 memory column list — the wire contract. Deliberately
            // NOT `base_weight`, `decay_score` or `embedding_status`, and
            // `vec_memories` is never read: derived data and vectors stay home.
            let mut stmt = tx.prepare(
                "SELECT id, mem_type, content, tags, source, scope, created_at, \
                        last_accessed, access_count, expires_at \
                 FROM memories ORDER BY id",
            )?;
            let rows = stmt.query_map([], |row| {
                let tags_json: String = row.get("tags")?;
                Ok(Record::Memory {
                    key: row.get("id")?,
                    mem_type: row.get("mem_type")?,
                    content: row.get("content")?,
                    tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                    source: row.get("source")?,
                    scope: row.get("scope")?,
                    created_at: row.get("created_at")?,
                    last_accessed: row.get("last_accessed")?,
                    access_count: row.get("access_count")?,
                    expires_at: row.get("expires_at")?,
                })
            })?;
            for row in rows {
                visit(ExportEvent::Record(row?))?;
            }
        }

        {
            // Endpoints exactly as stored (relates_to canonical (min, max),
            // directed kinds as given — D-05); deterministic order so two
            // exports of one database are byte-identical.
            let mut stmt = tx.prepare(
                "SELECT from_id, to_id, kind, created_at FROM memory_links \
                 ORDER BY from_id, to_id, kind",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(Record::Link {
                    from: row.get("from_id")?,
                    to: row.get("to_id")?,
                    kind: row.get("kind")?,
                    created_at: row.get("created_at")?,
                })
            })?;
            for row in rows {
                visit(ExportEvent::Record(row?))?;
            }
        }

        // Read-only: commit simply ends the snapshot.
        tx.commit()?;
        Ok(())
    }

    fn find_by_key(
        &self,
        source: Option<&str>,
        mem_type: MemoryType,
        content: &str,
        now: i64,
    ) -> Result<Option<i64>, MemoryError> {
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        // The INTEROP-01 dedup probe: all key values bound, never formatted
        // into the SQL (T-02-20). `source IS ?1` is SQLite's NULL-safe
        // equality: a `None` source binds NULL and matches the NULL the insert
        // stored (`source = ?1` would be NULL = NULL → never true, which made
        // every source-less re-import a duplicate — CR-01). The matching id
        // comes back so an importer can map onto the existing row (Phase 5
        // D-07).
        //
        // Only live rows count: `expires_at <= now` is the importer's own
        // "already expired" predicate for file rows (D-10), applied here to the
        // target so an edge is never hung on a row the next sweep deletes
        // (05-REVIEW WR-03). `ORDER BY id` pins which duplicate wins when the
        // key is not unique — the lowest id, by contract (05-REVIEW IN-01).
        let found = conn
            .query_row(
                "SELECT id FROM memories \
                 WHERE source IS ?1 AND mem_type = ?2 AND content = ?3 \
                   AND (expires_at IS NULL OR expires_at > ?4) \
                 ORDER BY id LIMIT 1",
                params![source, mem_type.as_wire_str(), content, now],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        Ok(found)
    }

    fn list(&self, args: ListArgs, _now: i64) -> Result<Vec<MemoryView>, MemoryError> {
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        let mem_type_filter: Option<String> = args.mem_type.map(|t| t.as_wire_str().to_string());

        // All filters parameterized; a NULL bound disables that predicate. The
        // tag filter matches whole tags exactly and case-sensitively via
        // json_each equality (API-03 / D-07).
        let mut stmt = conn.prepare(
            "SELECT id, mem_type, content, tags, source, scope, base_weight, \
                    decay_score, access_count, created_at, last_accessed, expires_at \
             FROM memories \
             WHERE (?1 IS NULL OR mem_type = ?1) \
               AND (?2 IS NULL OR scope = ?2) \
               AND (?3 IS NULL OR EXISTS (SELECT 1 FROM json_each(memories.tags) \
                                          WHERE json_each.value = ?3)) \
             ORDER BY created_at DESC, id DESC \
             LIMIT CASE WHEN ?4 IS NULL THEN -1 ELSE ?4 END",
        )?;

        let rows = stmt.query_map(
            params![mem_type_filter, args.scope, args.tag, args.limit],
            row_to_view,
        )?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn search(
        &self,
        args: SearchArgs,
        now: i64,
        weights: RankWeights,
        cfg: DecayConfig,
    ) -> Result<Vec<MemoryView>, MemoryError> {
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        let mem_type_filter: Option<String> = args.mem_type.map(|t| t.as_wire_str().to_string());
        let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let pinned_hl = cfg.half_life_secs * PINNED_HALF_LIFE_MULTIPLIER;

        // CRITICAL sign rule (RESEARCH Pattern 3): bm25() is SMALLER = better, so
        // negate it before blending additively with the decay term (larger =
        // better). The decay term is recomputed inline from last_accessed at `now`
        // (recompute-on-read, Open Question 3) rather than the materialised column,
        // so a recency bump immediately re-ranks. The CASE picks the longer
        // half-life for pinned types (DECISION/ARCHITECTURE/CONSTRAINT, D-08).
        // All params bound — the FTS5 MATCH string is never concatenated (T-02-01).
        // The tag filter is an exact, case-sensitive json_each equality
        // (API-03 / D-07); the FTS5 MATCH retry below is unaffected by it.
        let mut stmt = conn.prepare(
            "SELECT m.id, m.mem_type, m.content, m.tags, m.source, m.scope, \
                    m.base_weight, m.decay_score, m.access_count, m.created_at, \
                    m.last_accessed, m.expires_at \
             FROM memories_fts \
             JOIN memories m ON m.id = memories_fts.rowid \
             WHERE memories_fts MATCH ?1 \
               AND (?2 IS NULL OR m.mem_type = ?2) \
               AND (?3 IS NULL OR m.scope = ?3) \
               AND (?10 IS NULL OR EXISTS (SELECT 1 FROM json_each(m.tags) \
                                           WHERE json_each.value = ?10)) \
             ORDER BY ( (-bm25(memories_fts)) * ?4 \
                        + exp( -0.6931471805599453 * MAX(?6 - m.last_accessed, 0) \
                               / (CASE WHEN m.mem_type IN \
                                    ('DECISION','ARCHITECTURE','CONSTRAINT') \
                                  THEN ?8 ELSE ?7 END) ) * ?5 \
                      ) DESC \
             LIMIT ?9",
        )?;

        // The bound MATCH string parses at step time, not at prepare, so an
        // FTS5 parse error surfaces inside `run`. Revised T-04-15 (option-a):
        // the raw query runs first, so structured FTS5 syntax keeps its
        // meaning; FTS5-unparseable client input gets one sanitized retry; a
        // tokenless query is an empty vec. This is the same helper the hybrid
        // keyword leg uses, so both modes are equally forgiving. A valid query
        // that matches nothing yields an empty vec.
        let run = |match_query: &str| -> Result<Vec<MemoryView>, rusqlite::Error> {
            stmt.query_map(
                params![
                    match_query,
                    mem_type_filter,
                    args.scope,
                    weights.relevance,
                    weights.decay,
                    now,
                    cfg.half_life_secs,
                    pinned_hl,
                    limit,
                    args.tag,
                ],
                row_to_view,
            )?
            .collect()
        };
        match_with_sanitized_retry(&args.query, run)
    }

    fn keyword_candidates(&self, args: SearchArgs) -> Result<Vec<MemoryView>, MemoryError> {
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        let mem_type_filter: Option<String> = args.mem_type.map(|t| t.as_wire_str().to_string());
        // Same oversample as knn_search, so the two fused legs are symmetric
        // (SEARCH-04 / RESEARCH Pitfall 3).
        let fetch = (args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).max(1) * 4).min(MAX_KNN_K);

        // Pure relevance (RESEARCH Pitfall 1): bm25() is SMALLER = better, so
        // ascending order is best first, with no negation and no decay term.
        // Decay is applied once by the service, after fusion. The id tie-break
        // keeps the leg deterministic. FROM/JOIN, filters and the
        // match_with_sanitized_retry routing are copied from `search`, and every
        // value is bound, never concatenated (T-04-18).
        let mut stmt = conn.prepare(
            "SELECT m.id, m.mem_type, m.content, m.tags, m.source, m.scope, \
                    m.base_weight, m.decay_score, m.access_count, m.created_at, \
                    m.last_accessed, m.expires_at \
             FROM memories_fts \
             JOIN memories m ON m.id = memories_fts.rowid \
             WHERE memories_fts MATCH ?1 \
               AND (?2 IS NULL OR m.mem_type = ?2) \
               AND (?3 IS NULL OR m.scope = ?3) \
               AND (?4 IS NULL OR EXISTS (SELECT 1 FROM json_each(m.tags) \
                                          WHERE json_each.value = ?4)) \
             ORDER BY bm25(memories_fts) ASC, m.id ASC \
             LIMIT ?5",
        )?;

        // Revised T-04-15 (option-a): the same raw-first, one-sanitized-retry
        // helper as `search`, so a query classifies identically with the
        // embedder up or down. An FTS5-unparseable query never fails the leg,
        // and a tokenless one contributes no keyword candidates.
        let run = |match_query: &str| -> Result<Vec<MemoryView>, rusqlite::Error> {
            stmt.query_map(
                params![match_query, mem_type_filter, args.scope, args.tag, fetch],
                row_to_view,
            )?
            .collect()
        };
        match_with_sanitized_retry(&args.query, run)
    }

    fn forget(&self, id: i64) -> Result<bool, MemoryError> {
        let mut conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        let tx = conn.transaction()?;
        // Parameterized DELETEs. The FTS5 delete trigger syncs memories_fts,
        // but that is true for FTS ONLY — vec0 virtual tables ignore triggers,
        // so the vector row must be deleted explicitly here (RESEARCH Pitfall 3
        // / T-02-05: a forgotten memory must never resurface semantically).
        tx.execute("DELETE FROM vec_memories WHERE memory_id = ?1", params![id])?;
        let changed = tx.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        // Relation edges: `ON DELETE CASCADE` (foreign_keys = ON on every
        // connection, apply_pragmas) already removed this row's edges. The
        // explicit sweep below is the belt-and-braces backstop for any
        // connection that ever skipped the pragma — it also heals orphans such
        // a connection may have left behind (MCP-07 / T-04-12, milestone
        // Pitfall 9) and is what stops a recycled rowid from ever re-attaching
        // an old edge to an unrelated memory. Mirrors the vec_memories orphan
        // idiom in sweep_expired.
        tx.execute(
            "DELETE FROM memory_links WHERE from_id NOT IN (SELECT id FROM memories) \
             OR to_id NOT IN (SELECT id FROM memories)",
            [],
        )?;
        tx.commit()?;
        Ok(changed > 0)
    }

    fn link(
        &self,
        from_id: i64,
        to_id: i64,
        kind: LinkKind,
        now: i64,
    ) -> Result<LinkOutcome, MemoryError> {
        let mut conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        let tx = conn.transaction()?;

        // Pre-check BOTH endpoints inside the writer tx and report the first
        // missing id as data. Never rely on the FK violation: parsing a
        // SqliteFailure would surface a client mistake as a 500 (T-04-13).
        for id in [from_id, to_id] {
            let exists = tx
                .query_row("SELECT 1 FROM memories WHERE id = ?1", params![id], |_| {
                    Ok(())
                })
                .optional()?
                .is_some();
            if !exists {
                // The tx rolls back on drop; nothing was written.
                return Ok(LinkOutcome::NotFound { missing_id: id });
            }
        }

        // The (from_id, to_id, kind) primary key makes a duplicate an
        // idempotent no-op: changes() == 0 is still a success (D-02), now
        // reported as `Linked { created: false }` so an importer can count it
        // (Phase 5 D-12). The kind binds only the closed enum's wire form
        // (T-04-10). The endpoints are
        // the STORED pair: relates_to is canonicalised to (min, max), so b->a
        // after a->b hits the same key and stays one symmetric relation
        // (REVIEW WR-03); directed kinds bind the request order unchanged.
        // The pre-check above stays in request order, so NotFound still names
        // the request's from_id first.
        let (stored_from, stored_to) = kind.canonical_endpoints(from_id, to_id);
        tx.execute(
            "INSERT OR IGNORE INTO memory_links (from_id, to_id, kind, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![stored_from, stored_to, kind.as_wire_str(), now],
        )?;
        // Read before commit: changes() reports the most recent statement on
        // this connection, and INSERT OR IGNORE changes exactly one row when it
        // inserted and zero when the primary key already existed.
        let created = tx.changes() == 1;
        tx.commit()?;
        Ok(LinkOutcome::Linked { created })
    }

    fn unlink(&self, from_id: i64, to_id: i64, kind: LinkKind) -> Result<bool, MemoryError> {
        let conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        // The requested orientation always matches. For a symmetric kind
        // (relates_to) the reversed orientation matches too (?4 = 1), so the
        // relation can be removed from either end (REVIEW WR-03): that covers
        // the canonical (min, max) row link() writes, and is also the backstop
        // for any relates_to row a pre-fix dev build of the unreleased 0003
        // stored in request order. Every value is bound, never formatted.
        let removed = conn.execute(
            "DELETE FROM memory_links WHERE kind = ?3 \
               AND ((from_id = ?1 AND to_id = ?2) \
                    OR (?4 = 1 AND from_id = ?2 AND to_id = ?1))",
            params![from_id, to_id, kind.as_wire_str(), kind.is_symmetric()],
        )?;
        Ok(removed > 0)
    }

    fn update(
        &self,
        id: i64,
        patch: MemoryPatch,
        embedding: Option<Vec<f32>>,
        now: i64,
    ) -> Result<Option<UpdatedMemory>, MemoryError> {
        let mut conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        let tx = conn.transaction()?;

        // Current values, read inside the writer tx so the merge below cannot
        // race another write. Absent → clean not-found (the tx rolls back on
        // drop; nothing was written).
        let current = tx
            .query_row(
                "SELECT content, tags, scope, expires_at FROM memories WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((cur_content, cur_tags_json, cur_scope, cur_expires_at)) = current else {
            return Ok(None);
        };

        // D-08 merge in Rust: None = unchanged, Some(None) = clear,
        // Some(Some(v)) = set. The service has already rejected null/empty
        // content, so a `Some(None)` content is unreachable — treated as
        // unchanged rather than trusted to clear a NOT NULL column.
        let (content, content_changed) = match patch.content {
            Some(Some(new_content)) => (new_content, true),
            Some(None) | None => (cur_content, false),
        };
        let tags_json = match patch.tags {
            // Same JSON text encoding insert() writes.
            Some(Some(tags)) => serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string()),
            Some(None) => "[]".to_string(),
            None => cur_tags_json,
        };
        let scope = match patch.scope {
            Some(Some(scope)) => Some(scope),
            // NULL = global: the default insert() stores for an omitted scope.
            Some(None) => None,
            None => cur_scope,
        };
        let expires_at = match patch.ttl_secs {
            // Recomputed from patch time; ttl bounds already validated, so the
            // addition cannot overflow (MAX_TTL_SECS keeps it far from i64::MAX).
            Some(Some(ttl)) => Some(now + ttl),
            Some(None) => None,
            None => cur_expires_at,
        };

        // INVARIANT — id immutability: the row is updated IN PLACE, never
        // removed and re-inserted. `memories.id` is a plain INTEGER PRIMARY KEY
        // whose rowids SQLite may recycle, so a delete+insert "update" could
        // silently re-attach relation edges to an unrelated memory (T-04-08).
        //
        // D-09: the freshness bump rides in the same statement (bump_access
        // semantics) — an update is the strongest recency signal. decay_score
        // is reset to 1.0, the exact value of the decay curve at
        // last_accessed = now (insert() stores the same literal).
        //
        // The memories_au trigger re-mirrors the FTS5 content row for free.
        // All values bound — never formatted into the SQL (T-04-06).
        //
        // The bump saturates at i64::MAX (same expression as bump_access):
        // SQLite's `+` does not raise on integer overflow, it converts the
        // result to REAL, and rusqlite's `FromSql for i64` then rejects the
        // column on every later export_rows read (05-REVIEW WR-02).
        tx.execute(
            "UPDATE memories SET content = ?2, tags = ?3, scope = ?4, expires_at = ?5, \
                 last_accessed = ?6, \
                 access_count = CASE WHEN access_count < 9223372036854775807 \
                                     THEN access_count + 1 ELSE access_count END, \
                 decay_score = 1.0 \
             WHERE id = ?1",
            params![id, content, tags_json, scope, expires_at, now],
        )?;

        if content_changed {
            // vec0 ignores triggers: the stale vector must go explicitly, in
            // THIS transaction, or the old meaning would keep matching.
            tx.execute("DELETE FROM vec_memories WHERE memory_id = ?1", params![id])?;
            match embedding {
                Some(vector) => {
                    let blob: &[u8] = bytemuck::cast_slice(&vector);
                    tx.execute(
                        "INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)",
                        params![id, blob],
                    )?;
                    tx.execute(
                        "UPDATE memories SET embedding_status = 1 WHERE id = ?1",
                        params![id],
                    )?;
                }
                None => {
                    // Embed outage (or skipped): pending — the sweep backfill
                    // re-embeds it once the embedder recovers (SEARCH-03).
                    tx.execute(
                        "UPDATE memories SET embedding_status = 0 WHERE id = ?1",
                        params![id],
                    )?;
                }
            }
        }

        let updated = tx.query_row(
            "SELECT id, mem_type, content, tags, source, scope, base_weight, \
                    decay_score, access_count, created_at, last_accessed, expires_at, \
                    embedding_status \
             FROM memories WHERE id = ?1",
            params![id],
            |row| {
                Ok(UpdatedMemory {
                    view: row_to_view(row)?,
                    embedding_status: row.get("embedding_status")?,
                })
            },
        )?;
        tx.commit()?;
        Ok(Some(updated))
    }

    fn related(&self, id: i64, cap: usize) -> Result<Vec<RelatedLink>, MemoryError> {
        // Read pool: expansion is a pure read and never touches the writer.
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        // Both directions in one pass (D-03): the CASE picks the FAR endpoint
        // of each edge touching `id`. The from_id lookup rides the primary key
        // and the to_id lookup rides idx_links_to. Newest edges first, so the
        // LIMIT keeps the most recent (D-07); the id/kind tail only makes ties
        // deterministic. All values bound (T-04-10).
        let mut stmt = conn.prepare(
            "SELECT l.from_id, l.kind, m.id, m.mem_type, m.content, m.tags \
             FROM memory_links l \
             JOIN memories m \
               ON m.id = CASE WHEN l.from_id = ?1 THEN l.to_id ELSE l.from_id END \
             WHERE l.from_id = ?1 OR l.to_id = ?1 \
             ORDER BY l.created_at DESC, m.id DESC, l.kind ASC \
             LIMIT ?2",
        )?;
        let cap = i64::try_from(cap).unwrap_or(i64::MAX);
        let rows = stmt.query_map(params![id, cap], |row| {
            let from_id: i64 = row.get(0)?;
            let kind_str: String = row.get(1)?;
            let mem_type_str: String = row.get(3)?;
            let content: String = row.get(4)?;
            let tags_json: String = row.get(5)?;
            let kind = LinkKind::try_from(kind_str.as_str())
                .map_err(|_| corrupt_column("link kind", &kind_str))?;
            let mem_type = MemoryType::try_from(mem_type_str.as_str())
                .map_err(|_| corrupt_column("mem_type", &mem_type_str))?;
            // Seen from the anchor: an outgoing edge reads its stored kind,
            // an incoming one its natural reverse label (superseded_by, caused).
            let (direction, label) = if from_id == id {
                (LinkDirection::Out, kind.as_wire_str())
            } else {
                (LinkDirection::In, kind.reverse_label())
            };
            Ok(RelatedLink {
                id: row.get(2)?,
                kind: label.to_string(),
                direction,
                mem_type,
                snippet: snippet(&content),
                tags: serde_json::from_str(&tags_json).unwrap_or_default(),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn bump_access(&self, ids: &[i64], now: i64) -> Result<(), MemoryError> {
        if ids.is_empty() {
            return Ok(());
        }
        let conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        // Build a parameterized IN-list (?2, ?3, …); ?1 is `now`. The ids are
        // bound, never string-formatted into the SQL (T-02-01).
        let placeholders: String = (0..ids.len())
            .map(|i| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        // The increment saturates at i64::MAX: SQLite's `+` silently converts
        // an overflowing integer to REAL, which rusqlite's `i64` reader then
        // rejects on every later `export_rows` — one poisoned row would break
        // the whole backup path (05-REVIEW WR-02). An imported row may carry
        // any non-negative count verbatim (D-10), so the guard lives here.
        let sql = format!(
            "UPDATE memories SET last_accessed = ?1, \
                 access_count = CASE WHEN access_count < 9223372036854775807 \
                                     THEN access_count + 1 ELSE access_count END \
             WHERE id IN ({placeholders})"
        );
        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 1);
        params.push(&now);
        for id in ids {
            params.push(id);
        }
        conn.execute(&sql, params.as_slice())?;
        Ok(())
    }

    fn sweep_expired(&self, now: i64) -> Result<usize, MemoryError> {
        let mut conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        let tx = conn.transaction()?;
        // The ONLY memories delete the sweep performs (STORE-04). Bound
        // predicate: NULL `expires_at` rows are never matched, so a no-TTL
        // memory is never removed here. The FTS5 AFTER DELETE trigger keeps
        // memories_fts in sync — but vec0 ignores triggers (Pitfall 3), so
        // vectors orphaned by the TTL delete are cleared explicitly in the
        // SAME transaction (T-02-05: vec_memories never drifts from memories).
        let deleted = tx.execute(
            "DELETE FROM memories WHERE expires_at IS NOT NULL AND expires_at < ?1",
            params![now],
        )?;
        tx.execute(
            "DELETE FROM vec_memories WHERE memory_id NOT IN (SELECT id FROM memories)",
            [],
        )?;
        // Edges of the expired rows: cascaded by the FK, with the same
        // explicit backstop as forget() (MCP-07 / T-04-12).
        tx.execute(
            "DELETE FROM memory_links WHERE from_id NOT IN (SELECT id FROM memories) \
             OR to_id NOT IN (SELECT id FROM memories)",
            [],
        )?;
        tx.commit()?;
        Ok(deleted)
    }

    fn materialize_decay(&self, now: i64, cfg: &DecayConfig) -> Result<usize, MemoryError> {
        let conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        let pinned_hl = cfg.half_life_secs * PINNED_HALF_LIFE_MULTIPLIER;
        // UPDATE-only: recompute decay_score inline from last_accessed at `now`,
        // using the SAME exp/half-life math as the on-read search blend so the
        // materialized column matches the recompute-on-read value exactly
        // (STORE-03). The CASE selects the longer pinned half-life (D-08). This
        // statement physically cannot delete a row — decay never removes (STORE-04).
        let updated = conn.execute(
            "UPDATE memories SET decay_score = \
               exp( -0.6931471805599453 * MAX(?1 - last_accessed, 0) \
                    / (CASE WHEN mem_type IN \
                         ('DECISION','ARCHITECTURE','CONSTRAINT') \
                       THEN ?3 ELSE ?2 END) )",
            params![now, cfg.half_life_secs, pinned_hl],
        )?;
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // STORE-05 backup race and failure seams (REVIEW WR-01, IN-03). Real temp
    // files; vec0 registered first because SqliteStore::open runs 0002.
    // -----------------------------------------------------------------------

    /// A raw database at schema version 2 with one table, standing in for a
    /// v0.0.1 store whose upgrade is pending.
    fn raw_v2_db(path: &Path) -> Connection {
        register_vec_extension().expect("sqlite-vec registers");
        let conn = Connection::open(path).expect("create raw db");
        conn.execute_batch("CREATE TABLE pre_upgrade (x INTEGER); PRAGMA user_version = 2;")
            .expect("build v2 db");
        conn
    }

    fn backup_prefix(db: &Path) -> String {
        let name = db.file_name().and_then(|n| n.to_str()).expect("db name");
        format!("{name}.backup-pre-{LATEST_SCHEMA_VERSION:04}-")
    }

    fn dir_names(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .expect("read dir")
            .map(|e| {
                e.expect("dir entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn assert_no_backup_temp(dir: &Path) {
        let names = dir_names(dir);
        assert!(
            !names.iter().any(|n| n.contains(".backup-tmp-")),
            "no backup temp file may remain: {names:?}"
        );
    }

    #[test]
    fn a_backup_of_an_already_migrated_database_is_discarded_and_keeps_the_valid_one() {
        register_vec_extension().expect("sqlite-vec registers");
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("memory.db");
        // Another process already migrated: the DB is at LATEST.
        drop(SqliteStore::open(&db).expect("create a latest-schema db"));
        let valid = dir.path().join(format!("{}20000101", backup_prefix(&db)));
        std::fs::write(&valid, b"valid").expect("plant the genuine backup");
        let conn = Connection::open(&db).expect("open raw");

        // We probed 2 before the other process migrated.
        let outcome = pre_migration_backup(&conn, &db, 2).expect("not an error");

        assert!(
            outcome.is_none(),
            "a post-migration copy is never kept: {outcome:?}"
        );
        assert_eq!(
            std::fs::read(&valid).expect("the genuine backup survives"),
            b"valid",
            "an existing backup is never touched by a discarded copy"
        );
        assert_no_backup_temp(dir.path());
    }

    #[test]
    fn a_valid_backup_replaces_then_prunes_the_stale_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("memory.db");
        let conn = raw_v2_db(&db);
        let stale = dir.path().join(format!("{}20000101", backup_prefix(&db)));
        std::fs::write(&stale, b"stale").expect("plant stale backup");

        let backup = pre_migration_backup(&conn, &db, 2)
            .expect("backup succeeds")
            .expect("a genuine pre-migration copy is kept");

        let name = backup.file_name().and_then(|n| n.to_str()).expect("name");
        assert_eq!(name, format!("{}{}", backup_prefix(&db), utc_yyyymmdd()));
        let copy = Connection::open(&backup).expect("backup opens");
        let version: i64 = copy
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read copy version");
        assert_eq!(version, 2, "the backup carries the pre-migration schema");
        assert!(!stale.exists(), "the stale same-version backup is pruned");
        assert_no_backup_temp(dir.path());
    }

    #[test]
    fn a_failed_backup_write_never_removes_the_previous_backup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("memory.db");
        let conn = raw_v2_db(&db);
        let stale = dir.path().join(format!("{}20000101", backup_prefix(&db)));
        std::fs::write(&stale, b"previous").expect("plant previous backup");
        // A non-empty directory occupying this process's temp path: the copy
        // cannot even be created.
        let blocker = backup_temp_path(dir.path(), "memory.db");
        std::fs::create_dir(&blocker).expect("create blocker dir");
        std::fs::write(blocker.join("keep"), b"x").expect("populate blocker dir");

        let message = match pre_migration_backup(&conn, &db, 2) {
            Ok(outcome) => panic!("the backup must fail, got {outcome:?}"),
            Err(e) => e.to_string(),
        };

        assert!(message.contains("backup"), "names the backup: {message}");
        assert_eq!(
            std::fs::read(&stale).expect("the previous backup survives"),
            b"previous",
            "IN-03: nothing is deleted before a valid replacement exists"
        );
    }

    #[test]
    fn a_backup_that_cannot_be_moved_into_place_keeps_the_previous_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("memory.db");
        let conn = raw_v2_db(&db);
        let stale = dir.path().join(format!("{}20000101", backup_prefix(&db)));
        std::fs::write(&stale, b"previous").expect("plant previous backup");
        // A non-empty directory at today's final name: the copy is written,
        // but the rename into place fails.
        let target = dir
            .path()
            .join(format!("{}{}", backup_prefix(&db), utc_yyyymmdd()));
        std::fs::create_dir(&target).expect("create blocker dir");
        std::fs::write(target.join("keep"), b"x").expect("populate blocker dir");

        let message = match pre_migration_backup(&conn, &db, 2) {
            Ok(outcome) => panic!("the backup must fail, got {outcome:?}"),
            Err(e) => e.to_string(),
        };

        assert!(message.contains("backup"), "names the backup: {message}");
        assert_eq!(
            std::fs::read(&stale).expect("the previous backup survives"),
            b"previous",
            "IN-03: nothing is pruned before the new backup is in place"
        );
        assert_no_backup_temp(dir.path());
    }

    fn set_user_version(conn: &Connection, version: i64) {
        conn.pragma_update(None, "user_version", version)
            .expect("set user_version");
    }

    #[test]
    fn a_migration_failure_is_absorbed_only_when_another_process_reached_latest() {
        let simulated = || {
            Err::<(), _>(MemoryError::Internal(
                "simulated: table memory_links already exists".to_string(),
            ))
        };
        let mut conn = Connection::open_in_memory().expect("in-memory db");

        set_user_version(&conn, LATEST_SCHEMA_VERSION);
        migrate_tolerating_concurrent_upgrade(&mut conn, |_| simulated())
            .expect("a concurrent completed upgrade is success");

        set_user_version(&conn, 2);
        let err = migrate_tolerating_concurrent_upgrade(&mut conn, |_| simulated())
            .expect_err("a migration that left the DB behind still fails");
        assert!(
            err.to_string().contains("memory_links already exists"),
            "the original error is returned: {err}"
        );

        migrate_tolerating_concurrent_upgrade(&mut conn, |_| Ok(())).expect("Ok stays Ok");
    }

    /// The two REAL ways a second opener loses the 0003 race, reproduced
    /// deterministically with two connections on one WAL database: the loser
    /// probed version 2, then the winner committed the whole upgrade before
    /// the loser's 0003 statement ran (rusqlite_migration reads user_version
    /// outside its transaction). Both are real SQLite errors, and both must be
    /// absorbed because the re-probe sees the latest schema.
    #[test]
    fn a_real_lost_migration_race_is_absorbed() {
        register_vec_extension().expect("sqlite-vec registers");
        let relations_sql = include_str!("../../sql/0003_relations.sql");

        for stale_snapshot in [false, true] {
            let dir = tempfile::tempdir().expect("tempdir");
            let db = dir.path().join("memory.db");
            {
                let mut setup = Connection::open(&db).expect("create db");
                setup
                    .pragma_update(None, "journal_mode", "WAL")
                    .expect("WAL");
                migrations()
                    .to_version(&mut setup, 2)
                    .expect("schema 2 (v0.0.1)");
            }
            let mut winner = Connection::open(&db).expect("winner");
            winner
                .busy_timeout(Duration::from_millis(5000))
                .expect("winner busy_timeout");
            let mut loser = Connection::open(&db).expect("loser");
            loser
                .busy_timeout(Duration::from_millis(5000))
                .expect("loser busy_timeout");

            let mut observed = String::new();
            migrate_tolerating_concurrent_upgrade(&mut loser, |conn| {
                let attempt = (|| -> Result<(), rusqlite::Error> {
                    let tx = conn.transaction()?;
                    if stale_snapshot {
                        // Read first: the loser's snapshot predates the winner's commit.
                        tx.query_row("SELECT count(*) FROM memories", [], |row| {
                            row.get::<_, i64>(0)
                        })?;
                    }
                    migrations()
                        .to_latest(&mut winner)
                        .expect("the other process wins the upgrade");
                    tx.execute_batch(relations_sql)?;
                    tx.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
                    tx.commit()
                })();
                attempt.map_err(|e| {
                    observed = e.to_string();
                    MemoryError::from(e)
                })
            })
            .expect("a lost race against a completed upgrade is not a startup failure");

            assert!(
                !observed.is_empty(),
                "stale_snapshot={stale_snapshot}: the loser really failed"
            );
            let version: i64 = loser
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .expect("re-read version");
            assert_eq!(version, LATEST_SCHEMA_VERSION);
        }
    }

    #[test]
    fn max_knn_k_keeps_four_x_oversample_at_max_limit() {
        // WR-01: the oversample must stay 4x even at the largest valid limit.
        assert_eq!(MAX_KNN_K, crate::domain::MAX_LIMIT * 4);
        assert_eq!(MAX_KNN_K, 800);
        let k_at_max = (crate::domain::MAX_LIMIT * 4).min(MAX_KNN_K);
        assert_eq!(k_at_max, crate::domain::MAX_LIMIT * 4);
    }

    // -----------------------------------------------------------------------
    // option-a (T-04-15 revised): the classifier, the sanitizer, and the
    // always-parses fallback, pinned against the BUNDLED SQLite — the source
    // of truth, not the system sqlite3 CLI the planning probe used.
    // -----------------------------------------------------------------------

    /// An in-memory one-column FTS5 table shaped like `memories_fts(content)`.
    fn fts_probe_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE VIRTUAL TABLE t USING fts5(content);
             INSERT INTO t(content) VALUES ('sqlite with vec support');",
        )
        .expect("create fts5 probe table");
        conn
    }

    /// Bind `query` as the MATCH string of a statement that prepared fine, so
    /// any error is a step-time error — exactly what the store seam sees.
    fn probe(stmt: &mut rusqlite::Statement<'_>, query: &str) -> Result<i64, rusqlite::Error> {
        stmt.query_row([query], |row| row.get(0))
    }

    fn deep_parens() -> String {
        format!("{}a{}", "(".repeat(3000), ")".repeat(3000))
    }

    /// `a NOT a NOT a …` with `nots` chained `NOT`s. FTS5 never flattens `NOT`
    /// nodes, so the chain is a left-deep tree; past
    /// `SQLITE_FTS5_MAX_EXPR_DEPTH` (256) it fails at step time with
    /// `fts5 expression tree is too large (maximum depth 256)` (re-review WR-01).
    fn deep_not_chain(nots: usize) -> String {
        format!("a{}", " NOT a".repeat(nots))
    }

    /// Candidate probes from the planning table that FTS5 rejects. The first
    /// nine error in the bundled SQLite 3.51.3 exactly as in the sqlite3 3.46
    /// CLI probe (the empty string included), so none moved to the valid-raw
    /// group. The two `NOT` chains (re-review WR-01) pin the expression-depth
    /// limit at its boundary (257) and past it (300).
    fn erroring_probes() -> Vec<String> {
        vec![
            String::new(),
            "what is it?".to_string(),
            "\"unbalanced".to_string(),
            "AND".to_string(),
            "sqlite-vec".to_string(),
            "auth: tokens".to_string(),
            "*".to_string(),
            "NEAR(a b, x)".to_string(),
            deep_parens(),
            deep_not_chain(257),
            deep_not_chain(300),
        ]
    }

    /// Structured FTS5 syntax that is valid as written: it runs raw, and the
    /// sanitized retry never fires for it.
    fn valid_raw_probes() -> Vec<String> {
        vec![
            "\"exact phrase\"".to_string(),
            "sqlite AND vec".to_string(),
            "vec*".to_string(),
            "content: tokens".to_string(),
            "\"\"".to_string(),
        ]
    }

    #[test]
    fn every_bundled_fts5_parse_error_is_classified_as_match_input() {
        let conn = fts_probe_conn();
        let mut stmt = conn
            .prepare("SELECT count(*) FROM t WHERE t MATCH ?1")
            .expect("static statement prepares");
        let erroring = erroring_probes();
        assert!(!erroring.is_empty());
        for query in &erroring {
            let err = probe(&mut stmt, query)
                .expect_err(&format!("{:.40?} must error in the bundled SQLite", query));
            assert!(
                is_match_input_error(&err),
                "{:.40?} must be classified as client input, got: {err}",
                query
            );
        }
    }

    #[test]
    fn valid_structured_fts5_syntax_runs_raw_in_the_bundled_sqlite() {
        let conn = fts_probe_conn();
        let mut stmt = conn
            .prepare("SELECT count(*) FROM t WHERE t MATCH ?1")
            .expect("static statement prepares");
        for query in valid_raw_probes() {
            probe(&mut stmt, &query)
                .unwrap_or_else(|e| panic!("{query:?} is valid FTS5 and must run raw: {e}"));
        }
    }

    #[test]
    fn the_sanitized_fallback_always_parses() {
        let conn = fts_probe_conn();
        let mut stmt = conn
            .prepare("SELECT count(*) FROM t WHERE t MATCH ?1")
            .expect("static statement prepares");
        for query in erroring_probes().iter().chain(valid_raw_probes().iter()) {
            if let Some(fallback) = sanitize_fts_query(query) {
                probe(&mut stmt, &fallback).unwrap_or_else(|e| {
                    panic!("fallback {fallback:.60?} of {query:.40?} must parse: {e}")
                });
            }
        }
        // The fallback finds the row through a hyphenated query's words.
        let fallback = sanitize_fts_query("sqlite-vec").expect("has words");
        assert_eq!(probe(&mut stmt, &fallback).expect("parses"), 1);
    }

    #[test]
    fn the_depth_limit_is_exactly_256_nots_and_the_fallback_of_any_chain_parses() {
        // re-review WR-01. 256 chained NOTs is the deepest tree FTS5 accepts;
        // one more is the depth error, which must be classified as client input.
        let conn = fts_probe_conn();
        let mut stmt = conn
            .prepare("SELECT count(*) FROM t WHERE t MATCH ?1")
            .expect("static statement prepares");
        probe(&mut stmt, &deep_not_chain(256)).expect("256 NOTs is within the depth limit");
        let err = probe(&mut stmt, &deep_not_chain(257)).expect_err("257 NOTs exceeds it");
        assert!(
            err.to_string()
                .contains("fts5 expression tree is too large"),
            "unexpected bundled-SQLite message: {err}"
        );
        assert!(is_match_input_error(&err), "got: {err}");
        // The retry cannot hit the same limit: it quotes every token (so `NOT`
        // is a string, not an operator) and joins them with OR, which FTS5
        // flattens into one n-ary node. Even a 3000-NOT chain (6001 terms)
        // parses once sanitized.
        let fallback = sanitize_fts_query(&deep_not_chain(3000)).expect("has words");
        assert_eq!(
            probe(&mut stmt, &fallback).expect("flat OR chain parses"),
            0
        );
    }

    #[test]
    fn the_classifier_rejects_everything_that_is_not_an_fts5_parse_error() {
        let busy = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".to_string()),
        );
        assert!(!is_match_input_error(&busy));
        // The primary code gates too: a parse marker under SQLITE_BUSY is no match.
        let busy_with_marker = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("fts5: syntax error near \"?\"".to_string()),
        );
        assert!(!is_match_input_error(&busy_with_marker));
        // SQLITE_ERROR alone is not enough: the message must be an FTS5 parse.
        let malformed_json = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some("malformed JSON".to_string()),
        );
        assert!(!is_match_input_error(&malformed_json));
        assert!(!is_match_input_error(&corrupt_column("mem_type", "BOGUS")));
        // Positive control for the constructed form.
        let syntax = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some("fts5: syntax error near \"?\"".to_string()),
        );
        assert!(is_match_input_error(&syntax));
    }

    #[test]
    fn sanitize_fts_query_quotes_and_or_joins_word_tokens() {
        let table: [(&str, Option<&str>); 12] = [
            (
                "what vector store do we use?",
                Some(r#""what" OR "vector" OR "store" OR "do" OR "we" OR "use""#),
            ),
            ("don't retry", Some(r#""don" OR "t" OR "retry""#)),
            ("C++", Some(r#""C""#)),
            ("sqlite-vec", Some(r#""sqlite" OR "vec""#)),
            ("auth: tokens", Some(r#""auth" OR "tokens""#)),
            ("\"unbalanced", Some(r#""unbalanced""#)),
            ("AND", Some(r#""AND""#)),
            ("snake_case", Some(r#""snake_case""#)),
            ("über 日本語", Some(r#""über" OR "日本語""#)),
            ("", None),
            ("?!,", None),
            ("*", None),
        ];
        for (input, expected) in table {
            assert_eq!(
                sanitize_fts_query(input).as_deref(),
                expected,
                "sanitize_fts_query({input:?})"
            );
        }
    }
}
