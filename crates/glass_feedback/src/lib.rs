//! glass_feedback — Self-improving orchestrator feedback loop.
//!
//! Analyzes orchestrator runs, produces findings across three tiers
//! (config tuning, behavioral rules, prompt hints), applies changes
//! through a guarded lifecycle, and auto-rolls back regressions.

pub mod types;
pub mod io;
pub mod analyzer;
pub mod rules;
pub mod regression;
pub mod lifecycle;
pub mod llm;
pub mod defaults;

#[allow(unused_imports)]
pub use types::*;
