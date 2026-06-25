//! Schema versioning via `rusqlite_migration` (SQLite `user_version`).
//!
//! Phase 2 appends further `M::up(...)` steps (e.g. an embedding column); the
//! ordering and idempotency are handled by the migration library.

use rusqlite_migration::{Migrations, M};

/// The ordered set of forward migrations. Run on the writer connection at open.
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(include_str!("../../sql/0001_init.sql"))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_validate() {
        // rusqlite_migration can self-check that the migration set is well-formed.
        migrations()
            .validate()
            .expect("migration set should validate");
    }
}
