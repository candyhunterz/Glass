//! Integration tests for the Glass MCP server.
//!
//! These tests spawn `glass mcp serve` as a child process and communicate
//! via JSON-RPC over stdin/stdout (newline-delimited JSON framing, per rmcp).

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

// ---------------------------------------------------------------------------
// Helper: MCP test client
// ---------------------------------------------------------------------------

/// A lightweight MCP client that spawns `glass mcp serve` and speaks JSON-RPC
/// over its stdin/stdout pipes.
struct McpTestClient {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    /// Receiver for lines read from stdout by the reader thread.
    rx: mpsc::Receiver<String>,
}

impl McpTestClient {
    /// Spawn `glass mcp serve` with piped stdin/stdout, working dir set to a
    /// temp directory so the server creates a fresh (empty) history database.
    fn spawn(tmp_dir: &std::path::Path) -> Self {
        let glass_bin = env!("CARGO_BIN_EXE_glass");

        let mut child = Command::new(glass_bin)
            .args(["mcp", "serve"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(tmp_dir)
            .spawn()
            .expect("Failed to spawn glass mcp serve");

        let stdin = child.stdin.take().expect("Failed to open stdin");
        let stdout = child.stdout.take().expect("Failed to open stdout");

        // Spawn a reader thread that sends lines to a channel so we can
        // read with a timeout from the main test thread.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) if !l.is_empty() => {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {} // skip empty lines
                    Err(_) => break,
                }
            }
        });

        Self {
            child,
            stdin: Some(stdin),
            rx,
        }
    }

    /// Send a JSON-RPC message (newline-delimited).
    fn send(&mut self, msg: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin already closed");
        let serialized = serde_json::to_string(msg).expect("Failed to serialize");
        writeln!(stdin, "{}", serialized).expect("Failed to write to stdin");
        stdin.flush().expect("Failed to flush stdin");
    }

    /// Read the next JSON-RPC response with a timeout.
    fn recv(&self, timeout: Duration) -> Option<Value> {
        match self.rx.recv_timeout(timeout) {
            Ok(line) => serde_json::from_str(&line).ok(),
            Err(_) => None,
        }
    }

    /// Perform the initialize handshake (initialize request + initialized notification).
    /// Returns the initialize response.
    fn initialize(&mut self) -> Value {
        let init_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "test",
                    "version": "0.1.0"
                }
            }
        });
        self.send(&init_req);

        let resp = self
            .recv(Duration::from_secs(10))
            .expect("No response to initialize request within 10s");

        // Send initialized notification
        let initialized = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        self.send(&initialized);

        resp
    }

    /// Close stdin, triggering server shutdown.
    fn close_stdin(&mut self) {
        self.stdin.take();
    }
}

impl Drop for McpTestClient {
    fn drop(&mut self) {
        // Close stdin to trigger clean shutdown, then wait for process
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_mcp_initialize_handshake() {
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");
    let mut client = McpTestClient::spawn(tmp.path());

    let resp = client.initialize();

    // Verify JSON-RPC response structure
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);

    let result = &resp["result"];
    assert!(
        result.is_object(),
        "Expected 'result' object in response, got: {resp}"
    );

    // Server info
    let server_info = &result["serverInfo"];
    assert_eq!(
        server_info["name"], "glass-mcp",
        "Expected server name 'glass-mcp', got: {server_info}"
    );

    // Capabilities must include tools
    let capabilities = &result["capabilities"];
    assert!(
        capabilities.get("tools").is_some(),
        "Expected 'tools' in capabilities, got: {capabilities}"
    );
}

#[test]
fn test_tools_list() {
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");
    let mut client = McpTestClient::spawn(tmp.path());

    // Must initialize first
    client.initialize();

    // Send tools/list request
    let tools_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    client.send(&tools_req);

    let resp = client
        .recv(Duration::from_secs(10))
        .expect("No response to tools/list within 10s");

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);

    let tools = resp["result"]["tools"]
        .as_array()
        .expect("Expected 'tools' array in result");

    // Collect tool names
    let tool_names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    assert!(
        tool_names.contains(&"glass_history"),
        "Expected glass_history tool, found: {tool_names:?}"
    );
    assert!(
        tool_names.contains(&"glass_context"),
        "Expected glass_context tool, found: {tool_names:?}"
    );

    // Verify each tool has an inputSchema
    for tool in tools {
        let name = tool["name"].as_str().unwrap_or("unknown");
        assert!(
            tool.get("inputSchema").is_some(),
            "Tool '{name}' missing inputSchema"
        );
        let schema = &tool["inputSchema"];
        assert_eq!(
            schema["type"], "object",
            "Tool '{name}' inputSchema should be type 'object'"
        );
    }
}

#[test]
fn test_server_exits_on_stdin_close() {
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");
    let mut client = McpTestClient::spawn(tmp.path());

    // Initialize first so the server is fully running
    client.initialize();

    // Close stdin to trigger clean shutdown
    client.close_stdin();

    // Wait for the process to exit with a timeout
    // We poll try_wait in a loop with a deadline rather than blocking forever.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match client.child.try_wait() {
            Ok(Some(status)) => {
                assert!(
                    status.success(),
                    "Expected exit code 0 on clean shutdown, got: {status}"
                );
                return;
            }
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = client.child.kill();
                    panic!("Server did not exit within 10s after stdin closed");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("Error waiting for child process: {e}"),
        }
    }
}
