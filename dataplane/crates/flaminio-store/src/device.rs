//! Block devices the store runs on.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::os::unix::fs::FileExt;
use std::path::Path;

use flaminio_format::consts::{BLOCK_SIZE, BLOCK_SIZE_U64};

/// A device with positioned I/O and an explicit cache flush.
///
/// Writes are only durable after [`flush`](Self::flush) returns. Implementations must not
/// reorder a flush with the writes issued before it.
pub trait BlockDevice {
    /// Size in bytes.
    fn size(&self) -> u64;
    /// Reads exactly `buf.len()` bytes at `offset`.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
    /// Writes all of `data` at `offset`.
    fn write_at(&self, offset: u64, data: &[u8]) -> io::Result<()>;
    /// Makes every completed write durable.
    fn flush(&self) -> io::Result<()>;
}

impl<D: BlockDevice + ?Sized> BlockDevice for &D {
    fn size(&self) -> u64 {
        (**self).size()
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        (**self).read_at(offset, buf)
    }

    fn write_at(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        (**self).write_at(offset, data)
    }

    fn flush(&self) -> io::Result<()> {
        (**self).flush()
    }
}

/// Adapter so the format crate's journal scanner can read from any [`BlockDevice`].
pub(crate) struct Reader<'a, D: ?Sized>(pub &'a D);

impl<D: BlockDevice + ?Sized> flaminio_format::ReadAt for Reader<'_, D> {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.0.read_at(offset, buf)
    }
}

/// A regular file or block device, using buffered positioned I/O and `fdatasync`.
///
/// Correct but not fast: `O_DIRECT` and `io_uring` arrive with the Linux backend.
pub struct FileDevice {
    file: File,
    size: u64,
}

impl FileDevice {
    /// Opens `path` for reading and writing.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let size = file.seek(SeekFrom::End(0))?;
        Ok(Self { file, size })
    }
}

impl BlockDevice for FileDevice {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.file.read_exact_at(buf, offset)
    }

    fn write_at(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        self.file.write_all_at(data, offset)
    }

    fn flush(&self) -> io::Result<()> {
        self.file.sync_data()
    }
}

type Block = Box<[u8; BLOCK_SIZE]>;

/// An in-memory device with a simulated volatile write cache, for crash testing.
///
/// Writes land in the cache and become durable at the next flush. [`crash`](Self::crash)
/// returns the image a power loss would leave behind: every cached block independently
/// survives or not, and a surviving block may be torn at a 512-byte boundary.
/// [`fail_after`](Self::fail_after) makes the device fail, like a power cut in the middle of an
/// operation.
pub struct MemDevice {
    inner: RefCell<MemInner>,
}

struct MemInner {
    size: u64,
    durable: HashMap<u64, Block>,
    cache: BTreeMap<u64, Block>,
    mutations: u64,
    fail_after: Option<u64>,
    failed: bool,
}

impl MemDevice {
    /// An all-zero device of `size` bytes. Unwritten blocks take no memory.
    pub fn new(size: u64) -> Self {
        Self {
            inner: RefCell::new(MemInner {
                size,
                durable: HashMap::new(),
                cache: BTreeMap::new(),
                mutations: 0,
                fail_after: None,
                failed: false,
            }),
        }
    }

    /// After `n` more successful writes or flushes, every further write, flush and read fails.
    /// `None` disables failure injection.
    pub fn fail_after(&self, n: Option<u64>) {
        let mut d = self.inner.borrow_mut();
        d.fail_after = n.map(|n| d.mutations + n);
        d.failed = false;
    }

    /// Writes and flushes performed so far.
    pub fn mutations(&self) -> u64 {
        self.inner.borrow().mutations
    }

