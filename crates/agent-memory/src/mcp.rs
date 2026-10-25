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

use agent_memory_core::domain::{
    LinkKind, LinkOutcome, MemoryError, MemoryPatch, MemoryType, NewMemory,
};
use agent_memory_core::embed::Embedder;
use agent_memory_core::service::{
    ListArgs as ServiceListArgs, MemoryService, SearchArgs as ServiceSearchArgs,
};

/// Shared two-tier [`MemoryError`] → [`McpError`] mapping for every tool
/// service calls — the mirror of `rest/handlers.rs`'s `map_memory_error`
/// (closes review WR-04): client input (`InvalidType`, `InvalidQuery`,
/// `InvalidArgument`) reports
/// as `invalid_params`; internal failures (`Sqlite`, `Pool`, `Join`,
/// `Migration`, `Internal`) report as `internal_error`, so agents never enter
/// argument-repair loops over a DB fault (T-02G-03).
///
/// `Internal` covers server faults with no client cause, e.g. a poisoned writer
/// mutex (WR-02). MCP's clean not-found is data — forget's `Ok(false)` and
/// update's `Ok(None)` — never an error variant. No catch-all arm: a new `MemoryError`
/// variant must be classified here at compile time.
fn map_mcp_error(e: MemoryError) -> McpError {
    match &e {
        MemoryError::InvalidType(_)
        | MemoryError::InvalidQuery(_)
        | MemoryError::InvalidArgument(_) => McpError::invalid_params(e.to_string(), None),
        MemoryError::Sqlite(_)
        | MemoryError::Pool(_)
        | MemoryError::Join(_)
        | MemoryError::Migration(_)
        | MemoryError::Internal(_) => McpError::internal_error(e.to_string(), None),
        // Startup-only (D-13): raised by SqliteStore::open before the server
        // starts, so no tool call can produce it. Classified internal for
        // exhaustiveness; the refusal itself reaches stderr via main.rs.
        MemoryError::SchemaTooNew { .. } => McpError::internal_error(e.to_string(), None),
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
    /// Filter by an exact tag (case-sensitive). Omit for all.
    #[serde(default)]
    pub tag: Option<String>,
    /// Filter by logical scope. Omit for all scopes.
    #[serde(default)]
    pub scope: Option<String>,
    /// Cap the number of returned rows (1-200). Omitted lists every row.
    /// Required when `expand_links` is true.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Attach up to 10 one-hop related memories to each result as
    /// `related: [{id, kind, direction, type, snippet, tags}]`. `direction` is
    /// `out` (this memory -> neighbor) or `in` (neighbor -> this memory);
    /// `kind` reads naturally from this memory's side (an incoming supersedes
    /// is `superseded_by`, an incoming caused_by is `caused`). Most recent
    /// links first. Defaults to false (no `related` member at all). Requires
    /// `limit`: `expand_links=true` without a limit is rejected as
    /// invalid_params.
    #[serde(default)]
    pub expand_links: bool,
}

/// Arguments for `memory_search` (D-04/D-06). `query` is plain natural language
/// or FTS5 MATCH syntax; the remaining fields are optional filters and a
/// result cap.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// The search query: plain natural language or FTS5 MATCH syntax. Valid
    /// FTS5 expressions (AND/OR/NOT, quoted phrases, prefix*) run exactly as
    /// written; anything FTS5 cannot parse is retried once as its words OR-ed
    /// together; a query with no words contributes no keyword matches. A query
    /// is never rejected for its syntax, in either mode.
    pub query: String,
    /// Filter by memory type (UPPERCASE). Omit for all types.
    #[serde(default)]
    pub r#type: Option<String>,
    /// Filter by an exact tag (case-sensitive). Omit for all.
    #[serde(default)]
    pub tag: Option<String>,
    /// Filter by logical scope. Omit for all scopes.
    #[serde(default)]
    pub scope: Option<String>,
    /// Cap the number of returned rows.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Attach up to 10 one-hop related memories to each result as
    /// `related: [{id, kind, direction, type, snippet, tags}]`. `direction` is
    /// `out` (this memory -> neighbor) or `in` (neighbor -> this memory);
    /// `kind` reads naturally from this memory's side (an incoming supersedes
    /// is `superseded_by`, an incoming caused_by is `caused`). Most recent
    /// links first. Defaults to false (no `related` member at all).
    #[serde(default)]
    pub expand_links: bool,
}

/// Arguments for `memory_forget` (D-06).
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ForgetArgs {
    /// The numeric id of the memory to delete.
    pub id: i64,
}

/// Arguments for `memory_update` (MCP-06) — a JSON-merge-patch over one memory.
///
/// Omit a field to leave it unchanged; send `null` to clear it (`tags` → empty,
/// `scope` → global, `ttl_secs` → no expiry); send a value to set it. `content`
/// can be replaced but never cleared or emptied. At least one field besides
/// `id` is required. `type` and `source` cannot be changed.
///
/// Unknown keys are rejected (`deny_unknown_fields`, REVIEW WR-02): that covers
/// the immutable `type`/`source` and typos such as `ttl_sec`. In a merge-patch a
/// silently dropped member is a silent partial write, so the whole call fails
/// naming the field and nothing is written. rmcp deserializes the tool
/// `arguments` object straight into this type and reports the failure as a
/// tool error result (`isError: true`, "failed to deserialize parameters:
/// unknown field ...") before the handler runs. MCP `_meta` travels outside
/// `arguments`, so no legitimate key is lost.
///
/// `skip_serializing_if` on the patch fields is for the SCHEMA, not the wire:
/// it stops schemars from advertising `"default": null`. A client that fills
/// in schema defaults would otherwise send explicit nulls — and null means
/// "clear" here, so every patch would wipe tags/scope/ttl.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateArgs {
    /// The numeric id of the memory to update.
    pub id: i64,
    /// New content (re-indexed and re-embedded). Omit to keep; never null or empty.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_explicit_null"
    )]
    pub content: Option<Option<String>>,
    /// Replacement tag list. Omit to keep; `null` clears to an empty list.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_explicit_null"
    )]
    pub tags: Option<Option<Vec<String>>>,
    /// New logical scope. Omit to keep; `null` resets to global.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_explicit_null"
    )]
    pub scope: Option<Option<String>>,
    /// New time-to-live in seconds, counted from now. Omit to keep; `null` removes the expiry.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_explicit_null"
    )]
    pub ttl_secs: Option<Option<i64>>,
}

