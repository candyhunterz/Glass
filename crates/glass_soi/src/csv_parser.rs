//! Parser for CSV/TSV tabular output.
//!
//! Performs structural parsing of CSV/TSV data — detects delimiter,
//! extracts column headers, and reports row counts. Not a full CSV parser.

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

/// Detect whether the dominant delimiter is tab or comma.
fn detect_delimiter(lines: &[&str]) -> char {
    let mut commas: usize = 0;
    let mut tabs: usize = 0;
    for line in lines.iter().take(5) {
        commas += line.chars().filter(|&c| c == ',').count();
        tabs += line.chars().filter(|&c| c == '\t').count();
    }
    if tabs > commas {
        '\t'
    } else {
        ','
    }
}

/// Split a row by delimiter, respecting basic double-quote quoting.
fn split_row(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in line.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == delimiter && !in_quotes {
            fields.push(current.trim().to_string());
            current = String::new();
        } else {
            current.push(ch);
        }
    }
    fields.push(current.trim().to_string());
    fields
}

/// Parse CSV/TSV output into structural summary records.
pub fn parse(output: &str) -> ParsedOutput {
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();

    let lines: Vec<&str> = output.lines().collect();

    if lines.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::Csv), None);
    }

    let delimiter = detect_delimiter(&lines);
    let format_name = if delimiter == '\t' { "TSV" } else { "CSV" };

    let headers = split_row(lines[0], delimiter);
    let col_count = headers.len();
    let data_rows = if lines.len() > 1 { lines.len() - 1 } else { 0 };

    let col_display = if col_count > 5 {
        let shown: Vec<&str> = headers.iter().take(5).map(|s| s.as_str()).collect();
        format!("{}, ...", shown.join(", "))
    } else {
        headers.join(", ")
    };

    let mut records: Vec<OutputRecord> = Vec::new();

    records.push(OutputRecord::GenericDiagnostic {
        file: None,
        line: None,
        severity: Severity::Info,
        message: format!(
            "{format_name}: {col_count} columns ({col_display}), {data_rows} data rows"
        ),
    });

    let mut inconsistent = false;
    for line in lines.iter().skip(1) {
        let row = split_row(line, delimiter);
        if row.len() != col_count {
            inconsistent = true;
            break;
        }
    }
    if inconsistent {
        records.push(OutputRecord::GenericDiagnostic {
            file: None,
            line: None,
            severity: Severity::Info,
            message: "Some rows have inconsistent column counts".to_string(),
        });
    }

    let one_line = format!("{format_name}: {col_count} columns, {data_rows} rows");
    let token_estimate = one_line.split_whitespace().count() + records.len() * 5 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::Csv,
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
    fn csv_basic() {
        let output = "name,age,city\nAlice,30,NYC\nBob,25,LA\n";
        let parsed = parse(output);
        assert_eq!(parsed.output_type, OutputType::Csv);
        assert!(parsed.summary.one_line.contains("3 columns"));
        assert!(parsed.summary.one_line.contains("2 rows"));
    }

    #[test]
    fn tsv_detection() {
        let output = "name\tage\tcity\nAlice\t30\tNYC\n";
        let parsed = parse(output);
        assert!(parsed.summary.one_line.starts_with("TSV"));
    }

    #[test]
    fn csv_single_header_row() {
        let output = "name,age,city\n";
        let parsed = parse(output);
        assert!(parsed.summary.one_line.contains("0 rows"));
    }

    #[test]
    fn csv_many_columns() {
        let output = "a,b,c,d,e,f,g\n1,2,3,4,5,6,7\n";
        let parsed = parse(output);
        assert!(parsed.summary.one_line.contains("7 columns"));
        if let OutputRecord::GenericDiagnostic { message, .. } = &parsed.records[0] {
            assert!(message.contains("..."), "should truncate: {message}");
        }
    }

    #[test]
    fn empty_fallback() {
        let parsed = parse("");
        assert_eq!(parsed.output_type, OutputType::Csv);
        assert!(matches!(
            parsed.records[0],
            OutputRecord::FreeformChunk { .. }
        ));
    }
}
