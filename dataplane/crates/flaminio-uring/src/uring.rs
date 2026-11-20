//! A block device on io_uring with O_DIRECT.
#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use bytes::Bytes;
use flaminio_store::BlockDevice;
use io_uring::squeue::Entry;
use io_uring::{IoUring, opcode, types};

use crate::aligned::{AlignedBuf, is_aligned};

/// Submission queue size. Larger batches are submitted in windows of this many operations.
const QUEUE_DEPTH: u32 = 256;

/// A commit's flush is tagged with this bit and the commit's generation (writes are tagged by
/// a counter, which never reaches it).
const FLUSH_TAG: u64 = 1 << 63;

/// While armed, dropping this aborts: the kernel may still access buffers the unwind would
/// free. Arm immediately before the first SQ mutation; disarm only after every published
/// completion has been consumed (or after every owned commit write/flush has drained).
/// Declare after prepared buffers so this Drop runs before theirs.
struct OwnershipGuard {
    armed: bool,
}

impl OwnershipGuard {
    fn new() -> Self {
        Self { armed: false }
    }

    fn arm(&mut self) {
        self.armed = true;
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for OwnershipGuard {
    fn drop(&mut self) {
        if self.armed {
            eprintln!("flaminio-uring: ownership outstanding during unwind; aborting");
            std::process::abort();
        }
    }
}

/// Unrecoverable ownership: do not return an ordinary error.
fn ownership_fail(what: &str) -> ! {
    eprintln!("flaminio-uring: {what}; aborting");
    std::process::abort();
}

#[cfg(test)]
thread_local! {
    /// Fail preparation of the operation at this index (before any SQ publication).
    static TEST_PREPARE_FAIL_AT: Cell<Option<usize>> = const { Cell::new(None) };
    /// Inject one submit errno (taken once). Retryable vs fatal follows the real errno rules.
    static TEST_SUBMIT_ERRNO: Cell<Option<i32>> = const { Cell::new(None) };
    /// Injected (user_data, result) CQEs, consumed before reading the real completion queue.
    static TEST_CQE_SEQ: RefCell<VecDeque<(u64, i32)>> = const { RefCell::new(VecDeque::new()) };
    /// Panic once after the ownership guard is armed (publication/drain path).
    static TEST_PANIC_WHEN_ARMED: Cell<bool> = const { Cell::new(false) };
}

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
    /// 0/1: one fsync per commit; 2: up to two commits share one fdatasync.
    flush_coalesce: Cell<u8>,
    /// Defer the oldest ready flush while the store may still promote a staged partner.
    expect_coalesce_partner: Cell<bool>,
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
    /// The commits in flight, oldest first, and where their flushes are. Several may be in
    /// flight (pipelined commits): each one's flush is submitted once its writes, and those of
    /// every commit before it, have completed, so a completed flush makes every earlier commit
    /// durable too.
    current: VecDeque<(u64, Flush)>,
}

impl Commits {
    /// Operations in flight on the commit ring.
    fn in_flight(&self) -> usize {
        self.writes.len()
            + self
                .current
                .iter()
                .filter(|(_, f)| matches!(f, Flush::Submitted | Flush::Covered))
                .count()
    }
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
    /// This commit's own fsync is in flight (covers only itself, or is the tag for a coalesced
    /// group — see [`Flush::Covered`]).
    Submitted,
    /// Shares the previous commit's in-flight fsync; becomes Done when that CQE arrives.
    Covered,
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
            flush_coalesce: Cell::new(0),
            expect_coalesce_partner: Cell::new(false),
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
    /// error, but only after every submitted operation has completed. A preparation failure
    /// leaves that window unpublished; earlier windows of the same call may already have
    /// completed (not transactional across windows).
    fn run(&self, ops: &mut [Op<'_>]) -> io::Result<()> {
        let fd = types::Fd(self.file.as_raw_fd());
        let mut ring = self.ring.borrow_mut();
        let mut first_error = None;
        for window in ops.chunks_mut(QUEUE_DEPTH as usize) {
            // --- Preparation (no SQ publication) -------------------------------------------
            // Lengths first: an unsupported length never allocates a bounce buffer.
            let mut lens: Vec<u32> = Vec::with_capacity(window.len());
            for op in window.iter() {
                match op {
                    Op::Read(_, buf) => lens.push(len32(buf.len())?),
                    Op::Write(_, data) => lens.push(len32(data.len())?),
                    Op::Fsync => lens.push(0),
                }
            }

            let mut bounce: Vec<Option<AlignedBuf>> = Vec::with_capacity(window.len());
            for (i, op) in window.iter().enumerate() {
                #[cfg(test)]
                if TEST_PREPARE_FAIL_AT.with(|c| c.get()) == Some(i) {
                    return Err(io::Error::other("injected preparation failure"));
                }
                #[cfg(not(test))]
                let _ = i;
                match op {
                    Op::Read(_, buf) if self.needs_bounce(buf) => {
                        bounce.push(Some(AlignedBuf::zeroed(buf.len())));
                    }
                    Op::Write(_, data) if self.needs_bounce(data) => {
                        bounce.push(Some(AlignedBuf::copy_of(data)));
                    }
                    _ => bounce.push(None),
                }
            }

            let mut entries: Vec<Entry> = Vec::with_capacity(window.len());
            for (i, op) in window.iter_mut().enumerate() {
                let entry = match op {
                    Op::Read(offset, buf) => {
                        let ptr = match bounce[i].as_mut() {
                            Some(b) => b.as_mut_ptr(),
                            None => buf.as_mut_ptr(),
                        };
                        opcode::Read::new(fd, ptr, lens[i])
                            .offset(*offset)
                            .build()
                            .user_data(i as u64)
                    }
                    Op::Write(offset, data) => {
                        let ptr = match bounce[i].as_ref() {
                            Some(b) => b.as_ptr(),
                            None => data.as_ptr(),
                        };
                        opcode::Write::new(fd, ptr, lens[i])
                            .offset(*offset)
                            .build()
                            .user_data(i as u64)
                    }
                    Op::Fsync => opcode::Fsync::new(fd)
                        .flags(types::FsyncFlags::DATASYNC)
                        .build()
                        .user_data(i as u64),
                };
                entries.push(entry);
            }
            let mut seen = vec![false; window.len()];

            {
                let sq = ring.submission();
                if !sq.is_empty() {
                    ownership_fail("synchronous ring has leftover SQEs before publication");
                }
                if sq.capacity() < window.len() {
                    ownership_fail("synchronous window exceeds ring capacity");
                }
            }

            // Guard after prepared buffers: on unwind it aborts before `bounce` is dropped.
            let mut guard = OwnershipGuard::new();
            guard.arm();
            #[cfg(test)]
            if TEST_PANIC_WHEN_ARMED.with(|c| c.replace(false)) {
                panic!("injected panic while ownership guard armed");
            }

            // --- Publication: no allocation, length conversion, `?`, or expect --------------
            {
                let mut sq = ring.submission();
                for (i, entry) in entries.iter().enumerate() {
                    // SAFETY: each entry references either a caller buffer borrowed for the
                    // whole `run` call, or a bounce buffer in `bounce` (heap allocation; moving
                    // the `AlignedBuf` does not move those bytes). `guard` stays armed until
                    // every CQE for this window has been consumed below, then is disarmed
                    // before read copyback — the exact release point for these borrows.
                    if unsafe { sq.push(entry) }.is_err() {
                        ownership_fail(if i == 0 {
                            "unexpected SQ push failure before any entry was published"
                        } else {
                            "SQ push failed after publishing a prefix of the window"
                        });
                    }
                }
            }

            // --- Drain: every published op gets exactly one completion --------------------
            let mut done = 0;
            while done < window.len() {
                if let Err(e) = submit_and_wait_or_inject(&mut ring, window.len() - done) {
                    match e.raw_os_error() {
                        Some(libc::EINTR | libc::EAGAIN | libc::EBUSY) => {}
                        // The kernel may still own our buffers; returning would let them be
                        // freed under it. Stop the process instead (fail-stop).
                        _ => ownership_fail(&format!("io_uring_enter failed: {e}")),
                    }
                }
                for_each_cqe(&mut ring, |user_data, res| {
                    if user_data >= window.len() as u64 {
                        ownership_fail("CQE user_data outside the published window");
                    }
                    let i = user_data as usize;
                    if seen[i] {
                        ownership_fail("duplicate CQE for a window operation");
                    }
                    seen[i] = true;
                    done += 1;
                    let expected = match &window[i] {
                        Op::Read(_, buf) => buf.len(),
                        Op::Write(_, data) => data.len(),
                        Op::Fsync => 0,
                    };
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
                });
            }

            if ring.completion().next().is_some() {
                ownership_fail("leftover CQEs after draining a synchronous window");
            }
            if !ring.submission().is_empty() {
                ownership_fail("leftover SQEs after draining a synchronous window");
            }

            // Kernel owns no buffers from this window; disarm before copyback/drop of bounce.
            guard.disarm();

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

fn clone_err(e: &io::Error) -> io::Error {
    io::Error::new(e.kind(), e.to_string())
}

fn submit_and_wait_or_inject(ring: &mut IoUring, want: usize) -> io::Result<()> {
    #[cfg(test)]
    if let Some(errno) = TEST_SUBMIT_ERRNO.take() {
        return Err(io::Error::from_raw_os_error(errno));
    }
    ring.submit_and_wait(want).map(|_| ())
}

fn for_each_cqe(ring: &mut IoUring, mut f: impl FnMut(u64, i32)) {
    #[cfg(test)]
    {
        while let Some((user_data, result)) = TEST_CQE_SEQ.with(|q| q.borrow_mut().pop_front()) {
            f(user_data, result);
        }
    }
    for cqe in ring.completion() {
        f(cqe.user_data(), cqe.result());
    }
}

impl std::os::fd::AsRawFd for UringDevice {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.file.as_raw_fd()
    }
}

impl UringDevice {
    /// Submits `writes` on the commit ring, as part of commit `generation`. Aligned shared
    /// buffers are written as they are (a reference is held until completion); others are
    /// copied into aligned buffers. Preparation failures leave no new queued work and do not
    /// mutate generation accounting owned by the caller.
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
        let n = writes.len() as u64;
        if n > 0 && (st.next_tag >= FLUSH_TAG || FLUSH_TAG - st.next_tag < n) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "commit write tag space exhausted",
            ));
        }

