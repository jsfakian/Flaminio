//! The Flaminio replica store: chunk replicas on one raw device.
//!
//! It implements the runtime behavior of the [on-disk format](../../../../docs/format/on-disk-format-v1.md):
//! recovery (checkpoint + journal replay), allocation, redirect-on-write and deferred writes,
//! reads that verify checksums, snapshots, fencing and checkpoints.
//!
//! This first version is synchronous and single-threaded: one store is owned by one thread,
//! which matches the thread-per-core design. Device access goes through [`BlockDevice`], so the
//! same logic runs on files, raw devices and the crash-simulating [`MemDevice`]. Group commit
//! and `io_uring` come with the Linux I/O backend (MVP plan step 2).

mod bitmap;
mod device;
mod error;
mod state;
mod store;

pub use device::{BlockDevice, FileDevice, MemDevice};
pub use error::StoreError;
pub use state::ChunkKey;
pub use store::{Allocation, ChunkInfo, FsckReport, ReadReq, Stats, Store, StoreConfig, WriteReq};
