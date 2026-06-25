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
use std::sync::Mutex;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection};
use rusqlite_migration::Migrations;

use crate::domain::{MemoryError, MemoryType, MemoryView, NewMemory};
use crate::service::ListArgs;
use crate::store::{migrations::migrations, Store};

/// Apply the per-connection PRAGMAs that every connection (writer + pool) needs.
fn apply_pragmas(conn: &Connection) -> Result<(), rusqlite::Error> {
    // journal_mode returns a row; the others do not. Use pragma_update where we can.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(())
}

/// A pool customizer that runs [`apply_pragmas`] on each pooled read connection.
#[derive(Debug)]
struct PragmaCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for PragmaCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        apply_pragmas(conn)
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
        let mut writer = Connection::open(path)?;
        apply_pragmas(&writer)?;

        let migrations: Migrations<'static> = migrations();
        migrations.to_latest(&mut writer)?;

        // Smoke-check that FTS5 is available (bundled SQLite compiles it in).
        writer.query_row("SELECT count(*) FROM memories_fts", [], |row| {
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
}
