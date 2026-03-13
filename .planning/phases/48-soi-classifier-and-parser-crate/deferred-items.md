# Phase 48 Deferred Items

## Pre-existing test failures in jest.rs (discovered during 48-02)

These 5 tests in `crates/glass_soi/src/jest.rs` were failing before plan 48-02 changes.
They are stub tests written in plan 48-01 for the jest parser, which will be implemented
in plan 48-03.

Failing tests:
- `jest::tests::jest_ansi_pass_fail_statuses_correct`
- `jest::tests::jest_ansi_stripped_before_parsing`
- `jest::tests::jest_duration_extracted`
- `jest::tests::jest_failure_diff_extracted`
- `jest::tests::jest_test_name_includes_suite`

Action: Will be resolved when jest.rs parser is implemented (plan 48-03).
