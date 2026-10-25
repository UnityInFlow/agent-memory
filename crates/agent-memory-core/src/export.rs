//! Portable export format **v1** — the single definition of the on-disk
//! contract behind `agent-memory export` and `agent-memory import --from jsonl`
//! (DIST-04 / INTEROP-02).
//!
//! An export is a JSON Lines document: one JSON object per line, UTF-8, `\n`
//! terminated. **Line 1 is the mandatory header record**; every later line is
//! one memory or one link. Every object carries a `record` tag naming its
//! type; the set of tags is closed (`header`, `memory`, `link`).
//!
//! The moment one user writes a file this format is a compatibility contract
//! (Phase 5 D-03, rated one-way): removing a field or changing a meaning needs
//! `version: 2` plus a reader that still accepts version 1. Adding a field is
//! likewise a new version, because the reader is strict (below).
//!
//! # Header record (line 1)
//!
//! | field | type | meaning |
//! |---|---|---|
//! | `record` | `"header"` | tag |
//! | `format` | string | always [`FORMAT_NAME`] |
//! | `version` | integer | always [`FORMAT_VERSION`] for files this crate writes |
//! | `exported_at` | integer | unix seconds, from the service clock |
//! | `embedding_model` | string | the source database's `meta.embedding_model` pin |
//! | `embedding_dim` | integer | the source database's `meta.embedding_dim` pin |
//! | `schema_version` | integer | the source database's schema (`LATEST_SCHEMA_VERSION` at export time) |
//! | `agent_memory_version` | string | the exporting binary's `CARGO_PKG_VERSION` |
//! | `exported_rows` | integer | number of `memory` records that follow |
//! | `exported_links` | integer | number of `link` records that follow |
//!
//! `embedding_model` / `embedding_dim` are read from the source database's
//! `meta` table, never from constants, so the header states what the source
//! actually pinned. `exported_rows` / `exported_links` are a cheap truncation
//! check for a reader — they are counted inside the same read snapshot as the
//! rows themselves.
//!
//! # Memory record
//!
//! | field | type | meaning |
//! |---|---|---|
//! | `record` | `"memory"` | tag |
//! | `key` | integer | the source row id, used ONLY as an export-local key for `link` records; never written as an id on import (PITFALLS 18) |
//! | `type` | string | memory type wire string (`DECISION`, `PATTERN`, …); parsed through `MemoryType::try_from` on import (D-09) |
//! | `content` | string | the memory text, verbatim |
//! | `tags` | array of strings | tags, verbatim |
//! | `source` | string or null | provenance, verbatim |
//! | `scope` | string or null | scope, verbatim |
//! | `created_at` | integer | unix seconds, preserved on import |
//! | `last_accessed` | integer | unix seconds; the importer REFRESHES this to import time (D-10) |
//! | `access_count` | integer | preserved on import (provenance only) |
//! | `expires_at` | integer or null | absolute unix seconds; preserved on import, and a row already expired at import time is skipped and counted (D-10) |
//!
//! **Excluded as derived data (D-04):** `decay_score` (recomputed on read from
//! `last_accessed`), `base_weight` (a function of `type`), `embedding_status`
//! and every embedding vector. Vectors are pinned to the source machine's
//! model; the importer lands every row pending and the sweep re-embeds it
//! locally against the local pin (D-13), so a vector in the file could only
//! ever poison a differently-pinned corpus (PITFALLS 17).
//!
//! # Link record
//!
//! | field | type | meaning |
//! |---|---|---|
//! | `record` | `"link"` | tag |
//! | `from` | integer | the `key` of the source memory |
//! | `to` | integer | the `key` of the target memory |
//! | `kind` | string | `relates_to`, `supersedes` or `caused_by`; parsed through `LinkKind::try_from` on import (D-09) |
//! | `created_at` | integer | unix seconds, preserved on import (edge recency orders expansion — Phase 4 D-07) |
//!
//! Links are emitted **after all memory records** (D-05). Endpoints appear
//! exactly as stored: `relates_to` as `(min key, max key)` (the Phase 4 04-06
//! canonical form), directed kinds in the direction they were given.
//!
//! # Ordering and canonical output
//!
//! The exporter writes memories in ascending `key` order and links ordered by
//! `(from, to, kind)`, every row present in the database — expired rows
//! included, no scope or type filter (D-06; expiry is the importer's policy).
//! Optional fields (`source`, `scope`, `expires_at`) are always written
//! explicitly as `null`. Two exports of an unchanged database under the same
//! clock are therefore byte-identical.
//!
//! # Reader rules
//!
//! A hand-written file may omit an optional field (it reads as `null`), but is
//! otherwise read **strictly**: unknown fields are rejected
//! (`deny_unknown_fields`, the 04-07 precedent), the `record` set is closed,
//! field types are exact (`"1"` is not an integer, `1.5` is not a version),
//! and a `version` above [`FORMAT_VERSION`] is refused with an upgrade
//! message. The full per-line validation and error wording live in
//! `import::jsonl`; the user-facing summary is the README's "Export & import"
//! section.

