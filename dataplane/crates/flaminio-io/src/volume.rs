//! A volume: a linear block address space laid over fixed-size chunks in the replica store.
//!
//! The frontend hands this module a batch of block requests. It splits them at chunk
//! boundaries, runs all writes as one store group commit and all reads as one batch, and
//! returns one result per request, in the form ublk expects: bytes transferred, or a negative
//! errno.

use std::io;
use std::time::{Duration, Instant};

use flaminio_format::Uuid;
use flaminio_format::checksum::crc32c;
use flaminio_store::{BlockDevice, ChunkKey, ReadPlan, ReadReq, Store, StoreError, WriteReq};
use flaminio_uring::AlignedBuf;

use crate::proto::{UNIT, WriteItem};
use crate::remote::{COPY_UNITS, Health, Remote, SCAN_UNITS};

/// Block request operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Read into the buffer.
    Read,
    /// Write the buffer.
    Write,
    /// Make earlier writes durable. A no-op: every completed write is already durable.
    Flush,
    /// Discard the range (advisory; whole clusters read back as zeros).
    Discard,
}

/// One block request.
#[derive(Debug)]
pub struct Request<'a> {
    /// Operation.
    pub op: Op,
    /// Byte offset in the volume.
    pub offset: u64,
    /// Byte length.
    pub len: u64,
    /// Data buffer: at least `len` bytes for reads and writes; unused otherwise.
    pub buf: &'a mut [u8],
}

/// Shape of a volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeLayout {
    /// Volume identity.
    pub volume: Uuid,
    /// Size in bytes: a whole number of chunks.
    pub size: u64,
    /// Chunk size in bytes.
    pub chunk_size: u64,
}

/// A piece of a request that falls inside one chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    /// Chunk index.
    pub chunk: u64,
    /// Byte offset within the chunk.
    pub chunk_offset: u64,
    /// Byte offset within the request buffer.
    pub buf_offset: usize,
    /// Length in bytes.
    pub len: u64,
}

impl VolumeLayout {
    /// Number of chunks.
    pub fn chunks(&self) -> u64 {
        self.size / self.chunk_size
    }

    /// The store key of chunk `index`.
    pub fn key(&self, index: u64) -> ChunkKey {
        ChunkKey {
            volume: self.volume,
            index,
        }
    }

    /// Splits `[offset, offset + len)` at chunk boundaries. `None` if the range leaves the volume.
    pub fn split(&self, offset: u64, len: u64) -> Option<Vec<Piece>> {
        let end = offset.checked_add(len).filter(|&e| e <= self.size)?;
        let mut pieces = Vec::new();
        let mut o = offset;
        while o < end {
            let chunk = o / self.chunk_size;
            let chunk_offset = o % self.chunk_size;
            let n = (self.chunk_size - chunk_offset).min(end - o);
            pieces.push(Piece {
                chunk,
                chunk_offset,
                buf_offset: (o - offset) as usize,
                len: n,
            });
            o += n;
        }
        Some(pieces)
    }
}

/// A volume served from a local replica store, optionally mirrored to one remote replica
/// (MVP steps 4 and 5).
pub struct Volume<D: BlockDevice> {
    store: Store<D>,
    layout: VolumeLayout,
    epoch: u64,
    next_seq: u64,
    remote: Option<Remote>,
}

/// Errno values returned to the block layer.
const EIO: i32 = 5;
const EINVAL: i32 = 22;

pub(crate) fn errno(e: &StoreError) -> i32 {
    match e {
        StoreError::Invalid(_) => -EINVAL,
        _ => -EIO,
    }
}

impl<D: BlockDevice> Volume<D> {
    /// Opens the volume, creating any chunk that does not exist yet.
    pub fn open(mut store: Store<D>, layout: VolumeLayout, epoch: u64) -> Result<Self, StoreError> {
        if layout.chunk_size == 0
            || layout.size == 0
            || !layout.size.is_multiple_of(layout.chunk_size)
        {
            return Err(StoreError::Invalid(
                "volume size must be a non-zero multiple of the chunk size".into(),
            ));
        }
        let mut next_seq = 1;
        for i in 0..layout.chunks() {
            match store.chunk_info(layout.key(i)) {
                Some(info) if info.chunk_size != layout.chunk_size => {
                    return Err(StoreError::Invalid(format!(
                        "chunk {i} exists with size {}; reformat or use the same chunk size",
                        info.chunk_size
                    )));
                }
                Some(info) => next_seq = next_seq.max(info.last_write_seq + 1),
                None => store.create_chunk(layout.key(i), layout.chunk_size, layout.size, epoch)?,
            }
        }
        Ok(Self {
            store,
            layout,
            epoch,
            next_seq,
            remote: None,
        })
    }

