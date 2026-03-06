//! glass_pipes -- pipe-aware command parsing and stage management.

pub mod types;
pub mod parser;
pub mod classify;

pub use types::*;
pub use parser::{split_pipes, parse_pipeline};
pub use classify::{classify_pipeline, has_opt_out};
