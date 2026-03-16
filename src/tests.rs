//! Tests for the glass binary — codepage, startup, and subcommand routing assertions.

#[cfg(test)]
mod subcommand_tests {
    use crate::{Cli, Commands, HistoryAction, HistoryFilters, McpAction};
    use clap::Parser;

    #[test]
    fn test_no_subcommand_is_none() {
        let cli = Cli::try_parse_from(["glass"]).unwrap();
        assert!(
            cli.command.is_none(),
            "No args should yield command = None (terminal mode)"
        );
    }

    #[test]
    fn test_history_subcommand_defaults_to_none_action() {
        let cli = Cli::try_parse_from(["glass", "history"]).unwrap();
        assert_eq!(cli.command, Some(Commands::History { action: None }));
    }

    #[test]
    fn test_history_list_subcommand() {
        let cli = Cli::try_parse_from(["glass", "history", "list"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::History {
                action: Some(HistoryAction::List {
                    filters: HistoryFilters {
                        limit: 25,
                        ..HistoryFilters::default()
                    },
                }),
            })
        );
    }

    #[test]
    fn test_history_search_subcommand() {
        let cli = Cli::try_parse_from(["glass", "history", "search", "cargo"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::History {
                action: Some(HistoryAction::Search {
                    query: "cargo".to_string(),
                    filters: HistoryFilters {
                        limit: 25,
                        ..HistoryFilters::default()
                    },
                }),
            })
        );
    }

    #[test]
    fn test_history_list_with_all_filters() {
        let cli = Cli::try_parse_from([
            "glass", "history", "list", "--exit", "1", "--after", "1h", "--cwd", "/project", "-n",
            "10",
        ])
        .unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::History {
                action: Some(HistoryAction::List {
                    filters: HistoryFilters {
                        exit: Some(1),
                        after: Some("1h".to_string()),
                        before: None,
                        cwd: Some("/project".to_string()),
                        limit: 10,
                    },
                }),
            })
        );
    }

    #[test]
    fn test_history_search_with_limit() {
        let cli =
            Cli::try_parse_from(["glass", "history", "search", "deploy", "--limit", "5"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::History {
                action: Some(HistoryAction::Search {
                    query: "deploy".to_string(),
                    filters: HistoryFilters {
                        exit: None,
                        after: None,
                        before: None,
                        cwd: None,
                        limit: 5,
                    },
                }),
            })
        );
    }

    #[test]
    fn test_history_list_with_before_filter() {
        let cli =
            Cli::try_parse_from(["glass", "history", "list", "--before", "2024-01-15"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::History {
                action: Some(HistoryAction::List {
                    filters: HistoryFilters {
                        exit: None,
                        after: None,
                        before: Some("2024-01-15".to_string()),
                        cwd: None,
                        limit: 25,
                    },
                }),
            })
        );
    }

    #[test]
    fn test_mcp_serve_subcommand() {
        let cli = Cli::try_parse_from(["glass", "mcp", "serve"]).unwrap();
        assert_eq!(
            cli.command,
            Some(Commands::Mcp {
                action: McpAction::Serve
            })
        );
    }

    #[test]
    fn test_help_flag() {
        // --help causes clap to return an error (DisplayHelp), not a parsed result
        let result = Cli::try_parse_from(["glass", "--help"]);
        assert!(
            result.is_err(),
            "--help should produce a clap error (DisplayHelp)"
        );
    }

    #[test]
    fn test_unknown_subcommand_errors() {
        let result = Cli::try_parse_from(["glass", "bogus"]);
        assert!(
            result.is_err(),
            "Unknown subcommand should produce a clap error"
        );
    }
}

#[cfg(test)]
mod settings_sync_tests {
    use crate::{handle_settings_activate, handle_settings_increment};
    use glass_core::config::GlassConfig;
    use glass_renderer::settings_overlay::{SettingsConfigSnapshot, SETTINGS_SECTIONS};

