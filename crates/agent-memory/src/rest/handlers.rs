//! The REST handlers — thin adapters with ZERO business logic.
//!
//! Each handler copies the `mcp.rs` per-tool shape: validate the wire type via
//! `MemoryType::try_from` up front, build the exact core service arg structs,
//! make ONE service call, and map the error. The request DTOs mirror the MCP
//! tool argument structs field-for-field (`content`, `type`, `tags`, `source`,
//! `scope`, `ttl_secs`, `query`, `limit`, `id`) so both transports speak one
//! dialect. serde only — REST DTOs never derive a JSON Schema (that is the
//! MCP tool-schema concern).

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use agent_memory_core::domain::{
    LinkKind, LinkOutcome, MemoryError, MemoryPatch, MemoryType, NewMemory,
};
use agent_memory_core::embed::EmbedderHealth;
use agent_memory_core::service::{
    ListArgs as ServiceListArgs, SearchArgs as ServiceSearchArgs, SearchOutcome,
};

use super::ApiError;
use crate::mcp::AppState;

/// Body for `POST /api/memories` — mirrors `mcp::StoreArgs`.
#[derive(Debug, serde::Deserialize)]
pub struct StoreRequest {
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
    /// Optional logical scope; `null`/omitted means global.
    #[serde(default)]
    pub scope: Option<String>,
    /// Optional time-to-live in seconds; omitted means no expiry.
    #[serde(default)]
    pub ttl_secs: Option<i64>,
}

/// Body for `POST /api/search` — mirrors `mcp::SearchArgs`.
#[derive(Debug, serde::Deserialize)]
pub struct SearchRequest {
    /// The search query.
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
    /// Attach 1-hop `related` summaries to each row (MCP-07). Omitted = false
    /// (the v1.0 response, byte-identical).
    #[serde(default)]
    pub expand_links: bool,
}

/// Query string for `GET /api/memories` — mirrors `mcp::ListArgs`.
#[derive(Debug, serde::Deserialize)]
pub struct ListQuery {
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
    /// Attach 1-hop `related` summaries to each row (MCP-07). Omitted = false
    /// (the v1.0 response, byte-identical).
    #[serde(default)]
    pub expand_links: bool,
}

/// Body for `PATCH /api/memories/{id}` — mirrors `mcp::UpdateArgs` field for
/// field (the id comes from the path). A JSON-merge-patch: an absent member
/// keeps the field, `null` clears it (`tags` → `[]`, `scope` → global,
/// `ttl_secs` → no expiry), a value sets it. `content` is never clearable.
/// Uses the SAME explicit-null helper as the MCP DTO, so the two transports
/// cannot drift on null-vs-absent.
///
/// Unknown keys are rejected (`deny_unknown_fields`, REVIEW WR-02), exactly as
/// on the MCP DTO: the immutable `type`/`source` and typos such as `ttl_sec`
/// make the whole PATCH a 400 naming the field, and nothing is written. A
/// silently dropped merge-patch member would be a silent partial write.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateBody {
    /// New content (re-indexed and re-embedded). Never null or empty.
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_explicit_null"
    )]
    pub content: Option<Option<String>>,
    /// Replacement tag list; `null` clears to an empty list.
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_explicit_null"
    )]
    pub tags: Option<Option<Vec<String>>>,
    /// New logical scope; `null` resets to global.
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_explicit_null"
    )]
    pub scope: Option<Option<String>>,
    /// New time-to-live in seconds from now; `null` removes the expiry.
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_explicit_null"
    )]
    pub ttl_secs: Option<Option<i64>>,
}

/// Body for `POST /api/links` — mirrors `mcp::LinkArgs` field for field:
/// the directed edge `from_id -[kind]-> to_id`.
#[derive(Debug, serde::Deserialize)]
pub struct LinkBody {
    /// The id of the memory the edge starts at.
    pub from_id: i64,
    /// The id of the memory the edge points to. Must differ from `from_id`.
    pub to_id: i64,
    /// One of: relates_to, supersedes, caused_by.
    pub kind: String,
}

impl From<UpdateBody> for MemoryPatch {
    fn from(body: UpdateBody) -> Self {
        MemoryPatch {
            content: body.content,
            tags: body.tags,
            scope: body.scope,
            ttl_secs: body.ttl_secs,
        }
    }
}

/// Two-tier [`MemoryError`] → HTTP mapping, mirroring `mcp.rs`'s
/// `invalid_params` / `internal_error` split: `InvalidType`, `InvalidQuery`,
/// and `InvalidArgument` are client input (400); everything else — including
/// `Internal` (e.g. a poisoned writer lock, WR-02) — is 500. A missing resource
/// is never an error variant: handlers build their 404 from not-found *data*
/// (forget's `Ok(false)`). No catch-all arm: a new `MemoryError` variant must
/// be classified here at compile time.
fn map_memory_error(e: MemoryError) -> ApiError {
    match &e {
        MemoryError::InvalidType(_)
        | MemoryError::InvalidQuery(_)
        | MemoryError::InvalidArgument(_) => ApiError::BadRequest(e.to_string()),
        MemoryError::Sqlite(_)
        | MemoryError::Pool(_)
        | MemoryError::Join(_)
        | MemoryError::Migration(_)
        | MemoryError::Internal(_) => ApiError::Internal(e.to_string()),
        // Startup-only (D-13): raised by SqliteStore::open before the REST
        // server binds, so no request can produce it. Classified 500 for
        // exhaustiveness; the refusal itself reaches stderr via main.rs.
        MemoryError::SchemaTooNew { .. } => ApiError::Internal(e.to_string()),
    }
}

/// Validate an optional wire type string up front — a bad type is a clean 400
/// before anything touches the service (D-07 pattern).
fn parse_type_filter(wire: Option<&str>) -> Result<Option<MemoryType>, ApiError> {
    match wire {
        Some(t) => MemoryType::try_from(t)
            .map(Some)
            .map_err(|e| ApiError::BadRequest(e.to_string())),
        None => Ok(None),
    }
}

/// `POST /api/memories` → 201 `{"id": N}`.
pub async fn store_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StoreRequest>,
) -> Result<Response, ApiError> {
    let mem_type = MemoryType::try_from(req.r#type.as_str())
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let new = NewMemory {
        content: req.content,
        mem_type,
        tags: req.tags,
        source: req.source,
        scope: req.scope,
        ttl_secs: req.ttl_secs,
    };

    let id = state.service.store(new).await.map_err(map_memory_error)?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response())
}

