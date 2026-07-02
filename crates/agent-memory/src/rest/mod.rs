//! The axum REST mirror: thin HTTP adapters over [`crate::mcp::AppState`].
//!
//! Same architecture rule as `mcp.rs` (RESEARCH Anti-Pattern 1): handlers
//! contain NO SQL, NO decay math, and NO clock access. Each handler only
//! deserializes its request, calls [`MemoryService`], and maps the `Result`
//! to an HTTP response — so the REST surface can never diverge from the MCP
//! tools' behavior (API-01).
//!
//! Security (T-02-10): the daemon binds loopback by default and
//! [`ensure_bind_allowed`] refuses a non-loopback `--addr` unless the explicit
//! `--allow-remote` flag was passed.
//!
//! [`MemoryService`]: agent_memory_core::service::MemoryService

pub mod handlers;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};

use crate::mcp::AppState;

/// Typed HTTP error for the REST handlers — the two-tier mapping mirrors
/// `mcp.rs`'s `invalid_params` / `internal_error` split: bad input is a 400,
/// a missing resource is a 404, everything else is a 500. The body is always
/// `{"error": "..."}` so bad input can never surface as a 500.
#[derive(Debug)]
pub enum ApiError {
    /// Invalid client input (e.g. an unknown memory type) → 400.
    BadRequest(String),
    /// The referenced resource does not exist → 404.
    NotFound,
    /// An unexpected server-side failure → 500.
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            ApiError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

/// Refuse a non-loopback bind unless `--allow-remote` was passed (T-02-10).
///
/// The REST API is unauthenticated by design (single-user local tool), so a
/// non-loopback bind exposes read/write memory access to the network — that
/// must be an explicit, warned decision, never a silent default.
pub fn ensure_bind_allowed(addr: &SocketAddr, allow_remote: bool) -> anyhow::Result<()> {
    if addr.ip().is_loopback() || allow_remote {
        return Ok(());
    }
    anyhow::bail!(
        "refusing to bind non-loopback address {addr}: the REST API is \
         unauthenticated; pass --allow-remote to expose it beyond localhost"
    )
}

/// Build the REST router over the shared application state.
///
/// axum 0.8 path-parameter syntax: `{id}` (the 0.7 colon form no longer
/// matches).
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/api/memories",
            post(handlers::store_handler).get(handlers::list_handler),
        )
        .route("/api/search", post(handlers::search_handler))
        .route("/api/memories/{id}", delete(handlers::forget_handler))
        .route("/health", get(handlers::health_handler))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> SocketAddr {
        s.parse().expect("test address parses")
    }

    #[test]
    fn loopback_is_allowed_without_the_flag() {
        assert!(ensure_bind_allowed(&addr("127.0.0.1:7437"), false).is_ok());
        assert!(ensure_bind_allowed(&addr("[::1]:7437"), false).is_ok());
    }

    #[test]
    fn non_loopback_without_the_flag_is_refused_naming_the_flag() {
        let err = ensure_bind_allowed(&addr("0.0.0.0:7437"), false)
            .expect_err("non-loopback bind must be refused");
        assert!(
            err.to_string().contains("--allow-remote"),
            "refusal must name the --allow-remote flag, got: {err}"
        );
    }

    #[test]
    fn non_loopback_with_the_flag_is_allowed() {
        assert!(ensure_bind_allowed(&addr("0.0.0.0:7437"), true).is_ok());
    }
}
