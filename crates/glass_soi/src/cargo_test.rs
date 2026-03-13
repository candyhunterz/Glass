//! Parser for `cargo test` output.
//!
//! Stub implementation — Plan 48-02 will implement full parsing.

use crate::types::{OutputType, ParsedOutput};

/// Parse `cargo test` output into structured records.
///
/// Currently delegates to the freeform fallback.
/// Plan 48-02 will implement test-result and summary parsing.
pub fn parse(output: &str) -> ParsedOutput {
    crate::freeform_parse(output, Some(OutputType::RustTest), None)
}
