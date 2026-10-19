//! Block devices the store runs on.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::os::unix::fs::FileExt;
use std::path::Path;

use bytes::Bytes;
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

    /// Writes several buffers, possibly concurrently and in any order. Returns when all are
    /// complete. The ranges must not overlap.
    fn write_many(&self, writes: &[(u64, &[u8])]) -> io::Result<()> {
        writes.iter().try_for_each(|(o, d)| self.write_at(*o, d))
    }

    /// Reads several buffers, possibly concurrently. Returns when all are complete.
    fn read_many(&self, reads: &mut [(u64, &mut [u8])]) -> io::Result<()> {
        reads.iter_mut().try_for_each(|(o, b)| self.read_at(*o, b))
    }

    /// Starts writing data that the next commit will make durable (the store's early writes,
    /// to clusters no durable batch references), without waiting. The device keeps its own copy
    /// of the data. The next [`start_commit`](Self::start_commit)'s flush covers them, and the
    /// flush of a commit already in flight need not wait for them.
    ///
    /// The default writes them before returning.
    fn start_writes(&self, writes: &[(u64, Bytes)]) -> io::Result<()> {
        self.write_many(&as_slices(writes))
    }

    /// Starts a commit's I/O: writes `writes` (as [`write_many`](Self::write_many)), then
    /// flushes, without waiting for either. The flush also covers the early writes started
    /// before ([`start_writes`](Self::start_writes)). The device keeps its own copy of the
    /// data. Returns true if the commit is already complete and durable; otherwise
    /// [`poll_commit`](Self::poll_commit) reports its outcome. At most one commit is in flight.
    ///
    /// The default does it all before returning.
    fn start_commit(&self, writes: &[(u64, Bytes)]) -> io::Result<bool> {
        self.write_many(&as_slices(writes))?;
        self.flush()?;
        Ok(true)
    }

    /// The outcome of the commit in flight, once its writes and flush are complete (with
    /// `wait`, waits for them). `None` while it is still in flight. Called only after
    /// [`start_commit`](Self::start_commit) returned false.
    fn poll_commit(&self, wait: bool) -> Option<io::Result<()>> {
        let _ = wait;
        Some(Ok(()))
    }
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

    fn write_many(&self, writes: &[(u64, &[u8])]) -> io::Result<()> {
        (**self).write_many(writes)
    }

    fn read_many(&self, reads: &mut [(u64, &mut [u8])]) -> io::Result<()> {
        (**self).read_many(reads)
    }

    fn start_writes(&self, writes: &[(u64, Bytes)]) -> io::Result<()> {
        (**self).start_writes(writes)
    }

    fn start_commit(&self, writes: &[(u64, Bytes)]) -> io::Result<bool> {
        (**self).start_commit(writes)
    }

    fn poll_commit(&self, wait: bool) -> Option<io::Result<()>> {
        (**self).poll_commit(wait)
    }
}

impl<D: BlockDevice + ?Sized> BlockDevice for Box<D> {
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

    fn write_many(&self, writes: &[(u64, &[u8])]) -> io::Result<()> {
        (**self).write_many(writes)
    }

    fn read_many(&self, reads: &mut [(u64, &mut [u8])]) -> io::Result<()> {
        (**self).read_many(reads)
    }

    fn start_writes(&self, writes: &[(u64, Bytes)]) -> io::Result<()> {
        (**self).start_writes(writes)
    }

    fn start_commit(&self, writes: &[(u64, Bytes)]) -> io::Result<bool> {
        (**self).start_commit(writes)
    }

    fn poll_commit(&self, wait: bool) -> Option<io::Result<()>> {
        (**self).poll_commit(wait)
    }
}

/// Borrowed views of shared writes, for [`BlockDevice::write_many`].
fn as_slices(writes: &[(u64, Bytes)]) -> Vec<(u64, &[u8])> {
    writes.iter().map(|(o, d)| (*o, &d[..])).collect()
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
    flushes: u64,
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
                flushes: 0,
                fail_after: None,
                failed: false,
            }),
        }
    }

    /// Flushes performed so far.
    pub fn flushes(&self) -> u64 {
        self.inner.borrow().flushes
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

    /// The device image after a power loss in which exactly the unflushed blocks whose byte
    /// offset satisfies `keep` survive (whole, not torn).
    pub fn crash_keeping(&self, keep: impl Fn(u64) -> bool) -> MemDevice {
        let d = self.inner.borrow();
        let mut durable = d.durable.clone();
        for (&block, data) in &d.cache {
            if keep(block * BLOCK_SIZE_U64) {
                durable.insert(block, data.clone());
            }
        }
        let image = MemDevice::new(d.size);
        image.inner.borrow_mut().durable = durable;
        image
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
                flushes: 0,
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
        if !offset.is_multiple_of(BLOCK_SIZE_U64) || !len.is_multiple_of(BLOCK_SIZE) {
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
        d.flushes += 1;
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

/// Writes, with their data.
type OwnedWrites = Vec<(u64, Vec<u8>)>;

/// A [`MemDevice`] whose commits ([`BlockDevice::start_commit`]) stay in flight until
/// [`complete`](Self::complete) lands them, or until the store waits for them: for testing
/// asynchronous commits.
pub struct DeferredDevice {
    inner: MemDevice,
    pending: RefCell<Option<OwnedWrites>>,
    fail: std::cell::Cell<bool>,
}

impl DeferredDevice {
    pub fn new(inner: MemDevice) -> Self {
        Self {
            inner,
            pending: RefCell::new(None),
            fail: std::cell::Cell::new(false),
        }
    }

    /// True while a commit is in flight.
    pub fn in_flight(&self) -> bool {
        self.pending.borrow().is_some()
    }

    /// The writes of the commit in flight (its data writes and its journal batch).
    pub fn pending_writes(&self) -> usize {
        self.pending.borrow().as_ref().map_or(0, Vec::len)
    }

    /// Lands the commit in flight: its writes, then the flush.
    pub fn complete(&self) {
        let writes = self
            .pending
            .borrow_mut()
            .take()
            .expect("a commit in flight");
        for (o, d) in writes {
            self.inner.write_at(o, &d).expect("in memory");
        }
        self.inner.flush().expect("in memory");
    }

    /// Makes the commit in flight fail (its writes are lost).
    pub fn fail_commit(&self) {
        self.fail.set(true);
    }

    /// The device underneath, without what is still in flight: as a crash would leave it.
    pub fn into_inner(self) -> MemDevice {
        self.inner
    }
}

impl BlockDevice for DeferredDevice {
    fn size(&self) -> u64 {
        self.inner.size()
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.inner.read_at(offset, buf)
    }

    fn write_at(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        self.inner.write_at(offset, data)
    }

    fn flush(&self) -> io::Result<()> {
        self.inner.flush()
    }

    fn start_commit(&self, writes: &[(u64, Bytes)]) -> io::Result<bool> {
        let mut p = self.pending.borrow_mut();
        assert!(p.is_none(), "one commit in flight at a time");
        *p = Some(writes.iter().map(|(o, d)| (*o, d.to_vec())).collect());
        Ok(false)
    }

    fn poll_commit(&self, wait: bool) -> Option<io::Result<()>> {
        if self.fail.replace(false) {
            self.pending.borrow_mut().take();
            return Some(Err(io::Error::other("injected commit failure")));
        }
        if self.in_flight() {
            if !wait {
                return None;
            }
            self.complete();
        }
        Some(Ok(()))
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
