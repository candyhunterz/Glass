//! Parser for `npm` / `npx` output.
//!
//! Stub implementation — Plan 48-03 will implement full parsing.

use crate::types::{OutputType, ParsedOutput};

/// Parse npm output into structured records.
///
/// Currently delegates to the freeform fallback.
/// Plan 48-03 will implement package-event parsing.
pub fn parse(output: &str) -> ParsedOutput {
    crate::freeform_parse(output, Some(OutputType::Npm), None)
}
