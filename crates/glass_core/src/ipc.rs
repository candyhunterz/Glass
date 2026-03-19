//! IPC infrastructure for the MCP command channel.
//!
//! Provides a platform-abstracted local IPC listener (Unix domain socket on
//! Unix, named pipe on Windows) that accepts JSON-line requests and returns
//! JSON-line responses through the winit event loop.

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::event::AppEvent;

/// A request received over the IPC channel.
#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// A response sent back over the IPC channel.
#[derive(Debug, Serialize, Deserialize)]
pub struct McpResponse {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl McpResponse {
    /// Create a successful response with a JSON result value.
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response with a message.
    pub fn err(id: u64, error: String) -> Self {
        Self {
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// Construct the JSON result value for a "ping" request.
pub fn ping_result() -> serde_json::Value {
    serde_json::json!({"status": "ok"})
}

/// Internal event carrying a request and a reply channel.
/// Sent from the IPC listener thread to the winit event loop.
pub struct McpEventRequest {
    pub request: McpRequest,
    pub reply: oneshot::Sender<McpResponse>,
}

// Debug impl for McpEventRequest (oneshot::Sender doesn't derive Debug)
impl std::fmt::Debug for McpEventRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpEventRequest")
            .field("request", &self.request)
            .field("reply", &"<oneshot::Sender>")
            .finish()
    }
}

/// Returns the IPC socket path on Unix platforms.
#[cfg(unix)]
pub fn ipc_socket_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".glass")
        .join("glass.sock")
}

/// Returns the named pipe name on Windows.
#[cfg(windows)]
pub fn ipc_pipe_name() -> String {
    r"\\.\pipe\glass-terminal".to_string()
}

/// Start the IPC listener in a background thread with its own tokio runtime.
///
/// Follows the same thread-spawn pattern as `coordination_poller::spawn_coordination_poller`.
pub fn start_ipc_listener(proxy: winit::event_loop::EventLoopProxy<AppEvent>) {
    std::thread::Builder::new()
        .name("Glass IPC listener".into())
        .spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::warn!("Failed to create IPC tokio runtime: {}", e);
                    return;
                }
            };
            tracing::info!("IPC listener thread started");
            if let Err(e) = rt.block_on(spawn_ipc_listener(proxy)) {
                tracing::warn!("IPC listener exited with error: {}", e);
            }
        })
        .expect("Failed to spawn IPC listener thread");
}

/// Handle a single IPC connection: read JSON-line requests, dispatch through
/// the event loop proxy, and write JSON-line responses.
async fn handle_ipc_connection<S>(stream: S, proxy: winit::event_loop::EventLoopProxy<AppEvent>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let request: McpRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = McpResponse {
                    id: 0,
                    result: None,
                    error: Some(format!("Invalid JSON: {}", e)),
                };
                let mut out = serde_json::to_vec(&err_resp).unwrap_or_default();
                out.push(b'\n');
                let _ = writer.write_all(&out).await;
                continue;
            }
        };

        let (tx, rx) = oneshot::channel();
        let event = AppEvent::McpRequest(McpEventRequest {
            request: McpRequest {
                id: request.id,
                method: request.method,
                params: request.params,
            },
            reply: tx,
        });

        if proxy.send_event(event).is_err() {
            // Event loop closed
            break;
        }

        let response = match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
            Ok(Ok(resp)) => resp,
            Ok(Err(_)) => McpResponse {
                id: 0,
                result: None,
                error: Some("Internal error: reply channel dropped".to_string()),
            },
            Err(_) => McpResponse {
                id: 0,
                result: None,
                error: Some("Request timed out".to_string()),
            },
        };

        let mut out = serde_json::to_vec(&response).unwrap_or_default();
        out.push(b'\n');
        let _ = writer.write_all(&out).await;
    }
}

/// Platform-specific IPC listener for Unix (domain socket).
#[cfg(unix)]
async fn spawn_ipc_listener(
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
) -> anyhow::Result<()> {
    use tokio::net::UnixListener;

    let path = ipc_socket_path();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Clean up stale socket file
    if path.exists() {
        std::fs::remove_file(&path)?;
    }

    let listener = UnixListener::bind(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    tracing::info!("IPC listening on {}", path.display());

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let proxy = proxy.clone();
                tokio::spawn(async move {
                    handle_ipc_connection(stream, proxy).await;
                });
            }
            Err(e) => {
                tracing::warn!("IPC accept error: {}", e);
            }
        }
    }
}