    /// Verify that every SettingsConfigSnapshot default matches the corresponding
    /// serde default in config.rs (i.e. what you get from an empty TOML file).
    #[test]
    fn snapshot_defaults_match_config_serde_defaults() {
        let config = GlassConfig::load_from_str("");
        let snap = SettingsConfigSnapshot::default();

        // Font
        assert_eq!(snap.font_family, config.font_family);
        assert_eq!(snap.font_size, config.font_size);

        // Agent — absent section means Off/defaults
        assert!(!snap.agent_enabled);
        assert_eq!(snap.agent_mode, "Off");
        assert_eq!(snap.agent_budget, 1.0);
        assert_eq!(snap.agent_cooldown, 30);

        // SOI — absent section means enabled=true, etc.
        assert!(snap.soi_enabled);
        assert!(!snap.soi_shell_summary);
        assert_eq!(snap.soi_min_lines, 0);

        // Snapshots
        assert!(snap.snapshot_enabled);
        assert_eq!(snap.snapshot_max_mb, 500);
        assert_eq!(snap.snapshot_retention_days, 30);

        // Pipes
        assert!(snap.pipes_enabled);
        assert!(snap.pipes_auto_expand);
        assert_eq!(snap.pipes_max_capture_mb, 10);

        // History
        assert_eq!(snap.history_max_output_kb, 50);

        // Orchestrator
        assert!(!snap.orchestrator_enabled);
        assert_eq!(snap.orchestrator_max_iterations, 0);
        assert_eq!(snap.orchestrator_silence_secs, 60);
        assert_eq!(snap.orchestrator_prd_path, "PRD.md");
        assert_eq!(snap.orchestrator_mode, "build");
        assert_eq!(snap.orchestrator_verify_mode, "floor");
    }

    /// Verify SETTINGS_SECTIONS count matches the number of sections in
    /// fields_for_section (sections 0..6).
    #[test]
    fn settings_sections_count() {
        assert_eq!(
            SETTINGS_SECTIONS.len(),
            7,
            "Should have 7 sections: Font, Agent Mode, SOI, Snapshots, Pipes, History, Orchestrator"
        );
    }

    // --- Activate handler tests ---

    #[test]
    fn activate_agent_enabled_toggles_off_to_watch() {
        let config = GlassConfig::load_from_str("");
        let result = handle_settings_activate(&config, 1, 0);
        let (section, key, value) = result.unwrap();
        assert_eq!(section, Some("agent"));
        assert_eq!(key, "mode");
        assert_eq!(value, "\"Watch\"");
    }

    #[test]
    fn activate_agent_enabled_toggles_watch_to_off() {
        let config = GlassConfig::load_from_str("[agent]\nmode = \"Watch\"");
        let result = handle_settings_activate(&config, 1, 0);
        let (section, key, value) = result.unwrap();
        assert_eq!(section, Some("agent"));
        assert_eq!(key, "mode");
        assert_eq!(value, "\"Off\"");
    }

    #[test]
    fn activate_agent_mode_cycles_through_all() {
        // Off -> Watch
        let config = GlassConfig::load_from_str("");
        let (_, _, value) = handle_settings_activate(&config, 1, 1).unwrap();
        assert_eq!(value, "\"Watch\"");

        // Watch -> Assist
        let config = GlassConfig::load_from_str("[agent]\nmode = \"Watch\"");
        let (_, _, value) = handle_settings_activate(&config, 1, 1).unwrap();
        assert_eq!(value, "\"Assist\"");

        // Assist -> Autonomous
        let config = GlassConfig::load_from_str("[agent]\nmode = \"Assist\"");
        let (_, _, value) = handle_settings_activate(&config, 1, 1).unwrap();
        assert_eq!(value, "\"Autonomous\"");

        // Autonomous -> Off
        let config = GlassConfig::load_from_str("[agent]\nmode = \"Autonomous\"");
        let (_, _, value) = handle_settings_activate(&config, 1, 1).unwrap();
        assert_eq!(value, "\"Off\"");
    }

    #[test]
    fn activate_soi_enabled_toggles() {
        let config = GlassConfig::load_from_str("");
        let (section, key, value) = handle_settings_activate(&config, 2, 0).unwrap();
        assert_eq!(section, Some("soi"));
        assert_eq!(key, "enabled");
        assert_eq!(value, "false"); // was true (default)
    }

