//! Core domain, store, decay, and service logic for the agent-memory MCP server.
//!
//! This library is transport-agnostic: it knows nothing about MCP or stdio. The
//! `agent-memory` binary wraps these types in an `rmcp` server. Errors are typed
//! ([`domain::MemoryError`], via `thiserror`); the binary uses `anyhow` at its edges.

pub mod clock;
pub mod decay;
pub mod domain;
// `service` and `store` are added in Task 2 (SQLite store + MemoryService).
