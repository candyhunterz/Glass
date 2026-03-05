//! glass_snapshot — Content-addressed blob storage and snapshot metadata.

pub mod types;
pub mod blob_store;
pub mod db; // Implemented in Task 2

pub use blob_store::BlobStore;
pub use types::{SnapshotRecord, SnapshotFileRecord};