    /// Mirrors writes to a replica at `addr`. The connection is made by [`maintain`](Self::maintain).
    pub fn attach_remote(&mut self, addr: String, timeout: Duration) {
        self.remote = Some(Remote::new(addr, timeout));
    }

    /// State of the remote replica, if there is one.
    pub fn remote_health(&self) -> Option<Health> {
        self.remote.as_ref().map(Remote::health)
    }

    /// Background work for the remote replica, in small steps for up to about 10 ms: reconnect,
    /// compare ranges after a (re)start, copy units it is missing. Call it regularly.
    pub fn maintain(&mut self) {
        let start = Instant::now();
        while self.maintain_step() && start.elapsed() < Duration::from_millis(10) {}
        if let Some(r) = self.remote.as_mut() {
            r.report();
        }
    }

    /// One step of [`maintain`](Self::maintain). Returns whether more work is waiting.
    fn maintain_step(&mut self) -> bool {
        let Volume {
            store,
            layout,
            epoch,
            next_seq,
            remote,
        } = self;
        let Some(r) = remote.as_mut() else {
            return false;
        };
        let epoch = *epoch;
        if r.conn.is_none() {
            if Instant::now() < r.next_attempt {
                return false;
            }
            if let Err(e) = r.connect(layout, epoch) {
                r.fail(&e);
                return false;
            }
            eprintln!("replica {}: connected", r.addr);
        }
        if let Some((chunk, unit)) = r.scan {
            let units = layout.chunk_size / UNIT;
            let count = u64::from(SCAN_UNITS).min(units - unit) as u32;
            let compared = unit_digests(
                store,
                layout.key(chunk),
                unit,
                count,
                epoch,
                layout.chunk_size,
            )
            .map_err(io::Error::other)
            .and_then(|local| Ok((local, r.digests(epoch, chunk, unit, count)?)));
            match compared {
                Ok((local, theirs)) if local.len() == theirs.len() => {
                    for (i, (a, b)) in local.iter().zip(&theirs).enumerate() {
                        if a != b {
                            r.dirty.insert((chunk, unit + i as u64));
                        }
                    }
                    let next = unit + u64::from(count);
                    r.scan = if next < units {
                        Some((chunk, next))
                    } else if chunk + 1 < layout.chunks() {
                        Some((chunk + 1, 0))
                    } else {
                        None
                    };
                }
                Ok(_) => r.fail(&io::Error::other(
                    "replica returned the wrong number of digests",
                )),
                Err(e) => r.fail(&e),
            }
            return r.conn.is_some();
        }
        if !r.dirty.is_empty() {
            let batch: Vec<(u64, u64)> = r.dirty.iter().take(COPY_UNITS).copied().collect();
            let mut items = Vec::with_capacity(batch.len());
            for &(chunk, unit) in &batch {
                let mut data = vec![0u8; UNIT as usize];
                if let Err(e) = store.read(layout.key(chunk), unit * UNIT, &mut data, epoch) {
                    // The local copy cannot be read; there is nothing correct to send.
                    eprintln!(
                        "replica {}: cannot read local chunk {chunk} unit {unit} to resync it: {e}",
                        r.addr
                    );
                    return false;
                }
                *next_seq += 1;
                items.push(WriteItem {
                    chunk,
                    offset: unit * UNIT,
                    write_seq: *next_seq,
                    data,
                });
            }
            let n = items.len();
            match r
                .send_writes(epoch, items)
                .and_then(|()| r.finish_writes(n))
            {
                Ok(()) => batch.iter().for_each(|u| {
                    r.dirty.remove(u);
                }),
                Err(e) => r.fail(&e),
            }
        }
        r.conn.is_some() && (r.scan.is_some() || !r.dirty.is_empty())
    }

    /// The underlying store.
    pub fn store(&self) -> &Store<D> {
        &self.store
    }

    /// Plans an asynchronous read of `buf.len()` bytes at `offset` (see [`Store::plan_read`]).
    /// Returns one plan per chunk piece, with the piece's offset in `buf`. Pass them to
    /// [`finish_read`](Self::finish_read) once their device reads are done.
    pub fn plan_read(&self, offset: u64, buf: &mut [u8]) -> Result<Vec<(usize, ReadPlan)>, i32> {
        let pieces = self.layout.split(offset, buf.len() as u64).ok_or(-EINVAL)?;
        let mut plans = Vec::with_capacity(pieces.len());
        for p in pieces {
            let piece = &mut buf[p.buf_offset..p.buf_offset + p.len as usize];
            match self
                .store
                .plan_read(self.layout.key(p.chunk), p.chunk_offset, piece, self.epoch)
            {
                Ok(plan) => plans.push((p.buf_offset, plan)),
                Err(e) => {
                    self.finish_read(plans);
                    return Err(errno(&e));
                }
            }
        }
        Ok(plans)
    }