use serde::{Deserialize, Serialize};

/// The `format` value every header carries. A file whose header says anything
/// else is not an agent-memory export, whatever its `version`.
pub const FORMAT_NAME: &str = "agent-memory-export";

/// The format version this crate writes and the highest it reads.
pub const FORMAT_VERSION: u32 = 1;

/// One line of a format-v1 export file — the single definition of the wire
/// contract in both directions.
///
/// Serialised internally tagged on `record` (lowercase variant name first,
/// then the fields in declaration order), so the documented field order IS
/// the declaration order below. `deny_unknown_fields` applies inside every
/// variant: a typo or a field from a newer version is a parse error, never a
/// silently dropped value (D-11). `type` / `kind` stay `String` here so the
/// closed-enum conversion runs through `MemoryType::try_from` /
/// `LinkKind::try_from` in the importer and produces the established
/// client-tier messages (D-09).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "lowercase", deny_unknown_fields)]
pub enum Record {
    /// Line 1 of every file (D-03).
    Header {
        /// Always [`FORMAT_NAME`].
        format: String,
        /// The format version of this file.
        version: u32,
        /// Unix seconds at export time, from the service clock.
        exported_at: i64,
        /// The source database's `meta.embedding_model` pin.
        embedding_model: String,
        /// The source database's `meta.embedding_dim` pin.
        embedding_dim: u32,
        /// The source database's schema version.
        schema_version: i64,
        /// The exporting binary's `CARGO_PKG_VERSION`.
        agent_memory_version: String,
        /// Number of `memory` records in the file.
        exported_rows: u64,
        /// Number of `link` records in the file.
        exported_links: u64,
    },
    /// One memory row (D-04).
    Memory {
        /// Source row id — an export-local key only, never an id on import.
        key: i64,
        /// Memory type wire string.
        #[serde(rename = "type")]
        mem_type: String,
        /// The memory text.
        content: String,
        /// Tags, verbatim.
        tags: Vec<String>,
        /// Provenance, verbatim.
        source: Option<String>,
        /// Scope, verbatim.
        scope: Option<String>,
        /// Unix seconds; preserved on import.
        created_at: i64,
        /// Unix seconds; refreshed to import time on import (D-10).
        last_accessed: i64,
        /// Preserved on import (provenance only).
        access_count: i64,
        /// Absolute unix seconds or `null`; preserved on import.
        expires_at: Option<i64>,
    },
    /// One relation edge, emitted after all memories (D-05).
    Link {
        /// `key` of the source memory.
        from: i64,
        /// `key` of the target memory.
        to: i64,
        /// Link kind wire string.
        kind: String,
        /// Unix seconds; preserved on import.
        created_at: i64,
    },
}

/// What the store pins at the start of an export, before any row is read —
/// the header's inputs. Emitted exactly once per
/// [`crate::store::Store::export_rows`] call, as the first event, from the
/// same read snapshot the rows come from.
///
/// A store-to-service streaming message, not a wire type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSnapshot {
    /// `meta.embedding_model`.
    pub embedding_model: String,
    /// `meta.embedding_dim`, parsed.
    pub embedding_dim: u32,
    /// `count(*)` of `memories` in the snapshot.
    pub memories: u64,
    /// `count(*)` of `memory_links` in the snapshot.
    pub links: u64,
}

/// One step of the store-to-service export stream: the single leading
/// [`ExportEvent::Snapshot`], then one [`ExportEvent::Record`] per memory (in
/// ascending id order) and per link (after all memories).
///
/// A store-to-service streaming message, not a wire type.
#[derive(Debug, Clone, PartialEq)]
pub enum ExportEvent {
    /// The pin and counts for the header; always first, exactly once.
    Snapshot(ExportSnapshot),
    /// One memory or link record, in file order.
    Record(Record),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_record() -> Record {
        Record::Memory {
            key: 7,
            mem_type: "DECISION".to_string(),
            content: "c".to_string(),
            tags: vec!["a".to_string()],
            source: None,
            scope: Some("s".to_string()),
            created_at: 1,
            last_accessed: 2,
            access_count: 3,
            expires_at: None,
        }
    }

