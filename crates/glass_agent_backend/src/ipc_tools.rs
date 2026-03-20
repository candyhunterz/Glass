//! Synchronous IPC client for calling Glass MCP tools from API backends.
//!
//! Provides a blocking interface to the Glass GUI's IPC listener
//! (Unix domain socket or Windows named pipe), suitable for use from
//! the backend's conversation thread (a regular OS thread, not async).