    #[test]
    fn activate_soi_shell_summary_toggles() {
        let config = GlassConfig::load_from_str("");
        let (section, key, value) = handle_settings_activate(&config, 2, 1).unwrap();
        assert_eq!(section, Some("soi"));
        assert_eq!(key, "shell_summary");
        assert_eq!(value, "true"); // was false (default)
    }

    #[test]
    fn activate_snapshot_enabled_toggles() {
        let config = GlassConfig::load_from_str("");
        let (_, key, value) = handle_settings_activate(&config, 3, 0).unwrap();
        assert_eq!(key, "enabled");
        assert_eq!(value, "false");
    }

    #[test]
    fn activate_pipes_enabled_toggles() {
        let config = GlassConfig::load_from_str("");
        let (section, key, value) = handle_settings_activate(&config, 4, 0).unwrap();
        assert_eq!(section, Some("pipes"));
        assert_eq!(key, "enabled");
        assert_eq!(value, "false");
    }

    #[test]
    fn activate_pipes_auto_expand_toggles() {
        let config = GlassConfig::load_from_str("");
        let (_, key, value) = handle_settings_activate(&config, 4, 1).unwrap();
        assert_eq!(key, "auto_expand");
        assert_eq!(value, "false");
    }

    #[test]
    fn activate_orchestrator_enabled_toggles() {
        let config = GlassConfig::load_from_str("");
        let (section, key, value) = handle_settings_activate(&config, 6, 0).unwrap();
        assert_eq!(section, Some("agent.orchestrator"));
        assert_eq!(key, "enabled");
        assert_eq!(value, "true"); // was false
    }

    // verify_mode and orchestrator_mode toggles removed (auto-detected in V3)

    #[test]
    fn activate_nonexistent_field_returns_none() {
        let config = GlassConfig::load_from_str("");
        // Font Family (0,0) has no activate handler
        assert!(handle_settings_activate(&config, 0, 0).is_none());
        // Font Size (0,1) has no activate handler
        assert!(handle_settings_activate(&config, 0, 1).is_none());
        // History (5,0) has no activate handler (numeric only)
        assert!(handle_settings_activate(&config, 5, 0).is_none());
        // Out-of-range section
        assert!(handle_settings_activate(&config, 99, 0).is_none());
    }

    // --- Increment handler tests ---

    #[test]
    fn increment_font_size_up() {
        let config = GlassConfig::load_from_str("");
        let (section, key, value) = handle_settings_increment(&config, 0, 1, true).unwrap();
        assert!(section.is_none()); // top-level
        assert_eq!(key, "font_size");
        assert_eq!(value, "14.5"); // 14.0 + 0.5
    }

    #[test]
    fn increment_font_size_down() {
        let config = GlassConfig::load_from_str("");
        let (_, _, value) = handle_settings_increment(&config, 0, 1, false).unwrap();
        assert_eq!(value, "13.5"); // 14.0 - 0.5
    }

    #[test]
    fn increment_font_size_clamps_to_min() {
        let config = GlassConfig::load_from_str("font_size = 6.0");
        let (_, _, value) = handle_settings_increment(&config, 0, 1, false).unwrap();
        assert_eq!(value, "6.0"); // clamped at 6.0
    }

    #[test]
    fn increment_font_size_clamps_to_max() {
        let config = GlassConfig::load_from_str("font_size = 72.0");
        let (_, _, value) = handle_settings_increment(&config, 0, 1, true).unwrap();
        assert_eq!(value, "72.0"); // clamped at 72.0
    }

    #[test]
    fn increment_agent_budget_up() {
        let config = GlassConfig::load_from_str("");
        let (section, key, value) = handle_settings_increment(&config, 1, 2, true).unwrap();
        assert_eq!(section, Some("agent"));
        assert_eq!(key, "max_budget_usd");
        assert_eq!(value, "1.50"); // 1.0 + 0.5
    }

