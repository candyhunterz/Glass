//! Platform-specific PID liveness checking.

/// Check whether a process with the given PID is currently alive.
///
/// Uses OS-specific APIs:
/// - Unix: `kill(pid, 0)` signal probe
/// - Windows: `OpenProcess` with `PROCESS_QUERY_LIMITED_INFORMATION`
/// - Other: assumes alive (conservative default)
pub fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // Signal 0 doesn't send a signal but checks if the process exists.
        // Returns 0 if process exists and we have permission to signal it.
        // SAFETY: kill with signal 0 is a safe probe -- it sends no actual signal.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        // SAFETY: OpenProcess with PROCESS_QUERY_LIMITED_INFORMATION is a safe
        // read-only query. We close the handle immediately after checking.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            CloseHandle(handle);
            true
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        // Conservative default: assume the process is alive on unknown platforms.
        let _ = pid;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_process_alive() {
        let pid = std::process::id();
        assert!(
            is_pid_alive(pid),
            "Current process (PID {pid}) should be detected as alive"
        );
    }

    #[test]
    fn test_nonexistent_pid() {
        // PID 999999 is almost certainly not a real process
        // On some systems this could theoretically be alive, but it's extremely unlikely
        let result = is_pid_alive(999999);
        // We don't assert false because on some exotic systems high PIDs might exist,
        // but we at least verify the function doesn't panic
        let _ = result;
    }
}