/// Map an axum `Path`/`Query` extractor rejection to [`ApiError`], so a
/// malformed path segment or query parameter gets the same `{"error": ...}`
/// body as every other client error instead of axum's plain-text default
/// (REVIEW IN-02). The extractor's status tier is preserved: a parse failure
/// (4xx) is a 400, while a 5xx — an axum routing misconfiguration such as a
/// path-parameter count mismatch — stays a 500.
fn extractor_rejection(status: StatusCode, message: String) -> ApiError {
    if status.is_server_error() {
        ApiError::Internal(message)
    } else {
        ApiError::BadRequest(message)
    }
}

/// `GET /api/memories?type=&tag=&scope=&limit=&expand_links=` → 200 `Vec<MemoryView>`.
///
/// A malformed query string (`limit=abc`, `expand_links=1`) is a 400 with the
/// `{"error"}` body via [`extractor_rejection`].
pub async fn list_handler(
    State(state): State<Arc<AppState>>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(q) = query.map_err(|r| extractor_rejection(r.status(), r.body_text()))?;
    let mem_type = parse_type_filter(q.r#type.as_deref())?;

    let views = state
        .service
        .list(ServiceListArgs {
            mem_type,
            tag: q.tag,
            scope: q.scope,
            limit: q.limit,
            expand_links: q.expand_links,
        })
        .await
        .map_err(map_memory_error)?;

    Ok(Json(views).into_response())
}

/// `POST /api/search` → 200 [`SearchOutcome`] — the shared `{search_mode,
/// results}` envelope from plan 02-01, serialized as-is (no re-shaping), so
/// REST and MCP consumers see the identical wire form.
pub async fn search_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SearchRequest>,
) -> Result<Response, ApiError> {
    let mem_type = parse_type_filter(req.r#type.as_deref())?;

    let outcome: SearchOutcome = state
        .service
        .search(ServiceSearchArgs {
            query: req.query,
            mem_type,
            tag: req.tag,
            scope: req.scope,
            limit: req.limit,
            expand_links: req.expand_links,
        })
        .await
        .map_err(map_memory_error)?;

    Ok(Json(outcome).into_response())
}

/// `DELETE /api/memories/{id}` → 200 `{"id", "deleted": true}` or 404
/// `{"id", "deleted": false, "reason": "not_found"}` — the exact JSON bodies
/// the `mcp.rs` forget tool produces, with HTTP-native status codes. A
/// non-numeric id segment is a 400 `{"error"}` via [`extractor_rejection`].
pub async fn forget_handler(
    State(state): State<Arc<AppState>>,
    path: Result<Path<i64>, PathRejection>,
) -> Result<Response, ApiError> {
    let Path(id) = path.map_err(|r| extractor_rejection(r.status(), r.body_text()))?;
    let deleted = state.service.forget(id).await.map_err(map_memory_error)?;

    if deleted {
        Ok(Json(serde_json::json!({ "id": id, "deleted": true })).into_response())
    } else {
        Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "id": id, "deleted": false, "reason": "not_found" })),
        )
            .into_response())
    }
}

/// `PATCH /api/memories/{id}` → 200 [`UpdatedMemory`] (the full post-update
/// memory plus `embedding_status`), or 404 `{"id", "updated": false,
/// "reason": "not_found"}` — the exact JSON the `mcp.rs` update tool returns,
/// with HTTP-native status codes.
///
/// The body extractor's rejection is taken explicitly and reported as a 400
/// with the usual `{"error": ...}` body: a malformed or wrongly-typed patch is
/// client input, whatever axum's default status for it would be. A
/// non-numeric id segment gets the same body via [`extractor_rejection`]. Invalid
/// patch VALUES (null/empty content, out-of-bounds TTL, an empty patch) are
/// rejected by the service seam and map to 400 through [`map_memory_error`].
///
/// [`UpdatedMemory`]: agent_memory_core::domain::UpdatedMemory
pub async fn update_handler(
    State(state): State<Arc<AppState>>,
    path: Result<Path<i64>, PathRejection>,
    body: Result<Json<UpdateBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Path(id) = path.map_err(|r| extractor_rejection(r.status(), r.body_text()))?;
    let Json(body) = body.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;

    let updated = state
        .service
        .update(id, MemoryPatch::from(body))
        .await
        .map_err(map_memory_error)?;

    match updated {
        Some(updated) => Ok(Json(updated).into_response()),
        None => Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "id": id, "updated": false, "reason": "not_found" })),
        )
            .into_response()),
    }
}

/// Parse a wire `kind` up front: an unknown kind is a clean 400 naming the
/// allowed set, before anything touches the service (the D-07 type pattern).
fn parse_link_kind(wire: &str) -> Result<LinkKind, ApiError> {
    LinkKind::try_from(wire).map_err(map_memory_error)
}

/// `POST /api/links` → 200 `{"from_id", "to_id", "kind", "linked": true}`, or
/// 404 `{..., "linked": false, "reason": "not_found", "missing_id"}` when
/// either memory does not exist — the exact JSON the `mcp.rs` link tool
/// returns, with HTTP-native status codes. A duplicate link is a 200 no-op.
///
/// Every body rejection (malformed JSON, wrong types, missing content-type)
/// is a 400 `{"error"}`, like PATCH; a self-link or unknown kind is a 400
/// from the seam. A missing endpoint is never an FK-violation 500.
pub async fn link_handler(
    State(state): State<Arc<AppState>>,
    body: Result<Json<LinkBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(body) = body.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    let kind = parse_link_kind(&body.kind)?;

    let outcome = state
        .service
        .link(body.from_id, body.to_id, kind)
        .await
        .map_err(map_memory_error)?;

    match outcome {
        LinkOutcome::Linked => Ok(Json(serde_json::json!({
            "from_id": body.from_id,
            "to_id": body.to_id,
            "kind": kind,
            "linked": true
        }))
        .into_response()),
        LinkOutcome::NotFound { missing_id } => Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "from_id": body.from_id,
                "to_id": body.to_id,
                "kind": kind,
                "linked": false,
                "reason": "not_found",
                "missing_id": missing_id
            })),
        )
            .into_response()),
    }
}

