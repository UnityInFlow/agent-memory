//! The five REST handlers — thin adapters with ZERO business logic.
//!
//! Each handler copies the `mcp.rs` per-tool shape: validate the wire type via
//! `MemoryType::try_from` up front, build the exact core service arg structs,
//! make ONE service call, and map the error. The request DTOs mirror the MCP
//! tool argument structs field-for-field (`content`, `type`, `tags`, `source`,
//! `scope`, `ttl_secs`, `query`, `limit`) so both transports speak one dialect.
//! serde only — no schemars on REST DTOs.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use agent_memory_core::domain::{MemoryError, MemoryType, NewMemory};
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

/// Query string for `GET /api/memories` — mirrors `mcp::ListArgs`.
#[derive(Debug, serde::Deserialize)]
pub struct ListQuery {
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

/// Two-tier [`MemoryError`] → HTTP mapping, mirroring `mcp.rs`'s
/// `invalid_params` / `internal_error` split: `InvalidType` is client input
/// (400), `NotFound` is a missing resource (404), everything else is 500.
fn map_memory_error(e: MemoryError) -> ApiError {
    match &e {
        MemoryError::InvalidType(_) => ApiError::BadRequest(e.to_string()),
        MemoryError::NotFound => ApiError::NotFound,
        MemoryError::Sqlite(_)
        | MemoryError::Pool(_)
        | MemoryError::Join(_)
        | MemoryError::Migration(_) => ApiError::Internal(e.to_string()),
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

/// `GET /api/memories?type=&tag=&scope=&limit=` → 200 `Vec<MemoryView>`.
pub async fn list_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> Result<Response, ApiError> {
    let mem_type = parse_type_filter(q.r#type.as_deref())?;

    let views = state
        .service
        .list(ServiceListArgs {
            mem_type,
            tag: q.tag,
            scope: q.scope,
            limit: q.limit,
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
        })
        .await
        .map_err(map_memory_error)?;

    Ok(Json(outcome).into_response())
}

/// `DELETE /api/memories/{id}` → 200 `{"id", "deleted": true}` or 404
/// `{"id", "deleted": false, "reason": "not_found"}` — the exact JSON bodies
/// the `mcp.rs` forget tool produces, with HTTP-native status codes.
pub async fn forget_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
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
