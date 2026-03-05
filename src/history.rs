//! History subcommand dispatch and display formatting.
//!
//! Handles `glass history`, `glass history list`, and `glass history search`.

use crate::HistoryAction;

/// Entry point for all history subcommands.
pub fn run_history(_action: Option<HistoryAction>) {
    eprintln!("glass history: not yet implemented");
    std::process::exit(1);
}
