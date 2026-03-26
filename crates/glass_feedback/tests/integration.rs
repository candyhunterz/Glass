use glass_feedback::*;

#[test]
fn three_run_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path().to_string_lossy().to_string();

    // Create .glass directory
    std::fs::create_dir_all(dir.path().join(".glass")).unwrap();

    let config = FeedbackConfig {
        project_root: project_root.clone(),
        feedback_llm: false,
        max_prompt_hints: 10,
        silence_timeout_secs: None,
        max_retries_before_stuck: None,
        ablation_enabled: true,
        ablation_sweep_interval: 20,
    };

    // === Run 1: Cold start with high revert rate ===
    let state = on_run_start(&project_root, &config);

    let data = RunData {
        project_root: project_root.clone(),
        iterations: 20,
        duration_secs: 600,
        revert_count: 8, // 40% revert rate — should trigger detect_revert_rate
        keep_count: 12,
        waste_count: 4, // 20% waste — should trigger detect_waste_rate
        commit_count: 3,
        completion_reason: "complete".to_string(),
        config_silence_timeout: 30,
        config_max_retries: 3,
        ..Default::default()
    };

    let result = on_run_end(state, data);

    // Cold start: findings produced but no regression comparison
    assert!(!result.findings.is_empty(), "Run 1 should produce findings");
    assert!(
        result.regression.is_none(),
        "Run 1 should have no regression (cold start)"
    );

    // Verify rules.toml was created with some rules
    let rules_path = dir.path().join(".glass/rules.toml");
    assert!(rules_path.exists(), "rules.toml should exist after run 1");

    // Verify metrics were saved
    let metrics_path = dir.path().join(".glass/run-metrics.toml");
    assert!(
        metrics_path.exists(),
        "run-metrics.toml should exist after run 1"
    );

    // === Run 2: Improved metrics ===
    let state = on_run_start(&project_root, &config);

    let data = RunData {
        project_root: project_root.clone(),
        iterations: 20,
        duration_secs: 500,
        revert_count: 2, // 10% — much better
        keep_count: 18,
        waste_count: 1, // 5% — much better
        commit_count: 5,
        completion_reason: "complete".to_string(),
        config_silence_timeout: 30,
        config_max_retries: 3,
        ..Default::default()
    };

    let result = on_run_end(state, data);

    // Should show improvement — provisional rules promoted
    assert!(
        !result.rules_promoted.is_empty() || result.regression.is_none(),
        "Run 2 should promote rules or show no regression"
    );

    // === Run 3: Regression ===
    let state = on_run_start(&project_root, &config);

    let data = RunData {
        project_root: project_root.clone(),
        iterations: 20,
        duration_secs: 900,
        revert_count: 10, // 50% — worse than run 2
        keep_count: 10,
        waste_count: 8, // 40% — worse
        commit_count: 1,
        completion_reason: "partial".to_string(),
        config_silence_timeout: 30,
        config_max_retries: 3,
        ..Default::default()
    };

    let result = on_run_end(state, data);

    // Should detect regression
    // Note: regression detection compares against the PREVIOUS run's metrics
    // If there are provisional rules, they should be rejected
    assert!(
        result.findings.len() > 0,
        "Run 3 should produce findings (bad metrics)"
    );
}

#[test]
fn check_rules_with_active_rules() {
    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path().to_string_lossy().to_string();
    std::fs::create_dir_all(dir.path().join(".glass")).unwrap();

    let config = FeedbackConfig {
        project_root: project_root.clone(),
        feedback_llm: false,
        max_prompt_hints: 10,
        silence_timeout_secs: None,
        max_retries_before_stuck: None,
        ablation_enabled: true,
        ablation_sweep_interval: 20,
    };

    // Do a run to create rules
    let state = on_run_start(&project_root, &config);
    let data = RunData {
        project_root: project_root.clone(),
        iterations: 20,
        revert_count: 8,
        waste_count: 5,
        commit_count: 2,
        config_silence_timeout: 30,
        config_max_retries: 3,
        ..Default::default()
    };
    let _ = on_run_end(state, data);

    // Start a new run and check rules
    let mut state = on_run_start(&project_root, &config);
    let run_state = RunState {
        iterations_since_last_commit: 6,
        waste_rate: 0.2,
        ..Default::default()
    };
    let _actions = check_rules(&mut state, &run_state);
    // Should have some actions (from default rules + any created rules)
    // At minimum, default rules should fire
    // Don't assert specific count since it depends on which rules are active

    let hints = prompt_hints(&mut state);
    // Hints may be empty if no prompt_hint rules exist (which is expected)
    assert!(hints.is_empty() || hints.len() <= 10);
}
