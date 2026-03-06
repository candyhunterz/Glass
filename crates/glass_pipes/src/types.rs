/// A parsed pipeline with its stages and classification.
#[derive(Debug, Clone)]
pub struct Pipeline {
    /// Original full command text
    pub raw_command: String,
    /// Individual pipe stages
    pub stages: Vec<PipeStage>,
    /// Classification result (set after classification pass)
    pub classification: PipelineClassification,
}

/// A single stage in a pipeline (one command between pipe operators).
#[derive(Debug, Clone)]
pub struct PipeStage {
    /// The raw command text for this stage (trimmed)
    pub command: String,
    /// Index of this stage (0-based)
    pub index: usize,
    /// The base command name (first token, path-stripped)
    pub program: String,
    /// Whether this stage's program is TTY-sensitive
    pub is_tty: bool,
}

/// Classification of a pipeline for capture decisions.
#[derive(Debug, Clone)]
pub struct PipelineClassification {
    /// Whether any stage contains a TTY-sensitive command
    pub has_tty_command: bool,
    /// Which stages are TTY-sensitive (by index)
    pub tty_stages: Vec<usize>,
    /// Whether --no-glass opt-out flag is present
    pub opted_out: bool,
    /// Whether the pipeline should be captured
    pub should_capture: bool,
}

impl Default for PipelineClassification {
    fn default() -> Self {
        Self {
            has_tty_command: false,
            tty_stages: Vec::new(),
            opted_out: false,
            should_capture: true,
        }
    }
}

/// Policy controlling buffer size limits for stage capture.
#[derive(Debug, Clone)]
pub struct BufferPolicy {
    /// Maximum bytes before switching to head/tail sampling (default 10MB)
    pub max_bytes: usize,
    /// Size of head and tail samples when overflowed (default 512KB)
    pub sample_size: usize,
}

impl Default for BufferPolicy {
    fn default() -> Self {
        Self {
            max_bytes: 10 * 1024 * 1024,     // 10MB
            sample_size: 512 * 1024,          // 512KB
        }
    }
}

impl BufferPolicy {
    pub fn new(max_bytes: usize, sample_size: usize) -> Self {
        Self {
            max_bytes,
            sample_size,
        }
    }
}

/// Buffer that accumulates bytes for a single pipe stage.
///
/// Captures data up to the policy limit, then switches to head/tail
/// sampling mode. Full overflow logic implemented in Plan 02.
#[derive(Debug, Clone)]
pub struct StageBuffer {
    /// First bytes of captured data
    pub head: Vec<u8>,
    /// Last bytes of captured data (used in overflow mode)
    pub tail: Vec<u8>,
    /// Total bytes seen across all append calls
    pub total_bytes: usize,
    /// Buffer size policy
    pub policy: BufferPolicy,
    /// Whether we have exceeded the max_bytes limit
    pub overflow: bool,
}

/// Detect whether the given bytes are likely binary content.
///
/// Samples the first 8KB and returns true if more than 30% of bytes
/// are non-text control characters (excluding tab, newline, carriage return).
fn is_binary_data(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    let sample = &data[..data.len().min(8192)];
    let non_text = sample
        .iter()
        .filter(|&&b| b < 0x08 || (b >= 0x0E && b <= 0x1F))
        .count();
    non_text as f64 / sample.len() as f64 > 0.30
}

impl StageBuffer {
    pub fn new(policy: BufferPolicy) -> Self {
        Self {
            head: Vec::new(),
            tail: Vec::new(),
            total_bytes: 0,
            policy,
            overflow: false,
        }
    }

    /// Append data to the buffer.
    ///
    /// Accumulates into `head` until `max_bytes` is exceeded, then switches
    /// to overflow mode: head is truncated to `sample_size` and tail becomes
    /// a rolling window of the latest `sample_size` bytes.
    pub fn append(&mut self, data: &[u8]) {
        self.total_bytes += data.len();

        if !self.overflow {
            self.head.extend_from_slice(data);
            if self.head.len() > self.policy.max_bytes {
                // Transition to overflow mode
                self.overflow = true;
                // Keep the tail from the overflow data
                let all_data_len = self.head.len();
                if all_data_len > self.policy.sample_size {
                    let tail_start = all_data_len.saturating_sub(self.policy.sample_size);
                    self.tail = self.head[tail_start..].to_vec();
                }
                // Truncate head to sample_size
                self.head.truncate(self.policy.sample_size);
            }
        } else {
            // In overflow mode: extend tail, then trim front if too large
            self.tail.extend_from_slice(data);
            if self.tail.len() > self.policy.sample_size {
                let excess = self.tail.len() - self.policy.sample_size;
                self.tail.drain(..excess);
            }
        }
    }

