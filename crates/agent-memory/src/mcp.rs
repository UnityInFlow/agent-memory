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

use agent_memory_core::domain::{MemoryType, NewMemory};
use agent_memory_core::service::{ListArgs as ServiceListArgs, MemoryService};

/// Shared application state handed to every tool invocation.
pub struct AppState {
    pub service: MemoryService,
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

        let id = self
            .state
            .service
            .store(new)
            .await
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

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
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        let json = serde_json::to_string(&views)
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
        info.instructions =
            Some("Persistent typed agent memory: store and list typed memories.".to_string());
        info
    }
}
