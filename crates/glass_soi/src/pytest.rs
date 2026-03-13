//! Parser for `pytest` / `python -m pytest` output.
//!
//! Stub implementation — Plan 48-03 will implement full parsing.

use crate::types::{OutputType, ParsedOutput};

/// Parse pytest output into structured records.
///
/// Currently delegates to the freeform fallback.
/// Plan 48-03 will implement test-result and summary parsing.
pub fn parse(output: &str) -> ParsedOutput {
    crate::freeform_parse(output, Some(OutputType::Pytest), None)
}