    /// Ends plans from [`plan_read`](Self::plan_read).
    pub fn finish_read(&self, plans: Vec<(usize, ReadPlan)>) {
        for (_, plan) in plans {
            self.store.finish_read(plan);
        }
    }

    /// Processes a batch of requests. All writes (and discards) are applied before all reads;
    /// requests in one batch are concurrent, so any order is valid. Returns, per request, the
    /// bytes transferred or a negative errno.
    pub fn process(&mut self, reqs: &mut [Request<'_>]) -> Vec<i32> {
        let mut results: Vec<i32> = reqs
            .iter()
            .map(|r| r.len.min(i32::MAX as u64) as i32)
            .collect();
        let mut pieces = Vec::with_capacity(reqs.len());
        for (i, r) in reqs.iter().enumerate() {
            match self.layout.split(r.offset, r.len) {
                Some(p)
                    if r.op == Op::Flush || r.op == Op::Discard || r.buf.len() as u64 >= r.len =>
                {
                    pieces.push(p)
                }
                _ => {
                    results[i] = -EINVAL;
                    pieces.push(Vec::new());
                }
            }
        }

        // Writes: one group commit.
        let mut writes = Vec::new();
        let mut owner = Vec::new();
        for (i, r) in reqs.iter().enumerate() {
            if r.op != Op::Write || results[i] < 0 {
                continue;
            }
            for p in &pieces[i] {
                writes.push(WriteReq {
                    key: self.layout.key(p.chunk),
                    offset: p.chunk_offset,
                    data: &r.buf[p.buf_offset..p.buf_offset + p.len as usize],
                    write_seq: self.next_seq,
                    epoch: self.epoch,
                });
                self.next_seq += 1;
                owner.push(i);
            }
        }
        if !writes.is_empty() {
            // Send to the replica first, so both commit at the same time.
            let mut sent = false;
            if let Some(r) = self.remote.as_mut().filter(|r| r.conn.is_some()) {
                let items = writes
                    .iter()
                    .map(|w| WriteItem {
                        chunk: w.key.index,
                        offset: w.offset,
                        write_seq: w.write_seq,
                        data: w.data.to_vec(),
                    })
                    .collect();
                match r.send_writes(self.epoch, items) {
                    Ok(()) => sent = true,
                    Err(e) => r.fail(&e),
                }
            }
            let local = self.store.write_batch(&writes);
            if let Some(r) = self.remote.as_mut() {
                let mirrored = sent
                    && match r.finish_writes(writes.len()) {
                        Ok(()) => true,
                        Err(e) => {
                            r.fail(&e);
                            false
                        }
                    };
                if !mirrored {
                    // The replica may be missing these: resync them once it is back.
                    for w in &writes {
                        r.mark_dirty(w.key.index, w.offset, w.data.len() as u64);
                    }
                }
            }
            match local {
                Ok(rs) => {
                    for (r, &i) in rs.iter().zip(&owner) {
                        if let Err(e) = r {
                            results[i] = errno(e);
                        }
                    }
                }
                Err(_) => owner.iter().for_each(|&i| results[i] = -EIO),
            }
        }

        // Discards: advisory, one at a time (rare).
        for (i, r) in reqs.iter().enumerate() {
            if r.op != Op::Discard || results[i] < 0 {
                continue;
            }
            for p in &pieces[i] {
                if let Err(e) =
                    self.store
                        .discard(self.layout.key(p.chunk), p.chunk_offset, p.len, self.epoch)
                {
                    results[i] = errno(&e);
                }
                if let Some(r) = self.remote.as_mut() {
                    let forwarded = r.conn.is_some()
                        && match r.discard(self.epoch, p.chunk, p.chunk_offset, p.len) {
                            Ok(()) => true,
                            Err(e) => {
                                r.fail(&e);
                                false
                            }
                        };
                    if !forwarded {
                        r.mark_dirty(p.chunk, p.chunk_offset, p.len);
                    }
                }
            }
        }

        // Reads: one batch, every device read in flight together.
        let mut reads = Vec::new();
        let mut owner = Vec::new();
        for (i, r) in reqs.iter_mut().enumerate() {
            if r.op != Op::Read || results[i] < 0 {
                continue;
            }
            let mut rest: &mut [u8] = &mut r.buf[..r.len as usize];
            for p in &pieces[i] {
                let (piece, tail) = std::mem::take(&mut rest).split_at_mut(p.len as usize);
                rest = tail;
                reads.push(ReadReq {
                    key: self.layout.key(p.chunk),
                    offset: p.chunk_offset,
                    buf: piece,
                    epoch: self.epoch,
                });
                owner.push(i);
            }
        }
        if !reads.is_empty() {
            match self.store.read_batch(&mut reads) {
                Ok(rs) => {
                    for (r, &i) in rs.iter().zip(&owner) {
                        if let Err(e) = r {
                            results[i] = errno(e);
                        }
                    }
                }
                Err(_) => owner.iter().for_each(|&i| results[i] = -EIO),
            }
        }

        for (i, r) in reqs.iter().enumerate() {
            if r.op == Op::Flush && results[i] >= 0 {
                results[i] = 0;
            }
        }
        results
    }
}

/// CRC-32C of each 64 KiB unit of `count` units of a chunk from `first_unit` (clipped to the
/// chunk). Unmapped data reads as zeros, so two replicas agree exactly when their contents do.
pub(crate) fn unit_digests<D: BlockDevice>(
    store: &Store<D>,
    key: ChunkKey,
    first_unit: u64,
    count: u32,
    epoch: u64,
    chunk_size: u64,
) -> Result<Vec<u32>, StoreError> {
    const STEP: u64 = 16;
    let end = (first_unit + u64::from(count)).min(chunk_size / UNIT);
    let mut buf = AlignedBuf::zeroed((STEP * UNIT) as usize);
    let mut crcs = Vec::with_capacity(end.saturating_sub(first_unit) as usize);
    let mut u = first_unit;
    while u < end {
        let n = (end - u).min(STEP);
        let len = (n * UNIT) as usize;
        store.read(key, u * UNIT, &mut buf[..len], epoch)?;
        crcs.extend(buf[..len].chunks_exact(UNIT as usize).map(crc32c));
        u += n;
    }
    Ok(crcs)
}

impl<D: BlockDevice> Volume<D> {
    /// Digests of every unit of every chunk, for comparing replicas offline.
    pub fn digests(&self) -> Result<Vec<Vec<u32>>, StoreError> {
        (0..self.layout.chunks())
            .map(|c| {
                let units = (self.layout.chunk_size / UNIT) as u32;
                unit_digests(
                    &self.store,
                    self.layout.key(c),
                    0,
                    units,
                    self.epoch,
                    self.layout.chunk_size,
                )
            })
            .collect()
    }
}

#[cfg(test)]
impl<D: BlockDevice> Volume<D> {
    /// The replica's digests of every unit, over the target's own connection.
    pub(crate) fn remote_digests(&mut self) -> io::Result<Vec<Vec<u32>>> {
        let (layout, epoch) = (self.layout, self.epoch);
        let r = self
            .remote
            .as_mut()
            .ok_or_else(|| io::Error::other("no remote"))?;
        let units = (layout.chunk_size / UNIT) as u32;
        (0..layout.chunks())
            .map(|c| r.digests(epoch, c, 0, units))
            .collect()
    }

