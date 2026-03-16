//! Parser for single JSON object/array output.
//!
//! Performs structural parsing of JSON data — extracts top-level keys,
//! array lengths, and nested structure summaries. Distinct from `json_lines`
//! which handles newline-delimited JSON.

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

/// Describe the type of a JSON value briefly.
fn value_type_name(val: &serde_json::Value) -> &'static str {
    match val {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Parse single JSON object/array output into structural summary records.
pub fn parse(output: &str) -> ParsedOutput {
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();
    let trimmed = output.trim();

    let val: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return crate::freeform_parse(output, Some(OutputType::JsonObject), None),
    };

    let mut records: Vec<OutputRecord> = Vec::new();

    let one_line = match &val {
        serde_json::Value::Object(map) => {
            let key_count = map.len();
            let keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
            let key_display = if keys.len() > 5 {
                let shown: Vec<&str> = keys.iter().take(5).copied().collect();
                format!("{}, ...", shown.join(", "))
            } else {
                keys.join(", ")
            };

            records.push(OutputRecord::GenericDiagnostic {
                file: None,
                line: None,
                severity: Severity::Info,
                message: format!("JSON object with {key_count} keys: {key_display}"),
            });

            for (key, value) in map.iter().take(10) {
                records.push(OutputRecord::GenericDiagnostic {
                    file: None,
                    line: None,
                    severity: Severity::Info,
                    message: format!("  {key}: {}", value_type_name(value)),
                });
            }

            format!("JSON object ({key_count} keys)")
        }
        serde_json::Value::Array(arr) => {
            let len = arr.len();
            records.push(OutputRecord::GenericDiagnostic {
                file: None,
                line: None,
                severity: Severity::Info,
                message: format!("JSON array with {len} elements"),
            });

            if let Some(serde_json::Value::Object(first_map)) = arr.first() {
                let common_keys: Vec<&str> = first_map.keys().map(|k| k.as_str()).collect();
                if !common_keys.is_empty() {
                    let display = if common_keys.len() > 5 {
                        let shown: Vec<&str> = common_keys.iter().take(5).copied().collect();
                        format!("{}, ...", shown.join(", "))
                    } else {
                        common_keys.join(", ")
                    };
                    records.push(OutputRecord::GenericDiagnostic {
                        file: None,
                        line: None,
                        severity: Severity::Info,
                        message: format!("  element keys: {display}"),
                    });
                }
            }

            format!("JSON array ({len} elements)")
        }
        other => {
            let type_name = value_type_name(other);
            records.push(OutputRecord::GenericDiagnostic {
                file: None,
                line: None,
                severity: Severity::Info,
                message: format!("JSON {type_name}"),
            });
            format!("JSON {type_name}")
        }
    };

    let token_estimate = one_line.split_whitespace().count() + records.len() * 5 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::JsonObject,
        summary: OutputSummary {
            one_line,
            token_estimate,
            severity: Severity::Info,
        },
        records,
        raw_line_count,
        raw_byte_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_object_basic() {
        let output = r#"{"name": "Alice", "age": 30}"#;
        let parsed = parse(output);
        assert_eq!(parsed.output_type, OutputType::JsonObject);
        assert!(parsed.summary.one_line.contains("2 keys"));
    }

    #[test]
    fn json_array_of_objects() {
        let output = r#"[{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]"#;
        let parsed = parse(output);
        assert!(parsed.summary.one_line.contains("2 elements"));
        let has_keys_msg = parsed.records.iter().any(|r| {
            matches!(r, OutputRecord::GenericDiagnostic { message, .. } if message.contains("element keys"))
        });
        assert!(has_keys_msg, "should report element keys");
    }

    #[test]
    fn json_nested() {
        let output = r#"{"user": {"name": "Alice"}, "settings": {"theme": "dark"}}"#;
        let parsed = parse(output);
        assert!(parsed.summary.one_line.contains("2 keys"));
    }

    #[test]
    fn json_invalid_fallback() {
        let output = "this is not json {{{";
        let parsed = parse(output);
        assert_eq!(parsed.output_type, OutputType::JsonObject);
        assert!(matches!(
            parsed.records[0],
            OutputRecord::FreeformChunk { .. }
        ));
    }

    #[test]
    fn json_pretty_printed() {
        let output = "{\n  \"name\": \"Alice\",\n  \"age\": 30,\n  \"active\": true\n}\n";
        let parsed = parse(output);
        assert!(parsed.summary.one_line.contains("3 keys"));
    }
}
