//! A staging area for ublk zero copy: one slot per tag in a memfd, mapped into this process.
//! Data that must pass through the process is put in a slot, and moved to or from a request's
//! registered pages by `ReadFixed`/`WriteFixed` on the memfd's descriptor ([`Staging::fd`]).
#![allow(unsafe_code)]

use std::cell::Cell;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

/// Slots of `slot` bytes in a shared memfd mapping. Single-threaded (not `Sync`): a slot is
/// borrowed for the duration of a closure, and borrowing it twice at once panics.
pub struct Staging {
    file: File,
    base: *mut u8,
    slot: usize,
    slots: usize,
    borrowed: Vec<Cell<bool>>,
}

impl Staging {
    pub fn new(slots: usize, slot: usize) -> io::Result<Self> {
        let len = slots
            .checked_mul(slot)
            .ok_or_else(|| io::Error::other("staging too large"))?;
        // SAFETY: memfd_create takes a NUL-terminated name and returns a new descriptor, which
        // we own from here on.
        let fd = unsafe { libc::memfd_create(c"flaminio-staging".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: as above, `fd` is fresh and ours.
        let file = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        file.set_len(len as u64)?;
        // SAFETY: a new shared mapping of the whole file, which lives as long as `self` (the
        // mapping is removed in `drop`, before the file is closed).
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            file,
            base: base.cast(),
            slot,
            slots,
            borrowed: (0..slots).map(|_| Cell::new(false)).collect(),
        })
    }

    /// The memfd, for `ReadFixed`/`WriteFixed`.
    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    pub fn slot_len(&self) -> usize {
        self.slot
    }

    /// File offset of byte `at` of slot `i`.
    pub fn offset(&self, i: usize, at: usize) -> u64 {
        assert!(
            i < self.slots && at <= self.slot,
            "staging slot out of range"
        );
        (i * self.slot + at) as u64
    }

    /// The address of byte `at` of slot `i`, for a read of `len` bytes into it by the kernel.
    /// The slot is not borrowed: the caller must not run [`Staging::with_slot`] on it while the
    /// read is in flight. It must not be borrowed now either.
    pub fn addr(&self, i: usize, at: usize, len: usize) -> *mut u8 {
        assert!(
            i < self.slots && at.checked_add(len).is_some_and(|end| end <= self.slot),
            "staging slot out of range"
        );
        assert!(!self.borrowed[i].get(), "staging slot {i} is borrowed");
        self.base.wrapping_add(i * self.slot + at)
    }

    /// Runs `f` on slot `i`. The caller must not have I/O in flight on the slot meanwhile:
    /// the kernel would write the memory `f` sees.
    pub fn with_slot<R>(&self, i: usize, f: impl FnOnce(&mut [u8]) -> R) -> R {
        assert!(i < self.slots, "staging slot out of range");
        assert!(
            !self.borrowed[i].replace(true),
            "staging slot {i} borrowed twice"
        );
        // SAFETY: the slot is inside the mapping, which lives as long as `self`, and the flag
        // above makes this the only reference to it until `f` returns.
        let s = unsafe { std::slice::from_raw_parts_mut(self.base.add(i * self.slot), self.slot) };
        let r = f(s);
        self.borrowed[i].set(false);
        r
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        // SAFETY: the mapping was made in `new` with this length, and no slot is borrowed
        // (borrows end with their closures, which cannot outlive `self`).
        unsafe {
            libc::munmap(self.base.cast(), self.slots * self.slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::FileExt;

    #[test]
    fn slots_are_the_memfd() {
        let s = Staging::new(3, 4096).unwrap();
        s.with_slot(1, |b| b[..5].copy_from_slice(b"hello"));
        // What the slot holds is what a read of the memfd returns, and the other way round.
        let mut got = [0u8; 5];
        s.file.read_exact_at(&mut got, s.offset(1, 0)).unwrap();
        assert_eq!(&got, b"hello");
        s.file.write_all_at(b"world", s.offset(2, 10)).unwrap();
        assert_eq!(s.with_slot(2, |b| b[10..15].to_vec()), b"world");
    }

    #[test]
    fn addresses_are_inside_their_slot_and_aligned() {
        let s = Staging::new(3, 8192).unwrap();
        for (i, at) in [(0, 0), (1, 4096), (2, 0)] {
            let a = s.addr(i, at, 4096) as usize;
            assert_eq!(a % 4096, 0, "slot {i} at {at} is not page-aligned");
            assert_eq!(a - s.base as usize, i * 8192 + at);
        }
        // Up to the slot's last byte.
        assert_eq!(
            s.addr(2, 8191, 1) as usize - s.base as usize,
            2 * 8192 + 8191
        );
    }

    #[test]
    #[should_panic(expected = "staging slot out of range")]
    fn an_address_range_ends_inside_its_slot() {
        let s = Staging::new(2, 4096).unwrap();
        s.addr(0, 4096, 1);
    }

    #[test]
    #[should_panic(expected = "is borrowed")]
    fn a_borrowed_slot_has_no_address() {
        let s = Staging::new(1, 4096).unwrap();
        s.with_slot(0, |_| s.addr(0, 0, 4096));
    }

    #[test]
    #[should_panic(expected = "borrowed twice")]
    fn a_slot_is_borrowed_once() {
        let s = Staging::new(1, 4096).unwrap();
        s.with_slot(0, |_| s.with_slot(0, |_| ()));
    }
}
