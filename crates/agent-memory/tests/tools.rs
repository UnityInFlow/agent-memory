//! MCP-01..04 / SEARCH-01: the four memory tools over real stdio.
//!
//! Drives the binary through the JSON-RPC handshake (`initialize` →
//! `notifications/initialized`), calls the tools, and asserts on both the
//! responses and the persisted SQLite row. Plan 02 added the `memory_search` /
//! `memory_forget` cases.

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

    /// Call a tool by name and read its response, matching the given request id.
    fn call_tool(
        &mut self,
        id: i64,
        name: &str,
        arguments: serde_json::Value,
    ) -> serde_json::Value {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }));
        self.read_response(id)
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

/// Parse a `tools/call` text payload as a JSON array of memory views.
fn tool_views(response: &serde_json::Value) -> Vec<serde_json::Value> {
    let text = tool_text(response);
    let value: serde_json::Value = serde_json::from_str(&text).expect("tool result text is JSON");
    value
        .as_array()
        .expect("tool result is a JSON array")
        .clone()
}

#[test]
fn memory_search_returns_ranked_results_with_decay_then_empty_on_no_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");

    let mut server = Server::spawn(&db_path);
    server.initialize();

    // Store two memories that both contain "database".
    server.call_tool(
        2,
        "memory_store",
        serde_json::json!({ "content": "use sqlite for the database", "type": "DECISION" }),
    );
    server.call_tool(
        3,
        "memory_store",
        serde_json::json!({ "content": "the database connection pool config", "type": "PATTERN" }),
    );

    // Search "database" → both returned, each carries a decay_score.
    let resp = server.call_tool(
        4,
        "memory_search",
        serde_json::json!({ "query": "database" }),
    );
    let views = tool_views(&resp);
    assert_eq!(views.len(), 2, "both 'database' memories should match");
    for v in &views {
        assert!(
            v.get("decay_score").and_then(|d| d.as_f64()).is_some(),
            "each result must surface a decay_score, got: {v}"
        );
        assert!(v.get("id").and_then(|i| i.as_i64()).is_some());
    }

    // A non-matching query → empty list, NOT an error (MCP-02 / SEARCH-01).
    let resp = server.call_tool(
        5,
        "memory_search",
        serde_json::json!({ "query": "nonexistentkeyword" }),
    );
    assert!(
        resp.get("error").is_none(),
        "no-match search must not be a JSON-RPC error, got: {resp}"
    );
    let empty = tool_views(&resp);
    assert!(
        empty.is_empty(),
        "no-match search must return an empty list"
    );

    server.shutdown();
}

#[test]
fn memory_forget_deletes_then_search_and_list_omit_it_unknown_id_is_clean_not_found() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");

    let mut server = Server::spawn(&db_path);
    server.initialize();

    // Store a memory and capture its id.
    let store_resp = server.call_tool(
        2,
        "memory_store",
        serde_json::json!({ "content": "ephemeral keyword zebra", "type": "TODO" }),
    );
    let id: i64 = tool_text(&store_resp)
        .trim()
        .parse()
        .expect("memory_store returns a numeric id");

    // It is findable before deletion.
    let before =
        tool_views(&server.call_tool(3, "memory_search", serde_json::json!({ "query": "zebra" })));
    assert_eq!(
        before.len(),
        1,
        "the memory should be searchable before forget"
    );

    // Forget the existing id → deleted.
    let forget_resp = server.call_tool(4, "memory_forget", serde_json::json!({ "id": id }));
    assert!(
        forget_resp.get("error").is_none(),
        "forget of an existing id must not error: {forget_resp}"
    );
    let forget_text = tool_text(&forget_resp);
    let forget_json: serde_json::Value =
        serde_json::from_str(&forget_text).expect("forget returns JSON");
    assert_eq!(forget_json["deleted"], serde_json::json!(true));

    // Subsequent search AND list omit it (FTS5 mirror stayed in sync via trigger).
    let after_search =
        tool_views(&server.call_tool(5, "memory_search", serde_json::json!({ "query": "zebra" })));
    assert!(
        after_search.is_empty(),
        "deleted memory must not appear in search results"
    );
    let after_list = tool_views(&server.call_tool(6, "memory_list", serde_json::json!({})));
    assert!(
        after_list.is_empty(),
        "deleted memory must not appear in list results"
    );

    // Forget an unknown id → clean not-found, NOT a protocol error (MCP-04).
    let unknown_resp = server.call_tool(7, "memory_forget", serde_json::json!({ "id": 999_999 }));
    assert!(
        unknown_resp.get("error").is_none(),
        "unknown-id forget must be a clean result, not a JSON-RPC error: {unknown_resp}"
    );
    let unknown_json: serde_json::Value =
        serde_json::from_str(&tool_text(&unknown_resp)).expect("forget returns JSON");
    assert_eq!(unknown_json["deleted"], serde_json::json!(false));

    server.shutdown();
}
