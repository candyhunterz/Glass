//! OSC sequence scanner for shell integration hooks.
//!
//! Parses OSC 133 (shell integration A/B/C/D), OSC 7 (CWD file:// URL),
//! and OSC 9;9 (ConEmu CWD) from raw byte streams. Handles sequences
//! split across buffer boundaries.

/// Simple percent-decoding for file paths (spaces, etc.)
fn percent_decode_str(input: &str) -> String {
    let mut result = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                result.push(val);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(result).unwrap_or_else(|_| input.to_string())
}

/// Events produced by scanning OSC sequences from PTY output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscEvent {
    /// OSC 133;A - Shell prompt has started
    PromptStart,
    /// OSC 133;B - User input / command line has started
    CommandStart,
    /// OSC 133;C - Command is being executed
    CommandExecuted,
    /// OSC 133;D[;exit_code] - Command finished with optional exit code
    CommandFinished { exit_code: Option<i32> },
    /// OSC 7 or OSC 9;9 - Current working directory changed
    CurrentDirectory(String),
    /// OSC 133;S;{stage_count} - Pipeline with N stages detected
    PipelineStart { stage_count: usize },
    /// OSC 133;P;{index};{total_bytes};{temp_path} - Stage data available in temp file
    PipelineStage {
        index: usize,
        total_bytes: usize,
        temp_path: String,
    },
}

/// Internal state of the byte-level scanner.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ScanState {
    Ground,
    Escape, // saw \x1b
    Accumulating,
}

/// Byte-level OSC sequence scanner with split-buffer support.
///
/// Feed arbitrary chunks of PTY output via `scan()` and collect
/// recognized `OscEvent`s. State is preserved across calls to
/// handle sequences that span buffer boundaries.
pub struct OscScanner {
    state: ScanState,
    buffer: Vec<u8>,
    prev_was_esc: bool, // for detecting ST (\x1b\\) terminator while accumulating
}

impl OscScanner {
    pub fn new() -> Self {
        Self {
            state: ScanState::Ground,
            buffer: Vec::with_capacity(256),
            prev_was_esc: false,
        }
    }

    /// Scan a chunk of bytes and return any recognized OSC events.
    #[cfg_attr(feature = "perf", tracing::instrument(skip_all, level = "trace"))]
    pub fn scan(&mut self, data: &[u8]) -> Vec<OscEvent> {
        let mut events = Vec::new();

        for &byte in data {
            match self.state {
                ScanState::Ground => {
                    if byte == 0x1b {
                        self.state = ScanState::Escape;
                    }
                    // All other bytes in Ground are ignored (pass-through)
                }
                ScanState::Escape => {
                    if byte == b']' {
                        self.state = ScanState::Accumulating;
                        self.buffer.clear();
                        self.prev_was_esc = false;
                    } else {
                        // Not an OSC start, go back to ground
                        self.state = ScanState::Ground;
                    }
                }
                ScanState::Accumulating => {
                    if byte == 0x07 {
                        // BEL terminator
                        if let Some(event) = Self::parse_payload(&self.buffer) {
                            events.push(event);
                        }
                        self.buffer.clear();
                        self.prev_was_esc = false;
                        self.state = ScanState::Ground;
                    } else if byte == b'\\' && self.prev_was_esc {
                        // ST terminator (\x1b\\) — the \x1b was already pushed,
                        // remove it before parsing
                        self.buffer.pop(); // remove the \x1b we pushed
                        if let Some(event) = Self::parse_payload(&self.buffer) {
                            events.push(event);
                        }
                        self.buffer.clear();
                        self.prev_was_esc = false;
                        self.state = ScanState::Ground;
                    } else {
                        self.prev_was_esc = byte == 0x1b;
                        self.buffer.push(byte);
                    }
                }
            }
        }

        events
    }

