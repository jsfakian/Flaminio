//! A block device on io_uring with O_DIRECT.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use bytes::Bytes;
use flaminio_store::BlockDevice;
use io_uring::{IoUring, opcode, types};

use crate::aligned::{AlignedBuf, is_aligned};

/// Submission queue size. Larger batches are submitted in windows of this many operations.
const QUEUE_DEPTH: u32 = 256;

/// The flush's tag on the commit ring (writes are tagged by a counter).
const COMMIT_FLUSH: u64 = u64::MAX;

/// A file or block device driven through io_uring.
///
/// With `direct` (the normal case), the device is opened with `O_DIRECT`: no page cache, and
/// durability is exactly what [`flush`](BlockDevice::flush) (an `fdatasync`) provides.
/// Unaligned buffers are bounced through [`AlignedBuf`]s; pass aligned buffers to avoid the
/// copy. The device is single-threaded, like the store that owns it.
///
/// Commits ([`BlockDevice::start_commit`]) and early writes ([`BlockDevice::start_writes`]) run
/// on a second ring, so that they stay in flight while the first serves synchronous I/O. A
/// commit's flush is submitted once every write that belongs to it has completed, and holds back
/// nothing submitted after it. Register an eventfd with [`notify_commits`](Self::notify_commits)
/// to learn when one completes.
pub struct UringDevice {
    file: File,
    size: u64,
    direct: bool,
    ring: RefCell<IoUring>,
    commit_ring: RefCell<IoUring>,
    commits: RefCell<Commits>,
}

/// The commit ring's state.
#[derive(Default)]
struct Commits {
    /// Writes in flight, by tag: their buffers (the kernel's to read until they complete) and
    /// the commit they belong to.
    writes: HashMap<u64, (Held, u64)>,
    next_tag: u64,
    /// The commit the next writes belong to: the next one to start.
    next_gen: u64,
    /// Errors of completed writes, by commit.
    errors: BTreeMap<u64, io::Error>,
    /// The commit in flight, and where its flush is.
    current: Option<(u64, Flush)>,
}

/// A buffer the kernel reads for a write in flight.
enum Held {
    /// Shared and already aligned: written as is.
    Shared(Bytes),
    /// An aligned copy.
    Copied(AlignedBuf),
}

impl Held {
    fn new(data: &Bytes, direct: bool) -> Self {
        if !direct || is_aligned(data) {
            Held::Shared(data.clone())
        } else {
            Held::Copied(AlignedBuf::copy_of(data))
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Held::Shared(b) => b,
            Held::Copied(b) => b,
        }
    }
}

enum Flush {
    /// Waiting for the commit's writes.
    Waiting,
    Submitted,
    Done(Option<io::Error>),
}

