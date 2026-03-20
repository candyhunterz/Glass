# Glass Settings Audit: Fix Config ↔ Settings Overlay Sync Issues

## Goal

Audit and fix all disconnects between `~/.glass/config.toml` and the Glass settings overlay (Ctrl+Shift+,). When a user manually edits config.toml, the settings overlay must reflect those values accurately. When a user changes a setting in the overlay, the TOML file must be written correctly and the hot-reload must apply it.

## Background

The settings overlay is in `crates/glass_renderer/src/settings_overlay.rs`. The config struct is in `crates/glass_core/src/config.rs`. The snapshot builder and settings handlers are in `src/main.rs`.

The overlay works by:
1. Building a `SettingsConfigSnapshot` from `self.config` every frame (in `src/main.rs` around line 2454)
2. Displaying fields via `fields_for_section()` in `settings_overlay.rs`
3. Handling activate (Enter/Space) and increment (+/-) via `handle_settings_activate()` and `handle_settings_increment()` in `main.rs`
4. Writing changes via `update_config_field()` in `config.rs`, which triggers hot-reload

## Audit Tasks

### Task 1: Verify every SettingsConfigSnapshot field reads from the correct config field

For each field in `SettingsConfigSnapshot`, verify:
- It reads from the correct `self.config.*` path
- The default/fallback matches the serde default in `config.rs`
- The field type matches (e.g., bool shows ON/OFF, not true/false)

Known bug: `orchestrator_enabled` was reading from config instead of runtime state — already fixed. Look for similar issues.

### Task 2: Verify every activate handler writes the correct config field

For each `(section, field)` match arm in `handle_settings_activate()`:
- It writes to the correct TOML section and key
- String values are properly quoted for TOML (e.g., `"\"Watch\""` not `"Watch"`)
- Boolean toggles read the current value correctly before toggling

### Task 3: Verify every increment handler writes the correct config field

For each `(section, field)` match arm in `handle_settings_increment()`:
- It reads the correct current value
- Step size matches the field's intent
- Min/max bounds are correct
- The written value matches the config type (int vs float)

### Task 4: Verify serde defaults match snapshot defaults

Compare every `#[serde(default = "...")]` function in `config.rs` with the corresponding `Default` impl value in `SettingsConfigSnapshot`. They must match. If they don't, the settings overlay shows a different default than what the config actually uses.

### Task 5: Verify hot-reload → snapshot round-trip

For each config section, verify:
1. Write a value via `update_config_field()`
2. Hot-reload fires (ConfigReloaded event)
3. `self.config` is updated
4. Next frame's `SettingsConfigSnapshot` reflects the new value
5. Re-opening settings shows the correct value

Check specifically:
- Does `update_config_field` handle all section paths correctly? (dotted paths like `agent.orchestrator`, `agent.permissions`, etc.)
- Are there any fields that exist in the TOML but get lost on round-trip?
- Are there TOML fields that serde silently ignores (unknown fields)?

### Task 6: Write regression tests

Add tests in `config.rs` for any bugs found:
- Test that each config section round-trips correctly (write → parse → values match)
- Test that `update_config_field` correctly writes to nested sections

## Rules

- Run `cargo test --workspace` after each fix
- Run `cargo clippy --workspace -- -D warnings` after each fix
- Commit each fix separately with a descriptive message
- Do NOT change behavior — only fix sync issues between config and settings
- Do NOT add new settings or features
- Do NOT refactor the settings overlay architecture

## Success Criteria

- Every settings field accurately reflects the current config.toml
- Every settings toggle/increment correctly writes to config.toml
- Hot-reload properly updates all settings
- All serde defaults match snapshot defaults
- `cargo test --workspace` passes
- `cargo clippy --workspace -- -D warnings` clean
