//! Page-aligned heap buffers.
#![allow(unsafe_code)]

use std::alloc::{Layout, alloc, alloc_zeroed, dealloc, handle_alloc_error};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

/// Alignment of [`AlignedBuf`]: enough for `O_DIRECT` on any device with up to 4 KiB sectors.
pub const ALIGN: usize = 4096;

/// Whether `data` can be used for `O_DIRECT` I/O as is: address and length 4 KiB aligned.
pub fn is_aligned(data: &[u8]) -> bool {
    (data.as_ptr() as usize).is_multiple_of(ALIGN) && data.len().is_multiple_of(ALIGN)
}

/// A zero-initialized, 4 KiB-aligned byte buffer.
pub struct AlignedBuf {
    ptr: NonNull<u8>,
    len: usize,
}

// SAFETY: AlignedBuf exclusively owns its allocation, exactly like Vec<u8>, so moving it to or
// sharing references across threads is as safe as it is for Vec<u8>.
unsafe impl Send for AlignedBuf {}
// SAFETY: see Send; &AlignedBuf only gives out &[u8].
unsafe impl Sync for AlignedBuf {}

impl AlignedBuf {
    /// A zeroed buffer of `len` bytes.
    pub fn zeroed(len: usize) -> Self {
        let layout = Self::layout_for(len);
        // SAFETY: the layout has a non-zero size (layout_for rounds 0 up to ALIGN).
        let raw = unsafe { alloc_zeroed(layout) };
        let ptr = NonNull::new(raw).unwrap_or_else(|| handle_alloc_error(layout));
        Self { ptr, len }
    }

    /// An aligned copy of `data`.
    pub fn copy_of(data: &[u8]) -> Self {
        let layout = Self::layout_for(data.len());
        // SAFETY: the layout has a non-zero size (layout_for rounds 0 up to ALIGN).
        let raw = unsafe { alloc(layout) };
        let ptr = NonNull::new(raw).unwrap_or_else(|| handle_alloc_error(layout));
        // SAFETY: the allocation holds at least data.len() bytes and cannot overlap `data`; the
        // bytes past data.len() (up to ALIGN for an empty buffer) are never read.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.as_ptr(), data.len()) };
        Self {
            ptr,
            len: data.len(),
        }
    }

    /// An aligned copy of `data`, as shared bytes (for zero-copy writes: the store and the
    /// device take slices of it without copying).
    pub fn shared_copy_of(data: &[u8]) -> bytes::Bytes {
        bytes::Bytes::from_owner(Self::copy_of(data))
    }

    fn layout_for(len: usize) -> Layout {
        Layout::from_size_align(len.max(ALIGN), ALIGN).expect("buffer size overflows")
    }
}

impl AsRef<[u8]> for AlignedBuf {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl Deref for AlignedBuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        // SAFETY: ptr points to an initialized (zeroed) allocation of at least len bytes that
        // this value owns; the returned borrow is tied to &self.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl DerefMut for AlignedBuf {
    fn deref_mut(&mut self) -> &mut [u8] {
        // SAFETY: as in deref, and &mut self guarantees exclusive access.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        // SAFETY: ptr was allocated in zeroed() with exactly this layout and is freed once.
        unsafe { dealloc(self.ptr.as_ptr(), Self::layout_for(self.len)) }
    }
}

impl fmt::Debug for AlignedBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlignedBuf")
            .field("len", &self.len)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_zeroed_and_writable() {
        // Miri walks every byte: a large buffer there adds minutes and no coverage.
        let large = if cfg!(miri) { 2 * ALIGN } else { 1 << 20 };
        for len in [0, 1, 4096, 4097, large] {
            let mut b = AlignedBuf::zeroed(len);
            assert_eq!(b.len(), len);
            assert_eq!(b.as_ptr() as usize % ALIGN, 0);
            assert!(b.iter().all(|&x| x == 0));
            b.fill(7);
            assert!(b.iter().all(|&x| x == 7));
        }
        let c = AlignedBuf::copy_of(&[1, 2, 3]);
        assert_eq!(&c[..], &[1, 2, 3]);
        assert!(is_aligned(&AlignedBuf::zeroed(8192)));
        assert!(!is_aligned(&AlignedBuf::zeroed(8192)[1..4097]));
    }

    /// The paths the unsafe code serves, for Miri: an empty copy, shared bytes sliced and
    /// cloned, and buffers moved to, read on and dropped by another thread.
    #[test]
    fn shared_and_sent_across_threads() {
        assert!(AlignedBuf::copy_of(&[]).is_empty());
        let data: Vec<u8> = (0..=255).cycle().take(3 * ALIGN).collect();
        let shared = AlignedBuf::shared_copy_of(&data);
        let piece = shared.slice(ALIGN..2 * ALIGN);
        assert!(is_aligned(&piece));
        let mut owned = AlignedBuf::copy_of(&data[..ALIGN]);
        let handle = std::thread::spawn(move || {
            owned[0] = 9;
            (piece[..].to_vec(), owned[..2].to_vec())
        });
        let (read, written) = handle.join().unwrap();
        assert_eq!(read, &data[ALIGN..2 * ALIGN]);
        assert_eq!(written, [9, 1]);
        drop(shared);
    }
}
