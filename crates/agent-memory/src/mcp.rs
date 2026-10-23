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

use agent_memory_core::domain::{MemoryError, MemoryPatch, MemoryType, NewMemory};
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
    /// Filter by an exact tag (case-sensitive). Omit for all.
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

/// Arguments for `memory_update` (MCP-06) — a JSON-merge-patch over one memory.
///
/// Omit a field to leave it unchanged; send `null` to clear it (`tags` → empty,
/// `scope` → global, `ttl_secs` → no expiry); send a value to set it. `content`
/// can be replaced but never cleared or emptied. At least one field besides
/// `id` is required. `type` and `source` cannot be changed.
///
/// `skip_serializing_if` on the patch fields is for the SCHEMA, not the wire:
/// it stops schemars from advertising `"default": null`. A client that fills
/// in schema defaults would otherwise send explicit nulls — and null means
/// "clear" here, so every patch would wipe tags/scope/ttl.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
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

    #[tool(
        description = "Update a memory in place by id (JSON merge-patch): omit a field to keep it, send null to clear it (tags -> [], scope -> global, ttl_secs -> no expiry), send a value to set it. content can be replaced but never cleared; a new content is re-indexed and re-embedded. Returns the full updated memory including embedding_status (0 = re-embed pending), or a clean not-found result for an unknown id."
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
            "Persistent typed agent memory: store, list, search, update, and forget typed memories."
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
        let store = SqliteStore::open(&dir.path().join("mem.db")).expect("store opens");
        let embedder: Arc<dyn Embedder> = Arc::new(OllamaClient::new(DEAD_OLLAMA));
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
