//! The rmcp stdio server: thin tool adapters over [`MemoryService`].
//!
//! Per the architecture rule (RESEARCH Anti-Pattern 1), tool methods contain NO
//! SQL, NO decay math, and NO clock access. Each method only: deserializes its
//! `Parameters<T>`, calls the service, and maps the `Result` to a `CallToolResult`.

use std::convert::TryFrom;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};

use agent_memory_core::domain::{MemoryError, MemoryType, NewMemory};
use agent_memory_core::embed::Embedder;
use agent_memory_core::service::{
    ListArgs as ServiceListArgs, MemoryService, SearchArgs as ServiceSearchArgs,
};

/// Shared two-tier [`MemoryError`] → [`McpError`] mapping for all four tool
/// service calls — the mirror of `rest/handlers.rs`'s `map_memory_error`
/// (closes review WR-04): client input (`InvalidType`, `InvalidQuery`) reports
/// as `invalid_params`; internal failures (`Sqlite`, `Pool`, `Join`,
/// `Migration`, `NotFound`) report as `internal_error`, so agents never enter
/// argument-repair loops over a DB fault (T-02G-03).
///
/// `NotFound` as an *error* only arises from internal store conditions (e.g. a
/// poisoned writer mutex) — MCP's clean not-found is the `Ok(false)` forget
/// result, unchanged.
fn map_mcp_error(e: MemoryError) -> McpError {
    match &e {
        MemoryError::InvalidType(_) | MemoryError::InvalidQuery(_) => {
            McpError::invalid_params(e.to_string(), None)
        }
        MemoryError::Sqlite(_)
        | MemoryError::Pool(_)
        | MemoryError::Join(_)
        | MemoryError::Migration(_)
        | MemoryError::NotFound => McpError::internal_error(e.to_string(), None),
    }
}

/// Shared application state handed to every tool invocation (and to the REST
/// handlers — both transports adapt the same state, same service).
pub struct AppState {
    pub service: MemoryService,
    /// The embedder handle, exposed so the REST `/health` endpoint can report
    /// semantic availability without reaching into the service internals.
    pub embedder: Arc<dyn Embedder>,
}

/// Arguments for `memory_store` (D-05). Only `content` and `type` are required;
/// the rest default.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct StoreArgs {
    /// The memory content to persist.
    pub content: String,
    /// One of: DECISION, PATTERN, ERROR, TODO, ARCHITECTURE, CONSTRAINT.
    pub r#type: String,
    /// Optional free-form tags. Defaults to an empty list.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Optional provenance (e.g. a tool or file name).
    #[serde(default)]
    pub source: Option<String>,
    /// Optional logical scope; `null`/omitted means global (D-03).
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional time-to-live in seconds; omitted means no expiry.
    #[serde(default)]
    pub ttl_secs: Option<i64>,
}

/// Arguments for `memory_list` (D-06). All filters are optional.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListArgs {
    /// Filter by memory type (UPPERCASE). Omit for all types.
    #[serde(default)]
    pub r#type: Option<String>,
    /// Filter by a tag substring. Omit for all.
    #[serde(default)]
    pub tag: Option<String>,
    /// Filter by logical scope. Omit for all scopes.
    #[serde(default)]
    pub scope: Option<String>,
    /// Cap the number of returned rows.
    #[serde(default)]
    pub limit: Option<i64>,
}

/// Arguments for `memory_search` (D-04/D-06). `query` is the FTS5 keyword string;
/// the remaining fields are optional filters and a result cap.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// The keyword query (FTS5 MATCH syntax).
    pub query: String,
    /// Filter by memory type (UPPERCASE). Omit for all types.
    #[serde(default)]
    pub r#type: Option<String>,
    /// Filter by a tag substring. Omit for all.
    #[serde(default)]
    pub tag: Option<String>,
    /// Filter by logical scope. Omit for all scopes.
    #[serde(default)]
    pub scope: Option<String>,
    /// Cap the number of returned rows.
    #[serde(default)]
    pub limit: Option<i64>,
}

/// Arguments for `memory_forget` (D-06).
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ForgetArgs {
    /// The numeric id of the memory to delete.
    pub id: i64,
}

