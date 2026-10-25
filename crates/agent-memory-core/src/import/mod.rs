//! Importers for external memory sources (INTEROP-01, INTEROP-02).
//!
//! Invariants every importer guards:
//!
//! - **Tolerance is per importer.** [`gsd_state`] scrapes a human-edited text
//!   file, so messy input degrades to skip-counts — malformed lines, unknown
//!   sections and oversized items are skipped and counted, never a hard
//!   error. [`jsonl`] reads agent-memory's own format-v1 contract
//!   ([`crate::export`]) and is **strict**: any defect rejects the whole file
//!   as [`crate::domain::MemoryError::InvalidArgument`] naming the 1-based
//!   line, and nothing is written (Phase 5 D-11). Both must survive any text
//!   without panicking.
//! - **Idempotent**: every importer dedups against the store through
//!   [`crate::store::Store::find_by_key`] on the exact
//!   `(source, mem_type, content)` key, so re-running an import never
//!   duplicates — [`crate::service::MemoryService::import`] for gsd-state
//!   drafts, [`crate::service::MemoryService::import_jsonl`] for a parsed
//!   export file.
//! - **Data-only**: imported content is stored strictly as data. It is never
//!   executed or interpreted by agent-memory (T-02-20/T-02-21 — downstream
//!   prompt-injection screening composes via the ecosystem injection-scanner).

pub mod gsd_state;
pub mod jsonl;