    /// Simulates a lost connection; the next maintain() reconnects at once.
    pub(crate) fn drop_remote_connection(&mut self) {
        if let Some(r) = self.remote.as_mut() {
            r.conn = None;
            r.next_attempt = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flaminio_format::consts::MIB;
    use flaminio_format::layout::LayoutParams;
    use flaminio_format::mkfs::FormatOptions;
    use flaminio_store::{MemDevice, StoreConfig};

    const CHUNK: u64 = 4 * MIB;

    fn layout() -> VolumeLayout {
        VolumeLayout {
            volume: Uuid([5; 16]),
            size: 4 * CHUNK,
            chunk_size: CHUNK,
        }
    }

    fn volume() -> Volume<MemDevice> {
        let dev = MemDevice::new(256 * MIB);
        let opts = FormatOptions {
            disk_uuid: Uuid([1; 16]),
            cluster_uuid: Uuid::NIL,
            format_nonce: 9,
            created_unix_secs: 0,
            layout: LayoutParams {
                journal_size: Some(16 * MIB),
                ..Default::default()
            },
            label: String::new(),
        };
        Store::format(&dev, &opts).unwrap();
        Volume::open(
            Store::open(dev, StoreConfig::default()).unwrap(),
            layout(),
            1,
        )
        .unwrap()
    }

    #[test]
    fn split_at_chunk_boundaries() {
        let l = layout();
        assert_eq!(
            l.split(0, 4096).unwrap(),
            vec![Piece {
                chunk: 0,
                chunk_offset: 0,
                buf_offset: 0,
                len: 4096
            }]
        );
        let p = l.split(CHUNK - 8192, 16384).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(
            p[1],
            Piece {
                chunk: 1,
                chunk_offset: 0,
                buf_offset: 8192,
                len: 8192
            }
        );
        assert!(l.split(4 * CHUNK - 4096, 8192).is_none());
        assert!(l.split(u64::MAX, 4096).is_none());
    }

    #[test]
    fn batch_crossing_chunks_reads_back() {
        let mut v = volume();
        let mut a = vec![0xAA; 64 * 1024];
        let mut b = vec![0xBB; 8192];
        let mut f = Vec::new();
        let mut reqs = [
            Request {
                op: Op::Write,
                offset: CHUNK - 32 * 1024,
                len: 64 * 1024,
                buf: &mut a,
            },
            Request {
                op: Op::Write,
                offset: 3 * CHUNK,
                len: 8192,
                buf: &mut b,
            },
            Request {
                op: Op::Flush,
                offset: 0,
                len: 0,
                buf: &mut f,
            },
        ];
        assert_eq!(v.process(&mut reqs), vec![65536, 8192, 0]);
        let mut out1 = vec![0u8; 64 * 1024];
        let mut out2 = vec![0u8; 4096];
        let mut bad = vec![0u8; 4096];
        let mut reqs = [
            Request {
                op: Op::Read,
                offset: CHUNK - 32 * 1024,
                len: 64 * 1024,
                buf: &mut out1,
            },
            Request {
                op: Op::Read,
                offset: 2 * CHUNK,
                len: 4096,
                buf: &mut out2,
            },
            Request {
                op: Op::Read,
                offset: 4 * CHUNK,
                len: 4096,
                buf: &mut bad,
            },
        ];
        assert_eq!(v.process(&mut reqs), vec![65536, 4096, -EINVAL]);
        assert!(out1.iter().all(|&x| x == 0xAA));
        assert!(out2.iter().all(|&x| x == 0), "never written: zeros");
    }

    #[test]
    fn misaligned_requests_fail_alone() {
        let mut v = volume();
        let mut a = vec![1u8; 4096];
        let mut b = vec![2u8; 4096];
        let mut reqs = [
            Request {
                op: Op::Write,
                offset: 512,
                len: 4096,
                buf: &mut a,
            },
            Request {
                op: Op::Write,
                offset: 8192,
                len: 4096,
                buf: &mut b,
            },
        ];
        assert_eq!(v.process(&mut reqs), vec![-EINVAL, 4096]);
    }

    #[test]
    fn planned_reads_across_chunks() {
        let mut v = volume();
        let mut a = vec![0xCD; 64 * 1024];
        v.process(&mut [Request {
            op: Op::Write,
            offset: CHUNK - 32 * 1024,
            len: 64 * 1024,
            buf: &mut a,
        }]);
        let mut buf = vec![0u8; 64 * 1024];
        let plans = v.plan_read(CHUNK - 32 * 1024, &mut buf).unwrap();
        assert_eq!(plans.len(), 2, "one plan per chunk");
        for (base, plan) in &plans {
            for r in plan.reads() {
                let at = base + r.buf_offset;
                v.store()
                    .device()
                    .read_at(r.device_offset, &mut buf[at..at + r.len])
                    .unwrap();
            }
        }
        assert!(
            plans
                .iter()
                .all(|(base, p)| p.verify(&buf[*base..]).is_ok())
        );
        v.finish_read(plans);
        assert!(buf.iter().all(|&x| x == 0xCD));
        assert_eq!(v.plan_read(4 * CHUNK, &mut buf).unwrap_err(), -EINVAL);
    }

    #[test]
    fn reopen_keeps_data_and_sequence_numbers() {
        let mut v = volume();
        let mut a = vec![7u8; 4096];
        v.process(&mut [Request {
            op: Op::Write,
            offset: 4096,
            len: 4096,
            buf: &mut a,
        }]);
        let seq = v.next_seq;
        let dev = v.store.into_device();
        let mut v = Volume::open(
            Store::open(dev, StoreConfig::default()).unwrap(),
            layout(),
            1,
        )
        .unwrap();
        assert!(v.next_seq >= seq);
        let mut out = vec![0u8; 4096];
        assert_eq!(
            v.process(&mut [Request {
                op: Op::Read,
                offset: 4096,
                len: 4096,
                buf: &mut out
            }]),
            vec![4096]
        );
        assert!(out.iter().all(|&x| x == 7));
    }
}
