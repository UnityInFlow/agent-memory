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

/// The closed vocabulary of typed relations between two memories (MCP-07 /
/// D-01). Exactly three kinds ship in v0.1.0; adding one later is additive,
/// removing one is a contract break.
///
/// Edges are stored DIRECTED (D-03), and each kind reads `from <kind> to`:
///
/// - `relates_to` — a symmetric "see also"; the direction carries no meaning.
/// - `supersedes` — `from` replaces `to` (the newer memory points at the older).
/// - `caused_by` — `from` was caused by `to` (the effect points at its cause).
///
/// `relates_to` is ONE symmetric relation (REVIEW WR-03): it is stored with
/// canonical endpoints, (smaller id, larger id), whichever way round it was
/// linked ([`LinkKind::canonical_endpoints`]), so linking b→a after a→b is the
/// same relation, and it can be unlinked from either end. The `direction` that
/// 1-hop expansion reports for a `relates_to` neighbour therefore carries no
/// meaning. `supersedes` and `caused_by` keep D-03 directed identity: a→b and
/// b→a are two distinct relations.
///
/// The wire form is snake_case (`relates_to`, `supersedes`, `caused_by`).
/// Parse untrusted input with [`LinkKind::try_from`], which rejects anything
/// else with [`MemoryError::InvalidArgument`] naming the allowed set — so a
/// free-form kind string never reaches SQL (T-04-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    RelatesTo,
    Supersedes,
    CausedBy,
}

impl LinkKind {
    /// The snake_case wire form, matching the serde representation. This is
    /// also the exact value stored in `memory_links.kind`.
    pub fn as_wire_str(self) -> &'static str {
        match self {
            LinkKind::RelatesTo => "relates_to",
            LinkKind::Supersedes => "supersedes",
            LinkKind::CausedBy => "caused_by",
        }
    }

    /// The natural label of this kind read from the TARGET's side of the edge
    /// (D-03): `relates_to` is symmetric and stays `relates_to`; an incoming
    /// `supersedes` reads `superseded_by`; an incoming `caused_by` reads
    /// `caused`. Used by 1-hop expansion for `direction: "in"` entries.
    pub fn reverse_label(self) -> &'static str {
        match self {
            LinkKind::RelatesTo => "relates_to",
            LinkKind::Supersedes => "superseded_by",
            LinkKind::CausedBy => "caused",
        }
    }

    /// Whether the relation reads the same from both ends. True only for
    /// `relates_to`; a symmetric kind is stored canonically and unlinked
    /// from either end (REVIEW WR-03).
    pub fn is_symmetric(self) -> bool {
        match self {
            LinkKind::RelatesTo => true,
            LinkKind::Supersedes => false,
            LinkKind::CausedBy => false,
        }
    }

    /// The `(from_id, to_id)` pair this relation is STORED under.
    ///
    /// `relates_to` is canonicalised to `(min, max)`, so a→b and b→a land on
    /// the same primary key and a duplicate in either order is the D-02
    /// idempotent no-op. `supersedes` and `caused_by` are returned unchanged:
    /// their direction is part of their identity (D-03).
    ///
    /// Storage only: transports always echo the REQUEST's ids on the wire.
    pub fn canonical_endpoints(self, from_id: i64, to_id: i64) -> (i64, i64) {
        match self {
            LinkKind::RelatesTo => (from_id.min(to_id), from_id.max(to_id)),
            LinkKind::Supersedes => (from_id, to_id),
            LinkKind::CausedBy => (from_id, to_id),
        }
    }
}

impl std::fmt::Display for LinkKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

impl std::convert::TryFrom<&str> for LinkKind {
    type Error = MemoryError;

    fn try_from(s: &str) -> Result<Self, MemoryError> {
        match s {
            "relates_to" => Ok(LinkKind::RelatesTo),
            "supersedes" => Ok(LinkKind::Supersedes),
            "caused_by" => Ok(LinkKind::CausedBy),
            other => Err(MemoryError::InvalidArgument(format!(
                "kind must be one of relates_to, supersedes, caused_by (got '{other}')"
            ))),
        }
    }
}

