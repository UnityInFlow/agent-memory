//! Schema versioning via `rusqlite_migration` (SQLite `user_version`).
//!
//! Phase 2 appends further `M::up(...)` steps (e.g. an embedding column); the
//! ordering and idempotency are handled by the migration library.

use rusqlite_migration::{Migrations, M};

/// The ordered set of forward migrations. Run on the writer connection at open.
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../../sql/0001_init.sql")),
        M::up(include_str!("../../sql/0002_embeddings.sql")),
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
}
