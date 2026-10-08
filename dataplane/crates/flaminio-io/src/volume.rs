//! A volume: a linear block address space laid over fixed-size chunks in the replica store.
//!
//! The frontend hands this module a batch of block requests. It splits them at chunk
//! boundaries, runs all writes as one store group commit and all reads as one batch, and
//! returns one result per request, in the form ublk expects: bytes transferred, or a negative
//! errno.

use flaminio_format::Uuid;
use flaminio_store::{BlockDevice, ChunkKey, ReadReq, Store, StoreError, WriteReq};

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

/// A volume served from a local replica store (MVP step 4: one replica, RF=1).
pub struct Volume<D: BlockDevice> {
    store: Store<D>,
    layout: VolumeLayout,
    epoch: u64,
    next_seq: u64,
}

/// Errno values returned to the block layer.
const EIO: i32 = 5;
const EINVAL: i32 = 22;

fn errno(e: &StoreError) -> i32 {
    match e {
        StoreError::Invalid(_) => -EINVAL,
        _ => -EIO,
    }
}

impl<D: BlockDevice> Volume<D> {
    /// Opens the volume, creating any chunk that does not exist yet.
    pub fn open(mut store: Store<D>, layout: VolumeLayout, epoch: u64) -> Result<Self, StoreError> {
        if layout.chunk_size == 0 || layout.size == 0 || layout.size % layout.chunk_size != 0 {
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
        })
    }

    /// The volume's shape.
    pub fn layout(&self) -> VolumeLayout {
        self.layout
    }

    /// The underlying store.
    pub fn store(&self) -> &Store<D> {
        &self.store
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
            match self.store.write_batch(&writes) {
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
