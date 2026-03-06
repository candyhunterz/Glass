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
    /// Stub: just extends head with data (no overflow logic yet).
    /// Full implementation in Plan 02 Task 2.
    pub fn append(&mut self, data: &[u8]) {
        self.total_bytes += data.len();
        self.head.extend_from_slice(data);
    }

    /// Finalize the buffer into a FinalizedBuffer.
    ///
    /// Stub: always returns Complete (no overflow/binary detection yet).
    /// Full implementation in Plan 02 Task 2.
    pub fn finalize(self) -> FinalizedBuffer {
        FinalizedBuffer::Complete(self.head)
    }
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
