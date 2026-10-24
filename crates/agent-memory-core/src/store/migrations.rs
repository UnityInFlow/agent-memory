//! Schema versioning via `rusqlite_migration` (SQLite `user_version`).
//!
//! The chain is APPEND-ONLY: 0001/0002 shipped in v0.0.1 and are frozen
//! (STORE-05, proven by the committed-fixture test in
//! `tests/migration_hygiene.rs`). New schema lands as a new `M::up(...)` step,
//! with [`LATEST_SCHEMA_VERSION`] bumped alongside it. There are no `.down()`
//! migrations — the pre-migration backup in `SqliteStore::open` is the rollback
//! story (D-11/D-12). Ordering and idempotency are handled by the library.

use rusqlite_migration::{Migrations, M};

/// The schema version (`PRAGMA user_version`) this binary migrates to — the
/// number of `M::up` steps in [`migrations`]. `SqliteStore::open` probes the
/// DB's `user_version` against it BEFORE migrating: a DB ahead of it is refused
/// with an upgrade hint (D-13), and a non-empty DB behind it is backed up first
/// (D-11/D-12). Bump it in the same commit that appends a migration.
pub const LATEST_SCHEMA_VERSION: i64 = 3;

/// The ordered set of forward migrations. Run on the writer connection at open.
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../../sql/0001_init.sql")),
        M::up(include_str!("../../sql/0002_embeddings.sql")),
        M::up(include_str!("../../sql/0003_relations.sql")),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_validate() {
        // `Migrations::validate` opens its own in-memory connection, so the vec0
        // module must be registered process-globally first or migration 0002
        // (CREATE VIRTUAL TABLE ... USING vec0) cannot validate.
        crate::store::sqlite::register_vec_extension()
            .expect("sqlite-vec extension should register");
        // rusqlite_migration can self-check that the migration set is well-formed.
        migrations()
            .validate()
            .expect("migration set should validate");
    }

    #[test]
    fn latest_schema_version_matches_the_migration_chain() {
        // The D-13 probe and the D-11 backup trigger both compare against this
        // const, so it must equal what `to_latest` actually writes.
        crate::store::sqlite::register_vec_extension()
            .expect("sqlite-vec extension should register");
        let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        migrations()
            .to_latest(&mut conn)
            .expect("migrate to latest");
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read user_version");
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }
}