    fn to_json(rec: &Record) -> String {
        serde_json::to_string(rec).expect("serialize record")
    }

    fn parse(line: &str) -> Result<Record, serde_json::Error> {
        serde_json::from_str(line)
    }

    #[test]
    fn memory_record_serializes_in_documented_order_with_record_first() {
        assert_eq!(
            to_json(&memory_record()),
            r#"{"record":"memory","key":7,"type":"DECISION","content":"c","tags":["a"],"source":null,"scope":"s","created_at":1,"last_accessed":2,"access_count":3,"expires_at":null}"#
        );
    }

    #[test]
    fn header_record_serializes_in_documented_order_with_record_first() {
        let header = Record::Header {
            format: FORMAT_NAME.to_string(),
            version: FORMAT_VERSION,
            exported_at: 1_000,
            embedding_model: "nomic-embed-text".to_string(),
            embedding_dim: 768,
            schema_version: 3,
            agent_memory_version: "0.0.1".to_string(),
            exported_rows: 2,
            exported_links: 1,
        };
        assert_eq!(
            to_json(&header),
            r#"{"record":"header","format":"agent-memory-export","version":1,"exported_at":1000,"embedding_model":"nomic-embed-text","embedding_dim":768,"schema_version":3,"agent_memory_version":"0.0.1","exported_rows":2,"exported_links":1}"#
        );
    }

    #[test]
    fn link_record_serializes_in_documented_order_with_record_first() {
        let link = Record::Link {
            from: 1,
            to: 2,
            kind: "supersedes".to_string(),
            created_at: 5,
        };
        assert_eq!(
            to_json(&link),
            r#"{"record":"link","from":1,"to":2,"kind":"supersedes","created_at":5}"#
        );
    }

    #[test]
    fn every_record_round_trips_through_its_own_json() {
        let header = Record::Header {
            format: FORMAT_NAME.to_string(),
            version: FORMAT_VERSION,
            exported_at: 1,
            embedding_model: "m".to_string(),
            embedding_dim: 8,
            schema_version: 3,
            agent_memory_version: "v".to_string(),
            exported_rows: 0,
            exported_links: 0,
        };
        let link = Record::Link {
            from: 1,
            to: 2,
            kind: "relates_to".to_string(),
            created_at: 9,
        };
        for rec in [header, memory_record(), link] {
            let back = parse(&to_json(&rec)).expect("round trip parses");
            assert_eq!(back, rec);
        }
    }

    #[test]
    fn an_unknown_field_inside_a_memory_record_is_rejected() {
        let line = r#"{"record":"memory","key":7,"type":"DECISION","content":"c","tags":[],"source":null,"scope":null,"created_at":1,"last_accessed":2,"access_count":3,"expires_at":null,"bogus":1}"#;
        let err = parse(line).expect_err("unknown field must fail");
        assert!(
            err.to_string().contains("unknown field"),
            "expected an unknown-field error, got: {err}"
        );
    }

    #[test]
    fn an_unknown_record_tag_is_rejected() {
        let err = parse(r#"{"record":"memo","key":1}"#).expect_err("unknown tag must fail");
        assert!(
            err.to_string().contains("unknown variant"),
            "expected an unknown-variant error, got: {err}"
        );
    }

    #[test]
    fn missing_optional_fields_read_as_none() {
        let line = r#"{"record":"memory","key":7,"type":"DECISION","content":"c","tags":["a"],"created_at":1,"last_accessed":2,"access_count":3}"#;
        let rec = parse(line).expect("optional fields may be omitted");
        match rec {
            Record::Memory {
                source,
                scope,
                expires_at,
                ..
            } => {
                assert_eq!(source, None);
                assert_eq!(scope, None);
                assert_eq!(expires_at, None);
            }
            other => panic!("expected a memory record, got {other:?}"),
        }
    }

    #[test]
    fn a_string_key_is_rejected() {
        let line = r#"{"record":"memory","key":"1","type":"DECISION","content":"c","tags":[],"source":null,"scope":null,"created_at":1,"last_accessed":2,"access_count":3,"expires_at":null}"#;
        assert!(parse(line).is_err(), "a string key must not parse as i64");
    }

    #[test]
    fn a_fractional_header_version_is_rejected() {
        let line = r#"{"record":"header","format":"agent-memory-export","version":1.5,"exported_at":1,"embedding_model":"m","embedding_dim":8,"schema_version":3,"agent_memory_version":"v","exported_rows":0,"exported_links":0}"#;
        assert!(parse(line).is_err(), "1.5 must not parse as a u32 version");
    }
}
