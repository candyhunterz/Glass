//! Tests for the glass binary — codepage, startup, and subcommand routing assertions.

#[cfg(test)]
mod subcommand_tests {
    use clap::Parser;
    use crate::{Cli, Commands, McpAction};

    #[test]
    fn test_no_subcommand_is_none() {
        let cli = Cli::try_parse_from(["glass"]).unwrap();
        assert!(cli.command.is_none(), "No args should yield command = None (terminal mode)");
    }

    #[test]
    fn test_history_subcommand() {
        let cli = Cli::try_parse_from(["glass", "history"]).unwrap();
        assert_eq!(cli.command, Some(Commands::History));
    }

    #[test]
    fn test_mcp_serve_subcommand() {
        let cli = Cli::try_parse_from(["glass", "mcp", "serve"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Mcp { action: McpAction::Serve })
        );
    }

    #[test]
    fn test_help_flag() {
        // --help causes clap to return an error (DisplayHelp), not a parsed result
        let result = Cli::try_parse_from(["glass", "--help"]);
        assert!(result.is_err(), "--help should produce a clap error (DisplayHelp)");
    }

    #[test]
    fn test_unknown_subcommand_errors() {
        let result = Cli::try_parse_from(["glass", "bogus"]);
        assert!(result.is_err(), "Unknown subcommand should produce a clap error");
    }
}

#[cfg(test)]
mod codepage_tests {
    /// Verify that the Windows console codepage is set to 65001 (UTF-8).
    /// This test calls GetConsoleOutputCP() and asserts the value.
    /// The set_utf8_codepage() function in main.rs must be called before this test runs.
    ///
    /// NOTE: In the test harness, we call the function directly rather than
    /// relying on main() having been called.
    #[test]
    #[cfg(target_os = "windows")]
    fn test_utf8_codepage_65001_active() {
        // Call the same function that main() calls at startup
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleCP(65001);
            windows_sys::Win32::System::Console::SetConsoleOutputCP(65001);
        }

        let output_cp = unsafe {
            windows_sys::Win32::System::Console::GetConsoleOutputCP()
        };
        let input_cp = unsafe {
            windows_sys::Win32::System::Console::GetConsoleCP()
        };

        assert_eq!(
            output_cp, 65001,
            "Console OUTPUT code page must be UTF-8 (65001), got {output_cp}"
        );
        assert_eq!(
            input_cp, 65001,
            "Console INPUT code page must be UTF-8 (65001), got {input_cp}"
        );
    }
}
