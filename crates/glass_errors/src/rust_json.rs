//! Rust JSON diagnostic parser (cargo + raw rustc).

use crate::StructuredError;

/// Parse Rust JSON diagnostic output into structured errors.
pub(crate) fn parse_rust_json(_output: &str) -> Vec<StructuredError> {
    Vec::new() // Stub - implemented in Task 2
}
