use crate::types::{Pipeline, PipeStage, PipelineClassification};

/// Split a command string into pipe stages.
///
/// Respects single quotes, double quotes, backslash escapes, backtick escapes,
/// parenthesis depth, and distinguishes `|` (pipe) from `||` (logical OR).
/// Returns trimmed slices for each stage.
pub fn split_pipes(_command: &str) -> Vec<&str> {
    todo!("Implemented in Task 2")
}

/// Parse a command string into a Pipeline with typed stages.
///
/// Calls split_pipes to find stage boundaries, then tokenizes each stage
/// with shlex to extract the program name (first token, path-stripped).
pub fn parse_pipeline(_command: &str) -> Pipeline {
    todo!("Implemented in Task 2")
}