    /// Parse accumulated OSC payload bytes into an event.
    fn parse_payload(payload: &[u8]) -> Option<OscEvent> {
        let text = std::str::from_utf8(payload).ok()?;

        // Split on first ';' to get the OSC number
        let (osc_num, rest) = match text.find(';') {
            Some(pos) => (&text[..pos], &text[pos + 1..]),
            None => (text, ""),
        };

        match osc_num {
            "133" => Self::parse_osc133(rest),
            "7" => Self::parse_osc7(rest),
            "9" => Self::parse_osc9(rest),
            _ => None,
        }
    }

    /// Parse OSC 133 shell integration sequences.
    fn parse_osc133(params: &str) -> Option<OscEvent> {
        // params is everything after "133;"
        // Could be "A", "B", "C", "D", "D;0", "D;1", etc.
        let mut parts = params.splitn(2, ';');
        let marker = parts.next()?;

        match marker {
            "A" => Some(OscEvent::PromptStart),
            "B" => Some(OscEvent::CommandStart),
            "C" => Some(OscEvent::CommandExecuted),
            "D" => {
                let exit_code = parts.next().and_then(|s| s.parse::<i32>().ok());
                Some(OscEvent::CommandFinished { exit_code })
            }
            "S" => {
                // Pipeline start: OSC 133;S;{count}
                let count = parts.next()?.parse::<usize>().ok()?;
                Some(OscEvent::PipelineStart { stage_count: count })
            }
            "P" => {
                // Pipeline stage: OSC 133;P;{index};{total_bytes};{temp_path}
                let rest = parts.next()?;
                let mut sub = rest.splitn(3, ';');
                let index = sub.next()?.parse::<usize>().ok()?;
                let total_bytes = sub.next()?.parse::<usize>().ok()?;
                let temp_path = sub.next()?.to_string();
                if temp_path.is_empty() {
                    return None;
                }
                Some(OscEvent::PipelineStage {
                    index,
                    total_bytes,
                    temp_path,
                })
            }
            _ => None,
        }
    }

    /// Parse OSC 7 file:// CWD URL.
    fn parse_osc7(params: &str) -> Option<OscEvent> {
        // params is the URL, e.g. "file://HOST/C:/Users/foo"
        if let Ok(url) = url::Url::parse(params) {
            if url.scheme() == "file" {
                let path = url.path();
                // On Windows, url crate produces /C:/Users/foo — strip leading /
                let path = if path.len() >= 3
                    && path.as_bytes()[0] == b'/'
                    && path.as_bytes()[2] == b':'
                {
                    &path[1..]
                } else {
                    path
                };
                // Decode percent-encoding
                let decoded = percent_decode_str(path);
                return Some(OscEvent::CurrentDirectory(decoded));
            }
        }
        None
    }

    /// Parse OSC 9;9 ConEmu CWD.
    fn parse_osc9(params: &str) -> Option<OscEvent> {
        // params after "9;" should be "9;path"
        params
            .strip_prefix("9;")
            .map(|path| OscEvent::CurrentDirectory(path.to_string()))
    }
}