    /// Finalize the buffer into a FinalizedBuffer.
    ///
    /// Checks for binary data first, then returns the appropriate variant
    /// based on whether overflow occurred.
    pub fn finalize(self) -> FinalizedBuffer {
        // Check binary on head (first bytes of data)
        if is_binary_data(&self.head) {
            return FinalizedBuffer::Binary { size: self.total_bytes };
        }

        if self.overflow {
            FinalizedBuffer::Sampled {
                head: self.head,
                tail: self.tail,
                total_bytes: self.total_bytes,
            }
        } else {
            FinalizedBuffer::Complete(self.head)
        }
    }

    /// Get total bytes seen across all append calls.
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }
}

/// Captured data for a single pipeline stage, received from shell integration.
#[derive(Debug, Clone)]
pub struct CapturedStage {
    /// Stage index (0-based)
    pub index: usize,
    /// Total bytes the stage produced (before any buffering)
    pub total_bytes: usize,
    /// Finalized buffer data (read from temp file, processed through StageBuffer)
    pub data: FinalizedBuffer,
    /// Path to temp file containing raw stage output (if not yet read)
    pub temp_path: Option<String>,
}

/// Result of finalizing a stage buffer.
#[derive(Debug, Clone, PartialEq)]
pub enum FinalizedBuffer {
    /// All data fits in buffer
    Complete(Vec<u8>),
    /// Exceeded limit, head and tail samples retained
    Sampled {
        head: Vec<u8>,
        tail: Vec<u8>,
        total_bytes: usize,
    },
    /// Binary data detected
    Binary {
        size: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_policy() -> BufferPolicy {
        BufferPolicy::new(1024, 256) // Small sizes for testing
    }

    #[test]
    fn test_buffer_small_complete() {
        let mut buf = StageBuffer::new(test_policy());
        buf.append(&[0x41; 100]); // 100 bytes of 'A'
        let result = buf.finalize();
        assert_eq!(result, FinalizedBuffer::Complete(vec![0x41; 100]));
    }

    #[test]
    fn test_buffer_exact_limit_complete() {
        let mut buf = StageBuffer::new(test_policy());
        buf.append(&[0x41; 1024]); // Exactly max_bytes
        let result = buf.finalize();
        assert_eq!(result, FinalizedBuffer::Complete(vec![0x41; 1024]));
    }

    #[test]
    fn test_buffer_overflow_sampled() {
        let mut buf = StageBuffer::new(test_policy());
        // Append 1500 bytes (exceeds 1024 max), using distinct patterns
        let mut data = Vec::new();
        for i in 0u8..150 {
            data.extend_from_slice(&[i + 0x20; 10]); // 10 bytes each, printable
        }
        buf.append(&data); // 1500 bytes total
        assert_eq!(buf.total_bytes(), 1500);
        let result = buf.finalize();
        match result {
            FinalizedBuffer::Sampled { head, tail, total_bytes } => {
                assert_eq!(head.len(), 256); // sample_size
                assert_eq!(tail.len(), 256); // sample_size
                assert_eq!(total_bytes, 1500);
                // Head should be first 256 bytes
                assert_eq!(head, data[..256]);
                // Tail should be last 256 bytes
                assert_eq!(tail, data[data.len() - 256..]);
            }
            other => panic!("Expected Sampled, got {:?}", other),
        }
    }

    #[test]
    fn test_buffer_overflow_multiple_appends() {
        let mut buf = StageBuffer::new(test_policy());
        // First append: 800 bytes (under limit)
        buf.append(&[0x41; 800]);
        // Second append: 400 bytes (pushes over 1024 limit)
        buf.append(&[0x42; 400]);
        // Third append: 300 bytes (already in overflow, extends tail)
        buf.append(&[0x43; 300]);

        assert_eq!(buf.total_bytes(), 1500);
        let result = buf.finalize();
        match result {
            FinalizedBuffer::Sampled { head, tail, total_bytes } => {
                assert_eq!(head.len(), 256);
                assert_eq!(total_bytes, 1500);
                // Head should be first 256 bytes (all 'A')
                assert!(head.iter().all(|&b| b == 0x41));
                // Tail should be last 256 bytes -- the rolling window
                // After second append triggers overflow, tail gets last 256 of 1200 bytes
                // Then third append adds 300, tail grows to 556, drains to 256
                // So tail should be the last 256 bytes of the third chunk
                assert_eq!(tail.len(), 256);
            }
            other => panic!("Expected Sampled, got {:?}", other),
        }
    }

    #[test]
    fn test_buffer_sampled_head_is_first_bytes() {
        let mut buf = StageBuffer::new(test_policy());
        let mut data = vec![0x61; 256]; // 'a' for first 256
        data.extend_from_slice(&[0x62; 1244]); // 'b' for rest (total 1500)
        buf.append(&data);
        let result = buf.finalize();
        match result {
            FinalizedBuffer::Sampled { head, .. } => {
                assert!(head.iter().all(|&b| b == 0x61)); // First 256 = all 'a'
            }
            other => panic!("Expected Sampled, got {:?}", other),
        }
    }

    #[test]
    fn test_buffer_sampled_tail_is_last_bytes() {
        let mut buf = StageBuffer::new(test_policy());
        let mut data = vec![0x61; 1244]; // 'a' for first part
        data.extend_from_slice(&[0x62; 256]); // 'b' for last 256 (total 1500)
        buf.append(&data);
        let result = buf.finalize();
        match result {
            FinalizedBuffer::Sampled { tail, .. } => {
                assert!(tail.iter().all(|&b| b == 0x62)); // Last 256 = all 'b'
            }
            other => panic!("Expected Sampled, got {:?}", other),
        }
    }

    #[test]
    fn test_buffer_binary_detection() {
        let mut buf = StageBuffer::new(test_policy());
        // >30% non-text bytes (control chars in 0x01..0x07 range)
        let mut data = vec![0x01; 50]; // 50 non-text bytes
        data.extend_from_slice(&[0x41; 50]); // 50 printable bytes
        // 50% non-text -> binary
        buf.append(&data);
        let result = buf.finalize();
        assert_eq!(result, FinalizedBuffer::Binary { size: 100 });
    }

    #[test]
    fn test_buffer_binary_check_uses_8kb_sample() {
        let mut buf = StageBuffer::new(BufferPolicy::new(100_000, 256));
        // First 8KB is text, rest is binary-like
        let mut data = vec![0x41; 9000]; // 'A' text
        data.extend_from_slice(&[0x01; 9000]); // binary
        buf.append(&data);
        let result = buf.finalize();
        // is_binary_data only samples first 8192 bytes, which are all text
        match result {
            FinalizedBuffer::Complete(_) => {} // Expected: text detected
            other => panic!("Expected Complete (text first 8KB), got {:?}", other),
        }
    }

    #[test]
    fn test_buffer_empty_complete() {
        let buf = StageBuffer::new(test_policy());
        let result = buf.finalize();
        assert_eq!(result, FinalizedBuffer::Complete(vec![]));
    }

    #[test]
    fn test_buffer_multiple_small_appends() {
        let mut buf = StageBuffer::new(test_policy());
        buf.append(&[0x41; 50]);
        buf.append(&[0x42; 50]);
        buf.append(&[0x43; 50]);
        assert_eq!(buf.total_bytes(), 150);
        let result = buf.finalize();
        match result {
            FinalizedBuffer::Complete(data) => {
                assert_eq!(data.len(), 150);
                assert_eq!(&data[..50], &[0x41; 50]);
                assert_eq!(&data[50..100], &[0x42; 50]);
                assert_eq!(&data[100..], &[0x43; 50]);
            }
            other => panic!("Expected Complete, got {:?}", other),
        }
    }

    #[test]
    fn test_buffer_tail_rolling_window() {
        let mut buf = StageBuffer::new(test_policy());
        // Push over the limit
        buf.append(&[0x41; 1100]); // Triggers overflow
        // Now in overflow mode, append several chunks
        buf.append(&[0x42; 200]); // tail grows
        buf.append(&[0x43; 200]); // tail should roll, keeping latest 256

        let result = buf.finalize();
        match result {
            FinalizedBuffer::Sampled { tail, .. } => {
                assert_eq!(tail.len(), 256);
                // Last 200 bytes should be 0x43, preceding should be 0x42
                assert!(tail[56..].iter().all(|&b| b == 0x43));
                assert!(tail[..56].iter().all(|&b| b == 0x42));
            }
            other => panic!("Expected Sampled, got {:?}", other),
        }
    }

    // -- is_binary_data unit tests --

    #[test]
    fn test_is_binary_data_text() {
        assert!(!is_binary_data(b"hello world\n\ttab"));
    }

    #[test]
    fn test_is_binary_data_binary() {
        let data: Vec<u8> = (0..100).map(|i| if i < 40 { 0x01 } else { 0x41 }).collect();
        assert!(is_binary_data(&data)); // 40% non-text
    }

    #[test]
    fn test_is_binary_data_empty() {
        assert!(!is_binary_data(b""));
    }

    #[test]
    fn test_captured_stage_fields() {
        let stage = CapturedStage {
            index: 2,
            total_bytes: 4096,
            data: FinalizedBuffer::Complete(vec![0x41; 100]),
            temp_path: Some("/tmp/glass/stage_2".to_string()),
        };
        assert_eq!(stage.index, 2);
        assert_eq!(stage.total_bytes, 4096);
        assert!(matches!(stage.data, FinalizedBuffer::Complete(_)));
        assert_eq!(stage.temp_path, Some("/tmp/glass/stage_2".to_string()));
    }

    #[test]
    fn test_captured_stage_no_temp_path() {
        let stage = CapturedStage {
            index: 0,
            total_bytes: 1024,
            data: FinalizedBuffer::Binary { size: 1024 },
            temp_path: None,
        };
        assert_eq!(stage.index, 0);
        assert!(stage.temp_path.is_none());
    }
}