        // Prepare Held buffers and tags before any SQ mutation or ownership install.
        let mut prepared: Vec<(u64, u64, Held, u32)> = Vec::with_capacity(writes.len());
        for (i, ((offset, data), &len)) in writes.iter().zip(&lens).enumerate() {
            #[cfg(test)]
            if TEST_PREPARE_FAIL_AT.with(|c| c.get()) == Some(i) {
                return Err(io::Error::other("injected preparation failure"));
            }
            let tag = st.next_tag + i as u64;
            prepared.push((tag, *offset, Held::new(data, self.direct), len));
        }
        st.writes.reserve(writes.len());

        {
            let sq = ring.submission();
            if !sq.is_empty() {
                ownership_fail("commit ring has leftover SQEs before publication");
            }
            if sq.capacity() < writes.len() {
                ownership_fail("commit writes exceed ring capacity");
            }
        }

        let fd = types::Fd(self.file.as_raw_fd());
        let mut guard = OwnershipGuard::new();
        guard.arm();
        #[cfg(test)]
        if TEST_PANIC_WHEN_ARMED.with(|c| c.replace(false)) {
            panic!("injected panic while ownership guard armed");
        }

        {
            let mut sq = ring.submission();
            for (i, (tag, offset, buf, len)) in prepared.into_iter().enumerate() {
                let ptr = buf.bytes().as_ptr();
                let entry = opcode::Write::new(fd, ptr, len)
                    .offset(offset)
                    .build()
                    .user_data(tag);
                // Install ownership before the entry can reach the kernel.
                st.writes.insert(tag, (buf, generation));
                st.next_tag = tag + 1;
                // SAFETY: the buffer (an owned copy, or a reference to immutable shared bytes)
                // is in `st.writes` and stays there until this operation's completion is reaped
                // (`reap_commits`, also on drop under an armed `OwnershipGuard`); moving a
                // `Held` does not move the bytes it points to. `guard` stays armed through
                // `submit_or_abort` below — the release point for this publication window.
                if unsafe { sq.push(&entry) }.is_err() {
                    ownership_fail(if i == 0 {
                        "unexpected commit SQ push failure before any entry was published"
                    } else {
                        "commit SQ push failed after publishing a prefix"
                    });
                }
            }
        }
        submit_or_abort(ring, "a commit's writes");
        guard.disarm();
        Ok(())
    }

