//! Parser for `jest` / `npx jest` output.
//!
//! Stub implementation — Plan 48-03 will implement full parsing.

use crate::types::{OutputType, ParsedOutput};

/// Parse Jest output into structured records.
///
/// Currently delegates to the freeform fallback.
/// Plan 48-03 will implement test-result and summary parsing.
pub fn parse(output: &str) -> ParsedOutput {
    crate::freeform_parse(output, Some(OutputType::Jest), None)
}