impl Default for OscScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_start() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]133;A\x07");
        assert_eq!(events, vec![OscEvent::PromptStart]);
    }

    #[test]
    fn command_start() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]133;B\x07");
        assert_eq!(events, vec![OscEvent::CommandStart]);
    }

    #[test]
    fn command_executed() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]133;C\x07");
        assert_eq!(events, vec![OscEvent::CommandExecuted]);
    }

    #[test]
    fn command_finished_with_exit_code_zero() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]133;D;0\x07");
        assert_eq!(
            events,
            vec![OscEvent::CommandFinished { exit_code: Some(0) }]
        );
    }

    #[test]
    fn command_finished_with_exit_code_one() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]133;D;1\x07");
        assert_eq!(
            events,
            vec![OscEvent::CommandFinished { exit_code: Some(1) }]
        );
    }

    #[test]
    fn command_finished_no_exit_code() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]133;D\x07");
        assert_eq!(events, vec![OscEvent::CommandFinished { exit_code: None }]);
    }

    #[test]
    fn current_directory_osc7_bel() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]7;file://HOST/C:/Users/foo\x07");
        assert_eq!(events.len(), 1);
        match &events[0] {
            OscEvent::CurrentDirectory(path) => {
                assert_eq!(path, "C:/Users/foo");
            }
            other => panic!("Expected CurrentDirectory, got {:?}", other),
        }
    }

    #[test]
    fn current_directory_osc7_st_terminator() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]7;file://HOST/C:/Users/foo\x1b\\");
        assert_eq!(events.len(), 1);
        match &events[0] {
            OscEvent::CurrentDirectory(path) => {
                assert_eq!(path, "C:/Users/foo");
            }
            other => panic!("Expected CurrentDirectory, got {:?}", other),
        }
    }

    #[test]
    fn split_buffer_at_payload() {
        let mut s = OscScanner::new();
        let events1 = s.scan(b"\x1b]133;");
        assert!(events1.is_empty(), "No event before terminator");
        let events2 = s.scan(b"A\x07");
        assert_eq!(events2, vec![OscEvent::PromptStart]);
    }

    #[test]
    fn split_buffer_mid_osc() {
        let mut s = OscScanner::new();
        let events1 = s.scan(b"\x1b]");
        assert!(events1.is_empty());
        let events2 = s.scan(b"133;C\x07");
        assert_eq!(events2, vec![OscEvent::CommandExecuted]);
    }

    #[test]
    fn non_osc_data_no_events() {
        let mut s = OscScanner::new();
        let events = s.scan(b"Hello, World!\r\n");
        assert!(events.is_empty());
    }

    #[test]
    fn interleaved_normal_and_osc() {
        let mut s = OscScanner::new();
        let events = s.scan(b"before\x1b]133;A\x07middle\x1b]133;B\x07after");
        assert_eq!(events, vec![OscEvent::PromptStart, OscEvent::CommandStart]);
    }

    #[test]
    fn pipeline_start_variant_exists() {
        let event = OscEvent::PipelineStart { stage_count: 3 };
        match event {
            OscEvent::PipelineStart { stage_count } => assert_eq!(stage_count, 3),
            _ => panic!("Expected PipelineStart"),
        }
    }

    #[test]
    fn pipeline_stage_variant_exists() {
        let event = OscEvent::PipelineStage {
            index: 0,
            total_bytes: 1024,
            temp_path: "/tmp/glass/stage_0".into(),
        };
        match event {
            OscEvent::PipelineStage {
                index,
                total_bytes,
                temp_path,
            } => {
                assert_eq!(index, 0);
                assert_eq!(total_bytes, 1024);
                assert_eq!(temp_path, "/tmp/glass/stage_0");
            }
            _ => panic!("Expected PipelineStage"),
        }
    }

    #[test]
    fn osc_9_9_conemu_cwd() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]9;9;C:\\Users\\foo\x07");
        assert_eq!(events.len(), 1);
        match &events[0] {
            OscEvent::CurrentDirectory(path) => {
                assert_eq!(path, "C:\\Users\\foo");
            }
            other => panic!("Expected CurrentDirectory, got {:?}", other),
        }
    }

    #[test]
    fn parse_pipeline_start_bel() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]133;S;3\x07");
        assert_eq!(events, vec![OscEvent::PipelineStart { stage_count: 3 }]);
    }

    #[test]
    fn parse_pipeline_stage_st_terminator() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]133;P;0;1024;/tmp/glass/stage_0\x1b\\");
        assert_eq!(
            events,
            vec![OscEvent::PipelineStage {
                index: 0,
                total_bytes: 1024,
                temp_path: "/tmp/glass/stage_0".into(),
            }]
        );
    }

    #[test]
    fn parse_pipeline_stage_windows_path() {
        let mut s = OscScanner::new();
        let events =
            s.scan(b"\x1b]133;P;2;5000;C:/Users/test/AppData/Local/Temp/glass_1234/stage_2\x07");
        assert_eq!(
            events,
            vec![OscEvent::PipelineStage {
                index: 2,
                total_bytes: 5000,
                temp_path: "C:/Users/test/AppData/Local/Temp/glass_1234/stage_2".into(),
            }]
        );
    }

    #[test]
    fn pipeline_interleaved_with_normal() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]133;A\x07\x1b]133;S;2\x07\x1b]133;C\x07");
        assert_eq!(
            events,
            vec![
                OscEvent::PromptStart,
                OscEvent::PipelineStart { stage_count: 2 },
                OscEvent::CommandExecuted,
            ]
        );
    }

    #[test]
    fn invalid_pipeline_start_missing_count() {
        let mut s = OscScanner::new();
        let events = s.scan(b"\x1b]133;S\x07");
        assert!(events.is_empty());
    }

    #[test]
    fn invalid_pipeline_stage_missing_fields() {
        let mut s = OscScanner::new();
        // Only index, no total_bytes or temp_path
        let events = s.scan(b"\x1b]133;P;0\x07");
        assert!(events.is_empty());
    }

    /// Validates the pipeline contract: S;N announces N pipe boundaries,
    /// followed by exactly N stage data events (indices 0..N-1).
    /// This matches shell integration behavior where `a | b | c` has 2 pipe
    /// boundaries and produces tee files for stages 0 and 1.
    #[test]
    fn pipeline_stage_count_matches_stage_events() {
        let mut s = OscScanner::new();
        // Simulate `a | b | c`: 2 pipe boundaries, 2 tee files (stage 0, stage 1)
        let events = s.scan(
            b"\x1b]133;S;2\x07\
              \x1b]133;P;0;100;/tmp/glass/stage_0\x07\
              \x1b]133;P;1;200;/tmp/glass/stage_1\x07",
        );
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], OscEvent::PipelineStart { stage_count: 2 });
        // Verify stage indices are sequential 0..stage_count
        for i in 0..2 {
            match &events[i + 1] {
                OscEvent::PipelineStage { index, .. } => assert_eq!(*index, i),
                other => panic!("Expected PipelineStage, got {:?}", other),
            }
        }
    }

    /// Pipeline with many stages (>10) to verify no arbitrary limits.
    #[test]
    fn pipeline_many_stages() {
        let mut s = OscScanner::new();
        let mut input = b"\x1b]133;S;12\x07".to_vec();
        for i in 0..12 {
            input.extend_from_slice(
                format!(
                    "\x1b]133;P;{};{};/tmp/glass/stage_{}\x07",
                    i,
                    (i + 1) * 50,
                    i
                )
                .as_bytes(),
            );
        }
        let events = s.scan(&input);
        // 1 PipelineStart + 12 PipelineStage
        assert_eq!(events.len(), 13);
        assert_eq!(events[0], OscEvent::PipelineStart { stage_count: 12 });
        for i in 0..12 {
            match &events[i + 1] {
                OscEvent::PipelineStage {
                    index,
                    total_bytes,
                    temp_path,
                } => {
                    assert_eq!(*index, i);
                    assert_eq!(*total_bytes, (i + 1) * 50);
                    assert_eq!(*temp_path, format!("/tmp/glass/stage_{}", i));
                }
                other => panic!("Expected PipelineStage at {}, got {:?}", i, other),
            }
        }
    }

    /// Both BEL and ST terminators work for pipeline sequences (PowerShell
    /// uses BEL, bash uses ST).
    #[test]
    fn pipeline_bel_and_st_terminators() {
        let mut s = OscScanner::new();
        // S with BEL, P with ST
        let events = s.scan(b"\x1b]133;S;1\x07\x1b]133;P;0;50;/tmp/stage_0\x1b\\");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], OscEvent::PipelineStart { stage_count: 1 });
        match &events[1] {
            OscEvent::PipelineStage {
                index, temp_path, ..
            } => {
                assert_eq!(*index, 0);
                assert_eq!(*temp_path, "/tmp/stage_0");
            }
            other => panic!("Expected PipelineStage, got {:?}", other),
        }
    }
}