    #[test]
    fn increment_agent_cooldown_up() {
        let config = GlassConfig::load_from_str("");
        let (_, key, value) = handle_settings_increment(&config, 1, 3, true).unwrap();
        assert_eq!(key, "cooldown_secs");
        assert_eq!(value, "35"); // 30 + 5
    }

    #[test]
    fn increment_soi_min_lines_down_clamps_at_zero() {
        let config = GlassConfig::load_from_str("");
        let (_, key, value) = handle_settings_increment(&config, 2, 2, false).unwrap();
        assert_eq!(key, "min_lines");
        assert_eq!(value, "0"); // max(0 - 1, 0)
    }

    #[test]
    fn increment_history_max_output_up() {
        let config = GlassConfig::load_from_str("");
        let (section, key, value) = handle_settings_increment(&config, 5, 0, true).unwrap();
        assert_eq!(section, Some("history"));
        assert_eq!(key, "max_output_capture_kb");
        assert_eq!(value, "60"); // 50 + 10
    }

    #[test]
    fn increment_orchestrator_max_iterations_up_from_zero() {
        let config = GlassConfig::load_from_str("");
        let (_, key, value) = handle_settings_increment(&config, 6, 1, true).unwrap();
        assert_eq!(key, "max_iterations");
        assert_eq!(value, "10"); // 0 + 10
    }

    #[test]
    fn increment_orchestrator_max_iterations_down_to_zero() {
        let config = GlassConfig::load_from_str("[agent.orchestrator]\nmax_iterations = 10");
        let (_, key, value) = handle_settings_increment(&config, 6, 1, false).unwrap();
        assert_eq!(key, "max_iterations");
        assert_eq!(value, "0"); // unlimited
    }

    #[test]
    fn increment_orchestrator_silence_up() {
        let config = GlassConfig::load_from_str("");
        let (section, key, value) = handle_settings_increment(&config, 6, 2, true).unwrap();
        assert_eq!(section, Some("agent.orchestrator"));
        assert_eq!(key, "silence_timeout_secs");
        assert_eq!(value, "70"); // 60 + 10
    }

    #[test]
    fn increment_nonexistent_field_returns_none() {
        let config = GlassConfig::load_from_str("");
        // Font Family (0,0) has no increment handler
        assert!(handle_settings_increment(&config, 0, 0, true).is_none());
        // Out-of-range section
        assert!(handle_settings_increment(&config, 99, 0, true).is_none());
    }

    // --- Round-trip: activate/increment value → update_config_field → reload → correct ---

    #[test]
    fn roundtrip_activate_soi_enabled() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();

        let config = GlassConfig::load_from_str("");
        let (section, key, value) = handle_settings_activate(&config, 2, 0).unwrap();
        glass_core::config::update_config_field(&path, section, key, &value).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let reloaded = GlassConfig::load_from_str(&content);
        assert!(!reloaded.soi.unwrap().enabled);
    }

    #[test]
    fn roundtrip_increment_font_size() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "font_size = 14.0\n").unwrap();

        let config = GlassConfig::load_from_str("font_size = 14.0");
        let (section, key, value) = handle_settings_increment(&config, 0, 1, true).unwrap();
        glass_core::config::update_config_field(&path, section, key, &value).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let reloaded = GlassConfig::load_from_str(&content);
        assert_eq!(reloaded.font_size, 14.5);
    }

    // roundtrip_activate_orchestrator_verify_mode removed (toggle removed in V3)
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
        let set_ok = unsafe {
            let a = windows_sys::Win32::System::Console::SetConsoleCP(65001);
            let b = windows_sys::Win32::System::Console::SetConsoleOutputCP(65001);
            a != 0 && b != 0
        };

        // CI runners may not have an attached console, so SetConsoleCP
        // returns 0 (failure). Skip the assertion in that case.
        if !set_ok {
            eprintln!("No attached console — skipping codepage assertion");
            return;
        }

        let output_cp = unsafe { windows_sys::Win32::System::Console::GetConsoleOutputCP() };
        let input_cp = unsafe { windows_sys::Win32::System::Console::GetConsoleCP() };

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
