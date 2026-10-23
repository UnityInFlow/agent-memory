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
    snippet, LinkDirection, LinkKind, LinkOutcome, MemoryError, MemoryPatch, MemoryType,
    MemoryView, NewMemory, RelatedLink, UpdatedMemory,
};
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
    // journal_mode returns a row; the others do not. Use pragma_update where we can.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
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

/// Write the pre-migration backup of `src` (the open DB at `db_path`) to the
/// sibling file `{db}.backup-pre-{NNNN}-{YYYYMMDD}`, where `NNNN` is the
/// zero-padded schema version about to be migrated to (D-11). Generic over the
/// target version, so every future migration inherits it (D-12).
///
/// Retention is one backup per target schema version: every existing
/// `{db}.backup-pre-{NNNN}-*` entry is removed first; other versions' backups
/// are never touched. The copy uses the SQLite Online Backup API, which is
/// WAL-correct by construction (a raw file copy of a live WAL database is not).
/// The backup sits beside the DB, in the same directory and trust domain —
/// never a shared temp dir (T-04-03).
///
/// Any failure is returned as an error and ABORTS the open: a machine that
/// cannot write a backup must not run an unattended migration (T-04-02).
fn pre_migration_backup(src: &Connection, db_path: &Path) -> Result<PathBuf, MemoryError> {
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

    let entries = std::fs::read_dir(&dir)
        .map_err(|e| fail(format!("cannot read directory {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| fail(format!("cannot read directory entry: {e}")))?;
        let matches = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&prefix));
        if matches {
            let stale = entry.path();
            std::fs::remove_file(&stale).map_err(|e| {
                fail(format!(
                    "cannot remove previous backup {}: {e}",
                    stale.display()
                ))
            })?;
        }
    }

    let backup_path = dir.join(format!("{prefix}{}", utc_yyyymmdd()));
    let mut dst = Connection::open(&backup_path)
        .map_err(|e| fail(format!("cannot create {}: {e}", backup_path.display())))?;
    Backup::new(src, &mut dst)
        .and_then(|backup| backup.run_to_completion(64, Duration::from_millis(50), None))
        .map_err(|e| fail(format!("copy to {} failed: {e}", backup_path.display())))?;
    tracing::info!(
        backup = %backup_path.display(),
        target_schema_version = LATEST_SCHEMA_VERSION,
        "pre-migration backup written"
    );
    Ok(backup_path)
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
    /// 1. Probe `PRAGMA user_version`. A DB AHEAD of [`LATEST_SCHEMA_VERSION`]
    ///    is refused with [`MemoryError::SchemaTooNew`] (D-13). The probe runs
    ///    before even the per-connection PRAGMAs, because switching a non-WAL
    ///    file to WAL rewrites its header — a refused DB stays byte-for-byte
    ///    untouched.
    /// 2. If migrations are pending on a non-empty DB (`0 < user_version <
    ///    LATEST`, D-12), write a sibling online backup first (D-11). A backup
    ///    failure aborts the open; no migration runs. A brand-new file
    ///    (`user_version == 0`) has nothing to back up.
    /// 3. Migrate to latest.
    pub fn open(path: &Path) -> Result<Self, MemoryError> {
        // vec0 registration MUST precede Connection::open: auto-extensions only
        // apply to connections opened after registration (this also covers every
        // r2d2 pool connection created below).
        register_vec_extension()?;

        let mut writer = Connection::open(path)?;

        // (1) D-13 newer-schema refusal — read-only, before any write.
        let db_version: i64 = writer.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if db_version > LATEST_SCHEMA_VERSION {
            return Err(MemoryError::SchemaTooNew {
                db_version,
                binary_version: LATEST_SCHEMA_VERSION,
            });
        }

        prepare_connection(&writer)?;

        // (2) D-11/D-12 pre-migration backup. open() runs before serving, so
        // the writer is the only connection and can be the backup source.
        if db_version > 0 && db_version < LATEST_SCHEMA_VERSION {
            pre_migration_backup(&writer, path)?;
        }

        // (3) Migrate. rusqlite_migration's own DatabaseTooFarAhead check stays
        // as a backstop should the probe above ever be bypassed.
        let migrations: Migrations<'static> = migrations();
        migrations.to_latest(&mut writer)?;

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

/// Classify a post-bind FTS5 execution error at the store seam: a MATCH string
/// FTS5 cannot parse is CLIENT input → [`MemoryError::InvalidQuery`]; anything
/// else stays an internal [`MemoryError::Sqlite`].
///
/// Observed FTS5 query-parse messages from the bundled SQLite: a lone `"`
/// reports `unterminated string`; other malformed expressions (dangling
/// operators, bad NEAR syntax, …) report `fts5: syntax error`. The regression
/// test in `tests/fallback.rs` defines correctness; the string markers serve it.
///
/// Only errors surfacing AFTER the MATCH parameter is bound (statement step /
/// row iteration) go through here — a `conn.prepare` failure (e.g. missing
/// fts5 module) is an internal error, not client input.
fn map_fts_query_error(query: &str, e: rusqlite::Error) -> MemoryError {
    let msg = e.to_string();
    if msg.contains("fts5: syntax error") || msg.contains("unterminated string") {
        MemoryError::InvalidQuery(query.to_string())
    } else {
        MemoryError::Sqlite(e)
    }
}

impl Store for SqliteStore {
    fn insert(
        &self,
        new: NewMemory,
        embedding: Option<Vec<f32>>,
        now: i64,
    ) -> Result<i64, MemoryError> {
        let tags_json = serde_json::to_string(&new.tags).unwrap_or_else(|_| "[]".to_string());
        let base_weight = if new.mem_type.is_pinned() { 2.0 } else { 1.0 };
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
        tx.execute(
            "INSERT INTO memories \
             (mem_type, content, tags, source, scope, base_weight, decay_score, \
              access_count, created_at, last_accessed, expires_at, embedding_status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1.0, 0, ?7, ?7, ?8, ?9)",
            params![
                new.mem_type.as_wire_str(),
                new.content,
                tags_json,
                new.source,
                new.scope,
                base_weight,
                now,
                expires_at,
                embedding_status,
            ],
        )?;
        let id = tx.last_insert_rowid();
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

    fn insert_embedding(&self, memory_id: i64, embedding: Vec<f32>) -> Result<(), MemoryError> {
        let mut conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        let tx = conn.transaction()?;
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
        tx.execute(
            "UPDATE memories SET embedding_status = 1 WHERE id = ?1",
            params![memory_id],
        )?;
        tx.commit()?;
        Ok(())
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

    fn exists(
        &self,
        source: Option<&str>,
        mem_type: MemoryType,
        content: &str,
    ) -> Result<bool, MemoryError> {
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        // The INTEROP-01 dedup probe: all three key values bound, never
        // formatted into the SQL (T-02-20). `source IS ?1` is SQLite's
        // NULL-safe equality: a `None` source binds NULL and matches the NULL
        // the insert stored (`source = ?1` would be NULL = NULL → never true,
        // which made every source-less re-import a duplicate — CR-01).
        let found = conn
            .query_row(
                "SELECT 1 FROM memories \
                 WHERE source IS ?1 AND mem_type = ?2 AND content = ?3 LIMIT 1",
                params![source, mem_type.as_wire_str(), content],
                |_| Ok(()),
            )
            .optional()?;
        Ok(found.is_some())
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
        // (API-03 / D-07); the FTS5 error routing below is untouched.
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

        let rows = stmt.query_map(
            params![
                args.query,
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
        );

        // A malformed FTS5 MATCH string surfaces at step time (the bound MATCH
        // parses on execution, not prepare) and is mapped HERE, at the store
        // seam, to MemoryError::InvalidQuery via map_fts_query_error — which
        // both transports surface as a client error (REST 400 / MCP
        // invalid_params), never a panic and never a 500 (T-02-02, WR-05).
        // A valid query that matches nothing yields an empty vec.
        let rows = rows.map_err(|e| map_fts_query_error(&args.query, e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| map_fts_query_error(&args.query, e))?);
        }
        Ok(out)
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
        // idempotent no-op: changes() == 0 is still Linked (D-02). The kind
        // binds only the closed enum's wire form (T-04-10).
        tx.execute(
            "INSERT OR IGNORE INTO memory_links (from_id, to_id, kind, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![from_id, to_id, kind.as_wire_str(), now],
        )?;
        tx.commit()?;
        Ok(LinkOutcome::Linked)
    }

    fn unlink(&self, from_id: i64, to_id: i64, kind: LinkKind) -> Result<bool, MemoryError> {
        let conn = self
            .writer
            .lock()
            .map_err(|_| MemoryError::Internal("writer lock poisoned".into()))?;
        let removed = conn.execute(
            "DELETE FROM memory_links WHERE from_id = ?1 AND to_id = ?2 AND kind = ?3",
            params![from_id, to_id, kind.as_wire_str()],
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
        tx.execute(
            "UPDATE memories SET content = ?2, tags = ?3, scope = ?4, expires_at = ?5, \
                 last_accessed = ?6, access_count = access_count + 1, decay_score = 1.0 \
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
        let sql = format!(
            "UPDATE memories SET last_accessed = ?1, access_count = access_count + 1 \
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

    #[test]
    fn max_knn_k_keeps_four_x_oversample_at_max_limit() {
        // WR-01: the oversample must stay 4x even at the largest valid limit.
        assert_eq!(MAX_KNN_K, crate::domain::MAX_LIMIT * 4);
        assert_eq!(MAX_KNN_K, 800);
        let k_at_max = (crate::domain::MAX_LIMIT * 4).min(MAX_KNN_K);
        assert_eq!(k_at_max, crate::domain::MAX_LIMIT * 4);
    }
}
