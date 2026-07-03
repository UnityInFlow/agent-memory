//! API-01 gate: the REST mirror over the SAME store the MCP tools use.
//!
//! Spawns the real `agent-memory serve-rest` binary on an ephemeral loopback
//! port (`--addr 127.0.0.1:0`), parses the bound port from the stderr line
//! `REST listening on ...`, and exercises every endpoint over real HTTP with
//! reqwest: store (201), invalid type (400 — never 500), list with a type
//! filter, search carrying the shared `{search_mode, results}` envelope,
//! delete (200 then 404 on the second attempt), and health (200).
//!
//! The spawned process gets `AGENT_MEMORY_OLLAMA_URL=http://127.0.0.1:9`
//! (discard port → connection refused) so search deterministically runs in
//! keyword mode on ANY machine, with or without a local Ollama daemon.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn binary_path() -> String {
    // CARGO_BIN_EXE_<name> is set by cargo for integration tests of a binary crate.
    env!("CARGO_BIN_EXE_agent-memory").to_string()
}

/// A spawned `serve-rest` daemon bound to an ephemeral loopback port.
///
/// The child is reaped on Drop (kill + wait), so every exit path — including
/// assertion failures mid-test — cleans up the process.
struct RestServer {
    child: Child,
    port: u16,
    _dir: tempfile::TempDir,
}

impl RestServer {
    /// Spawn the binary, wait (bounded) for the `REST listening on` stderr
    /// line, and extract the bound port from it.
    fn spawn() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("rest.db");

        let mut child = Command::new(binary_path())
            .args(["serve-rest", "--addr", "127.0.0.1:0", "--db"])
            .arg(&db_path)
            // Deterministically dead Ollama URL: keyword mode on every machine.
            .env("AGENT_MEMORY_OLLAMA_URL", "http://127.0.0.1:9")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn agent-memory serve-rest");

        let stderr = child.stderr.take().expect("child stderr piped");
        let mut reader = BufReader::new(stderr);
        let deadline = Instant::now() + Duration::from_secs(10);

