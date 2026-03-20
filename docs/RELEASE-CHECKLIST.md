# Glass Release Checklist

**Target:** v3.0 public release
**Last updated:** 2026-03-20

## Blocking — Must complete before release

- [ ] **Manual E2E test: orchestrator with default provider**
  - Build release binary, launch Glass
  - Open a project with PRD.md
  - Press Ctrl+Shift+O, verify agent spawns
  - Let it run 2-3 iterations, verify activity overlay shows events
  - Verify checkpoint/respawn works
  - Press Ctrl+Shift+O to deactivate, verify clean shutdown
  - Verify no orphan processes

- [ ] **Manual E2E test: orchestrator with OpenAI provider**
  - Set `provider = "openai-api"` and `OPENAI_API_KEY` in env
  - Press Ctrl+Shift+O, verify GPT-based orchestrator works
  - Verify tool calling (glass_query, glass_context) works via IPC
  - Verify activity overlay shows events from OpenAI backend

- [ ] **Manual E2E test: orchestrator with Ollama provider**
  - Run `ollama serve` and pull a model
  - Set `provider = "ollama"` and `model = "llama3"` (or whatever is pulled)
  - Press Ctrl+Shift+O, verify local model orchestrator works

- [ ] **CI passing on master**
  - Push to main branch
  - Verify GitHub Actions pass (fmt, clippy, build+test on Linux/macOS/Windows)

- [ ] **GitHub release with binaries**
  - Tag v3.0
  - Build release binaries for Windows, macOS, Linux
  - Create GitHub release with changelog

## Important — Should complete before or shortly after release

- [ ] **CHANGELOG.md**
  - Document v1.0 through v3.0 milestones
  - Key features per version

- [ ] **CONTRIBUTING.md**
  - Build setup instructions
  - Code style (clippy -D warnings, cargo fmt)
  - PR process (master → main)
  - Test conventions (inline #[cfg(test)] modules)
  - Platform notes (ConPTY on Windows, forkpty on Unix)

- [ ] **LICENSE file verification**
  - Confirm MIT license file exists at repo root
  - Verify all crate Cargo.toml have `license = "MIT"`

- [ ] **Settings overlay interactive model picker**
  - Make Provider/Model fields cycleable with arrow keys
  - Dynamic model list from cached API responses
  - Currently read-only display (Phase 1 Task 8)

## Nice to have — Can ship after release

- [ ] **Screenshot/demo GIF in README**
  - Show terminal with block rendering, pipes, orchestrator overlay

- [ ] **Example Rhai scripts**
  - Create `examples/scripts/` with sample hooks

- [ ] **Scripting feature page in mdBook**
  - Dedicated page at `docs/src/features/scripting.md`

- [ ] **Reconcile config defaults across docs**
  - `soi.min_lines` (README vs mdBook)
  - Font defaults (platform-specific values differ across docs)
