//! Direct I/O for the replica store.
//!
//! - [`AlignedBuf`]: page-aligned buffers, as `O_DIRECT` requires.
//! - `UringDevice` (Linux): a [`BlockDevice`](flaminio_store::BlockDevice) on `io_uring`, with
//!   `O_DIRECT` so the page cache is bypassed. Batched reads and writes go to the kernel in one
//!   submission.
//! - `EventFd` (Linux): wakes a thread that waits on an io_uring.
//!
//! `unsafe` code in Flaminio lives only here, in the `aligned`, `eventfd` and `uring` modules.
//! Each block states why it is sound.

mod aligned;
#[cfg(target_os = "linux")]
mod eventfd;
#[cfg(target_os = "linux")]
mod uring;

pub use aligned::{ALIGN, AlignedBuf, is_aligned};
#[cfg(target_os = "linux")]
pub use eventfd::EventFd;
#[cfg(target_os = "linux")]
pub use uring::UringDevice;