/// Outcome of a link request (MCP-07 / D-02). A missing endpoint is DATA, not
/// an error: it names the first id that does not exist (`from_id` is checked
/// before `to_id`), so a client never sees a foreign-key failure (T-04-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOutcome {
    /// The edge exists now. `created` is `true` when the row was inserted by
    /// this call and `false` when the stored edge already existed — the D-02
    /// idempotent no-op, still a success. The distinction is storage-only
    /// (transports emit the same JSON either way); the JSONL importer counts
    /// the `false` case as `links_skipped_duplicates` (Phase 5 D-12).
    Linked { created: bool },
    /// No memory with `missing_id` exists; nothing was written.
    NotFound { missing_id: i64 },
}

/// Which way a [`RelatedLink`] edge points, seen from the result it is
/// attached to: `out` = the result is the edge's `from_id`, `in` = the
/// result is its `to_id` (D-03 / D-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkDirection {
    Out,
    In,
}

/// A trimmed 1-hop neighbor summary attached to a search/list result when
/// `expand_links` is set (MCP-07 / D-05) — enough for an agent to decide
/// whether to fetch the neighbor, never the full view.
///
/// `kind` is the direction-aware natural label: the stored kind for an
/// outgoing edge, [`LinkKind::reverse_label`] for an incoming one (so an
/// incoming `supersedes` reads `superseded_by`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RelatedLink {
    /// The neighbor memory's id.
    pub id: i64,
    /// Natural relation label from the anchor's point of view.
    pub kind: String,
    /// `out` (anchor → neighbor) or `in` (neighbor → anchor).
    pub direction: LinkDirection,
    /// The neighbor's memory type.
    #[serde(rename = "type")]
    pub mem_type: MemoryType,
    /// The neighbor's content, cut to [`RELATED_SNIPPET_CHARS`] characters
    /// (with a trailing `…` when cut).
    pub snippet: String,
    /// The neighbor's tags.
    pub tags: Vec<String>,
}

/// Maximum characters of neighbor content carried in a [`RelatedLink`]
/// snippet (D-05). Counted in Unicode scalar values, so a cut never splits a
/// UTF-8 codepoint. It may, however, split a grapheme cluster — a combining
/// accent sequence or a ZWJ emoji — leaving a dangling modifier before the
/// `…` (REVIEW IN-06). That is cosmetic and accepted under the D-05
/// discretion: grapheme-aware cutting would need a new dependency for no
/// functional gain.
pub const RELATED_SNIPPET_CHARS: usize = 160;

/// Cut `content` to [`RELATED_SNIPPET_CHARS`] characters on a char boundary,
/// appending `…` only when something was cut. The cut counts Unicode scalars,
/// not grapheme clusters, so it can split a combined emoji or accent sequence
/// (IN-06, documented and accepted).
pub fn snippet(content: &str) -> String {
    match content.char_indices().nth(RELATED_SNIPPET_CHARS) {
        Some((byte_idx, _)) => format!("{}…", &content[..byte_idx]),
        None => content.to_string(),
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
///
/// INVARIANT — field order is wire order: serde emits members in declaration
/// order, and the v1.0 envelope (`id`, `content`, `type`, `tags`, `scope`,
/// `decay_score`, `created_at`, `last_accessed`) is a golden-tested contract.
/// New fields append at the end and must be skipped when absent.
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
    /// 1-hop relation summaries (MCP-07 / D-04, D-05): `Some` only when the
    /// caller asked for `expand_links` (an explicit `[]` when the memory has
    /// no edges), `None` otherwise. `None` is SKIPPED on the wire, so a
    /// flag-less search/list envelope stays byte-identical to v1.0
    /// (RESEARCH Pitfall 7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related: Option<Vec<RelatedLink>>,
}

/// A JSON-merge-patch for `memory_update` (MCP-06 / D-08).
///
/// Every field is a double `Option`, so a transport can carry the three wire
/// states of a JSON member through to the core:
///
/// | wire            | value            | meaning                         |
/// |-----------------|------------------|---------------------------------|
/// | member absent   | `None`           | leave the field unchanged       |
/// | `null`          | `Some(None)`     | clear it (where clearing is legal) |
/// | a value         | `Some(Some(v))`  | set it                          |
///
/// Clearing means: `tags` → empty list, `scope` → global (`NULL`, the same
/// default [`NewMemory`] stores), `ttl_secs` → no expiry. `content` can never
/// be cleared or emptied (rejected by the service). `mem_type` and `source` are
/// deliberately NOT patchable (MCP-06 scope; type immutability also keeps
/// `base_weight` fixed).
///
/// Plain struct with no serde derives: the transports own deserialization
/// (they need an explicit-null helper — plain `Option<Option<T>>` collapses
/// `null` into "absent") and construct this directly.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MemoryPatch {
    pub content: Option<Option<String>>,
    pub tags: Option<Option<Vec<String>>>,
    pub scope: Option<Option<String>>,
    pub ttl_secs: Option<Option<i64>>,
}