/// The MCP server. Holds shared state and the generated tool router.
#[derive(Clone)]
pub struct MemoryMcp {
    state: Arc<AppState>,
    tool_router: ToolRouter<MemoryMcp>,
}

#[tool_router]
impl MemoryMcp {
    /// Build the server over the given application state.
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Store a typed memory; returns its numeric id. Only content and type are required."
    )]
    async fn memory_store(
        &self,
        Parameters(args): Parameters<StoreArgs>,
    ) -> Result<CallToolResult, McpError> {
        // Validate the type up front → clean error, never a panic (D-07).
        let mem_type = MemoryType::try_from(args.r#type.as_str())
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        let new = NewMemory {
            content: args.content,
            mem_type,
            tags: args.tags,
            source: args.source,
            scope: args.scope,
            ttl_secs: args.ttl_secs,
        };

        let id = self.state.service.store(new).await.map_err(map_mcp_error)?;

        Ok(CallToolResult::success(vec![Content::text(id.to_string())]))
    }

    #[tool(
        description = "List stored memories newest-first, with optional type/tag/scope/limit filters."
    )]
    async fn memory_list(
        &self,
        Parameters(args): Parameters<ListArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mem_type = match args.r#type {
            Some(ref t) => Some(
                MemoryType::try_from(t.as_str())
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?,
            ),
            None => None,
        };

        let views = self
            .state
            .service
            .list(ServiceListArgs {
                mem_type,
                tag: args.tag,
                scope: args.scope,
                limit: args.limit,
            })
            .await
            .map_err(map_mcp_error)?;

        let json = serde_json::to_string(&views)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Search stored memories by semantic similarity (local Ollama embeddings) blended with decay, falling back to keyword/FTS5 search when the embedder is unavailable. Returns {search_mode: 'semantic'|'keyword', results: [...]}; results is empty when nothing matches."
    )]
    async fn memory_search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mem_type = match args.r#type {
            Some(ref t) => Some(
                MemoryType::try_from(t.as_str())
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?,
            ),
            None => None,
        };

        // The service returns the shared SearchOutcome envelope; serialize it
        // WHOLE so the top-level search_mode surfaces degraded state (SEARCH-03).
        let outcome = self
            .state
            .service
            .search(ServiceSearchArgs {
                query: args.query,
                mem_type,
                tag: args.tag,
                scope: args.scope,
                limit: args.limit,
            })
            .await
            .map_err(map_mcp_error)?;

        let json = serde_json::to_string(&outcome)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Delete a memory by its numeric id. Returns a deleted result, or a clean not-found result if no such id exists."
    )]
    async fn memory_forget(
        &self,
        Parameters(args): Parameters<ForgetArgs>,
    ) -> Result<CallToolResult, McpError> {
        let deleted = self
            .state
            .service
            .forget(args.id)
            .await
            .map_err(map_mcp_error)?;

        // Ok(false) is a clean not-found, NOT an error (MCP-04). Both branches are
        // a successful tool call returning a small JSON status object.
        let status = if deleted {
            serde_json::json!({ "id": args.id, "deleted": true })
        } else {
            serde_json::json!({ "id": args.id, "deleted": false, "reason": "not_found" })
        };
        let json = serde_json::to_string(&status)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MemoryMcp {
    fn get_info(&self) -> ServerInfo {
        // ServerInfo (InitializeResult) is #[non_exhaustive]; start from Default
        // and set the fields we care about.
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = Implementation::from_build_env();
        info.instructions = Some(
            "Persistent typed agent memory: store, list, search, and forget typed memories."
                .to_string(),
        );
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_mcp_error_splits_client_and_internal_tiers() {
        let invalid_params_code = McpError::invalid_params("x", None).code;
        let internal_error_code = McpError::internal_error("x", None).code;

        assert_eq!(
            map_mcp_error(MemoryError::InvalidQuery("\"".into())).code,
            invalid_params_code,
            "a malformed FTS5 query is client input → invalid_params"
        );
        assert_eq!(
            map_mcp_error(MemoryError::InvalidType("BOGUS".into())).code,
            invalid_params_code,
            "an unknown memory type is client input → invalid_params"
        );
        assert_eq!(
            map_mcp_error(MemoryError::NotFound).code,
            internal_error_code,
            "NotFound-as-error only arises from internal store conditions → internal_error"
        );
    }
}
