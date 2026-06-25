//! The transport-agnostic memory service.
//!
//! [`MemoryService`] is the single business-logic seam every transport (the rmcp
//! tools, and a future REST layer) calls into. It owns the [`Store`] and the
//! injected [`Clock`], and wraps every blocking store call in `spawn_blocking` so
//! the async runtime is never blocked (RESEARCH Pitfall 2).
//!
//! `search`, `forget`, and the decay sweep are added in Plans 02/03.

use std::sync::Arc;

use crate::clock::Clock;
use crate::decay::DecayConfig;
use crate::domain::{MemoryError, MemoryType, MemoryView, NewMemory};
use crate::store::Store;

/// Filters for [`MemoryService::list`]. All fields are optional; a `None` field
/// disables that filter. `limit` caps the number of rows returned.
#[derive(Debug, Clone, Default)]
pub struct ListArgs {
    pub mem_type: Option<MemoryType>,
    pub tag: Option<String>,
    pub scope: Option<String>,
    pub limit: Option<i64>,
}

/// The core memory service: validates input, stamps timestamps from the injected
/// clock, and delegates persistence to the [`Store`].
#[derive(Clone)]
pub struct MemoryService {
    store: Arc<dyn Store>,
    clock: Arc<dyn Clock>,
    #[allow(dead_code)] // consumed by the decay sweep in Plan 03
    decay_cfg: DecayConfig,
}

impl MemoryService {
    /// Construct a service over a store and clock with the given decay config.
    pub fn new(store: Arc<dyn Store>, clock: Arc<dyn Clock>, decay_cfg: DecayConfig) -> Self {
        MemoryService {
            store,
            clock,
            decay_cfg,
        }
    }

    /// Store a new memory, returning its assigned id (MCP-01 / D-05).
    ///
    /// The type is validated by the caller building a [`NewMemory`] with a typed
    /// [`MemoryType`]; an invalid wire string is rejected before this point via
    /// [`MemoryType::try_from`], so no bad type ever reaches SQL (D-07).
    pub async fn store(&self, new: NewMemory) -> Result<i64, MemoryError> {
        let store = self.store.clone();
        let now = self.clock.now();
        tokio::task::spawn_blocking(move || store.insert(new, now))
            .await
            .map_err(MemoryError::Join)?
    }

    /// List stored memories newest-first with optional filters (MCP-03 / D-06).
    pub async fn list(&self, args: ListArgs) -> Result<Vec<MemoryView>, MemoryError> {
        let store = self.store.clone();
        let now = self.clock.now();
        tokio::task::spawn_blocking(move || store.list(args, now))
            .await
            .map_err(MemoryError::Join)?
    }
}