    /// Takes the commit ring's completions. Unknown, duplicate or impossible CQEs fail-stop.
    fn reap_commits(st: &mut Commits, ring: &mut IoUring) {
        for_each_cqe(ring, |tag, res| {
            if tag & FLUSH_TAG != 0 {
                let generation = tag & !FLUSH_TAG;
                let Some(pos) = st.current.iter().position(|(g, _)| *g == generation) else {
                    ownership_fail("flush CQE for unknown commit generation");
                };
                match &st.current[pos].1 {
                    Flush::Submitted => {}
                    Flush::Waiting => ownership_fail("flush CQE for a Waiting commit"),
                    Flush::Covered => ownership_fail("flush CQE for a Covered commit"),
                    Flush::Done(_) => ownership_fail("duplicate flush CQE"),
                }
                // Only result 0 is successful fsync; negative is an error; positive is impossible.
                let error = if res < 0 {
                    Some(io::Error::from_raw_os_error(-res))
                } else if res == 0 {
                    None
                } else {
                    ownership_fail("flush CQE with positive result");
                };
                // The tagged commit and any Covered partners that share this fsync.
                st.current[pos].1 = Flush::Done(error.as_ref().map(clone_err));
                for (_, flush) in st.current.iter_mut().skip(pos + 1) {
                    if matches!(flush, Flush::Covered) {
                        *flush = Flush::Done(error.as_ref().map(clone_err));
                    } else {
                        break;
                    }
                }
                return;
            }
            let Some((buf, generation)) = st.writes.remove(&tag) else {
                ownership_fail("write CQE for unknown or already-reaped tag");
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
            // `buf` drops here. If a Shared owner's destructor panics while other writes remain
            // outstanding, an armed Drop/publication guard aborts; ordinary poll keeps the
            // remaining Held values in `st.writes`.
            drop(buf);
        });
    }

    /// Submits each commit's flush, in order, once every write that belongs to it or to an
    /// earlier commit has completed. With flush coalesce, up to two ready commits share one
    /// `fdatasync`; the oldest waits for a partner while [`Self::set_expect_coalesce_partner`]
    /// is set and no second commit is in flight yet.
    fn advance_commit(&self, st: &mut Commits, ring: &mut IoUring) {
        let coalesce = self.flush_coalesce.get().max(1) as usize;
        // First Waiting commit whose writes (and earlier gens') are done.
        let Some(start) = st.current.iter().position(|(generation, flush)| {
            matches!(flush, Flush::Waiting) && !st.writes.values().any(|(_, g)| *g <= *generation)
        }) else {
            return;
        };
        // An earlier commit still in flight must finish first.
        if st
            .current
            .iter()
            .take(start)
            .any(|(_, f)| matches!(f, Flush::Waiting | Flush::Submitted | Flush::Covered))
        {
            return;
        }
        let mut n = 0usize;
        for (generation, flush) in st.current.iter().skip(start) {
            if !matches!(flush, Flush::Waiting) {
                break;
            }
            if st.writes.values().any(|(_, g)| *g <= *generation) {
                break;
            }
            n += 1;
            if n >= coalesce {
                break;
            }
        }
        if n == 0 {
            return;
        }
        if n == 1 && coalesce >= 2 {
            let has_later = st.current.len() > start + 1;
            if !has_later && self.expect_coalesce_partner.get() {
                return;
            }
            if has_later
                && matches!(
                    st.current.get(start + 1).map(|(_, f)| f),
                    Some(Flush::Waiting)
                )
            {
                // Partner's writes are still in flight.
                return;
            }
        }
        let take = n.min(coalesce);
        let tag_gen = st.current[start].0;
        let entry = opcode::Fsync::new(types::Fd(self.file.as_raw_fd()))
            .flags(types::FsyncFlags::DATASYNC)
            .build()
            .user_data(FLUSH_TAG | tag_gen);
        // SAFETY: references no buffer memory. make_room keeps the ring from filling; a push
        // failure here is an internal ownership failure and fail-stops.
        if unsafe { ring.submission().push(&entry) }.is_err() {
            ownership_fail("commit flush SQ push failed");
        }
        st.current[start].1 = Flush::Submitted;
        for i in 1..take {
            st.current[start + i].1 = Flush::Covered;
        }
        submit_or_abort(ring, "a commit's flush");
    }

    /// Waits until `n` more writes and a flush fit on the commit ring. Returns false if they
    /// do not fit even with nothing in flight.
    fn make_room(&self, st: &mut Commits, ring: &mut IoUring, n: usize) -> bool {
        while st.in_flight() + n + 1 > QUEUE_DEPTH as usize {
            if st.in_flight() == 0 {
                return false;
            }
            Self::wait_commits(ring);
            Self::reap_commits(st, ring);
            self.advance_commit(st, ring);
        }
        true
    }

    /// Waits for at least one completion on the commit ring.
    fn wait_commits(ring: &mut IoUring) {
        if let Err(e) = submit_and_wait_or_inject(ring, 1)
            && !matches!(
                e.raw_os_error(),
                Some(libc::EINTR | libc::EAGAIN | libc::EBUSY)
            )
        {
            ownership_fail(&format!("io_uring_enter failed waiting for a commit: {e}"));
        }
    }
}

/// Submits what is queued on `ring`. A failure here may leave entries with the kernel, buffers
/// included, so the process stops (fail-stop).
fn submit_or_abort(ring: &mut IoUring, what: &str) {
    loop {
        #[cfg(test)]
        let result = if let Some(errno) = TEST_SUBMIT_ERRNO.take() {
            Err(io::Error::from_raw_os_error(errno))
        } else {
            ring.submit()
        };
        #[cfg(not(test))]
        let result = ring.submit();
        match result {
            Ok(_) => return,
            Err(e)
                if matches!(
                    e.raw_os_error(),
                    Some(libc::EINTR | libc::EAGAIN | libc::EBUSY)
                ) => {}
            Err(e) => ownership_fail(&format!("io_uring_enter failed for {what}: {e}")),
        }
    }
}

impl Drop for UringDevice {
    fn drop(&mut self) {
        // Arm before any Held destructor can run: a panicking Bytes owner must not unwind
        // into destruction of remaining kernel-owned allocations. Disarm only after both
        // queues and all accepted writes/commits are drained.
        let mut guard = OwnershipGuard::new();
        guard.arm();
        let st = self.commits.get_mut();
        let ring = self.commit_ring.get_mut();
        loop {
            Self::reap_commits(st, ring);
            if st.in_flight() == 0 {
                guard.disarm();
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
    /// them. Too many for the ring even when empty: written at once instead.
    fn start_writes(&self, writes: &[(u64, Bytes)]) -> io::Result<()> {
        // Length preflight before any ownership or ring mutation for this request.
        for (_, d) in writes {
            let _ = len32(d.len())?;
        }
        let mut st = self.commits.borrow_mut();
        let mut ring = self.commit_ring.borrow_mut();
        if !self.make_room(&mut st, &mut ring, writes.len()) {
            drop((st, ring));
            let views: Vec<(u64, &[u8])> = writes.iter().map(|(o, d)| (*o, &d[..])).collect();
            return self.write_many(&views);
        }
        let generation = st.next_gen;
        self.submit_commit_writes(&mut st, &mut ring, writes, generation)
    }

    /// The writes on the commit ring, then an `fdatasync` once they, the early writes that
    /// belong to this commit, and the writes of every commit before it have completed. Commits
    /// already in flight stay so (pipelined commits); they complete in order. A commit too big
    /// for the ring even when empty is done synchronously. A pre-publication rejection does
    /// not advance `next_gen`, erase early-write errors, or strand early writes.
    fn start_commit(&self, writes: &[(u64, Bytes)]) -> io::Result<bool> {
        // Length/layout preflight before changing this request's generation.
        for (_, d) in writes {
            let _ = len32(d.len())?;
        }
        let mut st = self.commits.borrow_mut();
        let generation = st.next_gen;
        let mut ring = self.commit_ring.borrow_mut();
        if !self.make_room(&mut st, &mut ring, writes.len()) {
            // Nothing in flight: no earlier commit to stay ahead of.
            let early = st.errors.remove(&generation);
            if let Some(e) = early {
                // Sync path still accepts this generation as failed so early writes are not
                // stranded; only pre-publication rejection leaves next_gen unchanged.
                st.next_gen = generation + 1;
                return Err(e);
            }
            drop((st, ring));
            // Sync acceptance of this generation (mirrors advancing before I/O previously).
            self.commits.borrow_mut().next_gen = generation + 1;
            let views: Vec<(u64, &[u8])> = writes.iter().map(|(o, d)| (*o, &d[..])).collect();
            self.write_many(&views)?;
            self.flush()?;
            return Ok(true);
        }
        st.current.reserve(1);
        self.submit_commit_writes(&mut st, &mut ring, writes, generation)?;
        // Publication accepted: advance generation and record the waiting flush.
        st.next_gen = generation + 1;
        st.current.push_back((generation, Flush::Waiting));
        self.advance_commit(&mut st, &mut ring);
        Ok(false)
    }

    fn poll_commit(&self, wait: bool) -> Option<io::Result<()>> {
        let mut st = self.commits.borrow_mut();
        let mut ring = self.commit_ring.borrow_mut();
        loop {
            Self::reap_commits(&mut st, &mut ring);
            self.advance_commit(&mut st, &mut ring);
            // The oldest commit first: commits complete in order.
            match st.current.front() {
                None => return Some(Ok(())),
                Some(&(generation, Flush::Done(_))) => {
                    let Some((_, Flush::Done(flush))) = st.current.pop_front() else {
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

    fn set_flush_coalesce(&self, n: u8) {
        self.flush_coalesce.set(n);
    }

    fn set_expect_coalesce_partner(&self, expect: bool) {
        self.expect_coalesce_partner.set(expect);
        if !expect {
            // Partner will not join: flush any commit that was waiting alone.
            let mut st = self.commits.borrow_mut();
            let mut ring = self.commit_ring.borrow_mut();
            self.advance_commit(&mut st, &mut ring);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-only: fail preparation at `index` (before SQ publication). Clear with `None`.
    #[allow(dead_code)]
    fn inject_prepare_fail_at(index: Option<usize>) {
        TEST_PREPARE_FAIL_AT.with(|c| c.set(index));
    }

    /// Test-only: next submit returns this errno once.
    #[allow(dead_code)]
    fn inject_submit_errno(errno: Option<i32>) {
        TEST_SUBMIT_ERRNO.with(|c| c.set(errno));
    }

    /// Test-only: prepend injected CQEs consumed before the real completion queue.
    #[allow(dead_code)]
    fn inject_cqe_seq(seq: impl IntoIterator<Item = (u64, i32)>) {
        TEST_CQE_SEQ.with(|q| {
            let mut q = q.borrow_mut();
            q.clear();
            q.extend(seq);
        });
    }

    /// Test-only: panic once after the ownership guard is armed.
    #[allow(dead_code)]
    fn inject_panic_when_armed(panic: bool) {
        TEST_PANIC_WHEN_ARMED.with(|c| c.set(panic));
    }

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
    fn pipelined_commits_complete_in_order() {
        for direct in [false, true] {
            let (dev, path) = temp_device(8 << 20, direct);
            let bufs: Vec<Bytes> = (0..4u8)
                .map(|i| AlignedBuf::shared_copy_of(&[i + 1; 8192]))
                .collect();
            // Four commits in flight at once; the third has an error, which is its own.
            for (i, b) in bufs.iter().enumerate() {
                let mut writes = vec![(i as u64 * 65536, b.clone())];
                if i == 2 {
                    writes.push((u64::MAX - 8191, b.clone()));
                }
                assert!(!dev.start_commit(&writes).unwrap(), "direct={direct}");
            }
            let outcomes: Vec<bool> = (0..4)
                .map(|_| dev.poll_commit(true).unwrap().is_ok())
                .collect();
            assert_eq!(outcomes, [true, true, false, true], "direct={direct}");
            let mut buf = vec![0u8; 8192];
            for i in [0u8, 1, 3] {
                dev.read_at(u64::from(i) * 65536, &mut buf).unwrap();
                assert!(
                    buf.iter().all(|&x| x == i + 1),
                    "direct={direct} commit {i}"
                );
            }
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
