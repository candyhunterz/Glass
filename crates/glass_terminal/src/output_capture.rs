//! Output capture buffer for accumulating PTY bytes during command execution.
//!
//! `OutputBuffer` lives in the PTY reader thread and accumulates bytes
//! between `CommandExecuted` and `CommandFinished` shell integration events.
//! Alternate-screen applications (vim, less, top) are detected via raw byte
//! scanning and their output is excluded from capture.

/// Alt-screen enter sequence: `ESC[?1049h`
const ALT_SCREEN_ENTER: &[u8] = b"\x1b[?1049h";
/// Alt-screen leave sequence: `ESC[?1049l`
const ALT_SCREEN_LEAVE: &[u8] = b"\x1b[?1049l";

/// Buffer that accumulates PTY output bytes during command execution.
///
/// Lives entirely in the PTY reader thread -- no mutex needed.
/// Accumulates bytes between `CommandExecuted` and `CommandFinished`,
/// respecting a configurable maximum size and skipping alt-screen content.
pub struct OutputBuffer {
    buffer: Vec<u8>,
    capturing: bool,
    max_bytes: usize,
    total_seen: usize,
    alt_screen: bool,
    /// Tracks whether alt-screen was entered at any point during this capture.
    /// Once set, finish() returns None even if alt-screen was later exited.
    alt_screen_seen: bool,
}

