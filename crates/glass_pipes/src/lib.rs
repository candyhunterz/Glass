//! glass_pipes — pipe-aware command parsing and stage management.
//!
//! Detects shell pipelines (`cmd1 | cmd2 | cmd3`), splits them into
//! individual stages, and provides types for tracking per-stage captured
//! output. Used by the PTY layer and MCP server to power Glass's visual
//! pipe inspection feature.

pub mod parser;
pub mod types;

pub use parser::{parse_pipeline, split_pipes};
pub use types::*;
