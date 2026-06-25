//! Core domain types: the typed memory model, its wire-form validation, and the
//! library's typed error enum.
//!
//! `MemoryType` is the load-bearing type: it parses the UPPERCASE wire form via
//! [`TryFrom<&str>`], returning a clean `Err(MemoryError::InvalidType)` for unknown
//! values rather than panicking (D-07). Pinned types (D-08) decay more slowly.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The six typed categories of memory the system stores.
///
/// The wire form is UPPERCASE (`DECISION`, `PATTERN`, …). Parse untrusted input
/// with [`MemoryType::try_from`], which rejects unknown values with a clean error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MemoryType {
    Decision,
    Pattern,
    Error,
    Todo,
    Architecture,
    Constraint,
}

impl MemoryType {
    /// Pinned types (DECISION, ARCHITECTURE, CONSTRAINT) decay much slower than
    /// the rest (D-08). This is the single source of truth for the pin set.
    pub fn is_pinned(self) -> bool {
        matches!(
            self,
            MemoryType::Decision | MemoryType::Architecture | MemoryType::Constraint
        )
    }

    /// The UPPERCASE wire form, matching the serde representation.
    pub fn as_wire_str(self) -> &'static str {
        match self {
            MemoryType::Decision => "DECISION",
            MemoryType::Pattern => "PATTERN",
            MemoryType::Error => "ERROR",
            MemoryType::Todo => "TODO",
            MemoryType::Architecture => "ARCHITECTURE",
            MemoryType::Constraint => "CONSTRAINT",
        }
    }
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

impl std::convert::TryFrom<&str> for MemoryType {
    type Error = MemoryError;

    fn try_from(s: &str) -> Result<Self, MemoryError> {
        match s {
            "DECISION" => Ok(MemoryType::Decision),
            "PATTERN" => Ok(MemoryType::Pattern),
            "ERROR" => Ok(MemoryType::Error),
            "TODO" => Ok(MemoryType::Todo),
            "ARCHITECTURE" => Ok(MemoryType::Architecture),
            "CONSTRAINT" => Ok(MemoryType::Constraint),
            other => Err(MemoryError::InvalidType(other.to_string())),
        }
    }
}

/// Input shape for storing a new memory (D-05).
///
/// Only `content` and `mem_type` are conceptually required at the service boundary;
/// the remaining fields carry sensible defaults supplied by the caller
/// (`tags = []`, `source`/`scope` = `None`, `ttl_secs` = `None`).
#[derive(Debug, Clone)]
pub struct NewMemory {
    pub content: String,
    pub mem_type: MemoryType,
    pub tags: Vec<String>,
    pub source: Option<String>,
    pub scope: Option<String>,
    pub ttl_secs: Option<i64>,
}

/// The full stored record as persisted in the `memories` table.
#[derive(Debug, Clone, PartialEq)]
pub struct Memory {
    pub id: i64,
    pub mem_type: MemoryType,
    pub content: String,
    pub tags: Vec<String>,
    pub source: Option<String>,
    pub scope: Option<String>,
    pub base_weight: f64,
    pub decay_score: f64,
    pub access_count: i64,
    pub created_at: i64,
    pub last_accessed: i64,
    pub expires_at: Option<i64>,
}

/// The read/return shape exposed over the tool boundary (D-06).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryView {
    pub id: i64,
    pub content: String,
    #[serde(rename = "type")]
    pub mem_type: MemoryType,
    pub tags: Vec<String>,
    pub scope: Option<String>,
    pub decay_score: f64,
    pub created_at: i64,
    pub last_accessed: i64,
}

/// The library's typed error. `thiserror` per the ecosystem rule (libraries use
/// `thiserror`; the binary uses `anyhow` at its edges).
#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("invalid memory type '{0}' (expected one of DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT)")]
    InvalidType(String),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("blocking task join error: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),

    #[error("memory not found")]
    NotFound,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryFrom;

    #[test]
    fn try_from_accepts_all_six_uppercase_types() {
        assert_eq!(
            MemoryType::try_from("DECISION").unwrap(),
            MemoryType::Decision
        );
        assert_eq!(
            MemoryType::try_from("PATTERN").unwrap(),
            MemoryType::Pattern
        );
        assert_eq!(MemoryType::try_from("ERROR").unwrap(), MemoryType::Error);
        assert_eq!(MemoryType::try_from("TODO").unwrap(), MemoryType::Todo);
        assert_eq!(
            MemoryType::try_from("ARCHITECTURE").unwrap(),
            MemoryType::Architecture
        );
        assert_eq!(
            MemoryType::try_from("CONSTRAINT").unwrap(),
            MemoryType::Constraint
        );
    }

    #[test]
    fn try_from_rejects_unknown_type_without_panicking() {
        let err = MemoryType::try_from("BOGUS").unwrap_err();
        match err {
            MemoryError::InvalidType(got) => assert_eq!(got, "BOGUS"),
            other => panic!("expected InvalidType, got {other:?}"),
        }
    }

    #[test]
    fn try_from_is_case_sensitive_uppercase_only() {
        assert!(MemoryType::try_from("decision").is_err());
    }

    #[test]
    fn is_pinned_only_for_high_value_types() {
        assert!(MemoryType::Decision.is_pinned());
        assert!(MemoryType::Architecture.is_pinned());
        assert!(MemoryType::Constraint.is_pinned());
        assert!(!MemoryType::Pattern.is_pinned());
        assert!(!MemoryType::Error.is_pinned());
        assert!(!MemoryType::Todo.is_pinned());
    }

    #[test]
    fn display_matches_wire_form() {
        assert_eq!(MemoryType::Decision.to_string(), "DECISION");
        assert_eq!(MemoryType::Todo.to_string(), "TODO");
    }
}
