//! Output capture buffer for accumulating PTY bytes during command execution.
//!
//! `OutputBuffer` lives in the PTY reader thread and accumulates bytes
//! between `CommandExecuted` and `CommandFinished` shell integration events.
//! Alternate-screen applications (vim, less, top) are detected via raw byte
//! scanning and their output is excluded from capture.

/// Buffer that accumulates PTY output bytes during command execution.
pub struct OutputBuffer;

impl OutputBuffer {
    /// Create a new buffer with the given maximum byte capacity.
    pub fn new(_max_bytes: usize) -> Self {
        Self
    }

    /// Begin capturing output. Clears any previous data.
    pub fn start_capture(&mut self) {}

    /// Set alt-screen state directly.
    pub fn set_alt_screen(&mut self, _active: bool) {}

    /// Scan raw bytes for alt-screen enter/leave escape sequences.
    pub fn check_alt_screen(&mut self, _data: &[u8]) {}

    /// Append bytes to the buffer if currently capturing.
    pub fn append(&mut self, _data: &[u8]) {}

    /// Finish capture and return accumulated bytes (or None).
    pub fn finish(&mut self) -> Option<Vec<u8>> {
        None
    }

    /// Total bytes seen during this capture session (including those not buffered).
    pub fn total_seen(&self) -> usize {
        0
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
        assert_eq!(buf.total_seen(), 0); // total_seen resets after finish via start_capture
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