enum Op<'a> {
    Read(u64, &'a mut [u8]),
    Write(u64, &'a [u8]),
    Fsync,
}

impl UringDevice {
    /// Opens `path` read-write, with `O_DIRECT` if `direct`.
    ///
    /// Block devices are opened with `O_EXCL`: a second exclusive open (another Flaminio
    /// process) or a mounted filesystem makes it fail with `EBUSY`, instead of two stores
    /// writing checkpoints over each other.
    pub fn open(path: impl AsRef<Path>, direct: bool) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        let mut flags = libc::O_EXCL;
        if direct {
            flags |= libc::O_DIRECT;
        }
        options.custom_flags(flags);
        let mut file = options.open(path)?;
        let size = file.seek(SeekFrom::End(0))?;
        let ring = IoUring::builder()
            .setup_cqsize(2 * QUEUE_DEPTH)
            .build(QUEUE_DEPTH)?;
        let commit_ring = IoUring::builder()
            .setup_cqsize(2 * QUEUE_DEPTH)
            .build(QUEUE_DEPTH)?;
        Ok(Self {
            file,
            size,
            direct,
            ring: RefCell::new(ring),
            commit_ring: RefCell::new(commit_ring),
            commits: RefCell::new(Commits::default()),
        })
    }

    /// Signals `eventfd` whenever an operation of a commit completes.
    pub fn notify_commits(&self, eventfd: RawFd) -> io::Result<()> {
        self.commit_ring
            .borrow()
            .submitter()
            .register_eventfd(eventfd)
    }

    /// Whether the device bypasses the page cache.
    pub fn is_direct(&self) -> bool {
        self.direct
    }

    fn needs_bounce(&self, data: &[u8]) -> bool {
        self.direct && !is_aligned(data)
    }

    /// Runs operations, `QUEUE_DEPTH` at a time, and waits for all of them. Returns the first
    /// error, but only after every submitted operation has completed.
    fn run(&self, ops: &mut [Op<'_>]) -> io::Result<()> {
        let fd = types::Fd(self.file.as_raw_fd());
        let mut ring = self.ring.borrow_mut();
        let mut first_error = None;
        for window in ops.chunks_mut(QUEUE_DEPTH as usize) {
            // Bounce buffers for unaligned operations. They, and the caller's buffers, outlive
            // the operations: nothing below returns before every completion is reaped.
            let mut bounce: Vec<Option<AlignedBuf>> = Vec::with_capacity(window.len());
            {
                let mut sq = ring.submission();
                for (i, op) in window.iter_mut().enumerate() {
                    let entry = match op {
                        Op::Read(offset, buf) => {
                            let ptr = if self.needs_bounce(buf) {
                                bounce.push(Some(AlignedBuf::zeroed(buf.len())));
                                bounce[i].as_mut().expect("just pushed").as_mut_ptr()
                            } else {
                                bounce.push(None);
                                buf.as_mut_ptr()
                            };
                            opcode::Read::new(fd, ptr, len32(buf.len())?)
                                .offset(*offset)
                                .build()
                        }
                        Op::Write(offset, data) => {
                            let ptr = if self.needs_bounce(data) {
                                bounce.push(Some(AlignedBuf::copy_of(data)));
                                bounce[i].as_ref().expect("just pushed").as_ptr()
                            } else {
                                bounce.push(None);
                                data.as_ptr()
                            };
                            opcode::Write::new(fd, ptr, len32(data.len())?)
                                .offset(*offset)
                                .build()
                        }
                        Op::Fsync => {
                            bounce.push(None);
                            opcode::Fsync::new(fd)
                                .flags(types::FsyncFlags::DATASYNC)
                                .build()
                        }
                    };
                    // SAFETY: every buffer referenced by the entry (a caller buffer borrowed for
                    // the whole call, or a bounce buffer in `bounce`, whose heap allocation does
                    // not move when the Vec grows) stays alive and is not otherwise accessed
                    // until this window's completions have all been reaped below. The window is
                    // at most QUEUE_DEPTH entries, so the queue cannot be full.
                    unsafe {
                        sq.push(&entry.user_data(i as u64))
                            .expect("window fits the ring")
                    };
                }
            }

            let mut done = 0;
            while done < window.len() {
                if let Err(e) = ring.submit_and_wait(window.len() - done) {
                    match e.raw_os_error() {
                        Some(libc::EINTR | libc::EAGAIN | libc::EBUSY) => {}
                        // The kernel may still own our buffers; returning would let them be
                        // freed under it. Stop the process instead (fail-stop).
                        _ => {
                            eprintln!("flaminio-uring: io_uring_enter failed: {e}; aborting");
                            std::process::abort();
                        }
                    }
                }
                for cqe in ring.completion() {
                    done += 1;
                    let i = cqe.user_data() as usize;
                    let expected = match &window[i] {
                        Op::Read(_, buf) => buf.len(),
                        Op::Write(_, data) => data.len(),
                        Op::Fsync => 0,
                    };
                    let res = cqe.result();
                    let error = if res < 0 {
                        Some(io::Error::from_raw_os_error(-res))
                    } else if res as usize != expected {
                        Some(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            format!("short transfer: {res} of {expected} bytes"),
                        ))
                    } else {
                        None
                    };
                    if first_error.is_none() {
                        first_error = error;
                    }
                }
            }

            for (op, b) in window.iter_mut().zip(&bounce) {
                if let (Op::Read(_, buf), Some(b)) = (op, b) {
                    buf.copy_from_slice(&b[..buf.len()]);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

fn len32(len: usize) -> io::Result<u32> {
    u32::try_from(len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "I/O larger than 4 GiB"))
}

impl std::os::fd::AsRawFd for UringDevice {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.file.as_raw_fd()
    }
}

impl UringDevice {
    /// Submits `writes` on the commit ring, as part of commit `generation`. Aligned shared
    /// buffers are written as they are (a reference is held until completion); others are
    /// copied into aligned buffers.
    fn submit_commit_writes(
        &self,
        st: &mut Commits,
        ring: &mut IoUring,
        writes: &[(u64, Bytes)],
        generation: u64,
    ) -> io::Result<()> {
        let lens = writes
            .iter()
            .map(|(_, d)| len32(d.len()))
            .collect::<io::Result<Vec<u32>>>()?;
        let fd = types::Fd(self.file.as_raw_fd());
        {
            let mut sq = ring.submission();
            for ((offset, data), len) in writes.iter().zip(lens) {
                let buf = Held::new(data, self.direct);
                let tag = st.next_tag;
                st.next_tag += 1;
                let entry = opcode::Write::new(fd, buf.bytes().as_ptr(), len)
                    .offset(*offset)
                    .build()
                    .user_data(tag);
                // SAFETY: the buffer (an owned copy, or a reference to immutable shared bytes)
                // moves into `st.writes` below and stays there until this operation's
                // completion is reaped (`reap_commits`, also on drop); moving a `Held` does not
                // move the bytes it points to. The callers keep fewer than QUEUE_DEPTH
                // operations in flight on this ring: it fits.
                unsafe { sq.push(&entry).expect("fits the commit ring") };
                st.writes.insert(tag, (buf, generation));
            }
        }
        submit_or_abort(ring, "a commit's writes");
        Ok(())
    }

    /// Takes the commit ring's completions.
    fn reap_commits(st: &mut Commits, ring: &mut IoUring) {
        for cqe in ring.completion() {
            let tag = cqe.user_data();
            let res = cqe.result();
            if tag == COMMIT_FLUSH {
                let error = (res < 0).then(|| io::Error::from_raw_os_error(-res));
                if let Some((_, flush)) = st.current.as_mut() {
                    *flush = Flush::Done(error);
                }
                continue;
            }
            let Some((buf, generation)) = st.writes.remove(&tag) else {
                continue;
            };
            let len = buf.bytes().len();
            let error = if res < 0 {
                Some(io::Error::from_raw_os_error(-res))
            } else if res as usize != len {
                Some(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!("short commit write: {res} of {len} bytes"),
                ))
            } else {
                None
            };
            if let Some(e) = error {
                st.errors.entry(generation).or_insert(e);
            }
        }
    }

    /// Submits the current commit's flush once every write that belongs to it has completed.
    fn advance_commit(&self, st: &mut Commits, ring: &mut IoUring) {
        let Some((generation, flush @ Flush::Waiting)) = st.current.as_mut() else {
            return;
        };
        if st.writes.values().any(|(_, g)| *g <= *generation) {
            return;
        }
        let entry = opcode::Fsync::new(types::Fd(self.file.as_raw_fd()))
            .flags(types::FsyncFlags::DATASYNC)
            .build()
            .user_data(COMMIT_FLUSH);
        // SAFETY: references no memory.
        unsafe {
            ring.submission()
                .push(&entry)
                .expect("fits the commit ring")
        };
        *flush = Flush::Submitted;
        submit_or_abort(ring, "a commit's flush");
    }

    /// Waits for at least one completion on the commit ring.
    fn wait_commits(ring: &mut IoUring) {
        if let Err(e) = ring.submit_and_wait(1)
            && !matches!(
                e.raw_os_error(),
                Some(libc::EINTR | libc::EAGAIN | libc::EBUSY)
            )
        {
            eprintln!("flaminio-uring: io_uring_enter failed waiting for a commit: {e}; aborting");
            std::process::abort();
        }
    }
}

/// Submits what is queued on `ring`. A failure here may leave entries with the kernel, buffers
/// included, so the process stops (fail-stop).
fn submit_or_abort(ring: &mut IoUring, what: &str) {
    loop {
        match ring.submit() {
            Ok(_) => return,
            Err(e)
                if matches!(
                    e.raw_os_error(),
                    Some(libc::EINTR | libc::EAGAIN | libc::EBUSY)
                ) => {}
            Err(e) => {
                eprintln!("flaminio-uring: io_uring_enter failed for {what}: {e}; aborting");
                std::process::abort();
            }
        }
    }
}

impl Drop for UringDevice {
    fn drop(&mut self) {
        // The kernel may still read the buffers of writes in flight: wait for them first.
        let st = self.commits.get_mut();
        let ring = self.commit_ring.get_mut();
        loop {
            Self::reap_commits(st, ring);
            let flushing = matches!(st.current, Some((_, Flush::Submitted)));
            if st.writes.is_empty() && !flushing {
                return;
            }
            Self::wait_commits(ring);
        }
    }
}

impl BlockDevice for UringDevice {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.run(&mut [Op::Read(offset, buf)])
    }

    fn write_at(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        self.run(&mut [Op::Write(offset, data)])
    }

    fn flush(&self) -> io::Result<()> {
        self.run(&mut [Op::Fsync])
    }

    fn write_many(&self, writes: &[(u64, &[u8])]) -> io::Result<()> {
        let mut ops: Vec<Op<'_>> = writes.iter().map(|(o, d)| Op::Write(*o, d)).collect();
        self.run(&mut ops)
    }

    fn read_many(&self, reads: &mut [(u64, &mut [u8])]) -> io::Result<()> {
        let mut ops: Vec<Op<'_>> = reads.iter_mut().map(|(o, b)| Op::Read(*o, b)).collect();
        self.run(&mut ops)
    }

    /// Early writes go on the commit ring, as part of the next commit: its flush waits for
    /// them. Too many operations in flight: written at once instead.
    fn start_writes(&self, writes: &[(u64, Bytes)]) -> io::Result<()> {
        let mut st = self.commits.borrow_mut();
        if st.writes.len() + writes.len() + 1 >= QUEUE_DEPTH as usize {
            drop(st);
            let views: Vec<(u64, &[u8])> = writes.iter().map(|(o, d)| (*o, &d[..])).collect();
            return self.write_many(&views);
        }
        let generation = st.next_gen;
        let mut ring = self.commit_ring.borrow_mut();
        self.submit_commit_writes(&mut st, &mut ring, writes, generation)
    }

    /// The writes on the commit ring, then an `fdatasync` once they, and the early writes that
    /// belong to this commit, have completed. The data is copied into aligned buffers. With too
    /// many operations in flight, done synchronously (after this commit's early writes).
    fn start_commit(&self, writes: &[(u64, Bytes)]) -> io::Result<bool> {
        let mut st = self.commits.borrow_mut();
        assert!(st.current.is_none(), "one commit in flight at a time");
        let generation = st.next_gen;
        st.next_gen += 1;
        let mut ring = self.commit_ring.borrow_mut();
        if st.writes.len() + writes.len() + 1 >= QUEUE_DEPTH as usize {
            while st.writes.values().any(|(_, g)| *g <= generation) {
                Self::wait_commits(&mut ring);
                Self::reap_commits(&mut st, &mut ring);
            }
            let early = st.errors.remove(&generation);
            drop((st, ring));
            if let Some(e) = early {
                return Err(e);
            }
            let views: Vec<(u64, &[u8])> = writes.iter().map(|(o, d)| (*o, &d[..])).collect();
            self.write_many(&views)?;
            self.flush()?;
            return Ok(true);
        }
        self.submit_commit_writes(&mut st, &mut ring, writes, generation)?;
        st.current = Some((generation, Flush::Waiting));
        self.advance_commit(&mut st, &mut ring);
        Ok(false)
    }

    fn poll_commit(&self, wait: bool) -> Option<io::Result<()>> {
        let mut st = self.commits.borrow_mut();
        let mut ring = self.commit_ring.borrow_mut();
        loop {
            Self::reap_commits(&mut st, &mut ring);
            self.advance_commit(&mut st, &mut ring);
            match st.current {
                None => return Some(Ok(())),
                Some((generation, Flush::Done(_))) => {
                    let Some((_, Flush::Done(flush))) = st.current.take() else {
                        unreachable!("matched above")
                    };
                    // Errors of this commit's writes, early ones included, come first.
                    let mut error = None;
                    while let Some(entry) = st.errors.first_entry() {
                        if *entry.key() > generation {
                            break;
                        }
                        error.get_or_insert(entry.remove());
                    }
                    return Some(match error.or(flush) {
                        Some(e) => Err(e),
                        None => Ok(()),
                    });
                }
                Some(_) if !wait => return None,
                Some(_) => Self::wait_commits(&mut ring),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_device(size: u64, direct: bool) -> (UringDevice, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "flaminio-uring-{}-{size}-{direct}",
            std::process::id()
        ));
        let f = File::create(&path).unwrap();
        f.set_len(size).unwrap();
        drop(f);
        (UringDevice::open(&path, direct).unwrap(), path)
    }

    #[test]
    fn batched_io_roundtrip_aligned_and_bounced() {
        for direct in [false, true] {
            let (dev, path) = temp_device(16 << 20, direct);
            let aligned: Vec<AlignedBuf> = (0..600u32)
                .map(|i| AlignedBuf::copy_of(&[i as u8; 4096]))
                .collect();
            let writes: Vec<(u64, &[u8])> = aligned
                .iter()
                .enumerate()
                .map(|(i, b)| (i as u64 * 8192, &b[..]))
                .collect();
            dev.write_many(&writes).unwrap();
            // An unaligned (Vec) buffer must be bounced in direct mode.
            dev.write_at(4096, &vec![0xEE; 4096]).unwrap();
            dev.flush().unwrap();
            let mut bufs: Vec<Vec<u8>> = (0..600).map(|_| vec![0u8; 4096]).collect();
            let mut reads: Vec<(u64, &mut [u8])> = bufs
                .iter_mut()
                .enumerate()
                .map(|(i, b)| (i as u64 * 8192, &mut b[..]))
                .collect();
            dev.read_many(&mut reads).unwrap();
            for (i, b) in bufs.iter().enumerate() {
                assert!(b.iter().all(|&x| x == i as u8), "direct={direct} block {i}");
            }
            let mut one = vec![0u8; 4096];
            dev.read_at(4096, &mut one).unwrap();
            assert!(one.iter().all(|&x| x == 0xEE));
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn commits_complete_asynchronously_and_are_durable() {
        for direct in [false, true] {
            let (dev, path) = temp_device(12 << 20, direct);
            let efd = crate::EventFd::new().unwrap();
            dev.notify_commits(efd.as_raw_fd()).unwrap();
            // Half shared and aligned (written as they are), half not (copied).
            let writes: Vec<(u64, Bytes)> = (0..40u8)
                .map(|i| {
                    let d = vec![i; 8192];
                    let b = if i % 2 == 0 {
                        AlignedBuf::shared_copy_of(&d)
                    } else {
                        Bytes::from(d)
                    };
                    (u64::from(i) * 16384, b)
                })
                .collect();
            assert!(!dev.start_commit(&writes).unwrap(), "in flight");
            drop(writes); // the device holds what it needs
            assert!(dev.poll_commit(true).unwrap().is_ok());
            assert!(efd.drain(), "the eventfd was signalled");
            let mut buf = vec![0u8; 8192];
            for i in 0..40u8 {
                dev.read_at(u64::from(i) * 16384, &mut buf).unwrap();
                assert!(buf.iter().all(|&x| x == i), "direct={direct} write {i}");
            }
            // An error (an offset no file can have: negative as a file offset) is reported
            // once everything completed.
            let b = Bytes::from(vec![1u8; 4096]);
            assert!(
                !dev.start_commit(&[(0, b.clone()), (u64::MAX - 8191, b)])
                    .unwrap()
            );
            assert!(dev.poll_commit(true).unwrap().is_err());
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn early_writes_belong_to_the_next_commit() {
        for direct in [false, true] {
            let (dev, path) = temp_device(10 << 20, direct);
            let a = AlignedBuf::shared_copy_of(&[1u8; 8192]);
            let b = Bytes::from(vec![2u8; 8192]);
            let c = AlignedBuf::shared_copy_of(&[3u8; 4096]);
            assert!(!dev.start_commit(&[(0, a)]).unwrap());
            // Early writes for the next commit, while this one is in flight: they do not hold
            // it back, and an error among them is the next commit's.
            dev.start_writes(&[(65536, b.clone()), (u64::MAX - 8191, c.clone())])
                .unwrap();
            assert!(dev.poll_commit(true).unwrap().is_ok(), "direct={direct}");
            assert!(!dev.start_commit(&[(131072, c.clone())]).unwrap());
            assert!(dev.poll_commit(true).unwrap().is_err(), "direct={direct}");
            // Without errors, the next commit covers its early writes.
            dev.start_writes(&[(65536, b.clone())]).unwrap();
            assert!(!dev.start_commit(&[(131072, c.clone())]).unwrap());
            assert!(dev.poll_commit(true).unwrap().is_ok());
            let mut buf = vec![0u8; 8192];
            dev.read_at(65536, &mut buf).unwrap();
            assert!(buf.iter().all(|&x| x == 2), "direct={direct}");
            dev.read_at(0, &mut buf).unwrap();
            assert!(buf.iter().all(|&x| x == 1), "direct={direct}");
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn errors_are_reported_after_draining() {
        let (dev, path) = temp_device(1 << 20, false);
        let data = vec![1u8; 4096];
        let mut buf = vec![0u8; 4096];
        assert!(
            dev.read_at(2 << 20, &mut buf).is_err(),
            "read past the end is a short read"
        );
        dev.write_many(&[(0, &data[..]), (4096, &data[..])])
            .unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