/// `DELETE /api/links/{from_id}/{to_id}/{kind}` → 200 `{"from_id", "to_id",
/// "kind", "unlinked": true}`, or 404 `{..., "unlinked": false, "reason":
/// "not_found"}` when no such edge exists — the `mcp.rs` unlink tool's JSON.
/// An unknown `kind` segment is a 400; a non-numeric id segment is a 400
/// `{"error"}` via [`extractor_rejection`].
pub async fn unlink_handler(
    State(state): State<Arc<AppState>>,
    path: Result<Path<(i64, i64, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    let Path((from_id, to_id, kind)) =
        path.map_err(|r| extractor_rejection(r.status(), r.body_text()))?;
    let kind = parse_link_kind(&kind)?;
    let removed = state
        .service
        .unlink(from_id, to_id, kind)
        .await
        .map_err(map_memory_error)?;

    if removed {
        Ok(Json(serde_json::json!({
            "from_id": from_id,
            "to_id": to_id,
            "kind": kind,
            "unlinked": true
        }))
        .into_response())
    } else {
        Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "from_id": from_id,
                "to_id": to_id,
                "kind": kind,
                "unlinked": false,
                "reason": "not_found"
            })),
        )
            .into_response())
    }
}

/// `GET /health` → 200 `{"status": "ok", "embedder": ...}`.
///
/// The store ping is a cheap one-row list through the service (no SQL here);
/// embedder status comes from the `Arc<dyn Embedder>` carried in [`AppState`].
pub async fn health_handler(State(state): State<Arc<AppState>>) -> Result<Response, ApiError> {
    state
        .service
        .list(ServiceListArgs {
            limit: Some(1),
            ..Default::default()
        })
        .await
        .map_err(map_memory_error)?;

    let embedder = match state.embedder.health().await {
        Ok(EmbedderHealth::Ready) => "ready",
        Ok(EmbedderHealth::ModelMissing) => "model_missing",
        Ok(EmbedderHealth::Unreachable(_)) | Err(_) => "unreachable",
    };

    Ok(Json(serde_json::json!({ "status": "ok", "embedder": embedder })).into_response())
}

/// In-process handler tests: call each handler function directly with hand-built
/// extractors against a real tempdir SQLite store. The end-to-end HTTP surface
/// is covered separately by `tests/rest.rs` against the spawned binary — but a
/// SIGKILL'd child never flushes LLVM profile data, so THESE tests are what put
/// the handlers under the CI coverage gate.
///
/// The embedder points at a dead loopback port, so stores succeed with pending
/// embeddings and search deterministically serves the FTS5 keyword path
/// (SEARCH-03) — no network, no Ollama, no test flakiness.
#[cfg(test)]
mod tests {
    use super::*;

    use agent_memory_core::clock::SystemClock;
    use agent_memory_core::decay::DecayConfig;
    use agent_memory_core::embed::ollama::OllamaClient;
    use agent_memory_core::embed::Embedder;
    use agent_memory_core::service::MemoryService;
    use agent_memory_core::store::sqlite::SqliteStore;

    /// A loopback port nothing listens on: instant connection-refused, never a
    /// slow timeout — keyword fallback is exercised deterministically.
    const DEAD_OLLAMA: &str = "http://127.0.0.1:9";

    fn test_state(dir: &tempfile::TempDir) -> Arc<AppState> {
        state_with(dir, Arc::new(OllamaClient::new(DEAD_OLLAMA)))
    }

    /// A state whose embedder is UP (hash-seeded FakeEmbedder vectors), so
    /// search takes the hybrid path (SEARCH-04).
    fn live_state(dir: &tempfile::TempDir) -> Arc<AppState> {
        state_with(
            dir,
            Arc::new(agent_memory_core::embed::FakeEmbedder::with_vectors(
                std::collections::HashMap::new(),
            )),
        )
    }

    fn state_with(dir: &tempfile::TempDir, embedder: Arc<dyn Embedder>) -> Arc<AppState> {
        let store = SqliteStore::open(&dir.path().join("mem.db")).expect("store opens");
        let service = MemoryService::new(
            Arc::new(store),
            Arc::new(SystemClock),
            embedder.clone(),
            DecayConfig::default(),
        );
        Arc::new(AppState { service, embedder })
    }

    fn store_request(content: &str, mem_type: &str) -> StoreRequest {
        StoreRequest {
            content: content.to_string(),
            r#type: mem_type.to_string(),
            tags: vec!["rest".to_string()],
            source: Some("handler-test".to_string()),
            scope: Some("unit".to_string()),
            ttl_secs: None,
        }
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body reads");
        serde_json::from_slice(&bytes).expect("body is JSON")
    }

