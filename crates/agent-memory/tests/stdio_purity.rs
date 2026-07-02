//! MCP-05 gate: stdout carries ONLY JSON-RPC; all logs go to stderr.
//!
//! Spawns the built `agent-memory serve` binary, pipes a JSON-RPC `initialize`
//! frame to its stdin, and asserts that every non-empty stdout line parses as a
//! JSON-RPC object while log output lands on stderr. This is the highest-priority
//! correctness gate: any stray byte on stdout corrupts the MCP client.

use std::io::Write;
use std::process::{Command, Stdio};

fn binary_path() -> String {
    // CARGO_BIN_EXE_<name> is set by cargo for integration tests of a binary crate.
    env!("CARGO_BIN_EXE_agent-memory").to_string()
}

#[test]
fn stdout_is_pure_jsonrpc_and_logs_go_to_stderr() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");

    let mut child = Command::new(binary_path())
        .args(["serve", "--db"])
        .arg(&db_path)
        // Dead Ollama URL (discard port → connection refused): the embedder is
        // ACTIVE and its degrade warning path fires during the purity check, so
        // this proves MCP-05 holds with Phase-2 code live (RESEARCH Pitfall 4).
        .env("AGENT_MEMORY_OLLAMA_URL", "http://127.0.0.1:9")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn agent-memory serve");

    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"purity-test","version":"0.0.0"}}}"#;

    {
        let stdin = child.stdin.as_mut().expect("child stdin");
        writeln!(stdin, "{init}").expect("write initialize");
    }
    // Dropping stdin (closing the pipe) signals EOF so the server shuts down.
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("wait for child");

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");

    // Every non-empty stdout line MUST parse as a JSON object (JSON-RPC frame).
    let mut saw_a_frame = false;
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("stdout line is not JSON-RPC: {line:?} ({e})"));
        assert_eq!(
            value.get("jsonrpc").and_then(|v| v.as_str()),
            Some("2.0"),
            "stdout JSON-RPC frame must carry jsonrpc=2.0: {line}"
        );
        saw_a_frame = true;
    }
    assert!(
        saw_a_frame,
        "expected at least one JSON-RPC response on stdout, got: {stdout:?}"
    );

    // Logs (the migration + service INFO lines) must be on stderr, not stdout.
    assert!(
        stderr.contains("Service initialized") || stderr.contains("Database migrated"),
        "expected server logs on stderr, got: {stderr:?}"
    );
}
