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

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection};
use rusqlite_migration::Migrations;

use crate::decay::{DecayConfig, RankWeights, PINNED_HALF_LIFE_MULTIPLIER};
use crate::domain::{MemoryError, MemoryType, MemoryView, NewMemory};
use crate::service::{ListArgs, SearchArgs};
use crate::store::{migrations::migrations, Store};

/// Default cap on returned search rows when the caller omits `limit` (T-02-04).
const DEFAULT_SEARCH_LIMIT: i64 = 50;

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
    pub fn open(path: &Path) -> Result<Self, MemoryError> {
        // vec0 registration MUST precede Connection::open: auto-extensions only
        // apply to connections opened after registration (this also covers every
        // r2d2 pool connection created below).
        register_vec_extension()?;

        let mut writer = Connection::open(path)?;
        prepare_connection(&writer)?;

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
    })
}

impl Store for SqliteStore {
    fn insert(&self, new: NewMemory, now: i64) -> Result<i64, MemoryError> {
        let tags_json = serde_json::to_string(&new.tags).unwrap_or_else(|_| "[]".to_string());
        let base_weight = if new.mem_type.is_pinned() { 2.0 } else { 1.0 };
        let expires_at = new.ttl_secs.map(|ttl| now + ttl);

        let conn = self.writer.lock().map_err(|_| MemoryError::NotFound)?;
        conn.execute(
            "INSERT INTO memories \
             (mem_type, content, tags, source, scope, base_weight, decay_score, \
              access_count, created_at, last_accessed, expires_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1.0, 0, ?7, ?7, ?8)",
            params![
                new.mem_type.as_wire_str(),
                new.content,
                tags_json,
                new.source,
                new.scope,
                base_weight,
                now,
                expires_at,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    fn list(&self, args: ListArgs, _now: i64) -> Result<Vec<MemoryView>, MemoryError> {
        let conn = self.reads.get().map_err(MemoryError::Pool)?;
        let mem_type_filter: Option<String> = args.mem_type.map(|t| t.as_wire_str().to_string());

        // All filters parameterized; a NULL bound disables that predicate.
        let mut stmt = conn.prepare(
            "SELECT id, mem_type, content, tags, source, scope, base_weight, \
                    decay_score, access_count, created_at, last_accessed, expires_at \
             FROM memories \
             WHERE (?1 IS NULL OR mem_type = ?1) \
               AND (?2 IS NULL OR scope = ?2) \
               AND (?3 IS NULL OR tags LIKE '%' || ?3 || '%') \
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
        let mut stmt = conn.prepare(
            "SELECT m.id, m.mem_type, m.content, m.tags, m.source, m.scope, \
                    m.base_weight, m.decay_score, m.access_count, m.created_at, \
                    m.last_accessed, m.expires_at \
             FROM memories_fts \
             JOIN memories m ON m.id = memories_fts.rowid \
             WHERE memories_fts MATCH ?1 \
               AND (?2 IS NULL OR m.mem_type = ?2) \
               AND (?3 IS NULL OR m.scope = ?3) \
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
            ],
            row_to_view,
        );

        // A malformed FTS5 MATCH string surfaces here as a typed Sqlite error,
        // which the service maps to a clean invalid-params error — no panic
        // (T-02-02). A valid query that matches nothing yields an empty vec.
        let rows = rows?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn forget(&self, id: i64) -> Result<bool, MemoryError> {
        let conn = self.writer.lock().map_err(|_| MemoryError::NotFound)?;
        // Parameterized DELETE; the FTS5 delete trigger keeps the mirror in sync.
        let changed = conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    fn bump_access(&self, ids: &[i64], now: i64) -> Result<(), MemoryError> {
        if ids.is_empty() {
            return Ok(());
        }
        let conn = self.writer.lock().map_err(|_| MemoryError::NotFound)?;
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
        let conn = self.writer.lock().map_err(|_| MemoryError::NotFound)?;
        // The ONLY delete the sweep performs (STORE-04). Bound predicate: NULL
        // `expires_at` rows are never matched, so a no-TTL memory is never removed
        // here. The FTS5 AFTER DELETE trigger keeps the mirror in sync.
        let deleted = conn.execute(
            "DELETE FROM memories WHERE expires_at IS NOT NULL AND expires_at < ?1",
            params![now],
        )?;
        Ok(deleted)
    }

    fn materialize_decay(&self, now: i64, cfg: &DecayConfig) -> Result<usize, MemoryError> {
        let conn = self.writer.lock().map_err(|_| MemoryError::NotFound)?;
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
