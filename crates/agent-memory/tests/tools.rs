//! MCP-01 / MCP-03: `memory_store` and `memory_list` over real stdio.
//!
//! Drives the binary through the JSON-RPC handshake (`initialize` →
//! `notifications/initialized`), calls the tools, and asserts on both the
//! responses and the persisted SQLite row. Plan 02 extends this file for
//! `memory_search` / `memory_forget`.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

fn binary_path() -> String {
    env!("CARGO_BIN_EXE_agent-memory").to_string()
}

/// A spawned server with line-buffered stdio, driven request/response.
struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Server {
    fn spawn(db_path: &Path) -> Self {
        let mut child = Command::new(binary_path())
            .args(["serve", "--db"])
            .arg(db_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn agent-memory serve");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Server {
            child,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, frame: &serde_json::Value) {
        writeln!(self.stdin, "{frame}").expect("write frame");
        self.stdin.flush().expect("flush");
    }

    /// Read lines until one parses as a JSON-RPC response with the given id.
    fn read_response(&mut self, id: i64) -> serde_json::Value {
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line).expect("read line");
            assert!(n > 0, "server closed stdout before responding to id {id}");
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: serde_json::Value =
                serde_json::from_str(trimmed).expect("response is JSON-RPC");
            if value.get("id").and_then(|v| v.as_i64()) == Some(id) {
                return value;
            }
        }
    }

    fn initialize(&mut self) {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "tools-test", "version": "0.0.0" }
            }
        }));
        let _ = self.read_response(1);
        // The initialized notification has no id and expects no response.
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
    }

    fn shutdown(mut self) {
        drop(self.stdin);
        let _ = self.child.wait();
    }
}

/// Pull the text payload out of a successful `tools/call` result.
fn tool_text(response: &serde_json::Value) -> String {
    response
        .pointer("/result/content/0/text")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no text content in tool result: {response}"))
        .to_string()
}

#[test]
fn memory_store_returns_id_with_null_scope_then_list_returns_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");

    let mut server = Server::spawn(&db_path);
    server.initialize();

    // memory_store with a bare {content, type} (scope omitted → NULL).
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "memory_store",
            "arguments": { "content": "remember the milk", "type": "TODO" }
        }
    }));
    let store_resp = server.read_response(2);
    let id_text = tool_text(&store_resp);
    let id: i64 = id_text
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("memory_store should return a numeric id, got: {id_text:?}"));
    assert!(id > 0, "id should be positive");

    // memory_list should return the stored memory.
    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": { "name": "memory_list", "arguments": {} }
    }));
    let list_resp = server.read_response(3);
    let list_text = tool_text(&list_resp);
    let views: serde_json::Value =
        serde_json::from_str(&list_text).expect("memory_list returns JSON");
    let arr = views.as_array().expect("list is a JSON array");
    assert_eq!(arr.len(), 1, "exactly one stored memory expected");
    assert_eq!(arr[0]["content"], "remember the milk");
    assert_eq!(arr[0]["type"], "TODO");

    server.shutdown();

    // Confirm the persisted row has a NULL scope (MCP-01: omitted scope → NULL).
    let conn = rusqlite::Connection::open(&db_path).expect("open db for assertion");
    let scope: Option<String> = conn
        .query_row("SELECT scope FROM memories WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .expect("row should exist");
    assert!(scope.is_none(), "omitted scope must persist as NULL");
}

#[test]
fn memory_store_rejects_invalid_type_cleanly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");

    let mut server = Server::spawn(&db_path);
    server.initialize();

    server.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "memory_store",
            "arguments": { "content": "bad", "type": "NONSENSE" }
        }
    }));
    let resp = server.read_response(2);
    // A clean error, never a panic / crash. Either a JSON-RPC error or an
    // is_error tool result is acceptable; the server must still be alive.
    let is_jsonrpc_error = resp.get("error").is_some();
    let is_tool_error = resp
        .pointer("/result/isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(
        is_jsonrpc_error || is_tool_error,
        "invalid type should yield a clean error, got: {resp}"
    );

    server.shutdown();
}
