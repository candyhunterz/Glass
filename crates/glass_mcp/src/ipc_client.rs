//! IPC client for connecting to the Glass GUI process.
//!
//! Provides [`IpcClient`] which connects to the Glass GUI's IPC listener
//! (Unix domain socket on Unix, named pipe on Windows) and exchanges
//! JSON-line requests/responses.
//!
//! The client creates a fresh connection per request, gracefully handling
//! GUI restarts and absence.
//!
//! Socket/pipe paths are duplicated here to avoid pulling in the heavy
//! `glass_core` dependency (which brings winit). See `glass_core::ipc` for
//! the canonical definitions.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// A request sent to the Glass GUI over IPC.
/// Mirrors `glass_core::ipc::McpRequest`.
#[derive(Debug, Serialize)]
struct ClientRequest {
    id: u64,
    method: String,
    params: serde_json::Value,
}

/// A response received from the Glass GUI over IPC.
/// Mirrors `glass_core::ipc::McpResponse`.
#[derive(Debug, Deserialize)]
struct ClientResponse {
    #[serde(rename = "id")]
    _id: u64,
    result: Option<serde_json::Value>,
    error: Option<String>,
}

/// IPC client for communicating with the Glass GUI process.
///
/// Creates a fresh connection per request (handles GUI restarts).
/// Construction is cheap -- no eager connection.
pub struct IpcClient {
    next_id: AtomicU64,
}

impl Default for IpcClient {
    fn default() -> Self {
        Self::new()
    }
}

impl IpcClient {
    /// Create a new IPC client. Does not connect eagerly.
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
        }
    }

    /// Send a request to the Glass GUI and return the result.
    ///
    /// Connects fresh on each call. Returns `Err` with a human-readable
    /// message if the GUI is not running or the request times out.
    pub async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let request = ClientRequest {
            id,
            method: method.to_string(),
            params,
        };

        let mut stream = connect()
            .await
            .map_err(|e| format!("Glass GUI is not running ({})", e))?;

        // Serialize request as a JSON line
        let mut payload = serde_json::to_vec(&request)
            .map_err(|e| format!("Failed to serialize request: {}", e))?;
        payload.push(b'\n');

        stream
            .write_all(&payload)
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;

        // Read response line with 5-second timeout
        let response_line = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut lines = BufReader::new(&mut stream).lines();
            lines
                .next_line()
                .await
                .map_err(|e| format!("Failed to read response: {}", e))
        })
        .await
        .map_err(|_| "Request timed out (5s)".to_string())?
        .and_then(|opt| opt.ok_or_else(|| "Connection closed before response".to_string()))?;

        let resp: ClientResponse = serde_json::from_str(&response_line)
            .map_err(|e| format!("Invalid response JSON: {}", e))?;

        if let Some(err) = resp.error {
            return Err(err);
        }

        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }
}

// ---------------------------------------------------------------------------
// Platform-specific connection helpers
// ---------------------------------------------------------------------------

/// Returns the IPC socket path on Unix platforms.
///
/// # Duplication note
/// This function is intentionally duplicated from `glass_core::ipc::ipc_socket_path()`.
/// `glass_mcp` is loaded by the standalone MCP server process which must stay lightweight;
/// adding a dependency on `glass_core` would pull in `winit`, `wgpu`, and other heavy GUI
/// crates that are not needed and would inflate binary size / compile times significantly.
/// If the path ever changes, update both this file and `crates/glass_core/src/ipc.rs`.
#[cfg(unix)]
fn ipc_socket_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".glass")
        .join("glass.sock")
}

/// Returns the named pipe name on Windows.
///
/// # Duplication note
/// This function is intentionally duplicated from `glass_core::ipc::ipc_pipe_name()`.
/// See `ipc_socket_path` doc comment above for the rationale.
/// If the pipe name ever changes, update both this file and `crates/glass_core/src/ipc.rs`.
#[cfg(windows)]
fn ipc_pipe_name() -> String {
    r"\\.\pipe\glass-terminal".to_string()
}

/// Connect to the Glass GUI's IPC listener.
#[cfg(unix)]
async fn connect() -> Result<tokio::net::UnixStream, String> {
    let path = ipc_socket_path();
    tokio::net::UnixStream::connect(&path)
        .await
        .map_err(|e| format!("{}: {}", path.display(), e))
}

/// Connect to the Glass GUI's IPC listener.
#[cfg(windows)]
async fn connect() -> Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin, String> {
    use tokio::net::windows::named_pipe::ClientOptions;

    let pipe_name = ipc_pipe_name();
    ClientOptions::new()
        .open(&pipe_name)
        .map_err(|e| format!("{}: {}", pipe_name, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_client_without_connecting() {
        // IpcClient::new() should always succeed, even without a running GUI
        let client = IpcClient::new();
        assert_eq!(client.next_id.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn atomic_id_increments_across_calls() {
        let client = IpcClient::new();
        let id1 = client.next_id.fetch_add(1, Ordering::Relaxed);
        let id2 = client.next_id.fetch_add(1, Ordering::Relaxed);
        let id3 = client.next_id.fetch_add(1, Ordering::Relaxed);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
    }

    #[tokio::test]
    async fn send_request_returns_error_when_gui_not_running() {
        // Skip if Glass GUI is actually running — the connection will succeed,
        // which invalidates the test premise.
        if connect().await.is_ok() {
            return;
        }
        let client = IpcClient::new();
        let result = client.send_request("ping", serde_json::json!({})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Glass GUI is not running"),
            "Expected 'Glass GUI is not running' but got: {}",
            err
        );
    }

    #[tokio::test]
    async fn send_request_serializes_and_deserializes_json() {
        // Spin up a mock TCP server that echoes back a response
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = tokio::io::split(stream);
            let mut lines = BufReader::new(reader).lines();

            if let Ok(Some(line)) = lines.next_line().await {
                let req: serde_json::Value = serde_json::from_str(&line).unwrap();
                let resp = serde_json::json!({
                    "id": req["id"],
                    "result": {"status": "ok"},
                });
                let mut out = serde_json::to_vec(&resp).unwrap();
                out.push(b'\n');
                writer.write_all(&out).await.unwrap();
            }
        });

        // Connect via TCP directly to test serialization logic
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();

        let request = ClientRequest {
            id: 42,
            method: "ping".to_string(),
            params: serde_json::json!({}),
        };
        let mut payload = serde_json::to_vec(&request).unwrap();
        payload.push(b'\n');
        writer.write_all(&payload).await.unwrap();

        let resp_line = lines.next_line().await.unwrap().unwrap();
        let resp: ClientResponse = serde_json::from_str(&resp_line).unwrap();
        assert_eq!(resp._id, 42);
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());

        let result = resp.result.unwrap();
        assert_eq!(result["status"], "ok");

        server.await.unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn pipe_name_matches_gui_listener() {
        assert_eq!(ipc_pipe_name(), r"\\.\pipe\glass-terminal");
    }

    #[cfg(unix)]
    #[test]
    fn socket_path_ends_in_glass_sock() {
        let path = ipc_socket_path();
        assert!(path.to_str().unwrap().ends_with("glass.sock"));
    }
}