    #[tokio::test]
    async fn store_returns_201_with_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        let resp = store_handler(
            State(state),
            Json(store_request(
                "maven group is io.github.unityinflow",
                "DECISION",
            )),
        )
        .await
        .expect("store succeeds")
        .into_response();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = body_json(resp).await;
        assert!(body["id"].as_i64().expect("id is a number") > 0);
    }

    #[tokio::test]
    async fn store_rejects_invalid_type_with_400() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        let err = store_handler(State(state), Json(store_request("x", "NOT_A_TYPE")))
            .await
            .expect_err("invalid type is rejected");

        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_json(resp).await;
        assert!(body["error"]
            .as_str()
            .expect("error message")
            .contains("NOT_A_TYPE"));
    }

    #[tokio::test]
    async fn list_filters_by_type_and_rejects_bad_type() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        store_handler(
            State(state.clone()),
            Json(store_request("todo item", "TODO")),
        )
        .await
        .expect("store succeeds");
        store_handler(
            State(state.clone()),
            Json(store_request("a decision", "DECISION")),
        )
        .await
        .expect("store succeeds");

        let resp = list_handler(
            State(state.clone()),
            Ok(Query(ListQuery {
                r#type: Some("TODO".to_string()),
                tag: None,
                scope: None,
                limit: None,
                expand_links: false,
            })),
        )
        .await
        .expect("list succeeds")
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        let rows = body.as_array().expect("list is an array");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["content"], "todo item");

        let err = list_handler(
            State(state),
            Ok(Query(ListQuery {
                r#type: Some("bogus".to_string()),
                tag: None,
                scope: None,
                limit: None,
                expand_links: false,
            })),
        )
        .await
        .expect_err("bad type filter is rejected");
        assert_eq!(err.into_response().status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn search_returns_keyword_envelope_with_dead_embedder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        store_handler(
            State(state.clone()),
            Json(store_request(
                "row level security controls database access",
                "PATTERN",
            )),
        )
        .await
        .expect("store succeeds");

        let resp = search_handler(
            State(state),
            Json(SearchRequest {
                query: "database".to_string(),
                r#type: None,
                tag: None,
                scope: None,
                limit: Some(10),
                expand_links: false,
            }),
        )
        .await
        .expect("search succeeds")
        .into_response();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        // Dead Ollama → the service degrades to the FTS5 keyword path and says so.
        assert_eq!(body["search_mode"], "keyword");
        let results = body["results"].as_array().expect("results array");
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn search_with_a_lone_quote_returns_200_with_an_empty_keyword_answer() {
        // User decision option-a reverses the Gap 2 / WR-05 400: a lone
        // double-quote has no word tokens, so the keyword path has nothing to
        // match and answers 200 with an empty result set — never a 400 or 500.
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        store_handler(
            State(state.clone()),
            Json(store_request("any searchable row", "PATTERN")),
        )
        .await
        .expect("store succeeds");

        let resp = search_handler(
            State(state),
            Json(SearchRequest {
                query: "\"".to_string(),
                r#type: None,
                tag: None,
                scope: None,
                limit: None,
                expand_links: false,
            }),
        )
        .await
        .expect("a lone quote is answered, never rejected")
        .into_response();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["search_mode"], "keyword", "{body}");
        assert_eq!(
            body["results"].as_array().map(Vec::len),
            Some(0),
            "a tokenless query has no keyword matches: {body}"
        );
    }

    fn search_request(query: &str) -> SearchRequest {
        SearchRequest {
            query: query.to_string(),
            r#type: None,
            tag: None,
            scope: None,
            limit: Some(10),
            expand_links: false,
        }
    }

    #[tokio::test]
    async fn search_reports_hybrid_with_a_live_embedder_and_keyword_with_a_dead_one() {
        // SEARCH-04 wire contract: embedder up -> "hybrid", embedder down ->
        // "keyword"; the envelope keys are the same in both modes.
        for (expected_mode, live) in [("hybrid", true), ("keyword", false)] {
            let dir = tempfile::tempdir().expect("tempdir");
            let state = if live {
                live_state(&dir)
            } else {
                test_state(&dir)
            };
            store_handler(
                State(state.clone()),
                Json(store_request(
                    "row level security controls database access",
                    "PATTERN",
                )),
            )
            .await
            .expect("store succeeds");

            let resp = search_handler(State(state), Json(search_request("database")))
                .await
                .expect("search succeeds")
                .into_response();
            assert_eq!(resp.status(), StatusCode::OK);
            let body = body_json(resp).await;
            assert_eq!(body["search_mode"], expected_mode, "{body}");
            let mut keys: Vec<&str> = body
                .as_object()
                .expect("envelope is an object")
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(keys, vec!["results", "search_mode"], "{body}");
            assert_eq!(body["results"].as_array().map(Vec::len), Some(1), "{body}");
        }
    }

    #[tokio::test]
    async fn queries_fts5_cannot_parse_are_200_in_hybrid_mode_too() {
        // T-04-15 revised (option-a): with the embedder UP, a query FTS5
        // cannot parse is answered exactly as in keyword mode — 200 with the
        // hybrid envelope, never a 400, a 500, or SQLite text in the body.
        let dir = tempfile::tempdir().expect("tempdir");
        let state = live_state(&dir);
        store_handler(
            State(state.clone()),
            Json(store_request("any searchable row", "PATTERN")),
        )
        .await
        .expect("store succeeds");

        for input in ["\"", "what is the plan?", "sqlite-vec"] {
            let resp = search_handler(State(state.clone()), Json(search_request(input)))
                .await
                .unwrap_or_else(|_| panic!("{input:?} must be answered in hybrid mode"))
                .into_response();
            assert_eq!(resp.status(), StatusCode::OK, "{input:?}");
            let body = body_json(resp).await;
            assert_eq!(body["search_mode"], "hybrid", "{input:?}: {body}");
            assert!(
                !body.to_string().contains("sqlite error"),
                "{input:?}: {body}"
            );
        }
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
    async fn search_answers_every_query_shape_in_both_modes() {
        for (expected_mode, live) in [("hybrid", true), ("keyword", false)] {
            let dir = tempfile::tempdir().expect("tempdir");
            let state = if live {
                live_state(&dir)
            } else {
                test_state(&dir)
            };
            let mut ids = Vec::new();
            for content in CORPUS {
                let resp = store_handler(
                    State(state.clone()),
                    Json(store_request(content, "PATTERN")),
                )
                .await
                .expect("store succeeds")
                .into_response();
                ids.push(body_json(resp).await["id"].as_i64().expect("id"));
            }

            let queries = TARGETED
                .iter()
                .map(|(q, target)| (*q, Some(*target)))
                .chain(TOKENLESS_OR_NO_HIT.iter().map(|q| (*q, None)));
            for (input, target) in queries {
                let resp = search_handler(State(state.clone()), Json(search_request(input)))
                    .await
                    .unwrap_or_else(|_| panic!("{expected_mode}: {input:?} must be answered"))
                    .into_response();
                assert_eq!(resp.status(), StatusCode::OK, "{expected_mode}: {input:?}");
                let body = body_json(resp).await;
                assert!(
                    !body.to_string().contains("sqlite error"),
                    "{input:?}: {body}"
                );
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
    async fn search_storage_failure_is_500_without_sqlite_text() {
        // T-04-19: a broken FTS index (renamed away underneath the server)
        // reaches the REST client only as the generic internal error.
        for live in [true, false] {
            let dir = tempfile::tempdir().expect("tempdir");
            let state = if live {
                live_state(&dir)
            } else {
                test_state(&dir)
            };
            stored_id(&state, "ledger reconciliation runbook").await;
            agent_memory_core::store::sqlite::register_vec_extension().expect("register vec0");
            rusqlite::Connection::open(dir.path().join("mem.db"))
                .expect("open db")
                .execute_batch("ALTER TABLE memories_fts RENAME TO memories_fts_offline;")
                .expect("rename fts table");

            let resp = match search_handler(State(state), Json(search_request("ledger"))).await {
                Ok(_) => panic!("live={live}: a broken FTS index must fail the search"),
                Err(err) => err.into_response(),
            };
            assert_eq!(
                resp.status(),
                StatusCode::INTERNAL_SERVER_ERROR,
                "live={live}"
            );
            let body = body_json(resp).await;
            let error = body["error"].as_str().expect("500 body carries an error");
            assert!(
                error.contains("search failed due to a storage error"),
                "live={live}: {body}"
            );
            assert!(
                !error.to_lowercase().contains("sqlite"),
                "live={live}: no SQLite text may reach the client: {body}"
            );
        }
    }

    #[tokio::test]
    async fn search_with_zero_limit_returns_400_never_500() {
        // API-02 / D-01: an out-of-bounds limit is CLIENT input — 400 with the
        // range in the body, never a 500 and never a silent clamp.
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        let err = search_handler(
            State(state),
            Json(SearchRequest {
                query: "anything".to_string(),
                r#type: None,
                tag: None,
                scope: None,
                limit: Some(0),
                expand_links: false,
            }),
        )
        .await
        .expect_err("limit=0 is rejected");

        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_json(resp).await;
        assert!(
            body["error"]
                .as_str()
                .expect("400 body carries a string error field")
                .contains("limit must be between"),
            "the error must carry the valid range, got: {body}"
        );
    }

    #[tokio::test]
    async fn store_with_out_of_bounds_ttl_returns_400_never_500() {
        // API-02 / D-02: a negative TTL must reject before any row lands.
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        let mut req = store_request("never lands", "TODO");
        req.ttl_secs = Some(-1);
        let err = store_handler(State(state), Json(req))
            .await
            .expect_err("ttl_secs=-1 is rejected");

        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_json(resp).await;
        assert!(
            body["error"]
                .as_str()
                .expect("400 body carries a string error field")
                .contains("ttl_secs must be between"),
            "the error must carry the valid range, got: {body}"
        );
    }

    #[tokio::test]
    async fn list_with_negative_limit_returns_400_never_500() {
        // API-02 / D-01: negative list limit previously bound as SQL LIMIT -5
        // (= unlimited) — it must now reject as client input.
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        let err = list_handler(
            State(state),
            Ok(Query(ListQuery {
                r#type: None,
                tag: None,
                scope: None,
                limit: Some(-5),
                expand_links: false,
            })),
        )
        .await
        .expect_err("limit=-5 is rejected");

        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_json(resp).await;
        assert!(
            body["error"]
                .as_str()
                .expect("400 body carries a string error field")
                .contains("limit must be between"),
            "the error must carry the valid range, got: {body}"
        );
    }

    #[tokio::test]
    async fn forget_deletes_then_404s_with_not_found_body() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        let created = store_handler(State(state.clone()), Json(store_request("bye", "ERROR")))
            .await
            .expect("store succeeds")
            .into_response();
        let id = body_json(created).await["id"].as_i64().expect("id");

        let resp = forget_handler(State(state.clone()), Ok(Path(id)))
            .await
            .expect("forget succeeds")
            .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["deleted"], true);

        // Second delete of the same id: 404 with the exact mcp.rs not-found body.
        let resp = forget_handler(State(state), Ok(Path(id)))
            .await
            .expect("forget of a missing id is a clean handler result")
            .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = body_json(resp).await;
        assert_eq!(body["deleted"], false);
        assert_eq!(body["reason"], "not_found");
    }

    #[tokio::test]
    async fn health_reports_ok_with_unreachable_embedder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        let resp = health_handler(State(state))
            .await
            .expect("health succeeds")
            .into_response();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["embedder"], "unreachable");
    }

    // ---- PATCH /api/memories/{id} (MCP-06 / D-08) -------------------------
    //
    // Every patch below is a RAW JSON string pushed through axum's own `Json`
    // extractor (`FromRequest`), so the explicit-null deserializer and the
    // rejection mapping are under test — not a hand-built Rust struct.

    use agent_memory_core::clock::Clock;
    use agent_memory_core::domain::MAX_TTL_SECS;
    use axum::extract::FromRequest;

    async fn patch_raw(state: &Arc<AppState>, id: i64, raw: &str) -> Response {
        patch_request(state, id, raw, Some("application/json")).await
    }

    async fn patch_request(
        state: &Arc<AppState>,
        id: i64,
        raw: &str,
        content_type: Option<&str>,
    ) -> Response {
        let mut builder = axum::http::Request::builder()
            .method("PATCH")
            .uri(format!("/api/memories/{id}"));
        if let Some(ct) = content_type {
            builder = builder.header(axum::http::header::CONTENT_TYPE, ct);
        }
        let req = builder
            .body(axum::body::Body::from(raw.to_string()))
            .expect("request builds");
        let body = Json::<UpdateBody>::from_request(req, &()).await;
        match update_handler(State(state.clone()), Ok(Path(id)), body).await {
            Ok(resp) => resp,
            Err(err) => err.into_response(),
        }
    }

    /// Store a memory with tags `["rest"]`, scope `"unit"` and the given TTL.
    async fn store_with_ttl(state: &Arc<AppState>, ttl_secs: Option<i64>) -> i64 {
        let mut req = store_request("patch target", "PATTERN");
        req.ttl_secs = ttl_secs;
        let resp = store_handler(State(state.clone()), Json(req))
            .await
            .expect("store succeeds")
            .into_response();
        body_json(resp).await["id"].as_i64().expect("id")
    }

    /// Run a sweep at `at` and report whether `id` survived it.
    async fn survives_sweep_at(state: &Arc<AppState>, id: i64, at: i64) -> bool {
        state.service.sweep(at).await.expect("sweep runs");
        state
            .service
            .list(ServiceListArgs::default())
            .await
            .expect("list")
            .iter()
            .any(|v| v.id == id)
    }

    async fn ok_body(resp: Response) -> serde_json::Value {
        assert_eq!(resp.status(), StatusCode::OK, "expected 200");
        body_json(resp).await
    }

    async fn assert_400_containing(resp: Response, needle: &str) {
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "expected 400 (never 500) for a patch that should contain {needle:?}"
        );
        let body = body_json(resp).await;
        let message = body["error"]
            .as_str()
            .expect("400 body has an error string");
        assert!(
            message.contains(needle),
            "error {message:?} should contain {needle:?}"
        );
    }

    #[tokio::test]
    async fn patch_tags_matrix_absent_null_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        // absent → unchanged
        let id = store_with_ttl(&state, None).await;
        let body = ok_body(patch_raw(&state, id, r#"{"scope": "elsewhere"}"#).await).await;
        assert_eq!(body["tags"], serde_json::json!(["rest"]));
        assert_eq!(
            body["content"], "patch target",
            "absent content is unchanged"
        );

        // null → cleared to an empty list
        let id = store_with_ttl(&state, None).await;
        let body = ok_body(patch_raw(&state, id, r#"{"tags": null}"#).await).await;
        assert_eq!(body["tags"], serde_json::json!([]));
        assert_eq!(body["scope"], "unit", "an unrelated field is unchanged");

        // value → set
        let id = store_with_ttl(&state, None).await;
        let body = ok_body(patch_raw(&state, id, r#"{"tags": ["x", "y"]}"#).await).await;
        assert_eq!(body["tags"], serde_json::json!(["x", "y"]));
    }

    #[tokio::test]
    async fn patch_scope_matrix_absent_null_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        let id = store_with_ttl(&state, None).await;
        let body = ok_body(patch_raw(&state, id, r#"{"tags": ["t"]}"#).await).await;
        assert_eq!(body["scope"], "unit", "absent scope is unchanged");

        let id = store_with_ttl(&state, None).await;
        let body = ok_body(patch_raw(&state, id, r#"{"scope": null}"#).await).await;
        assert_eq!(
            body["scope"],
            serde_json::Value::Null,
            "null resets to global"
        );

        let id = store_with_ttl(&state, None).await;
        let body = ok_body(patch_raw(&state, id, r#"{"scope": "other"}"#).await).await;
        assert_eq!(body["scope"], "other");
    }

    #[tokio::test]
    async fn patch_ttl_matrix_absent_null_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let now = SystemClock.now();
        let past_old_ttl = now + 3_600; // well past the stored 60 s TTL

        // absent → the 60 s expiry still applies
        let id = store_with_ttl(&state, Some(60)).await;
        ok_body(patch_raw(&state, id, r#"{"tags": ["keep-ttl"]}"#).await).await;
        assert!(
            !survives_sweep_at(&state, id, past_old_ttl).await,
            "an absent ttl_secs leaves the expiry in place"
        );

        // null → no expiry: survives a sweep far past the old TTL
        let id = store_with_ttl(&state, Some(60)).await;
        ok_body(patch_raw(&state, id, r#"{"ttl_secs": null}"#).await).await;
        assert!(
            survives_sweep_at(&state, id, past_old_ttl).await,
            "ttl_secs: null removes the expiry"
        );

        // value → a new TTL counted from patch time
        let id = store_with_ttl(&state, Some(60)).await;
        ok_body(patch_raw(&state, id, r#"{"ttl_secs": 86400}"#).await).await;
        assert!(
            survives_sweep_at(&state, id, past_old_ttl).await,
            "the new 1-day TTL replaced the 60 s one"
        );
        assert!(
            !survives_sweep_at(&state, id, now + 2 * 86_400).await,
            "the new TTL itself still expires"
        );
    }

    #[tokio::test]
    async fn patch_content_null_or_empty_is_400_and_a_content_update_is_200() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let id = store_with_ttl(&state, None).await;

        for raw in [
            r#"{"content": null}"#,
            r#"{"content": ""}"#,
            r#"{"content": "   "}"#,
        ] {
            assert_400_containing(
                patch_raw(&state, id, raw).await,
                "content must be a non-empty string",
            )
            .await;
        }

        let body = ok_body(patch_raw(&state, id, r#"{"content": "rewritten"}"#).await).await;
        assert_eq!(body["id"], id);
        assert_eq!(body["content"], "rewritten");
        assert_eq!(body["type"], "PATTERN");
        // Dead embedder → the re-embed is pending, and the response says so.
        assert_eq!(body["embedding_status"], 0);
    }

    #[tokio::test]
    async fn patch_out_of_bounds_ttl_is_400_never_500_never_clamped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let id = store_with_ttl(&state, None).await;

        let too_big = MAX_TTL_SECS + 1;
        for raw in [
            r#"{"ttl_secs": 0}"#.to_string(),
            r#"{"ttl_secs": -1}"#.to_string(),
            format!(r#"{{"ttl_secs": {too_big}}}"#),
        ] {
            assert_400_containing(
                patch_raw(&state, id, &raw).await,
                "ttl_secs must be between",
            )
            .await;
        }
    }

    #[tokio::test]
    async fn patch_of_an_unknown_id_is_404_with_the_not_found_body() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);

        let resp = patch_raw(&state, 777, r#"{"content": "nobody home"}"#).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body_json(resp).await,
            serde_json::json!({ "id": 777, "updated": false, "reason": "not_found" })
        );
    }

    #[tokio::test]
    async fn malformed_or_empty_patches_are_400_never_500() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let id = store_with_ttl(&state, None).await;

        // An empty patch is a client error, not a silent no-op.
        assert_400_containing(patch_raw(&state, id, "{}").await, "at least one of").await;

        // Extractor rejections — syntax, type, missing content type — are all
        // reported as 400 with the standard error body.
        for (raw, content_type) in [
            (r#"{"tags": ["#, Some("application/json")),
            (r#"{"ttl_secs": "soon"}"#, Some("application/json")),
            (r#"{"tags": "not-a-list"}"#, Some("application/json")),
            (r#"{"tags": ["x"]}"#, None),
        ] {
            let resp = patch_request(&state, id, raw, content_type).await;
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "patch {raw:?} (content-type {content_type:?}) must be a 400"
            );
            let body = body_json(resp).await;
            assert!(
                !body["error"].as_str().expect("error string").is_empty(),
                "the 400 carries a message: {body}"
            );
        }

        // None of the rejected patches touched the row.
        let rows = state
            .service
            .list(ServiceListArgs::default())
            .await
            .expect("list");
        assert_eq!(rows[0].tags, vec!["rest".to_string()]);
    }

    #[tokio::test]
    async fn patch_with_an_unknown_or_immutable_field_is_400_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let id = store_with_ttl(&state, None).await;

        for (raw, field) in [
            (r#"{"content": "x", "type": "DECISION"}"#, "type"),
            (r#"{"content": "x", "source": "elsewhere"}"#, "source"),
            (r#"{"ttl_sec": 60}"#, "ttl_sec"),
        ] {
            let resp = patch_raw(&state, id, raw).await;
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "{raw} must be a 400"
            );
            let body = body_json(resp).await;
            let message = body["error"]
                .as_str()
                .expect("400 body has an error string");
            assert!(
                message.contains("unknown field") && message.contains(field),
                "{raw}: error {message:?} should name the unknown field"
            );
        }
        assert_400_containing(
            patch_raw(&state, id, r#"{"content": "x", "type": "DECISION"}"#).await,
            "unknown field",
        )
        .await;

        // Nothing was written: content, type, tags and scope are as stored.
        let rows = state
            .service
            .list(ServiceListArgs::default())
            .await
            .expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].content, "patch target");
        assert_eq!(rows[0].mem_type, MemoryType::Pattern);
        assert_eq!(rows[0].tags, vec!["rest".to_string()]);
        assert_eq!(rows[0].scope.as_deref(), Some("unit"));
    }

    // --- MCP-07 link / unlink -------------------------------------------------

    async fn link_request(
        state: &Arc<AppState>,
        raw: &str,
        content_type: Option<&str>,
    ) -> Response {
        let mut builder = axum::http::Request::builder()
            .method("POST")
            .uri("/api/links");
        if let Some(ct) = content_type {
            builder = builder.header(axum::http::header::CONTENT_TYPE, ct);
        }
        let req = builder
            .body(axum::body::Body::from(raw.to_string()))
            .expect("request builds");
        let body = Json::<LinkBody>::from_request(req, &()).await;
        match link_handler(State(state.clone()), body).await {
            Ok(resp) => resp,
            Err(err) => err.into_response(),
        }
    }

    async fn link_raw(state: &Arc<AppState>, raw: &str) -> Response {
        link_request(state, raw, Some("application/json")).await
    }

    async fn unlink_path(state: &Arc<AppState>, from: i64, to: i64, kind: &str) -> Response {
        match unlink_handler(State(state.clone()), Ok(Path((from, to, kind.to_string())))).await {
            Ok(resp) => resp,
            Err(err) => err.into_response(),
        }
    }

    async fn stored_id(state: &Arc<AppState>, content: &str) -> i64 {
        let resp = store_handler(
            State(state.clone()),
            Json(store_request(content, "DECISION")),
        )
        .await
        .expect("store succeeds")
        .into_response();
        body_json(resp).await["id"].as_i64().expect("id")
    }

    #[tokio::test]
    async fn post_links_links_idempotently_and_delete_unlinks_then_404s() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let a = stored_id(&state, "effect").await;
        let b = stored_id(&state, "cause").await;
        let raw = format!(r#"{{"from_id": {a}, "to_id": {b}, "kind": "caused_by"}}"#);
        let linked = serde_json::json!({
            "from_id": a, "to_id": b, "kind": "caused_by", "linked": true
        });

        for attempt in ["first", "duplicate"] {
            let resp = link_raw(&state, &raw).await;
            assert_eq!(resp.status(), StatusCode::OK, "{attempt} link is a 200");
            assert_eq!(body_json(resp).await, linked, "{attempt}");
        }

        let resp = unlink_path(&state, a, b, "caused_by").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            body_json(resp).await,
            serde_json::json!({
                "from_id": a, "to_id": b, "kind": "caused_by", "unlinked": true
            })
        );

        let resp = unlink_path(&state, a, b, "caused_by").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body_json(resp).await,
            serde_json::json!({
                "from_id": a, "to_id": b, "kind": "caused_by",
                "unlinked": false, "reason": "not_found"
            })
        );
    }

    #[tokio::test]
    async fn relates_to_link_and_unlink_bodies_echo_the_request_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let a = stored_id(&state, "first see-also").await;
        let b = stored_id(&state, "second see-also").await;
        assert!(b > a, "b sorts after a, so canonical storage is (a, b)");

        let resp = link_raw(
            &state,
            &format!(r#"{{"from_id": {b}, "to_id": {a}, "kind": "relates_to"}}"#),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            body_json(resp).await,
            serde_json::json!({
                "from_id": b, "to_id": a, "kind": "relates_to", "linked": true
            })
        );

        let resp = unlink_path(&state, b, a, "relates_to").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            body_json(resp).await,
            serde_json::json!({
                "from_id": b, "to_id": a, "kind": "relates_to", "unlinked": true
            })
        );
    }

    #[tokio::test]
    async fn post_links_to_a_missing_memory_is_404_naming_it_never_500() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let a = stored_id(&state, "present").await;

        let resp = link_raw(
            &state,
            &format!(r#"{{"from_id": 999999, "to_id": {a}, "kind": "relates_to"}}"#),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body_json(resp).await,
            serde_json::json!({
                "from_id": 999_999, "to_id": a, "kind": "relates_to",
                "linked": false, "reason": "not_found", "missing_id": 999_999
            })
        );
    }

    #[tokio::test]
    async fn bad_link_requests_are_400_never_500() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let a = stored_id(&state, "solo").await;

        let resp = link_raw(
            &state,
            &format!(r#"{{"from_id": {a}, "to_id": {a}, "kind": "supersedes"}}"#),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body_json(resp).await["error"],
            format!("invalid argument: from_id and to_id must differ (got {a})")
        );

        let kind_error =
            "invalid argument: kind must be one of relates_to, supersedes, caused_by (got 'blocks')";
        let resp = link_raw(
            &state,
            &format!(r#"{{"from_id": {a}, "to_id": 2, "kind": "blocks"}}"#),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(resp).await["error"], kind_error);

        let resp = unlink_path(&state, a, 2, "blocks").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_json(resp).await["error"], kind_error);

        for (raw, content_type) in [
            (r#"{"from_id": 1, "to_id": 2"#, Some("application/json")),
            (
                r#"{"from_id": "one", "to_id": 2, "kind": "relates_to"}"#,
                Some("application/json"),
            ),
            (
                r#"{"from_id": 1, "kind": "relates_to"}"#,
                Some("application/json"),
            ),
            (r#"{"from_id": 1, "to_id": 2, "kind": "relates_to"}"#, None),
        ] {
            let resp = link_request(&state, raw, content_type).await;
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "link body {raw:?} (content-type {content_type:?}) must be a 400"
            );
        }
    }

    #[tokio::test]
    async fn expand_links_from_raw_query_and_body_on_list_and_search() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let a = stored_id(&state, "pulsar effect").await;
        let b = stored_id(&state, "pulsar cause").await;
        let resp = link_raw(
            &state,
            &format!(r#"{{"from_id": {a}, "to_id": {b}, "kind": "caused_by"}}"#),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let list_raw = |raw_query: &'static str| {
            let state = state.clone();
            async move {
                let uri: axum::http::Uri = format!("/api/memories{raw_query}")
                    .parse()
                    .expect("uri parses");
                let query = Query::<ListQuery>::try_from_uri(&uri).expect("query parses");
                list_handler(State(state), Ok(query))
                    .await
                    .expect("list succeeds")
                    .into_response()
            }
        };

        // GET /api/memories?expand_links=true
        let rows = body_json(list_raw("?expand_links=true").await).await;
        let rows = rows.as_array().expect("array");
        let effect = rows.iter().find(|r| r["id"] == a).expect("effect row");
        assert_eq!(effect["related"][0]["id"], b);
        assert_eq!(effect["related"][0]["kind"], "caused_by");
        assert_eq!(effect["related"][0]["direction"], "out");
        let cause = rows.iter().find(|r| r["id"] == b).expect("cause row");
        assert_eq!(cause["related"][0]["kind"], "caused");
        assert_eq!(cause["related"][0]["direction"], "in");

        // Absent or false: the v1.0 rows, no related member.
        for q in ["", "?expand_links=false"] {
            let rows = body_json(list_raw(q).await).await;
            assert!(
                rows.as_array()
                    .expect("array")
                    .iter()
                    .all(|r| r.get("related").is_none()),
                "{q:?}"
            );
        }

        // A non-boolean query value is rejected by the typed extractor.
        let uri: axum::http::Uri = "/api/memories?expand_links=yes".parse().expect("uri");
        assert!(Query::<ListQuery>::try_from_uri(&uri).is_err());

        // POST /api/search with a raw JSON body.
        let search_raw = |raw: &'static str| {
            let state = state.clone();
            async move {
                let req = axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/search")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(raw))
                    .expect("request builds");
                let Json(body) = Json::<SearchRequest>::from_request(req, &())
                    .await
                    .expect("body parses");
                search_handler(State(state), Json(body))
                    .await
                    .expect("search succeeds")
                    .into_response()
            }
        };
        let body =
            body_json(search_raw(r#"{"query": "effect", "expand_links": true}"#).await).await;
        assert_eq!(body["search_mode"], "keyword");
        assert_eq!(body["results"][0]["related"][0]["id"], b);
        let body = body_json(search_raw(r#"{"query": "effect"}"#).await).await;
        assert!(body["results"][0].get("related").is_none(), "{body}");
    }

    /// The real router over a loopback listener: route wiring, the
    /// `{from_id}/{to_id}/{kind}` path extractor and the JSON body all run
    /// exactly as they do in `serve-rest`.
    #[tokio::test]
    async fn links_routes_work_over_real_http() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let a = stored_id(&state, "newer").await;
        let b = stored_id(&state, "older").await;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, crate::rest::build_router(state))
                .await
                .expect("serve");
        });
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("client");
        let base = format!("http://{addr}");

        let resp = client
            .post(format!("{base}/api/links"))
            .json(&serde_json::json!({ "from_id": a, "to_id": b, "kind": "supersedes" }))
            .send()
            .await
            .expect("POST /api/links");
        assert_eq!(resp.status().as_u16(), 200);

        let rows: serde_json::Value = client
            .get(format!("{base}/api/memories?expand_links=true"))
            .send()
            .await
            .expect("GET /api/memories?expand_links=true")
            .json()
            .await
            .expect("json rows");
        let newer = rows
            .as_array()
            .expect("array")
            .iter()
            .find(|r| r["id"] == a)
            .expect("row a");
        assert_eq!(newer["related"][0]["id"], b);
        assert_eq!(newer["related"][0]["kind"], "supersedes");

        let resp = client
            .delete(format!("{base}/api/links/{a}/{b}/blocks"))
            .send()
            .await
            .expect("DELETE with a bad kind");
        assert_eq!(resp.status().as_u16(), 400);

        let resp = client
            .delete(format!("{base}/api/links/{a}/{b}/supersedes"))
            .send()
            .await
            .expect("DELETE /api/links");
        assert_eq!(resp.status().as_u16(), 200);
        let body: serde_json::Value = resp.json().await.expect("json body");
        assert_eq!(body["unlinked"], true);

        let resp = client
            .delete(format!("{base}/api/links/{a}/{b}/supersedes"))
            .send()
            .await
            .expect("DELETE again");
        assert_eq!(resp.status().as_u16(), 404);

        server.abort();
    }

    #[tokio::test]
    async fn malformed_path_and_query_params_get_the_json_error_body() {
        // REVIEW IN-02: path and query extractor rejections used to be axum's
        // plain-text default; every client error now carries {"error"}.
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir);
        let a = stored_id(&state, "target").await;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, crate::rest::build_router(state))
                .await
                .expect("serve");
        });
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("client");
        let base = format!("http://{addr}");

        let requests = [
            (
                "PATCH /api/memories/abc",
                client
                    .patch(format!("{base}/api/memories/abc"))
                    .json(&serde_json::json!({ "content": "valid body" })),
            ),
            (
                "DELETE /api/memories/abc",
                client.delete(format!("{base}/api/memories/abc")),
            ),
            (
                "DELETE /api/links/x/2/relates_to",
                client.delete(format!("{base}/api/links/x/{a}/relates_to")),
            ),
            (
                "GET /api/memories?expand_links=1",
                client.get(format!("{base}/api/memories?expand_links=1")),
            ),
            (
                "GET /api/memories?limit=abc",
                client.get(format!("{base}/api/memories?limit=abc")),
            ),
        ];
        for (label, request) in requests {
            let resp = request.send().await.expect(label);
            assert_eq!(resp.status().as_u16(), 400, "{label} is a client error");
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            assert!(
                content_type.starts_with("application/json"),
                "{label}: JSON body, got content-type {content_type:?}"
            );
            let body: serde_json::Value = resp.json().await.expect("JSON error body");
            let message = body["error"].as_str().unwrap_or_default();
            assert!(
                !message.is_empty(),
                "{label}: non-empty error string, got {body}"
            );
        }

        // The malformed PATCH wrote nothing.
        let rows: serde_json::Value = client
            .get(format!("{base}/api/memories"))
            .send()
            .await
            .expect("GET /api/memories")
            .json()
            .await
            .expect("json rows");
        assert_eq!(rows[0]["content"], "target");

        server.abort();
    }

    #[test]
    fn map_memory_error_puts_internal_faults_in_the_500_tier() {
        // WR-02: a poisoned writer lock is a server fault — 500, never 404.
        let err = map_memory_error(MemoryError::Internal("writer lock poisoned".into()));
        assert!(
            matches!(&err, ApiError::Internal(msg) if msg.contains("writer lock poisoned")),
            "Internal must map to ApiError::Internal, got {err:?}"
        );
        assert_eq!(
            err.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );

        // The client tier is unchanged.
        let bad = map_memory_error(MemoryError::InvalidArgument(
            "limit must be between 1 and 200 (got 0)".into(),
        ));
        assert_eq!(bad.into_response().status(), StatusCode::BAD_REQUEST);
    }
}