    /// The device image after a power loss. `seed` chooses which cached blocks survive.
    pub fn crash(&self, seed: u64) -> MemDevice {
        let d = self.inner.borrow();
        let mut rng = SplitMix(seed);
        let mut durable = d.durable.clone();
        for (&block, data) in &d.cache {
            match rng.next() % 4 {
                0 | 1 => {}
                2 => {
                    durable.insert(block, data.clone());
                }
                _ => {
                    // Torn: a prefix of 512-byte sectors is new, the rest is old.
                    let sectors = 1 + (rng.next() % 7) as usize;
                    let mut torn = durable
                        .get(&block)
                        .cloned()
                        .unwrap_or_else(|| Box::new([0; BLOCK_SIZE]));
                    torn[..sectors * 512].copy_from_slice(&data[..sectors * 512]);
                    durable.insert(block, torn);
                }
            }
        }
        MemDevice {
            inner: RefCell::new(MemInner {
                size: d.size,
                durable,
                cache: BTreeMap::new(),
                mutations: 0,
                fail_after: None,
                failed: false,
            }),
        }
    }
}

impl MemInner {
    fn check(&mut self, mutation: bool) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::other("simulated power loss"));
        }
        if mutation {
            if self.fail_after.is_some_and(|n| self.mutations >= n) {
                self.failed = true;
                return Err(io::Error::other("simulated power loss"));
            }
            self.mutations += 1;
        }
        Ok(())
    }

    fn bounds(&self, offset: u64, len: usize) -> io::Result<()> {
        if offset % BLOCK_SIZE_U64 != 0 || len % BLOCK_SIZE != 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "unaligned I/O"));
        }
        if offset
            .checked_add(len as u64)
            .is_none_or(|end| end > self.size)
        {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "I/O past end of device",
            ));
        }
        Ok(())
    }
}

impl BlockDevice for MemDevice {
    fn size(&self) -> u64 {
        self.inner.borrow().size
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let mut d = self.inner.borrow_mut();
        d.check(false)?;
        d.bounds(offset, buf.len())?;
        for (i, out) in buf.chunks_exact_mut(BLOCK_SIZE).enumerate() {
            let block = offset / BLOCK_SIZE_U64 + i as u64;
            match d.cache.get(&block).or_else(|| d.durable.get(&block)) {
                Some(b) => out.copy_from_slice(&b[..]),
                None => out.fill(0),
            }
        }
        Ok(())
    }

    fn write_at(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        let mut d = self.inner.borrow_mut();
        d.check(true)?;
        d.bounds(offset, data.len())?;
        for (i, src) in data.chunks_exact(BLOCK_SIZE).enumerate() {
            let block = offset / BLOCK_SIZE_U64 + i as u64;
            d.cache
                .insert(block, Box::new(src.try_into().expect("block sized")));
        }
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        let mut d = self.inner.borrow_mut();
        d.check(true)?;
        let cache = std::mem::take(&mut d.cache);
        d.durable.extend(cache);
        Ok(())
    }
}

/// Small deterministic PRNG for crash images.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flushed_writes_survive_crash_unflushed_may_not() {
        let dev = MemDevice::new(1 << 20);
        dev.write_at(0, &[1; BLOCK_SIZE]).unwrap();
        dev.flush().unwrap();
        dev.write_at(0, &[2; BLOCK_SIZE]).unwrap();
        let mut seen = std::collections::HashSet::new();
        for seed in 0..64 {
            let img = dev.crash(seed);
            let mut b = [0u8; BLOCK_SIZE];
            img.read_at(0, &mut b).unwrap();
            seen.insert((b[0], b[BLOCK_SIZE - 1]));
        }
        assert!(seen.contains(&(1, 1)), "old version");
        assert!(seen.contains(&(2, 2)), "new version");
        assert!(seen.contains(&(2, 1)), "torn version");
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn fail_after_simulates_power_cut() {
        let dev = MemDevice::new(1 << 20);
        dev.fail_after(Some(1));
        dev.write_at(0, &[1; BLOCK_SIZE]).unwrap();
        assert!(dev.flush().is_err());
        assert!(dev.write_at(0, &[1; BLOCK_SIZE]).is_err());
        let mut b = [0u8; BLOCK_SIZE];
        assert!(dev.read_at(0, &mut b).is_err());
    }
}
