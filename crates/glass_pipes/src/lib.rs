//! glass_pipes -- pipe-aware command parsing and stage management.

pub mod parser;
pub mod types;

pub use parser::{parse_pipeline, split_pipes};
pub use types::*;
