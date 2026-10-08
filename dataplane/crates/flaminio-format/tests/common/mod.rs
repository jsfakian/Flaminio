//! Test helpers shared by integration tests.

use std::collections::BTreeMap;
use std::io;

use flaminio_format::ReadAt;
use flaminio_format::consts::{BLOCK_SIZE, BLOCK_SIZE_U64};
use flaminio_format::mkfs::FormatPlan;

/// A sparse in-memory block device: unwritten blocks read as zeros, so multi-TiB devices are
/// cheap.
pub struct SparseDevice {
    pub size: u64,
    blocks: BTreeMap<u64, Box<[u8; BLOCK_SIZE]>>,
}

impl SparseDevice {
    pub fn new(size: u64) -> Self {
        Self {
            size,
            blocks: BTreeMap::new(),
        }
    }

    pub fn write(&mut self, offset: u64, data: &[u8]) {
        assert!(
            offset % BLOCK_SIZE_U64 == 0 && data.len() % BLOCK_SIZE == 0,
            "unaligned write"
        );
        assert!(offset + data.len() as u64 <= self.size, "write past end");
        for (i, chunk) in data.chunks_exact(BLOCK_SIZE).enumerate() {
            let block = offset / BLOCK_SIZE_U64 + i as u64;
            self.blocks
                .insert(block, Box::new(chunk.try_into().unwrap()));
        }
    }

    pub fn zero(&mut self, offset: u64) {
        self.write(offset, &[0; BLOCK_SIZE]);
    }

    pub fn read_block(&self, offset: u64) -> Vec<u8> {
        let mut b = vec![0; BLOCK_SIZE];
        self.read_at(offset, &mut b).unwrap();
        b
    }

    /// Performs a format plan, phase by phase. `phases` limits how many phases run, to simulate
    /// a crash.
    pub fn apply(&mut self, plan: &FormatPlan, phases: usize) {
        if phases >= 1 {
            for &o in &plan.wipe {
                self.zero(o);
            }
        }
        if phases >= 2 {
            for (o, p) in &plan.metadata {
                self.write(*o, &p[..]);
            }
        }
        if phases >= 3 {
            for (o, p) in &plan.superblocks {
                self.write(*o, &p[..]);
            }
        }
    }
}

impl ReadAt for SparseDevice {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        if offset + buf.len() as u64 > self.size {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "past end"));
        }
        let mut done = 0;
        while done < buf.len() {
            let pos = offset + done as u64;
            let (block, within) = (pos / BLOCK_SIZE_U64, (pos % BLOCK_SIZE_U64) as usize);
            let n = (BLOCK_SIZE - within).min(buf.len() - done);
            match self.blocks.get(&block) {
                Some(b) => buf[done..done + n].copy_from_slice(&b[within..within + n]),
                None => buf[done..done + n].fill(0),
            }
            done += n;
        }
        Ok(())
    }
}