/// The `memory_update` response (D-10): the full post-update [`MemoryView`]
/// plus `embedding_status`, so a caller can observe a pending re-embed (`0`)
/// after an embedder outage — the sweep backfills it to `1` later.
///
/// The view is `#[serde(flatten)]`ed, so on the wire this is the familiar
/// MemoryView object with one extra member. `MemoryView` itself is untouched,
/// which keeps every existing list/search envelope byte-identical.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdatedMemory {
    #[serde(flatten)]
    pub view: MemoryView,
    /// `1` = a vector is stored for the current content; `0` = pending.
    pub embedding_status: i64,
}

/// Smallest accepted `limit` on search/list (D-01/D-03).
pub const MIN_LIMIT: i64 = 1;

/// Largest accepted `limit` on search/list (D-01/D-03). Aligned with the KNN
/// oversample cap so a valid limit never exceeds what the KNN leg honors.
pub const MAX_LIMIT: i64 = 200;

/// Smallest accepted `ttl_secs` on store/import (D-02/D-03) — a zero or
/// negative TTL would create an already-expired row.
pub const MIN_TTL_SECS: i64 = 1;

/// Largest accepted `ttl_secs` on store/import (D-02/D-03): ~100 years.
/// Keeps `now + ttl_secs` far below `i64::MAX`, so the `expires_at`
/// arithmetic can never overflow (RESEARCH Pitfall 1).
pub const MAX_TTL_SECS: i64 = 3_155_760_000;

/// Hard cap on the 1-hop neighbors attached to ONE search/list result when
/// `expand_links` is set (MCP-07 / D-07). Overflow keeps the most recent
/// edges (`memory_links.created_at` descending). Fixed — there is no
/// caller-tunable knob in v0.1.0.
///
/// What this bounds (REVIEW WR-04, user decision option-a, 2026-10-23): each
/// result carries at most `MAX_EXPANDED_LINKS` neighbours. Search is always
/// bounded by its limit (1–[`MAX_LIMIT`], default 50), and
/// [`crate::service::MemoryService::list`] rejects `expand_links` without a
/// `limit` as [`MemoryError::InvalidArgument`] (REST 400 / MCP
/// `invalid_params`) before any store hop. So every expanded search or list
/// returns at most `200 × 10` summaries, and T-04-11's bound covers every
/// expanded response. A list without `expand_links` still returns every row
/// when `limit` is omitted (v1.0 behaviour, RESEARCH Pitfall 6).
pub const MAX_EXPANDED_LINKS: usize = 10;

