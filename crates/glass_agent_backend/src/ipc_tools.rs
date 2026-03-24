//! Synchronous IPC client for calling Glass MCP tools from API backends.
//!
//! Provides a blocking interface to the Glass GUI's IPC listener
//! (Unix domain socket or Windows named pipe), suitable for use from
//! the backend's conversation thread (a regular OS thread, not async).

use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// A request sent to the Glass GUI over IPC.
#[derive(Debug, Serialize)]
struct IpcRequest {
    id: u64,
    method: String,
    params: serde_json::Value,
}

/// A response received from the Glass GUI over IPC.
#[derive(Debug, Deserialize)]
struct IpcResponse {
    result: Option<serde_json::Value>,
    error: Option<String>,
}

/// Synchronous IPC client for communicating with the Glass GUI process.
///
/// Creates a fresh connection per request (handles GUI restarts).
/// Construction is cheap — no eager connection.
pub struct SyncIpcClient {
    next_id: AtomicU64,
}

impl Default for SyncIpcClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncIpcClient {
    /// Create a new synchronous IPC client. Does not connect eagerly.
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
        }
    }

    /// Call a Glass MCP tool by name and return the result.
    ///
    /// Opens a fresh connection per call. Returns `Err` with a human-readable
    /// message if the GUI is not running or the request times out.
    pub fn call_tool(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let request = IpcRequest {
            id,
            method: method.to_string(),
            params,
        };

        let mut stream = connect().map_err(|e| format!("Glass GUI is not running ({})", e))?;

        set_read_timeout(&stream)?;

        // Serialize request as a JSON line
        let mut payload = serde_json::to_vec(&request)
            .map_err(|e| format!("Failed to serialize request: {}", e))?;
        payload.push(b'\n');

        stream
            .write_all(&payload)
            .map_err(|e| format!("Failed to send request: {}", e))?;

        // Read response line
        let mut response_line = String::new();
        BufReader::new(&mut stream)
            .read_line(&mut response_line)
            .map_err(|e| format!("Failed to read response: {}", e))?;

        if response_line.is_empty() {
            return Err("Connection closed before response".to_string());
        }

        let resp: IpcResponse = serde_json::from_str(response_line.trim_end())
            .map_err(|e| format!("Invalid response JSON: {}", e))?;

        if let Some(err) = resp.error {
            return Err(err);
        }

        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }
}

// ---------------------------------------------------------------------------
// Platform-specific helpers
// ---------------------------------------------------------------------------

/// Opaque stream type used internally (type alias per platform).
#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;

/// Opaque stream type used internally (type alias per platform).
#[cfg(windows)]
type Stream = std::fs::File;

/// Connect to the Glass GUI's IPC listener.
///
/// On Unix this opens a Unix domain socket at `~/.glass/glass.sock`.
/// On Windows this opens the named pipe `\\.\pipe\glass-terminal`.
#[cfg(unix)]
fn connect() -> Result<Stream, String> {
    let path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".glass")
        .join("glass.sock");
    std::os::unix::net::UnixStream::connect(&path)
        .map_err(|e| format!("{}: {}", path.display(), e))
}

/// Connect to the Glass GUI's IPC listener.
///
/// On Unix this opens a Unix domain socket at `~/.glass/glass.sock`.
/// On Windows this opens the named pipe `\\.\pipe\glass-terminal`.
#[cfg(windows)]
fn connect() -> Result<Stream, String> {
    let pipe_name = r"\\.\pipe\glass-terminal";
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name)
        .map_err(|e| format!("{}: {}", pipe_name, e))
}

/// Set a 5-second read timeout on the stream.
///
/// On Unix: calls `set_read_timeout`.
/// On Windows: named pipes do not support `set_read_timeout`; this is a no-op.
#[cfg(unix)]
fn set_read_timeout(stream: &Stream) -> Result<(), String> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("Failed to set read timeout: {}", e))
}

/// Set a 5-second read timeout on the stream.
///
/// On Unix: calls `set_read_timeout`.
/// On Windows: named pipes do not support `set_read_timeout`; this is a no-op.
#[cfg(windows)]
fn set_read_timeout(_stream: &Stream) -> Result<(), String> {
    // Named pipes do not support set_read_timeout directly; rely on the
    // caller's thread-level timeout mechanism if needed.
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_creates_without_connecting() {
        // SyncIpcClient::new() should always succeed, even without a running GUI.
        let client = SyncIpcClient::new();
        assert_eq!(client.next_id.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn call_tool_returns_error_when_gui_not_running() {
        // Skip if the Glass GUI is actually running — the connection will
        // succeed, which invalidates the test premise.
        if connect().is_ok() {
            return;
        }
        let client = SyncIpcClient::new();
        let result = client.call_tool("ping", serde_json::json!({}));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Glass GUI is not running"),
            "Expected 'Glass GUI is not running' but got: {}",
            err
        );
    }
}