impl UpdateArgs {
    /// Split into the target id and the core patch, field-for-field.
    fn into_patch(self) -> (i64, MemoryPatch) {
        (
            self.id,
            MemoryPatch {
                content: self.content,
                tags: self.tags,
                scope: self.scope,
                ttl_secs: self.ttl_secs,
            },
        )
    }
}

/// Arguments for `memory_link` (MCP-07): create the relation
/// `from_id -[kind]-> to_id`. `relates_to` is symmetric (one relation however
/// it is linked); `supersedes` and `caused_by` are directed.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LinkArgs {
    /// The id of the memory the edge starts at.
    pub from_id: i64,
    /// The id of the memory the edge points to. Must differ from `from_id`.
    pub to_id: i64,
    /// One of: relates_to (symmetric "see also": linking b->a after a->b is
    /// the same relation), supersedes (from replaces to), caused_by (from was
    /// caused by to).
    pub kind: String,
}

/// Arguments for `memory_unlink` (MCP-07): remove the relation
/// `from_id -[kind]-> to_id`. For supersedes and caused_by, direction and kind
/// must match the stored edge; a symmetric relates_to can be removed from
/// either end.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UnlinkArgs {
    /// The id the edge starts at.
    pub from_id: i64,
    /// The id the edge points to.
    pub to_id: i64,
    /// One of: relates_to (symmetric: removable from either end), supersedes,
    /// caused_by (direction must match).
    pub kind: String,
}