/// The library's typed error. `thiserror` per the ecosystem rule (libraries use
/// `thiserror`; the binary uses `anyhow` at its edges).
#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("invalid memory type '{0}' (expected one of DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT)")]
    InvalidType(String),

    /// Client tier (REST 400 / MCP `invalid_params`). **Since v0.1.0, search
    /// never produces it** (user decision option-a, T-04-15 revised): the
    /// keyword leg retries FTS5-unparseable input once as its words OR-ed
    /// together, and treats a tokenless query as no keyword matches, in both
    /// modes. The variant is kept so both transport error mappers stay
    /// exhaustive and the client-tier error vocabulary stays stable.
    #[error("invalid search query {0:?}: not a valid FTS5 match expression")]
    InvalidQuery(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("blocking task join error: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),

    /// A server-side fault with no client-input cause — e.g. a poisoned writer
    /// mutex after a panic while it was held (WR-02). Always the internal tier
    /// (REST 500 / MCP `internal_error`), never a not-found: not-found is data
    /// (`Ok(false)` / `Ok(None)`), not an error variant.
    #[error("internal error: {0}")]
    Internal(String),

    /// The database was written by a NEWER agent-memory: its schema version is
    /// ahead of what this binary can migrate to (D-13). Startup-only — raised by
    /// `SqliteStore::open` before any write and before serving, so it never
    /// crosses a transport; the binary reports it on stderr and exits. The
    /// message names only the two version integers and the public upgrade
    /// command (T-04-05).
    #[error(
        "database schema is version {db_version} but this binary supports up to version \
         {binary_version} — upgrade agent-memory (brew upgrade unityinflow/tap/agent-memory) \
         or point --db at a different file"
    )]
    SchemaTooNew {
        db_version: i64,
        binary_version: i64,
    },
}

/// Validate an optional `limit` against the D-01 bounds. `None` always passes
/// (omitted keeps the v1.0 defaults: search 50, list unlimited). The shared
/// seam helper — every service method that accepts a limit calls this (D-06).
pub(crate) fn validate_limit(limit: Option<i64>) -> Result<(), MemoryError> {
    if let Some(l) = limit {
        if !(MIN_LIMIT..=MAX_LIMIT).contains(&l) {
            return Err(MemoryError::InvalidArgument(format!(
                "limit must be between {MIN_LIMIT} and {MAX_LIMIT} (got {l})"
            )));
        }
    }
    Ok(())
}

/// Validate an optional `ttl_secs` against the D-02 bounds. `None` always
/// passes (omitted means no expiry). Called by `store()` AND per-draft by
/// `import()` so every insert path shares the one seam (RESEARCH Pitfall 3).
pub(crate) fn validate_ttl(ttl_secs: Option<i64>) -> Result<(), MemoryError> {
    if let Some(t) = ttl_secs {
        if !(MIN_TTL_SECS..=MAX_TTL_SECS).contains(&t) {
            return Err(MemoryError::InvalidArgument(format!(
                "ttl_secs must be between {MIN_TTL_SECS} and {MAX_TTL_SECS} (got {t})"
            )));
        }
    }
    Ok(())
}

/// Validate a [`MemoryPatch`] at the service seam, before any embed or store
/// hop (MCP-06 / D-08; Phase 3 D-06 — bad input is always the client tier):
///
/// - a patch with every field absent is a client error, not a silent no-op;
/// - `content` may not be `null`, empty, or whitespace-only;
/// - a present `ttl_secs` goes through the shared [`validate_ttl`] bounds.
pub(crate) fn validate_patch(patch: &MemoryPatch) -> Result<(), MemoryError> {
    if patch.content.is_none()
        && patch.tags.is_none()
        && patch.scope.is_none()
        && patch.ttl_secs.is_none()
    {
        return Err(MemoryError::InvalidArgument(
            "at least one of content, tags, scope, ttl_secs must be provided".to_string(),
        ));
    }
    match &patch.content {
        None => {}
        Some(Some(content)) if !content.trim().is_empty() => {}
        Some(Some(_)) | Some(None) => {
            return Err(MemoryError::InvalidArgument(
                "content must be a non-empty string".to_string(),
            ));
        }
    }
    // Only a present value is bounds-checked: absent (keep) and null (clear
    // the expiry) both flatten to None, which validate_ttl always accepts.
    validate_ttl(patch.ttl_secs.flatten())
}

/// Validate a link's endpoints at the service seam (MCP-07 / D-02): a memory
/// cannot be linked to itself. Runs before any store hop, so a self-link is
/// the client tier even when the id does not exist. (The 0003 `CHECK` is the
/// belt-and-braces backstop; it is never what a client observes.)
pub(crate) fn validate_link_endpoints(from_id: i64, to_id: i64) -> Result<(), MemoryError> {
    if from_id == to_id {
        return Err(MemoryError::InvalidArgument(format!(
            "from_id and to_id must differ (got {from_id})"
        )));
    }
    Ok(())
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
