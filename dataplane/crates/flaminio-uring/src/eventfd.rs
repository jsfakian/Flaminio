//! An eventfd: lets other threads wake a thread that waits on an io_uring.
#![allow(unsafe_code)]

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

/// A non-blocking eventfd. Any thread may signal it; the waiting thread reads it (for
/// example with an io_uring read) to learn it was signalled.
#[derive(Debug)]
pub struct EventFd(File);

impl EventFd {
    pub fn new() -> io::Result<Self> {
        // SAFETY: eventfd takes no pointers; on success the returned descriptor is new and
        // owned by nobody else, so OwnedFd may take it.
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: as above, `fd` is a fresh descriptor that we own.
        Ok(Self(File::from(unsafe { OwnedFd::from_raw_fd(fd) })))
    }

    /// Wakes the waiter. Never blocks: the counter saturating only means a wake-up is pending.
    pub fn signal(&self) {
        let _ = (&self.0).write(&1u64.to_ne_bytes());
    }

    /// Clears pending signals without blocking.
    pub fn drain(&self) {
        let mut b = [0u8; 8];
        let _ = (&self.0).read(&mut b);
    }
}

impl AsRawFd for EventFd {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}