/// Parse a wire `kind` up front (the `MemoryType` precedent): an unknown kind
/// is `invalid_params` naming the allowed set, and never reaches the service.
fn parse_link_kind(wire: &str) -> Result<LinkKind, McpError> {
    LinkKind::try_from(wire).map_err(map_mcp_error)
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
        description = "List stored memories newest-first, with optional type/tag/scope/limit filters. Set expand_links=true to attach each memory's linked memories as trimmed related summaries; expand_links requires a limit (1-200), and a call with expand_links=true and no limit is rejected as invalid_params. Without expand_links, an omitted limit lists every memory."
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
                expand_links: args.expand_links,
            })
            .await
            .map_err(map_mcp_error)?;

        let json = serde_json::to_string(&views)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Search stored memories. When the local Ollama embedder is reachable, search is hybrid: a keyword (FTS5) ranking and a semantic (embedding) ranking are fused with Reciprocal Rank Fusion (k=60), then weighted by decay so fresher memories win ties. When the embedder is unavailable, search falls back to keyword/FTS5 ranking alone. Returns {search_mode: 'hybrid'|'keyword', results: [...]}; results is empty when nothing matches. The query may be plain natural language or FTS5 MATCH syntax: valid FTS5 expressions (AND/OR/NOT, quoted phrases, prefix*) run exactly as written, anything FTS5 cannot parse is retried once as its words OR-ed together, and a query with no words contributes no keyword matches, so a query is never rejected for its syntax, in either mode. The semantic ranking applies type/tag/scope filters after its 4x-limit candidate cut, so a heavily filtered query may return fewer than limit results. Only returned results count as accessed. Set expand_links=true to attach each result's linked memories as trimmed related summaries."
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
                expand_links: args.expand_links,
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

    #[tool(
        description = "Update a memory in place by id (JSON merge-patch): omit a field to keep it, send null to clear it (tags -> [], scope -> global, ttl_secs -> no expiry), send a value to set it. content can be replaced but never cleared; a new content is re-indexed and re-embedded. Unknown or immutable fields (such as type and source, or a misspelled field) are rejected with an error naming the field and nothing is written. Returns the full updated memory including embedding_status (0 = re-embed pending), or a clean not-found result for an unknown id."
    )]
    async fn memory_update(
        &self,
        Parameters(args): Parameters<UpdateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (id, patch) = args.into_patch();
        let updated = self
            .state
            .service
            .update(id, patch)
            .await
            .map_err(map_mcp_error)?;

        // Ok(None) is a clean not-found, NOT an error — the memory_forget shape.
        let json = match updated {
            Some(updated) => serde_json::to_string(&updated),
            None => serde_json::to_string(
                &serde_json::json!({ "id": id, "updated": false, "reason": "not_found" }),
            ),
        }
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Link two memories with a typed relation from_id -[kind]-> to_id. kind is one of: relates_to (a symmetric see-also: linking b->a after a->b is the same relation), supersedes (from_id replaces the older to_id), caused_by (from_id was caused by to_id); supersedes and caused_by are directed. Linking an existing relation again is a no-op success; from_id and to_id must differ. The result always echoes the from_id and to_id you sent. Returns {from_id, to_id, kind, linked: true}, or {linked: false, reason: 'not_found', missing_id} when either memory does not exist."
    )]
    async fn memory_link(
        &self,
        Parameters(args): Parameters<LinkArgs>,
    ) -> Result<CallToolResult, McpError> {
        let kind = parse_link_kind(&args.kind)?;
        let outcome = self
            .state
            .service
            .link(args.from_id, args.to_id, kind)
            .await
            .map_err(map_mcp_error)?;

        // A missing endpoint is a clean not-found, NOT an error — the
        // memory_forget shape, plus the id that was missing (D-02).
        let status = match outcome {
            LinkOutcome::Linked { .. } => serde_json::json!({
                "from_id": args.from_id,
                "to_id": args.to_id,
                "kind": kind,
                "linked": true
            }),
            LinkOutcome::NotFound { missing_id } => serde_json::json!({
                "from_id": args.from_id,
                "to_id": args.to_id,
                "kind": kind,
                "linked": false,
                "reason": "not_found",
                "missing_id": missing_id
            }),
        };
        let json = serde_json::to_string(&status)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Remove the relation from_id -[kind]-> to_id; kind is relates_to, supersedes or caused_by. For supersedes and caused_by the direction must match the stored edge; relates_to is symmetric and can be removed from either end. Returns {from_id, to_id, kind, unlinked: true}, or a clean {unlinked: false, reason: 'not_found'} result when no such edge exists."
    )]
    async fn memory_unlink(
        &self,
        Parameters(args): Parameters<UnlinkArgs>,
    ) -> Result<CallToolResult, McpError> {
        let kind = parse_link_kind(&args.kind)?;
        let removed = self
            .state
            .service
            .unlink(args.from_id, args.to_id, kind)
            .await
            .map_err(map_mcp_error)?;

        let status = if removed {
            serde_json::json!({
                "from_id": args.from_id,
                "to_id": args.to_id,
                "kind": kind,
                "unlinked": true
            })
        } else {
            serde_json::json!({
                "from_id": args.from_id,
                "to_id": args.to_id,
                "kind": kind,
                "unlinked": false,
                "reason": "not_found"
            })
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
            "Persistent typed agent memory: store, list, search, update, link/unlink, and forget typed memories."
                .to_string(),
        );
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use agent_memory_core::clock::SystemClock;
    use agent_memory_core::decay::DecayConfig;
    use agent_memory_core::embed::ollama::OllamaClient;
    use agent_memory_core::store::sqlite::SqliteStore;

    /// A loopback port nothing listens on: stores land pending (status 0) and
    /// no test depends on a local Ollama.
    const DEAD_OLLAMA: &str = "http://127.0.0.1:9";

    fn test_server(dir: &tempfile::TempDir) -> MemoryMcp {
        server_with(dir, Arc::new(OllamaClient::new(DEAD_OLLAMA)))
    }

    /// A server whose embedder is UP (hash-seeded FakeEmbedder vectors), so
    /// memory_search takes the hybrid path (SEARCH-04).
    fn live_server(dir: &tempfile::TempDir) -> MemoryMcp {
        server_with(
            dir,
            Arc::new(agent_memory_core::embed::FakeEmbedder::with_vectors(
                std::collections::HashMap::new(),
            )),
        )
    }

    fn server_with(dir: &tempfile::TempDir, embedder: Arc<dyn Embedder>) -> MemoryMcp {
        let store = SqliteStore::open(&dir.path().join("mem.db")).expect("store opens");
        let service = MemoryService::new(
            Arc::new(store),
            Arc::new(SystemClock),
            embedder.clone(),
            DecayConfig::default(),
        );
        MemoryMcp::new(Arc::new(AppState { service, embedder }))
    }

    /// The JSON text payload of a successful tool result.
    fn result_json(result: &CallToolResult) -> serde_json::Value {
        let value = serde_json::to_value(result).expect("tool result serializes");
        let text = value
            .pointer("/content/0/text")
            .and_then(|t| t.as_str())
            .unwrap_or_else(|| panic!("no text content in tool result: {value}"));
        serde_json::from_str(text).expect("tool text is JSON")
    }

    async fn store_one(server: &MemoryMcp, content: &str) -> i64 {
        let args: StoreArgs = serde_json::from_value(serde_json::json!({
            "content": content,
            "type": "PATTERN",
            "tags": ["before"],
            "scope": "project-a",
            "ttl_secs": 3600
        }))
        .expect("store args deserialize");
        let result = server
            .memory_store(Parameters(args))
            .await
            .expect("store succeeds");
        result_json(&result).as_i64().expect("store returns an id")
    }

    fn update_args(json: serde_json::Value) -> UpdateArgs {
        serde_json::from_value(json).expect("update args deserialize")
    }

    #[tokio::test]
    async fn memory_update_returns_the_full_updated_memory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);
        let id = store_one(&server, "old wording").await;

        let result = server
            .memory_update(Parameters(update_args(serde_json::json!({
                "id": id,
                "content": "new wording",
                "tags": ["after"]
            }))))
            .await
            .expect("update succeeds");

        let body = result_json(&result);
        assert_eq!(body["id"], id);
        assert_eq!(body["content"], "new wording");
        assert_eq!(body["type"], "PATTERN");
        assert_eq!(body["tags"], serde_json::json!(["after"]));
        assert_eq!(body["scope"], "project-a", "omitted scope is unchanged");
        // Dead embedder: the re-embed is pending, and the caller can see it.
        assert_eq!(body["embedding_status"], 0);
    }

    #[tokio::test]
    async fn memory_update_of_an_unknown_id_is_a_clean_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);

        let result = server
            .memory_update(Parameters(update_args(serde_json::json!({
                "id": 999_999,
                "content": "nobody home"
            }))))
            .await
            .expect("an unknown id is a successful tool call, not an error");

        assert_eq!(
            result_json(&result),
            serde_json::json!({ "id": 999_999, "updated": false, "reason": "not_found" })
        );
    }

    #[tokio::test]
    async fn memory_update_with_empty_content_is_invalid_params() {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);
        let id = store_one(&server, "keep me").await;

        let err = server
            .memory_update(Parameters(update_args(serde_json::json!({
                "id": id,
                "content": ""
            }))))
            .await
            .expect_err("empty content is rejected");
        assert_eq!(err.code, McpError::invalid_params("x", None).code);
        assert!(err.message.contains("content must be a non-empty string"));
    }

    /// Raw JSON text → `UpdateArgs` → core patch: the deserializer under test.
    fn patch_from_raw(raw: &str) -> MemoryPatch {
        serde_json::from_str::<UpdateArgs>(raw)
            .expect("raw update args deserialize")
            .into_patch()
            .1
    }

    #[test]
    fn raw_update_args_carry_all_three_wire_states_into_the_patch() {
        // D-08 / RESEARCH Pitfall 4: absent → None, null → Some(None),
        // value → Some(Some(v)) — for each clearable field.
        assert_eq!(patch_from_raw(r#"{"id": 1}"#), MemoryPatch::default());

        let p = patch_from_raw(r#"{"id": 1, "ttl_secs": null, "tags": null, "scope": null}"#);
        assert_eq!(p.ttl_secs, Some(None));
        assert_eq!(p.tags, Some(None));
        assert_eq!(p.scope, Some(None));
        assert_eq!(p.content, None, "content stays absent");

        let p = patch_from_raw(
            r#"{"id": 1, "ttl_secs": 90, "tags": ["a"], "scope": "s", "content": "c"}"#,
        );
        assert_eq!(p.ttl_secs, Some(Some(90)));
        assert_eq!(p.tags, Some(Some(vec!["a".to_string()])));
        assert_eq!(p.scope, Some(Some("s".to_string())));
        assert_eq!(p.content, Some(Some("c".to_string())));

        // content: null survives deserialization as an explicit null, so the
        // service can reject it (it must never collapse into "absent").
        assert_eq!(
            patch_from_raw(r#"{"id": 1, "content": null}"#).content,
            Some(None)
        );

        let (id, _) = serde_json::from_str::<UpdateArgs>(r#"{"id": 42, "tags": []}"#)
            .expect("deserializes")
            .into_patch();
        assert_eq!(id, 42);
    }

    async fn update_raw(server: &MemoryMcp, raw: &str) -> Result<CallToolResult, McpError> {
        let args: UpdateArgs = serde_json::from_str(raw).expect("raw update args deserialize");
        server.memory_update(Parameters(args)).await
    }

    async fn update_ok(server: &MemoryMcp, raw: &str) -> serde_json::Value {
        result_json(&update_raw(server, raw).await.expect("update succeeds"))
    }

    #[tokio::test]
    async fn memory_update_tags_and_scope_matrix_absent_null_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);

        // tags
        let id = store_one(&server, "t1").await;
        let body = update_ok(&server, &format!(r#"{{"id": {id}, "scope": "x"}}"#)).await;
        assert_eq!(
            body["tags"],
            serde_json::json!(["before"]),
            "absent → unchanged"
        );
        let id = store_one(&server, "t2").await;
        let body = update_ok(&server, &format!(r#"{{"id": {id}, "tags": null}}"#)).await;
        assert_eq!(body["tags"], serde_json::json!([]), "null → empty");
        let id = store_one(&server, "t3").await;
        let body = update_ok(&server, &format!(r#"{{"id": {id}, "tags": ["n"]}}"#)).await;
        assert_eq!(body["tags"], serde_json::json!(["n"]), "value → set");

        // scope
        let id = store_one(&server, "s1").await;
        let body = update_ok(&server, &format!(r#"{{"id": {id}, "tags": ["k"]}}"#)).await;
        assert_eq!(body["scope"], "project-a", "absent → unchanged");
        let id = store_one(&server, "s2").await;
        let body = update_ok(&server, &format!(r#"{{"id": {id}, "scope": null}}"#)).await;
        assert_eq!(body["scope"], serde_json::Value::Null, "null → global");
        let id = store_one(&server, "s3").await;
        let body = update_ok(&server, &format!(r#"{{"id": {id}, "scope": "project-b"}}"#)).await;
        assert_eq!(body["scope"], "project-b", "value → set");
    }

    #[tokio::test]
    async fn memory_update_ttl_matrix_absent_null_value() {
        use agent_memory_core::clock::Clock;

        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);
        let now = SystemClock.now();
        // store_one sets a 3600 s TTL; this is well past it.
        let past_old_ttl = now + 7_200;

        let survives = |id: i64, at: i64| {
            let server = server.clone();
            async move {
                server.state.service.sweep(at).await.expect("sweep");
                let listed = server
                    .memory_list(Parameters(ListArgs {
                        r#type: None,
                        tag: None,
                        scope: None,
                        limit: None,
                        expand_links: false,
                    }))
                    .await
                    .expect("list");
                result_json(&listed)
                    .as_array()
                    .expect("list is an array")
                    .iter()
                    .any(|v| v["id"] == id)
            }
        };

        let id = store_one(&server, "ttl absent").await;
        update_ok(&server, &format!(r#"{{"id": {id}, "tags": ["x"]}}"#)).await;
        assert!(
            !survives(id, past_old_ttl).await,
            "absent → the old TTL still applies"
        );

        let id = store_one(&server, "ttl null").await;
        update_ok(&server, &format!(r#"{{"id": {id}, "ttl_secs": null}}"#)).await;
        assert!(survives(id, past_old_ttl).await, "null → no expiry");

        let id = store_one(&server, "ttl value").await;
        update_ok(&server, &format!(r#"{{"id": {id}, "ttl_secs": 86400}}"#)).await;
        assert!(
            survives(id, past_old_ttl).await,
            "value → the new TTL replaced the old"
        );
        assert!(!survives(id, now + 2 * 86_400).await, "…and still expires");
    }

    #[tokio::test]
    async fn memory_update_invalid_patches_are_invalid_params_never_internal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);
        let id = store_one(&server, "untouched").await;
        let invalid_params = McpError::invalid_params("x", None).code;
        let too_big = agent_memory_core::domain::MAX_TTL_SECS + 1;

        for (raw, needle) in [
            (format!(r#"{{"id": {id}}}"#), "at least one of"),
            (
                format!(r#"{{"id": {id}, "content": null}}"#),
                "content must be a non-empty string",
            ),
            (
                format!(r#"{{"id": {id}, "content": ""}}"#),
                "content must be a non-empty string",
            ),
            (
                format!(r#"{{"id": {id}, "ttl_secs": 0}}"#),
                "ttl_secs must be between",
            ),
            (
                format!(r#"{{"id": {id}, "ttl_secs": -3}}"#),
                "ttl_secs must be between",
            ),
            (
                format!(r#"{{"id": {id}, "ttl_secs": {too_big}}}"#),
                "ttl_secs must be between",
            ),
        ] {
            let err = update_raw(&server, &raw)
                .await
                .expect_err("invalid patch is rejected");
            assert_eq!(
                err.code, invalid_params,
                "{raw}: client tier, never internal_error"
            );
            assert!(err.message.contains(needle), "{raw}: got {:?}", err.message);
        }

        // Still exactly as stored.
        let body = update_ok(&server, &format!(r#"{{"id": {id}, "scope": "probe"}}"#)).await;
        assert_eq!(body["content"], "untouched");
        assert_eq!(body["tags"], serde_json::json!(["before"]));
    }

    #[test]
    fn update_args_schema_makes_only_id_required() {
        let schema =
            serde_json::to_value(schemars::schema_for!(UpdateArgs)).expect("schema serializes");
        assert_eq!(
            schema["required"],
            serde_json::json!(["id"]),
            "patch fields are optional in the tool schema: {schema}"
        );
        for field in ["content", "tags", "scope", "ttl_secs"] {
            let property = &schema["properties"][field];
            assert!(
                property.is_object(),
                "{field} is described in the tool schema: {schema}"
            );
            // null means "clear" in a merge-patch: advertising it as the
            // default would invite clients to wipe fields they never touched.
            assert!(
                property.get("default").is_none(),
                "{field} must not advertise a default: {property}"
            );
        }
        // deny_unknown_fields is advertised, so a schema-driven client knows
        // that type/source and misspelled keys are refused (REVIEW WR-02).
        assert_eq!(
            schema["additionalProperties"],
            serde_json::json!(false),
            "unknown fields are refused in the tool schema: {schema}"
        );
    }

    #[test]
    fn update_args_reject_unknown_and_immutable_fields() {
        for (raw, field) in [
            (r#"{"id": 1, "content": "x", "type": "DECISION"}"#, "type"),
            (r#"{"id": 1, "source": "y"}"#, "source"),
            (r#"{"id": 1, "ttl_sec": 60}"#, "ttl_sec"),
        ] {
            let err = serde_json::from_str::<UpdateArgs>(raw)
                .expect_err("an unknown or immutable field is refused");
            let message = err.to_string();
            assert!(
                message.contains("unknown field") && message.contains(field),
                "{raw}: got {message:?}"
            );
        }
    }

    async fn link_raw(server: &MemoryMcp, raw: &str) -> Result<CallToolResult, McpError> {
        let args: LinkArgs = serde_json::from_str(raw).expect("raw link args deserialize");
        server.memory_link(Parameters(args)).await
    }

    async fn unlink_raw(server: &MemoryMcp, raw: &str) -> Result<CallToolResult, McpError> {
        let args: UnlinkArgs = serde_json::from_str(raw).expect("raw unlink args deserialize");
        server.memory_unlink(Parameters(args)).await
    }

    #[tokio::test]
    async fn memory_link_and_unlink_round_trip_with_idempotent_duplicates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);
        let a = store_one(&server, "newer decision").await;
        let b = store_one(&server, "older decision").await;
        let raw = format!(r#"{{"from_id": {a}, "to_id": {b}, "kind": "supersedes"}}"#);
        let linked = serde_json::json!({
            "from_id": a, "to_id": b, "kind": "supersedes", "linked": true
        });

        let first = link_raw(&server, &raw).await.expect("link succeeds");
        assert_eq!(result_json(&first), linked);
        // D-02: the identical link again is the same success, not an error.
        let again = link_raw(&server, &raw).await.expect("duplicate is a no-op");
        assert_eq!(result_json(&again), linked);

        let removed = unlink_raw(&server, &raw).await.expect("unlink succeeds");
        assert_eq!(
            result_json(&removed),
            serde_json::json!({
                "from_id": a, "to_id": b, "kind": "supersedes", "unlinked": true
            })
        );
        let missing = unlink_raw(&server, &raw)
            .await
            .expect("a missing edge is a successful tool call");
        assert_eq!(
            result_json(&missing),
            serde_json::json!({
                "from_id": a, "to_id": b, "kind": "supersedes",
                "unlinked": false, "reason": "not_found"
            })
        );
    }

    #[tokio::test]
    async fn relates_to_link_and_unlink_responses_echo_the_request_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);
        let a = store_one(&server, "first see-also").await;
        let b = store_one(&server, "second see-also").await;
        assert!(b > a, "b sorts after a, so canonical storage is (a, b)");
        // Stored canonically as (a, b), yet the wire reports what was sent.
        let raw = format!(r#"{{"from_id": {b}, "to_id": {a}, "kind": "relates_to"}}"#);

        let linked = link_raw(&server, &raw).await.expect("link succeeds");
        let body = result_json(&linked);
        assert_eq!(body["from_id"], b, "{body}");
        assert_eq!(body["to_id"], a, "{body}");
        assert_eq!(body["kind"], "relates_to", "{body}");
        assert_eq!(body["linked"], true, "{body}");

        let removed = unlink_raw(&server, &raw).await.expect("unlink succeeds");
        let body = result_json(&removed);
        assert_eq!(body["from_id"], b, "{body}");
        assert_eq!(body["to_id"], a, "{body}");
        assert_eq!(body["kind"], "relates_to", "{body}");
        assert_eq!(body["unlinked"], true, "{body}");

        // A not-found body names the request's ids and its missing id.
        let missing = link_raw(
            &server,
            &format!(r#"{{"from_id": 999999, "to_id": {a}, "kind": "relates_to"}}"#),
        )
        .await
        .expect("a missing endpoint is a successful tool call");
        assert_eq!(
            result_json(&missing),
            serde_json::json!({
                "from_id": 999_999, "to_id": a, "kind": "relates_to",
                "linked": false, "reason": "not_found", "missing_id": 999_999
            })
        );
    }

    #[test]
    fn link_and_unlink_descriptions_state_relates_to_symmetry() {
        let tools = MemoryMcp::tool_router().list_all();
        let description_of = |name: &str| {
            tools
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("{name} is registered"))
                .description
                .as_deref()
                .unwrap_or_default()
                .to_string()
        };
        let link = description_of("memory_link");
        let unlink = description_of("memory_unlink");
        assert!(link.contains("symmetric"), "{link}");
        assert!(unlink.contains("symmetric"), "{unlink}");
        assert!(unlink.contains("either end"), "{unlink}");
        assert!(
            unlink.contains("For supersedes and caused_by the direction must match"),
            "{unlink}"
        );
    }

    #[tokio::test]
    async fn memory_link_to_a_missing_memory_is_a_clean_not_found_naming_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);
        let a = store_one(&server, "present").await;

        let result = link_raw(
            &server,
            &format!(r#"{{"from_id": {a}, "to_id": 999999, "kind": "caused_by"}}"#),
        )
        .await
        .expect("a missing endpoint is a successful tool call, never internal_error");
        assert_eq!(
            result_json(&result),
            serde_json::json!({
                "from_id": a, "to_id": 999_999, "kind": "caused_by",
                "linked": false, "reason": "not_found", "missing_id": 999_999
            })
        );
    }

    #[tokio::test]
    async fn memory_link_self_links_and_unknown_kinds_are_invalid_params() {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);
        let a = store_one(&server, "solo").await;
        let invalid_params = McpError::invalid_params("x", None).code;

        let err = link_raw(
            &server,
            &format!(r#"{{"from_id": {a}, "to_id": {a}, "kind": "relates_to"}}"#),
        )
        .await
        .expect_err("self-link is rejected");
        assert_eq!(err.code, invalid_params);
        assert_eq!(
            err.message,
            format!("invalid argument: from_id and to_id must differ (got {a})")
        );

        for call in ["link", "unlink"] {
            let raw = format!(r#"{{"from_id": {a}, "to_id": 7, "kind": "blocks"}}"#);
            let err = match call {
                "link" => link_raw(&server, &raw).await,
                _ => unlink_raw(&server, &raw).await,
            }
            .expect_err("unknown kind is rejected");
            assert_eq!(err.code, invalid_params, "{call}: client tier");
            assert_eq!(
                err.message,
                "invalid argument: kind must be one of relates_to, supersedes, caused_by (got 'blocks')",
                "{call}"
            );
        }
    }

    #[test]
    fn link_args_schemas_require_all_three_fields_and_advertise_no_defaults() {
        for schema in [
            serde_json::to_value(schemars::schema_for!(LinkArgs)).expect("schema"),
            serde_json::to_value(schemars::schema_for!(UnlinkArgs)).expect("schema"),
        ] {
            assert_eq!(
                schema["required"],
                serde_json::json!(["from_id", "to_id", "kind"]),
                "{schema}"
            );
            for field in ["from_id", "to_id", "kind"] {
                assert!(
                    schema["properties"][field].get("default").is_none(),
                    "{field} must not advertise a default: {schema}"
                );
            }
        }
    }

    #[tokio::test]
    async fn expand_links_from_raw_args_attaches_related_on_search_and_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);
        let anchor = store_one(&server, "quasar anchor").await;
        let long_neighbor = format!("{} end", "neighbor ".repeat(40));
        let neighbor = store_one(&server, &long_neighbor).await;
        link_raw(
            &server,
            &format!(r#"{{"from_id": {neighbor}, "to_id": {anchor}, "kind": "supersedes"}}"#),
        )
        .await
        .expect("link");

        // memory_search, flag on — raw wire JSON through the real DTO.
        let args: SearchArgs =
            serde_json::from_str(r#"{"query": "quasar", "expand_links": true}"#).expect("args");
        let body = result_json(
            &server
                .memory_search(Parameters(args))
                .await
                .expect("search"),
        );
        let related = &body["results"][0]["related"];
        assert_eq!(related[0]["id"], neighbor);
        assert_eq!(related[0]["kind"], "superseded_by");
        assert_eq!(related[0]["direction"], "in");
        assert_eq!(related[0]["type"], "PATTERN");
        let snippet = related[0]["snippet"].as_str().expect("snippet");
        assert!(snippet.ends_with('…'), "long content is cut: {snippet}");
        assert_eq!(snippet.chars().count(), 161);

        // memory_search, flag absent — no related member anywhere.
        let args: SearchArgs = serde_json::from_str(r#"{"query": "quasar"}"#).expect("args");
        let result = server
            .memory_search(Parameters(args))
            .await
            .expect("search");
        let text = serde_json::to_value(&result).expect("serializes")["content"][0]["text"]
            .as_str()
            .expect("text")
            .to_string();
        assert!(!text.contains("related"), "{text}");

        // memory_list without a limit: expand_links is rejected (REVIEW WR-04
        // option-a) as invalid_params, naming the fix.
        let args: ListArgs = serde_json::from_str(r#"{"expand_links": true}"#).expect("args");
        let err = server
            .memory_list(Parameters(args))
            .await
            .expect_err("expand_links without a limit must be rejected");
        assert_eq!(err.code, McpError::invalid_params("x", None).code);
        assert!(
            err.message
                .contains("limit is required when expand_links is true"),
            "{}",
            err.message
        );

        // memory_list, flag on (with a limit) and off.
        let args: ListArgs =
            serde_json::from_str(r#"{"expand_links": true, "limit": 10}"#).expect("args");
        let body = result_json(&server.memory_list(Parameters(args)).await.expect("list"));
        let rows = body.as_array().expect("array");
        let anchor_row = rows.iter().find(|r| r["id"] == anchor).expect("anchor");
        assert_eq!(anchor_row["related"][0]["id"], neighbor);
        let neighbor_row = rows.iter().find(|r| r["id"] == neighbor).expect("neighbor");
        assert_eq!(neighbor_row["related"][0]["kind"], "supersedes");
        assert_eq!(neighbor_row["related"][0]["direction"], "out");

        let args: ListArgs = serde_json::from_str("{}").expect("args");
        let body = result_json(&server.memory_list(Parameters(args)).await.expect("list"));
        assert!(body
            .as_array()
            .expect("array")
            .iter()
            .all(|r| r.get("related").is_none()));
    }

    #[test]
    fn expand_links_is_an_optional_boolean_in_both_tool_schemas() {
        for schema in [
            serde_json::to_value(schemars::schema_for!(SearchArgs)).expect("schema"),
            serde_json::to_value(schemars::schema_for!(ListArgs)).expect("schema"),
        ] {
            let property = &schema["properties"]["expand_links"];
            assert_eq!(property["type"], "boolean", "{schema}");
            assert_eq!(property["default"], false, "{schema}");
            let required = schema["required"].as_array().cloned().unwrap_or_default();
            assert!(
                !required.contains(&serde_json::json!("expand_links")),
                "{schema}"
            );
        }
        // A non-boolean is rejected by deserialization (the typed seam).
        assert!(
            serde_json::from_str::<SearchArgs>(r#"{"query": "q", "expand_links": "yes"}"#).is_err()
        );
    }

    fn search_args(json: serde_json::Value) -> SearchArgs {
        serde_json::from_value(json).expect("search args deserialize")
    }

    #[tokio::test]
    async fn memory_search_reports_hybrid_when_the_embedder_is_up_and_keyword_when_down() {
        // SEARCH-04 wire contract on the MCP transport, from raw tool args.
        for (expected_mode, live) in [("hybrid", true), ("keyword", false)] {
            let dir = tempfile::tempdir().expect("tempdir");
            let server = if live {
                live_server(&dir)
            } else {
                test_server(&dir)
            };
            let id = store_one(&server, "hybrid recall over the ledger").await;

            let result = server
                .memory_search(Parameters(search_args(serde_json::json!({
                    "query": "ledger"
                }))))
                .await
                .expect("search succeeds");
            let body = result_json(&result);
            assert_eq!(body["search_mode"], expected_mode, "{body}");
            assert_eq!(body["results"][0]["id"], id, "{body}");
            assert_eq!(
                body.as_object().map(serde_json::Map::len),
                Some(2),
                "envelope is exactly {{search_mode, results}}: {body}"
            );
        }
    }

    #[tokio::test]
    async fn memory_search_answers_queries_fts5_cannot_parse_in_hybrid_mode() {
        // User decision option-a (T-04-15 revised): a query FTS5 cannot parse
        // is answered in hybrid mode exactly as in keyword mode — never
        // invalid_params, never internal_error, never SQLite text.
        let dir = tempfile::tempdir().expect("tempdir");
        let server = live_server(&dir);
        store_one(&server, "any searchable row").await;

        for input in ["\"", "what is the plan?", "sqlite-vec"] {
            let result = server
                .memory_search(Parameters(search_args(serde_json::json!({
                    "query": input
                }))))
                .await
                .unwrap_or_else(|e| panic!("{input:?} must be answered in hybrid mode: {e:?}"));
            let raw = serde_json::to_string(&result).expect("tool result serializes");
            assert!(!raw.contains("sqlite error"), "{input:?}: {raw}");
            let body = result_json(&result);
            assert_eq!(body["search_mode"], "hybrid", "{input:?}: {body}");
        }
    }

    #[test]
    fn memory_search_description_enumerates_exactly_hybrid_and_keyword() {
        let tools = MemoryMcp::tool_router().list_all();
        let search = tools
            .iter()
            .find(|t| t.name == "memory_search")
            .expect("memory_search is registered");
        let description = search.description.as_deref().unwrap_or_default();
        // The whole enumeration, delimited on both sides, so a third mode on
        // either side (the retired semantic one included) breaks the match.
        assert!(
            description.contains("{search_mode: 'hybrid'|'keyword', results"),
            "{description}"
        );
        assert!(description.contains("k=60"), "{description}");
        // Option-a (T-04-15 revised): the forgiving query contract is what
        // agents read, and the retired invalid_params promise is gone.
        assert!(
            description.contains("plain natural language"),
            "{description}"
        );
        assert!(description.contains("never rejected"), "{description}");
        assert!(!description.contains("invalid_params"), "{description}");
    }

    /// The shared option-a corpus (mirrors agent-memory-core tests/hybrid.rs):
    /// five rows, a targeted decision table (query -> 1-based corpus index
    /// recalled first in both modes), and tokenless or no-hit queries.
    const CORPUS: [&str; 5] = [
        "We chose sqlite-vec for local vector similarity search",
        "auth tokens rotate every hour",
        "C++ build flags need care",
        "don't retry the import blindly",
        "the unbalanced ledger is reconciled nightly",
    ];
    const TARGETED: [(&str, usize); 7] = [
        ("what vector store do we use?", 1),
        ("sqlite-vec", 1),
        ("don't retry", 4),
        ("auth, tokens", 2),
        ("auth: tokens", 2),
        ("C++", 3),
        ("\"unbalanced", 5),
    ];
    const TOKENLESS_OR_NO_HIT: [&str; 4] = ["AND", "", "?!,", "*"];

    #[tokio::test]
    async fn memory_search_answers_every_query_shape_in_both_modes() {
        for (expected_mode, live) in [("hybrid", true), ("keyword", false)] {
            let dir = tempfile::tempdir().expect("tempdir");
            let server = if live {
                live_server(&dir)
            } else {
                test_server(&dir)
            };
            let mut ids = Vec::new();
            for content in CORPUS {
                ids.push(store_one(&server, content).await);
            }

            let queries = TARGETED
                .iter()
                .map(|(q, target)| (*q, Some(*target)))
                .chain(TOKENLESS_OR_NO_HIT.iter().map(|q| (*q, None)));
            for (input, target) in queries {
                let result = server
                    .memory_search(Parameters(search_args(serde_json::json!({
                        "query": input
                    }))))
                    .await
                    .unwrap_or_else(|e| {
                        panic!("{expected_mode}: {input:?} must be answered, got {e:?}")
                    });
                let raw = serde_json::to_string(&result).expect("tool result serializes");
                assert!(!raw.contains("sqlite error"), "{input:?}: {raw}");
                let body = result_json(&result);
                assert_eq!(body["search_mode"], expected_mode, "{input:?}: {body}");
                let results = body["results"].as_array().expect("results array");
                match target {
                    Some(index) => assert_eq!(
                        results.first().and_then(|r| r["id"].as_i64()),
                        Some(ids[index - 1]),
                        "{expected_mode}: {input:?} must recall corpus row {index} first: {body}"
                    ),
                    None if live => assert!(
                        !results.is_empty(),
                        "hybrid: {input:?} must answer from the semantic leg: {body}"
                    ),
                    None => assert!(
                        results.is_empty(),
                        "keyword: {input:?} has no keyword matches: {body}"
                    ),
                }
            }
        }
    }

    #[tokio::test]
    async fn memory_search_storage_failure_is_internal_error_without_sqlite_text() {
        // T-04-19: a broken FTS index (renamed away underneath the server)
        // reaches the agent only as the generic internal error, in both modes.
        let internal_error_code = McpError::internal_error("x", None).code;
        for live in [true, false] {
            let dir = tempfile::tempdir().expect("tempdir");
            let server = if live {
                live_server(&dir)
            } else {
                test_server(&dir)
            };
            store_one(&server, "ledger reconciliation runbook").await;
            agent_memory_core::store::sqlite::register_vec_extension().expect("register vec0");
            rusqlite::Connection::open(dir.path().join("mem.db"))
                .expect("open db")
                .execute_batch("ALTER TABLE memories_fts RENAME TO memories_fts_offline;")
                .expect("rename fts table");

            let err = server
                .memory_search(Parameters(search_args(serde_json::json!({
                    "query": "ledger"
                }))))
                .await
                .expect_err("a broken FTS index must fail the search");
            assert_eq!(err.code, internal_error_code, "live={live}: {err:?}");
            assert!(
                err.message.contains("search failed due to a storage error"),
                "live={live}: {err:?}"
            );
            assert!(
                !err.message.to_lowercase().contains("sqlite"),
                "live={live}: no SQLite text may reach the agent: {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn memory_store_storage_failure_is_internal_error_without_sqlite_text() {
        // D-14 (04-REVIEW IN-01): a write-path storage fault — the `memories`
        // table renamed away underneath the server — reaches the agent only
        // as the generic internal error, like search since T-04-19.
        let internal_error_code = McpError::internal_error("x", None).code;
        let dir = tempfile::tempdir().expect("tempdir");
        let server = test_server(&dir);
        agent_memory_core::store::sqlite::register_vec_extension().expect("register vec0");
        rusqlite::Connection::open(dir.path().join("mem.db"))
            .expect("open db")
            .execute_batch("ALTER TABLE memories RENAME TO memories_offline;")
            .expect("rename memories table");

        let args: StoreArgs = serde_json::from_value(serde_json::json!({
            "content": "will not land",
            "type": "DECISION"
        }))
        .expect("store args deserialize");
        let err = server
            .memory_store(Parameters(args))
            .await
            .expect_err("a missing memories table must fail the store");
        assert_eq!(err.code, internal_error_code, "{err:?}");
        assert!(
            err.message.contains("store failed due to a storage error"),
            "{err:?}"
        );
        assert!(
            !err.message.to_lowercase().contains("sqlite"),
            "no SQLite text may reach the agent: {err:?}"
        );
    }

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
            map_mcp_error(MemoryError::InvalidArgument(
                "limit must be between 1 and 200 (got 0)".into()
            ))
            .code,
            invalid_params_code,
            "an out-of-bounds argument is client input → invalid_params"
        );
        assert_eq!(
            map_mcp_error(MemoryError::Internal("writer lock poisoned".into())).code,
            internal_error_code,
            "a poisoned writer lock is a server fault → internal_error (WR-02)"
        );
        assert_ne!(
            map_mcp_error(MemoryError::Internal("writer lock poisoned".into())).code,
            invalid_params_code,
            "a server fault must never read as client input"
        );
    }
}
