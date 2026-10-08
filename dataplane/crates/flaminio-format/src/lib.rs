//! Flaminio on-disk format, version 1.
//!
//! This crate is the reference implementation of
//! [`docs/format/on-disk-format-v1.md`](../../../../docs/format/on-disk-format-v1.md).
//! It is **sans-I/O**: it encodes and decodes buffers and decides layouts, but never touches a
//! device itself. The only exception is the small [`ReadAt`] trait, which lets the journal
//! scanner pull bytes from whatever the caller provides. That keeps the format logic testable
//! with in-memory devices and fuzzable without a filesystem.
//!
//! Every decoder treats its input as untrusted. Malformed input produces a [`FormatError`],
//! never a panic.

pub mod checksum;
mod codec;
pub mod consts;
mod error;
mod io;
pub mod journal;
pub mod layout;
pub mod meta;
pub mod mkfs;
pub mod page;
pub mod record;
pub mod superblock;
mod uuid;

pub use error::FormatError;
pub use io::ReadAt;
pub use uuid::Uuid;