impl OutputBuffer {
    /// Create a new buffer with the given maximum byte capacity.
    ///
    /// Pre-allocates `min(max_bytes, 65536)` to avoid massive allocs for high limits.
    pub fn new(max_bytes: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(max_bytes.min(65536)),
            capturing: false,
            max_bytes,
            total_seen: 0,
            alt_screen: false,
            alt_screen_seen: false,
        }
    }

    /// Begin capturing output. Clears any previous data and resets all state.
    pub fn start_capture(&mut self) {
        self.buffer.clear();
        self.capturing = true;
        self.total_seen = 0;
        self.alt_screen = false;
        self.alt_screen_seen = false;
    }

    /// Set alt-screen state directly.
    pub fn set_alt_screen(&mut self, active: bool) {
        self.alt_screen = active;
        if active {
            self.alt_screen_seen = true;
        }
    }

    /// Scan raw bytes for alt-screen enter/leave escape sequences.
    ///
    /// Detects `\x1b[?1049h` (enter) and `\x1b[?1049l` (leave) in the byte stream.
    /// These sequences are rarely split across read buffers.
    pub fn check_alt_screen(&mut self, data: &[u8]) {
        if data.len() >= ALT_SCREEN_ENTER.len() {
            for window in data.windows(ALT_SCREEN_ENTER.len()) {
                if window == ALT_SCREEN_ENTER {
                    self.alt_screen = true;
                    self.alt_screen_seen = true;
                } else if window == ALT_SCREEN_LEAVE {
                    self.alt_screen = false;
                }
            }
        }
    }

    /// Append bytes to the buffer if currently capturing.
    ///
    /// No-op when not capturing, when alt-screen is active, or when buffer is full.
    /// Always tracks `total_seen` for bytes that arrive while capturing.
    pub fn append(&mut self, data: &[u8]) {
        if !self.capturing || self.alt_screen {
            return;
        }
        self.total_seen += data.len();
        let remaining = self.max_bytes.saturating_sub(self.buffer.len());
        if remaining > 0 {
            let take = data.len().min(remaining);
            self.buffer.extend_from_slice(&data[..take]);
        }
    }

    /// Finish capture and return accumulated bytes.
    ///
    /// Returns `None` if not capturing or if alt-screen was entered during capture.
    /// Returns `Some(bytes)` otherwise (may be empty if command produced no output).
    /// Resets capturing state to false.
    pub fn finish(&mut self) -> Option<Vec<u8>> {
        if !self.capturing {
            return None;
        }
        self.capturing = false;
        if self.alt_screen_seen {
            return None;
        }
        Some(std::mem::take(&mut self.buffer))
    }

    /// Total bytes seen during this capture session (including those not buffered).
    pub fn total_seen(&self) -> usize {
        self.total_seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_creates_non_capturing_buffer() {
        let mut buf = OutputBuffer::new(1024);
        // finish() on a non-capturing buffer returns None
        assert_eq!(buf.finish(), None);
    }

    #[test]
    fn test_start_capture_clears_previous() {
        let mut buf = OutputBuffer::new(1024);
        buf.start_capture();
        buf.append(b"first");
        // Start new capture — old data should be gone
        buf.start_capture();
        buf.append(b"second");
        let result = buf.finish().unwrap();
        assert_eq!(result, b"second");
    }

    #[test]
    fn test_append_accumulates_when_capturing() {
        let mut buf = OutputBuffer::new(1024);
        buf.start_capture();
        buf.append(b"hello ");
        buf.append(b"world");
        let result = buf.finish().unwrap();
        assert_eq!(result, b"hello world");
    }

    #[test]
    fn test_append_noop_when_not_capturing() {
        let mut buf = OutputBuffer::new(1024);
        // Not capturing — append should be a no-op
        buf.append(b"ignored");
        assert_eq!(buf.finish(), None);
    }

    #[test]
    fn test_append_noop_when_alt_screen() {
        let mut buf = OutputBuffer::new(1024);
        buf.start_capture();
        buf.set_alt_screen(true);
        buf.append(b"vim stuff");
        // Alt-screen was entered, so finish returns None
        assert_eq!(buf.finish(), None);
    }

    #[test]
    fn test_append_stops_after_max_bytes() {
        let mut buf = OutputBuffer::new(10);
        buf.start_capture();
        buf.append(b"12345678901234567890"); // 20 bytes, max is 10
        let result = buf.finish().unwrap();
        assert_eq!(result.len(), 10);
        assert_eq!(result, b"1234567890");
    }

    #[test]
    fn test_total_seen_tracks_all_bytes() {
        let mut buf = OutputBuffer::new(10);
        buf.start_capture();
        buf.append(b"12345678901234567890"); // 20 bytes
        assert_eq!(buf.total_seen(), 20);
    }

    #[test]
    fn test_check_alt_screen_detects_enter() {
        let mut buf = OutputBuffer::new(1024);
        buf.start_capture();
        buf.append(b"before");
        buf.check_alt_screen(b"\x1b[?1049h"); // enter alt screen
        buf.append(b"vim output");
        // Alt screen entered -> finish returns None
        assert_eq!(buf.finish(), None);
    }

    #[test]
    fn test_check_alt_screen_detects_leave() {
        let mut buf = OutputBuffer::new(1024);
        buf.start_capture();
        buf.check_alt_screen(b"\x1b[?1049h"); // enter
        buf.check_alt_screen(b"\x1b[?1049l"); // leave
        // Alt screen was entered during capture, finish still returns None
        // because the alt_screen was active at some point
        assert_eq!(buf.finish(), None);
    }

    #[test]
    fn test_finish_returns_none_when_not_capturing() {
        let mut buf = OutputBuffer::new(1024);
        assert_eq!(buf.finish(), None);
    }

    #[test]
    fn test_finish_resets_capturing() {
        let mut buf = OutputBuffer::new(1024);
        buf.start_capture();
        buf.append(b"data");
        let _ = buf.finish();
        // After finish, should no longer be capturing
        assert_eq!(buf.finish(), None);
    }

    #[test]
    fn test_check_alt_screen_embedded_in_data() {
        let mut buf = OutputBuffer::new(1024);
        buf.start_capture();
        // Alt-screen sequence embedded in other data
        let data = b"some text\x1b[?1049hmore text";
        buf.check_alt_screen(data);
        buf.append(b"after");
        assert_eq!(buf.finish(), None);
    }

    #[test]
    fn test_multiple_appends_with_cap() {
        let mut buf = OutputBuffer::new(10);
        buf.start_capture();
        buf.append(b"12345"); // 5 bytes
        buf.append(b"67890"); // 5 more = 10 total (at cap)
        buf.append(b"XXXXX"); // 5 more, all should be dropped
        let result = buf.finish().unwrap();
        assert_eq!(result, b"1234567890");
        assert_eq!(buf.total_seen(), 15); // total_seen retains value after finish
        // start_capture resets total_seen
        buf.start_capture();
        assert_eq!(buf.total_seen(), 0);
    }

    #[test]
    fn test_finish_returns_some_empty_when_no_output() {
        let mut buf = OutputBuffer::new(1024);
        buf.start_capture();
        // No append calls
        let result = buf.finish();
        assert_eq!(result, Some(vec![]));
    }
}
