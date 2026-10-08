//! In-memory allocation bitmap with dirty tracking per on-disk bitmap page.

use std::collections::BTreeSet;

use flaminio_format::consts::BITS_PER_BITMAP_PAGE;
use flaminio_format::meta::BitmapPage;

pub(crate) struct Bitmap {
    words: Vec<u64>,
    len: u64,
    ones: u64,
    dirty: BTreeSet<u64>,
    cursor: u64,
    first_fit: bool,
}

impl Bitmap {
    pub(crate) fn new(len: u64) -> Self {
        Self {
            words: vec![0; len.div_ceil(64) as usize],
            len,
            ones: 0,
            dirty: BTreeSet::new(),
            cursor: 0,
            first_fit: false,
        }
    }

    /// Always allocate the lowest free bits instead of continuing from a rotating cursor.
    pub(crate) fn set_first_fit(&mut self, first_fit: bool) {
        self.first_fit = first_fit;
    }

    /// Loads the bits of on-disk page `page_index` without marking anything dirty.
    pub(crate) fn load_page(&mut self, page_index: u64, page: &BitmapPage) {
        let first = page_index * BITS_PER_BITMAP_PAGE;
        let count = BITS_PER_BITMAP_PAGE.min(self.len.saturating_sub(first));
        for i in 0..count {
            if page.get(i) {
                let bit = first + i;
                self.words[(bit / 64) as usize] |= 1 << (bit % 64);
            }
        }
        self.ones = self.words.iter().map(|w| u64::from(w.count_ones())).sum();
    }

    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    pub(crate) fn ones(&self) -> u64 {
        self.ones
    }

    pub(crate) fn free(&self) -> u64 {
        self.len - self.ones
    }

    pub(crate) fn get(&self, i: u64) -> bool {
        i < self.len && self.words[(i / 64) as usize] & (1 << (i % 64)) != 0
    }

    pub(crate) fn set(&mut self, i: u64, value: bool) {
        assert!(i < self.len, "bit {i} out of range");
        let word = &mut self.words[(i / 64) as usize];
        let mask = 1 << (i % 64);
        if (*word & mask != 0) != value {
            *word ^= mask;
            if value {
                self.ones += 1;
            } else {
                self.ones -= 1;
            }
            self.dirty.insert(i / BITS_PER_BITMAP_PAGE);
        }
    }

    /// Allocates up to `max` consecutive free bits, searching from a rotating cursor (or from 0
    /// with first-fit). Returns the first bit and how many were allocated.
    pub(crate) fn alloc_run(&mut self, max: u64) -> Option<(u64, u64)> {
        if max == 0 || self.ones == self.len {
            return None;
        }
        let from = if self.first_fit { 0 } else { self.cursor };
        let start = self.find_free(from).or_else(|| self.find_free(0))?;
        let mut n = 0;
        while n < max && start + n < self.len && !self.get(start + n) {
            self.set(start + n, true);
            n += 1;
        }
        self.cursor = (start + n) % self.len;
        Some((start, n))
    }

    fn find_free(&self, from: u64) -> Option<u64> {
        let mut i = from;
        while i < self.len {
            let w = self.words[(i / 64) as usize];
            if w == u64::MAX {
                i = (i / 64 + 1) * 64;
                continue;
            }
            if w & (1 << (i % 64)) == 0 {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// The on-disk form of page `page_index`.
    pub(crate) fn to_page(&self, page_index: u64) -> BitmapPage {
        let first = page_index * BITS_PER_BITMAP_PAGE;
        let mut page = BitmapPage::new(first);
        let count = BITS_PER_BITMAP_PAGE.min(self.len.saturating_sub(first));
        for i in 0..count {
            if self.get(first + i) {
                page.set(i, true);
            }
        }
        page
    }

    pub(crate) fn take_dirty(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.dirty).into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_runs_and_wraps() {
        let mut b = Bitmap::new(200);
        assert_eq!(b.alloc_run(10), Some((0, 10)));
        b.set(15, true);
        assert_eq!(
            b.alloc_run(10),
            Some((10, 5)),
            "run stops at an allocated bit"
        );
        assert_eq!(b.alloc_run(1000), Some((16, 184)), "run stops at the end");
        assert_eq!(b.alloc_run(1), None);
        b.set(3, false);
        assert_eq!(b.alloc_run(5), Some((3, 1)), "search wraps to the start");
        assert_eq!(b.ones(), 200);
    }

    #[test]
    fn page_roundtrip_and_dirty_tracking() {
        let len = 2 * BITS_PER_BITMAP_PAGE + 5;
        let mut b = Bitmap::new(len);
        b.set(1, true);
        b.set(len - 1, true);
        assert_eq!(b.take_dirty(), vec![0, 2]);
        let mut c = Bitmap::new(len);
        for p in 0..3 {
            c.load_page(p, &b.to_page(p));
        }
        assert!(c.get(1) && c.get(len - 1) && !c.get(2));
        assert_eq!(c.ones(), 2);
        assert!(c.take_dirty().is_empty());
    }
}
