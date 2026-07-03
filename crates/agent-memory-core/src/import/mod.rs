//! Importers for external memory sources (INTEROP-01).
//!
//! Invariants every importer guards:
//!
//! - **Tolerant**: messy input degrades to skip-counts — malformed lines,
//!   unknown sections, and oversized items are skipped and counted, never a
//!   hard error. An importer must survive any text file without panicking.
//! - **Idempotent**: importers produce [`crate::domain::NewMemory`] drafts that
//!   [`crate::service::MemoryService::import`] dedups against the store on the
//!   exact `(source, mem_type, content)` key, so re-running an import never
//!   duplicates.
//! - **Data-only**: imported content is stored strictly as data. It is never
//!   executed or interpreted by agent-memory (T-02-20/T-02-21 — downstream
//!   prompt-injection screening composes via the ecosystem injection-scanner).

pub mod gsd_state;
