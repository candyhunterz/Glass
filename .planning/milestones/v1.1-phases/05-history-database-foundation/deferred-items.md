# Deferred Items - Phase 05

## Pre-existing Issues (Not caused by current plan changes)

### 1. glass_history test_resolve_db_path_global_fallback fails on machines with ~/.glass/

- **File:** crates/glass_history/src/lib.rs:78
- **Issue:** Test expects global fallback to `~/.glass/global-history.db` but `resolve_db_path()` walks up from the temp dir and finds the real `~/.glass/` directory on the developer's machine, returning `~/.glass/history.db` instead.
- **Root cause:** Test does not mock or isolate from the real home directory filesystem.
- **Discovered during:** Plan 05-02, Task 1 workspace test verification
- **Fix suggestion:** Use environment variable override or pass home_dir as parameter for testability.
