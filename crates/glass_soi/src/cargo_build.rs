//! Parser for `cargo build` / `cargo check` / `cargo clippy` output.
//!
//! Stub implementation — Plan 48-02 will implement full parsing.

use crate::types::{OutputType, ParsedOutput};

/// Parse Rust compiler output into structured records.
///
/// Currently delegates to the freeform fallback.
/// Plan 48-02 will implement JSON-line and human-readable parsing.
pub fn parse(output: &str, command_hint: Option<&str>) -> ParsedOutput {
    crate::freeform_parse(output, Some(OutputType::RustCompiler), command_hint)
}