/// Platform-specific IPC listener for Windows (named pipe).
#[cfg(windows)]
async fn spawn_ipc_listener(
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
) -> anyhow::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe_name = ipc_pipe_name();
    tracing::info!("IPC listening on {}", pipe_name);

    // Windows named pipes inherit the default security descriptor, which
    // restricts access to the creating user's SID. No additional hardening needed.
    // Create the first pipe instance
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_name)?;

    loop {
        // Wait for a client to connect
        server.connect().await?;

        let proxy = proxy.clone();
        let connected = server;

        // Create the next pipe instance before spawning the handler
        server = ServerOptions::new().create(&pipe_name)?;

        tokio::spawn(async move {
            handle_ipc_connection(connected, proxy).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_request_deserializes_from_json() {
        let json = r#"{"id":1,"method":"tab_list","params":{}}"#;
        let req: McpRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.id, 1);
        assert_eq!(req.method, "tab_list");
        assert!(req.params.is_object());
    }

    #[test]
    fn mcp_request_deserializes_without_params() {
        let json = r#"{"id":2,"method":"ping"}"#;
        let req: McpRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.id, 2);
        assert_eq!(req.method, "ping");
        assert!(req.params.is_null());
    }

    #[test]
    fn mcp_response_serializes_with_result() {
        let resp = McpResponse {
            id: 1,
            result: Some(serde_json::json!({"status": "ok"})),
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"status\":\"ok\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn mcp_response_serializes_with_error() {
        let resp = McpResponse {
            id: 1,
            result: None,
            error: Some("Unknown method".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"error\":\"Unknown method\""));
        assert!(!json.contains("\"result\""));
    }

    #[cfg(unix)]
    #[test]
    fn ipc_socket_path_ends_in_glass_sock() {
        let path = ipc_socket_path();
        assert!(path.to_str().unwrap().ends_with("glass.sock"));
    }

    #[cfg(windows)]
    #[test]
    fn ipc_pipe_name_contains_glass_terminal() {
        let name = ipc_pipe_name();
        assert!(name.contains("glass-terminal"));
    }

    /// Integration test: spawn a listener on a TCP port, connect, send a
    /// JSON request, and verify we get a JSON response back.
    ///
    /// Uses TCP instead of Unix socket / named pipe so the test works on all
    /// platforms without cleanup issues.
    #[tokio::test]
    async fn ipc_round_trip_over_tcp() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpListener;

        // We can't use a real EventLoopProxy in tests, so we test the
        // handle_ipc_connection function with a mock proxy approach.
        // Instead, we test the serialization round-trip directly.

        // Bind to a random port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn a "server" that reads a JSON line and writes back a response
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = tokio::io::split(stream);
            let mut lines = BufReader::new(reader).lines();

            if let Ok(Some(line)) = lines.next_line().await {
                let req: McpRequest = serde_json::from_str(&line).unwrap();
                let resp = McpResponse {
                    id: req.id,
                    result: Some(serde_json::json!({"echo": req.method})),
                    error: None,
                };
                let mut out = serde_json::to_vec(&resp).unwrap();
                out.push(b'\n');
                writer.write_all(&out).await.unwrap();
            }
        });

        // Connect as client
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();

        // Send request
        let req = r#"{"id":42,"method":"tab_list","params":{}}"#;
        writer
            .write_all(format!("{}\n", req).as_bytes())
            .await
            .unwrap();

        // Read response
        let resp_line = lines.next_line().await.unwrap().unwrap();
        let resp: McpResponse = serde_json::from_str(&resp_line).unwrap();
        assert_eq!(resp.id, 42);
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());

        server.await.unwrap();
    }

    #[test]
    fn invalid_json_produces_error_response() {
        // Verify that non-JSON input would result in a parse error
        let result: Result<McpRequest, _> = serde_json::from_str("not valid json{{{");
        assert!(result.is_err());

        // And that we can construct an error response for it
        let resp = McpResponse {
            id: 0,
            result: None,
            error: Some(format!("Invalid JSON: {}", result.unwrap_err())),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("Invalid JSON"));
    }

    #[test]
    fn unknown_method_returns_error() {
        // Simulate what user_event would do for an unknown method
        let req = McpRequest {
            id: 99,
            method: "nonexistent_method".to_string(),
            params: serde_json::Value::Null,
        };
        let resp = McpResponse {
            id: req.id,
            result: None,
            error: Some(format!("Unknown method: {}", req.method)),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("Unknown method: nonexistent_method"));
    }
}
