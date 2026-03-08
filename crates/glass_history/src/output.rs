//! Output processing pipeline for captured command output.
//!
//! Provides ANSI stripping, binary detection, head+tail truncation,
//! and a unified `process_output` entry point.

/// Strip all ANSI escape sequences from raw bytes, returning a UTF-8 string.
pub fn strip_ansi(input: &[u8]) -> String {
    let stripped = strip_ansi_escapes::strip(input);
    String::from_utf8_lossy(&stripped).into_owned()
}

/// Detect whether the given bytes are likely binary content.
///
/// Samples the first 8KB and returns true if more than 30% of bytes
/// are non-printable (excluding `\n`, `\r`, `\t`).
pub fn is_binary(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    let sample = &data[..data.len().min(8192)];
    let non_printable = sample
        .iter()
        .filter(|&&b| b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t')
        .count();
    non_printable as f64 / sample.len() as f64 > 0.30
}

/// Truncate text using head+tail split if it exceeds `max_bytes`.
///
/// Keeps the first half and last half of the text with a
/// `[...truncated N bytes...]` marker in between. Respects UTF-8
/// character boundaries.
pub fn truncate_head_tail(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let half = max_bytes / 2;
    let skipped = text.len() - max_bytes;
    // Find safe UTF-8 boundaries
    let head_end = text.floor_char_boundary(half);
    let tail_start = text.ceil_char_boundary(text.len() - half);
    format!(
        "{}\n[...truncated {} bytes...]\n{}",
        &text[..head_end],
        skipped,
        &text[tail_start..],
    )
}

/// Process raw command output bytes into storable text.
///
/// Pipeline:
/// 1. `None` input -> `None` output (alt-screen / no capture)
/// 2. Strip ANSI escape sequences
/// 3. Detect binary content -> placeholder if binary
/// 4. Convert to UTF-8 (lossy)
/// 5. Truncate if exceeds `max_kb` kilobytes
pub fn process_output(raw: Option<Vec<u8>>, max_kb: u32) -> Option<String> {
    let raw = raw?;

    // Check for binary content before any processing
    if is_binary(&raw) {
        return Some(format!("[binary output: {} bytes]", raw.len()));
    }

    // Strip ANSI escape sequences
    let stripped = strip_ansi_escapes::strip(&raw);

    // Convert to UTF-8 (lossy)
    let text = String::from_utf8_lossy(&stripped).into_owned();

    // Truncate if exceeds max size
    let max_bytes = (max_kb as usize) * 1024;
    if text.len() > max_bytes {
        Some(truncate_head_tail(&text, max_bytes))
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- strip_ansi tests --

    #[test]
    fn test_strip_ansi_basic_color() {
        assert_eq!(strip_ansi(b"\x1b[31mhello\x1b[0m"), "hello");
    }

    #[test]
    fn test_strip_ansi_no_escapes() {
        assert_eq!(strip_ansi(b"no escapes"), "no escapes");
    }

    #[test]
    fn test_strip_ansi_24bit_color() {
        assert_eq!(strip_ansi(b"\x1b[38;2;255;0;0mcolor\x1b[0m"), "color");
    }

    #[test]
    fn test_strip_ansi_empty() {
        assert_eq!(strip_ansi(b""), "");
    }

    // -- is_binary tests --

    #[test]
    fn test_is_binary_normal_text() {
        assert!(!is_binary(b"hello world\n"));
    }

    #[test]
    fn test_is_binary_high_nonprintable() {
        // >30% non-printable bytes (excluding \n\r\t)
        let mut data = vec![0u8; 40]; // 40 null bytes
        data.extend_from_slice(b"hello world this is normal text here!!!!"); // 40 printable
                                                                             // 40/80 = 50% non-printable -> binary
        assert!(is_binary(&data));
    }

    #[test]
    fn test_is_binary_empty() {
        assert!(!is_binary(b""));
    }

    #[test]
    fn test_is_binary_whitespace_ok() {
        // \n, \r, \t should NOT count as non-printable
        assert!(!is_binary(b"line1\nline2\rline3\ttab"));
    }

    // -- truncate_head_tail tests --

    #[test]
    fn test_truncate_short_text() {
        assert_eq!(truncate_head_tail("short", 1000), "short");
    }

    #[test]
    fn test_truncate_long_text() {
        let text = "a".repeat(100);
        let result = truncate_head_tail(&text, 50);
        assert!(result.contains("[...truncated"));
        assert!(result.contains("bytes...]"));
        // Should have head + marker + tail
        assert!(result.len() < 120); // much less than original + marker overhead
    }

    #[test]
    fn test_truncate_preserves_utf8() {
        // Multi-byte chars: each is 4 bytes
        let text = "\u{1F600}".repeat(30); // 120 bytes of emoji
        let result = truncate_head_tail(&text, 50);
        // Should not panic and should be valid UTF-8
        assert!(result.contains("[...truncated"));
    }

    // -- process_output tests --

    #[test]
    fn test_process_output_none() {
        assert_eq!(process_output(None, 50), None);
    }

    #[test]
    fn test_process_output_normal() {
        let raw = b"hello world\n".to_vec();
        let result = process_output(Some(raw), 50);
        assert_eq!(result, Some("hello world\n".to_string()));
    }

    #[test]
    fn test_process_output_binary() {
        let mut data = vec![0u8; 100]; // all null bytes = binary
        data.extend_from_slice(b"some text");
        let result = process_output(Some(data.clone()), 50);
        let text = result.unwrap();
        assert!(text.starts_with("[binary output:"));
        assert!(text.contains("bytes]"));
    }

    #[test]
    fn test_process_output_large_truncated() {
        let raw = b"x".repeat(2048); // 2KB
        let result = process_output(Some(raw), 1); // max 1KB
        let text = result.unwrap();
        assert!(text.contains("[...truncated"));
    }

    #[test]
    fn test_process_output_strips_ansi() {
        let raw = b"\x1b[31mhello\x1b[0m".to_vec();
        let result = process_output(Some(raw), 50);
        assert_eq!(result, Some("hello".to_string()));
    }
}
