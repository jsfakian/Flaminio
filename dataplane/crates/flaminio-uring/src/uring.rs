//! A block device on io_uring with O_DIRECT.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use flaminio_store::BlockDevice;
use io_uring::{IoUring, opcode, types};

use crate::aligned::{AlignedBuf, is_aligned};

/// Submission queue size. Larger batches are submitted in windows of this many operations.
const QUEUE_DEPTH: u32 = 256;

/// A file or block device driven through io_uring.
///
/// With `direct` (the normal case), the device is opened with `O_DIRECT`: no page cache, and
/// durability is exactly what [`flush`](BlockDevice::flush) (an `fdatasync`) provides.
/// Unaligned buffers are bounced through [`AlignedBuf`]s; pass aligned buffers to avoid the
/// copy. The device is single-threaded, like the store that owns it.
pub struct UringDevice {
    file: File,
    size: u64,
    direct: bool,
    ring: RefCell<IoUring>,
}

enum Op<'a> {
    Read(u64, &'a mut [u8]),
    Write(u64, &'a [u8]),
    Fsync,
}

impl UringDevice {
    /// Opens `path` read-write, with `O_DIRECT` if `direct`.
    pub fn open(path: impl AsRef<Path>, direct: bool) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        if direct {
            options.custom_flags(libc::O_DIRECT);
        }
        let mut file = options.open(path)?;
        let size = file.seek(SeekFrom::End(0))?;
        let ring = IoUring::builder()
            .setup_cqsize(2 * QUEUE_DEPTH)
            .build(QUEUE_DEPTH)?;
        Ok(Self {
            file,
            size,
            direct,
            ring: RefCell::new(ring),
        })
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
        let mut ops: Vec<Op<'_>> = reads
            .iter_mut()
            .map(|(o, b)| Op::Read(*o, &mut **b))
            .collect();
        self.run(&mut ops)
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