        let port = loop {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for the 'REST listening on' stderr line"
            );
            let mut line = String::new();
            let n = reader.read_line(&mut line).expect("read stderr line");
            if n == 0 {
                // EOF: the child exited before listening (e.g. serve-rest is
                // not a known subcommand yet). Kill/wait and fail loudly.
                let _ = child.kill();
                let status = child.wait().expect("wait for exited child");
                panic!(
                    "server exited (status {status}) before emitting \
                     'REST listening on' — does the serve-rest subcommand exist?"
                );
            }
            if let Some(tail) = line.split("REST listening on").nth(1) {
                let addr = tail.trim();
                let port_str = addr
                    .rsplit(':')
                    .next()
                    .unwrap_or_else(|| panic!("no port in listening line: {line:?}"));
                let port: u16 = port_str
                    .trim()
                    .parse()
                    .unwrap_or_else(|_| panic!("unparseable port in line: {line:?}"));
                break port;
            }
        };

        // Keep draining stderr on a background thread so the child can never
        // block on a full pipe buffer while the test runs.
        std::thread::spawn(move || {
            let mut sink = String::new();
            loop {
                sink.clear();
                match reader.read_line(&mut sink) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        });

        RestServer {
            child,
            port,
            _dir: dir,
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for RestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[tokio::test]
async fn rest_end_to_end_store_list_search_forget_health() {
    let server = RestServer::spawn();
    let base = server.base_url();
    let client = reqwest::Client::new();

    // POST /api/memories → 201 with a numeric id.
    let resp = client
        .post(format!("{base}/api/memories"))
        .json(&serde_json::json!({
            "content": "use WAL mode for sqlite",
            "type": "DECISION",
            "tags": ["db"]
        }))
        .send()
        .await
        .expect("store request");
    assert_eq!(resp.status(), 201, "store must return 201 Created");
    let body: serde_json::Value = resp.json().await.expect("store body is JSON");
    let id = body
        .get("id")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| panic!("store body must carry a numeric id, got: {body}"));
    assert!(id > 0, "id should be positive");

    // POST /api/memories with an invalid type → 400 with an error field
    // (never a 500 for bad input).
    let resp = client
        .post(format!("{base}/api/memories"))
        .json(&serde_json::json!({ "content": "x", "type": "BOGUS" }))
        .send()
        .await
        .expect("invalid-type store request");
    assert_eq!(resp.status(), 400, "invalid memory type must map to 400");
    let body: serde_json::Value = resp.json().await.expect("400 body is JSON");
    assert!(
        body.get("error").and_then(|e| e.as_str()).is_some(),
        "400 body must carry an error field, got: {body}"
    );

    // GET /api/memories?type=DECISION → 200 array containing the stored memory.
    let resp = client
        .get(format!("{base}/api/memories?type=DECISION"))
        .send()
        .await
        .expect("list request");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("list body is JSON");
    let arr = body.as_array().expect("list body is a JSON array");
    assert!(
        arr.iter().any(|v| {
            v.get("id").and_then(|i| i.as_i64()) == Some(id)
                && v.get("type").and_then(|t| t.as_str()) == Some("DECISION")
        }),
        "list?type=DECISION must contain the stored memory, got: {body}"
    );

    // POST /api/search → 200 with the shared {search_mode, results} envelope;
    // the dead Ollama URL forces keyword mode.
    let resp = client
        .post(format!("{base}/api/search"))
        .json(&serde_json::json!({ "query": "sqlite" }))
        .send()
        .await
        .expect("search request");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("search body is JSON");
    assert_eq!(
        body.get("search_mode").and_then(|m| m.as_str()),
        Some("keyword"),
        "dead Ollama URL must surface search_mode 'keyword', got: {body}"
    );
    let results = body
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("search body must carry a results array: {body}"));
    assert!(
        results
            .iter()
            .any(|v| v.get("id").and_then(|i| i.as_i64()) == Some(id)),
        "search results must contain the stored id {id}, got: {body}"
    );

    // POST /api/search with a malformed FTS5 query (a lone double-quote) in
    // keyword mode → 400 with an error body — CLIENT input, never a 500.
    let resp = client
        .post(format!("{base}/api/search"))
        .json(&serde_json::json!({ "query": "\"" }))
        .send()
        .await
        .expect("malformed-query search request");
    assert_eq!(
        resp.status(),
        400,
        "malformed FTS5 query must map to 400, never 500"
    );
    let body: serde_json::Value = resp.json().await.expect("400 body is JSON");
    assert!(
        body.get("error").and_then(|e| e.as_str()).is_some(),
        "400 body must carry an error field, got: {body}"
    );

    // DELETE /api/memories/{id} → 200 deleted:true, then 404 on the SAME id.
    let resp = client
        .delete(format!("{base}/api/memories/{id}"))
        .send()
        .await
        .expect("delete request");
    assert_eq!(resp.status(), 200, "first delete must return 200");
    let body: serde_json::Value = resp.json().await.expect("delete body is JSON");
    assert_eq!(body.get("deleted"), Some(&serde_json::json!(true)));

    let resp = client
        .delete(format!("{base}/api/memories/{id}"))
        .send()
        .await
        .expect("second delete request");
    assert_eq!(resp.status(), 404, "double delete must return 404");
    let body: serde_json::Value = resp.json().await.expect("404 body is JSON");
    assert_eq!(body.get("deleted"), Some(&serde_json::json!(false)));
    assert_eq!(
        body.get("reason").and_then(|r| r.as_str()),
        Some("not_found"),
        "404 delete body must carry reason not_found, got: {body}"
    );

    // GET /health → 200 with status ok.
    let resp = client
        .get(format!("{base}/health"))
        .send()
        .await
        .expect("health request");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("health body is JSON");
    assert_eq!(
        body.get("status").and_then(|s| s.as_str()),
        Some("ok"),
        "health must report status ok, got: {body}"
    );
}

/// Bind guard: a non-loopback `--addr` without `--allow-remote` must be
/// refused with an error that names the flag (T-02-10).
#[test]
fn serve_rest_refuses_non_loopback_bind_without_allow_remote() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("rest.db");

    let output = Command::new(binary_path())
        .args(["serve-rest", "--addr", "0.0.0.0:0", "--db"])
        .arg(&db_path)
        .env("AGENT_MEMORY_OLLAMA_URL", "http://127.0.0.1:9")
        .stdin(Stdio::null())
        .output()
        .expect("run agent-memory serve-rest with a non-loopback addr");

    assert!(
        !output.status.success(),
        "non-loopback bind without --allow-remote must exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--allow-remote"),
        "refusal must name the --allow-remote flag, stderr was: {stderr}"
    );
}
