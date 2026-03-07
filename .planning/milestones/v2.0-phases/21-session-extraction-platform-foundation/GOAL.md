# Phase 21: Session Extraction & Platform Foundation

## Goal

Extract the single-session assumption from WindowContext into a SessionMux abstraction layer (glass_mux crate), add SessionId routing to AppEvent, and add platform cfg gates for shell detection, config paths, and keyboard modifier mapping. Glass must run identically to v1.3 on Windows through the new SessionMux layer (zero user-visible change).

## Key Deliverables

- New `glass_mux` crate with Session, SessionMux, SplitTree, Tab, ViewportLayout structs
- Session struct extracted from WindowContext fields (PTY, Term, BlockManager, HistoryDb, SnapshotStore, etc.)
- SessionId added to all PTY-originated AppEvent variants
- SessionMux in single-tab/single-session mode wrapping existing behavior
- Refactored WindowContext using SessionMux instead of inline terminal fields
- cfg-gated shell detection (zsh on macOS, $SHELL on Linux, pwsh on Windows)
- Platform config/data paths via `dirs` crate
- Platform action modifier helper (Cmd on macOS, Ctrl+Shift elsewhere)
- Shell integration scripts for zsh and bash (Linux/macOS)

## Test Gate

Glass runs identically to v1.3 on Windows through the new SessionMux layer. No user-visible change. All existing tests pass.

## Dependencies

None -- this is the foundation phase.

## Research Notes

- Pure refactoring of existing code into new structs. Well-understood Rust patterns.
- WezTerm's Mux architecture provides a validated reference.
- Skip research-phase -- standard patterns.
