//! The replica store: recovery, the write and read paths, and checkpoints.

use std::borrow::Cow;
use std::collections::HashSet;

use flaminio_format::FormatError;
use flaminio_format::checksum::{block_checksums, crc32c};
use flaminio_format::consts::{BITS_PER_BITMAP_PAGE, BLOCK_SIZE, BLOCK_SIZE_U64, MIB};
use flaminio_format::journal::{BatchBuilder, JournalPosition, JournalScanner, place_batch};
use flaminio_format::layout::Layout;
use flaminio_format::meta::{
    BitmapKind, BitmapPage, CheckpointHeader, ChunkHeader, ChunkState, MapPage, PageTable, Phys,
};
use flaminio_format::mkfs::{self, FormatOptions};
use flaminio_format::page::{PageBuf, PageType, PairChoice, select_pair};
use flaminio_format::record::Record;
use flaminio_format::superblock::{Superblock, select_superblock};

use crate::bitmap::Bitmap;
use crate::device::{BlockDevice, Reader};
use crate::error::StoreError;
use crate::state::{
    Chunk, ChunkKey, Layer, MapPageState, PageSlot, PageTableState, Resolved, State,
};

/// How free space is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allocation {
    /// Continue from where the last allocation ended. Spreads writes and avoids rescanning.
    NextFit,
    /// Always take the lowest free space. Keeps data packed at the start of the device, and
    /// reuses freed space as soon as it is allowed to (useful to stress reuse in tests).
    FirstFit,
}

/// Tunables.
#[derive(Debug, Clone)]
pub struct StoreConfig {
    /// Checkpoint when the live part of the journal exceeds this fraction of it.
    pub checkpoint_threshold: f64,
    /// Largest journal batch. Capped at a quarter of the journal.
    pub max_batch_bytes: usize,
    /// Free-space policy for data clusters and metadata pages.
    pub allocation: Allocation,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            checkpoint_threshold: 0.5,
            max_batch_bytes: 4 * MIB as usize,
            allocation: Allocation::NextFit,
        }
    }
}

/// Public view of one chunk replica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkInfo {
    /// Chunk identity.
    pub key: ChunkKey,
    /// Chunk size in bytes.
    pub chunk_size: u64,
    /// Volume size in bytes.
    pub volume_size: u64,
    /// Requests below this epoch are rejected.
    pub fenced_epoch: u64,
    /// Highest applied write sequence number.
    pub last_write_seq: u64,
    /// Number of layers (snapshots + 1).
    pub layers: usize,
    /// Snapshot generation of the top layer.
    pub top_snapshot_gen: u64,
}

/// Space and journal usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    /// Total data clusters.
    pub data_clusters: u64,
    /// Allocated data clusters (including ones freed since the last checkpoint).
    pub data_used: u64,
    /// Total page-pool pairs.
    pub pool_pairs: u64,
    /// Allocated page-pool pairs.
    pub pool_used: u64,
    /// Chunk replicas on the disk.
    pub chunks: usize,
    /// Last completed checkpoint.
    pub checkpoint_seq: u64,
    /// Journal bytes that would be replayed after a crash.
    pub journal_live_bytes: u64,
}

/// Result of a consistency check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsckReport {
    /// Data clusters referenced by a map entry.
    pub referenced_clusters: u64,
    /// Data clusters freed but not yet reusable.
    pub pending_clusters: u64,
    /// 4 KiB blocks whose checksum was verified (deep check only).
    pub verified_blocks: u64,
}

/// The writer's view of the circular journal.
struct Journal {
    offset: u64,
    size: u64,
    /// Where replay would start: the last checkpoint's journal start.
    start: JournalPosition,
    /// Where the next batch goes.
    head: JournalPosition,
}

impl Journal {
    fn is_empty(&self) -> bool {
        self.start == self.head
    }

    fn live_bytes(&self) -> u64 {
        let (s, h) = (self.start.offset, self.head.offset);
        if self.is_empty() {
            0
        } else if s < h {
            h - s
        } else {
            self.size - s + h
        }
    }

    /// Where a batch of `len` bytes can go without overwriting batches still needed for replay.
    fn place(&self, len: u64) -> Option<u64> {
        let at = place_batch(self.size, self.head.offset, len)?;
        if self.is_empty() {
            return Some(at);
        }
        let (s, h) = (self.start.offset, self.head.offset);
        let fits = if s < h {
            // Live region is [s, h): the batch fits after it, or before s at the start.
            at == h || at + len < s
        } else {
            // Live region wraps: [s, size) and [0, h). Only the gap [h, s) is free.
            at == h && at + len < s
        };
        fits.then_some(at)
    }

    fn advance(&mut self, at: u64, len: u64) {
        self.head = JournalPosition {
            offset: (at + len) % self.size,
            seq: self.head.seq + 1,
        };
    }
}

/// Work prepared by the planner, committed as one journal batch.
struct Plan<'a> {
    records: Vec<Record<'a>>,
    /// Redirect-on-write data, written before the batch.
    data_writes: Vec<(u64, Cow<'a, [u8]>)>,
    /// Data clusters reserved for `data_writes`. Released if the plan is not committed.
    reserved: Vec<(u64, u64)>,
}

impl<'a> Plan<'a> {
    fn records(records: Vec<Record<'a>>) -> Self {
        Self {
            records,
            data_writes: Vec::new(),
            reserved: Vec::new(),
        }
    }
}

/// One write of a [`Store::write_batch`].
#[derive(Debug, Clone, Copy)]
pub struct WriteReq<'a> {
    /// Chunk to write.
    pub key: ChunkKey,
    /// Byte offset within the chunk (4 KiB aligned).
    pub offset: u64,
    /// Data (a multiple of 4 KiB).
    pub data: &'a [u8],
    /// The target's write sequence number.
    pub write_seq: u64,
    /// The writer's epoch.
    pub epoch: u64,
}

/// One read of a [`Store::read_batch`].
#[derive(Debug)]
pub struct ReadReq<'a> {
    /// Chunk to read.
    pub key: ChunkKey,
    /// Byte offset within the chunk (4 KiB aligned).
    pub offset: u64,
    /// Destination (a multiple of 4 KiB).
    pub buf: &'a mut [u8],
    /// The reader's epoch.
    pub epoch: u64,
}

/// Chunk replicas on one device. See the crate documentation.
pub struct Store<D: BlockDevice> {
    dev: D,
    sb: Superblock,
    state: State,
    journal: Journal,
    config: StoreConfig,
    poisoned: bool,
}

fn read_block<D: BlockDevice + ?Sized>(dev: &D, offset: u64) -> Result<Box<PageBuf>, StoreError> {
    let mut b = Box::new([0u8; BLOCK_SIZE]);
    dev.read_at(offset, &mut b[..])?;
    Ok(b)
}

/// Reads a page pair and returns the current slot's offset and bytes (spec §4).
fn read_pair<D: BlockDevice + ?Sized>(
    dev: &D,
    pair_offset: u64,
    page_type: PageType,
    nonce: u64,
    max_seq: u64,
) -> Result<Option<(u8, u64, Box<PageBuf>)>, StoreError> {
    let a = read_block(dev, pair_offset)?;
    let b = read_block(dev, pair_offset + BLOCK_SIZE_U64)?;
    Ok(
        select_pair(&a[..], &b[..], page_type, nonce, pair_offset, max_seq)
            .current
            .map(|(slot, _)| {
                let offset = PairChoice::slot_offset(pair_offset, slot);
                (slot, offset, if slot == 0 { a } else { b })
            }),
    )
}

impl<D: BlockDevice> Store<D> {
    /// Formats `dev` (spec §2), flushing between the three phases of the plan.
    pub fn format(dev: &D, options: &FormatOptions) -> Result<Superblock, StoreError> {
        let plan = mkfs::plan(dev.size(), options)?;
        let zero = [0u8; BLOCK_SIZE];
        for &o in &plan.wipe {
            dev.write_at(o, &zero)?;
        }
        dev.flush()?;
        for (o, p) in &plan.metadata {
            dev.write_at(*o, &p[..])?;
        }
        dev.flush()?;
        for (o, p) in &plan.superblocks {
            dev.write_at(*o, &p[..])?;
        }
        dev.flush()?;
        Ok(plan.superblock)
    }

    /// Opens a formatted device: loads the last checkpoint, replays the journal, and writes a
    /// fresh checkpoint (spec §7).
    pub fn open(dev: D, config: StoreConfig) -> Result<Self, StoreError> {
        let size = dev.size();
        if size < 2 * BLOCK_SIZE_U64 {
            return Err(StoreError::corrupt("device too small"));
        }
        let a = read_block(&dev, 0)?;
        let b = read_block(&dev, (size & !(BLOCK_SIZE_U64 - 1)) - BLOCK_SIZE_U64)?;
        let (sb, _) = select_superblock(&a[..], &b[..])?;
        if sb.requires_read_only() {
            return Err(StoreError::ReadOnly);
        }
        let layout = sb.layout;
        if size < layout.device_size {
            return Err(StoreError::corrupt(
                "device is smaller than when it was formatted",
            ));
        }
        let nonce = sb.format_nonce;
        let mut state = State::new(layout);
        let first_fit = config.allocation == Allocation::FirstFit;
        state.data.set_first_fit(first_fit);
        state.pool.set_first_fit(first_fit);

        // Checkpoint header.
        let (slot, offset, page) = read_pair(
            &dev,
            layout.checkpoint_offset,
            PageType::Checkpoint,
            nonce,
            u64::MAX,
        )?
        .ok_or_else(|| StoreError::corrupt("no valid checkpoint header"))?;
        let (k, ckpt) = CheckpointHeader::decode(&page[..], nonce, offset, &layout)?;
        state.checkpoint_seq = k;
        state.checkpoint_slot.current = Some(slot);

        // Allocation bitmaps.
        for (kind, pairs, base) in [
            (
                BitmapKind::Data,
                layout.data_bitmap_pairs,
                layout.data_bitmap_offset,
            ),
            (
                BitmapKind::Pool,
                layout.pool_bitmap_pairs,
                layout.pool_bitmap_offset,
            ),
        ] {
            let page_type = if kind == BitmapKind::Data {
                PageType::DataBitmap
            } else {
                PageType::PoolBitmap
            };
            for i in 0..pairs {
                let pair = base + i * 2 * BLOCK_SIZE_U64;
                let (slot, offset, page) =
                    read_pair(&dev, pair, page_type, nonce, k)?.ok_or_else(|| {
                        StoreError::corrupt(format!("{kind:?} bitmap page {i} missing"))
                    })?;
                let (_, bm) =
                    BitmapPage::decode(&page[..], kind, nonce, offset, i * BITS_PER_BITMAP_PAGE)?;
                let (bitmap, slots) = match kind {
                    BitmapKind::Data => (&mut state.data, &mut state.data_pages),
                    BitmapKind::Pool => (&mut state.pool, &mut state.pool_pages),
                };
                bitmap.load_page(i, &bm);
                slots[i as usize].current = Some(slot);
            }
        }

        // Chunks.
        for s in 0..layout.chunk_slots {
            let Some((slot, offset, page)) = read_pair(
                &dev,
                layout.chunk_slot_offset(s),
                PageType::ChunkHeader,
                nonce,
                k,
            )?
            else {
                continue;
            };
            state.headers[s as usize].current = Some(slot);
            let (_, header) = ChunkHeader::decode(&page[..], nonce, offset, &layout)?;
            if header.state != ChunkState::Free {
                let chunk = load_chunk(&dev, &state, s as u32, header, nonce, k)?;
                if state.by_key.insert(chunk.key, s as u32).is_some() {
                    return Err(StoreError::corrupt(format!(
                        "chunk {} appears twice",
                        chunk.key
                    )));
                }
                state.chunks[s as usize] = Some(chunk);
            }
        }

        // Replay.
        let start = JournalPosition {
            offset: ckpt.journal_start_offset,
            seq: ckpt.journal_start_seq,
        };
        let reader = Reader(&dev);
        let mut scan = JournalScanner::new(&reader, &layout, nonce, start);
        for batch in scan.by_ref() {
            let batch = batch?;
            for record in batch.records() {
                for (o, d) in state.apply(&record?)? {
                    dev.write_at(o, d)?;
                }
            }
        }
        let head = scan.position();
        let journal = Journal {
            offset: layout.journal_offset,
            size: layout.journal_size,
            start,
            head: JournalPosition {
                offset: head.offset % layout.journal_size,
                seq: head.seq,
            },
        };
        let config = StoreConfig {
            max_batch_bytes: config
                .max_batch_bytes
                .min((layout.journal_size / 4) as usize)
                / BLOCK_SIZE
                * BLOCK_SIZE,
            ..config
        };
        let mut store = Self {
            dev,
            sb,
            state,
            journal,
            config,
            poisoned: false,
        };
        store.checkpoint_inner()?;
        Ok(store)
    }

    /// The disk's superblock.
    pub fn superblock(&self) -> &Superblock {
        &self.sb
    }

    /// The underlying device.
    pub fn device(&self) -> &D {
        &self.dev
    }

    /// Gives the device back, without checkpointing. Recovery on the next open is cheap.
    pub fn into_device(self) -> D {
        self.dev
    }

    /// Runs a mutating operation. Device and consistency failures poison the store: in-memory
    /// state no longer matches the disk, and only a reopen (recovery) can fix that.
    fn guard<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        if self.poisoned {
            return Err(StoreError::Poisoned);
        }
        let result = f(self);
        if matches!(
            result,
            Err(StoreError::Io(_)
                | StoreError::Corrupt(_)
                | StoreError::Format(_)
                | StoreError::Scan(_))
        ) {
            self.poisoned = true;
        }
        result
    }

    fn check_epoch(&self, slot: u32, epoch: u64) -> Result<(), StoreError> {
        let fenced = self.state.chunk(slot).fenced_epoch;
        if epoch < fenced {
            return Err(StoreError::StaleEpoch { fenced, got: epoch });
        }
        Ok(())
    }

    /// Validates an I/O request and returns the chunk's slot.
    fn check_io(
        &self,
        key: ChunkKey,
        offset: u64,
        len: u64,
        epoch: u64,
    ) -> Result<u32, StoreError> {
        let slot = self.state.slot_of(key)?;
        self.check_epoch(slot, epoch)?;
        let size = self.state.chunk(slot).chunk_size;
        if len == 0
            || !offset.is_multiple_of(BLOCK_SIZE_U64)
            || !len.is_multiple_of(BLOCK_SIZE_U64)
            || offset.checked_add(len).is_none_or(|e| e > size)
        {
            return Err(StoreError::invalid(format!(
                "I/O at {offset}+{len} must be 4 KiB aligned, non-empty and inside the {size}-byte chunk"
            )));
        }
        Ok(slot)
    }

    /// Prepares and commits, checkpointing once to make journal room if needed.
    fn run<'a>(
        &mut self,
        mut make: impl FnMut(&mut Self) -> Result<Plan<'a>, StoreError>,
    ) -> Result<(), StoreError> {
        for attempt in 0..2 {
            let plan = make(self)?;
            if self.commit(plan)? {
                return Ok(());
            }
            if attempt == 0 {
                self.checkpoint_inner()?;
            }
        }
        Err(StoreError::invalid("request does not fit in the journal"))
    }

    fn release(&mut self, reserved: &[(u64, u64)]) {
        for &(start, n) in reserved {
            for c in start..start + n {
                self.state.data.set(c, false);
            }
        }
    }

    /// Commits a plan as one journal batch. Returns false (and changes nothing) if the journal
    /// has no room.
    fn commit(&mut self, plan: Plan<'_>) -> Result<bool, StoreError> {
        let mut batch = BatchBuilder::new(self.config.max_batch_bytes);
        for r in &plan.records {
            if let Err(e) = batch.push(r) {
                self.release(&plan.reserved);
                return Err(match e {
                    FormatError::BatchFull => {
                        StoreError::invalid("request too large for one journal batch")
                    }
                    e => StoreError::Format(e),
                });
            }
        }
        self.execute(batch, plan)
    }

    /// The commit protocol (format spec §6.2): redirect-on-write data, flush, journal batch,
    /// flush, apply (including in-place deferred writes). Returns false, after releasing the
    /// plan's reservations, if the journal has no room for the batch.
    fn execute(&mut self, batch: BatchBuilder, plan: Plan<'_>) -> Result<bool, StoreError> {
        let len = batch.total_len();
        let Some(at) = self.journal.place(len) else {
            self.release(&plan.reserved);
            return Ok(false);
        };
        if !plan.data_writes.is_empty() {
            let writes: Vec<(u64, &[u8])> =
                plan.data_writes.iter().map(|(o, d)| (*o, &d[..])).collect();
            self.dev.write_many(&writes)?;
            self.dev.flush()?;
        }
        let bytes = batch.finish(self.sb.format_nonce, self.journal.head.seq, at);
        self.dev.write_at(self.journal.offset + at, &bytes)?;
        self.dev.flush()?;
        self.journal.advance(at, len);
        let mut in_place = Vec::new();
        for r in &plan.records {
            in_place.extend(self.state.apply(r)?);
        }
        if !in_place.is_empty() {
            self.dev.write_many(&in_place)?;
        }
        if self.journal.live_bytes() as f64
            > self.journal.size as f64 * self.config.checkpoint_threshold
        {
            self.checkpoint_inner()?;
        }
        Ok(true)
    }

    // ---------------------------------------------------------------------------------------
    // Chunk management

    /// Creates an empty chunk replica.
    pub fn create_chunk(
        &mut self,
        key: ChunkKey,
        chunk_size: u64,
        volume_size: u64,
        epoch: u64,
    ) -> Result<(), StoreError> {
        self.guard(|s| {
            if s.state.by_key.contains_key(&key) {
                return Err(StoreError::ChunkExists(key));
            }
            s.state.validate_chunk_size(chunk_size)?;
            s.run(|s| {
                let slot = s
                    .state
                    .chunks
                    .iter()
                    .position(Option::is_none)
                    .ok_or(StoreError::OutOfSpace("chunk slots"))?;
                Ok(Plan::records(vec![Record::ChunkCreate {
                    slot: slot as u32,
                    volume_uuid: key.volume,
                    chunk_index: key.index,
                    chunk_size,
                    volume_size,
                    fenced_epoch: epoch,
                }]))
            })
        })
    }

    /// Deletes a chunk replica and everything in it.
    pub fn delete_chunk(&mut self, key: ChunkKey, epoch: u64) -> Result<(), StoreError> {
        self.guard(|s| {
            let slot = s.state.slot_of(key)?;
            s.check_epoch(slot, epoch)?;
            s.run(|_| Ok(Plan::records(vec![Record::ChunkDelete { slot }])))
        })
    }

    /// Raises the fenced epoch of every chunk of `volume` on this disk (ARCHITECTURE §8.4).
    /// Fails with [`StoreError::StaleEpoch`] if a chunk is already fenced at a higher epoch.
    pub fn fence(&mut self, volume: flaminio_format::Uuid, epoch: u64) -> Result<(), StoreError> {
        self.guard(|s| {
            let fenced = s
                .state
                .chunks
                .iter()
                .flatten()
                .filter(|c| c.key.volume == volume)
                .map(|c| c.fenced_epoch);
            let (mut highest, mut lowest) = (None, None);
            for f in fenced {
                highest = highest.max(Some(f));
                lowest = Some(lowest.map_or(f, |l: u64| l.min(f)));
            }
            match (highest, lowest) {
                (Some(h), _) if epoch < h => Err(StoreError::StaleEpoch {
                    fenced: h,
                    got: epoch,
                }),
                (Some(_), Some(l)) if epoch > l => s.run(|_| {
                    Ok(Plan::records(vec![Record::Fence {
                        volume_uuid: volume,
                        epoch,
                    }]))
                }),
                _ => Ok(()),
            }
        })
    }

    /// Freezes the current contents as a snapshot: pushes a new empty top layer.
    pub fn snapshot(
        &mut self,
        key: ChunkKey,
        snapshot_gen: u64,
        epoch: u64,
    ) -> Result<(), StoreError> {
        self.guard(|s| {
            let slot = s.state.slot_of(key)?;
            s.check_epoch(slot, epoch)?;
            let chunk = s.state.chunk(slot);
            if chunk.layers.len() >= s.state.layout.max_layers as usize {
                return Err(StoreError::OutOfSpace("snapshot layers"));
            }
            let top = chunk
                .layers
                .last()
                .expect("at least one layer")
                .snapshot_gen;
            if snapshot_gen <= top {
                return Err(StoreError::invalid(format!(
                    "snapshot generation {snapshot_gen} must exceed {top}"
                )));
            }
            s.run(|_| {
                Ok(Plan::records(vec![Record::LayerCreate {
                    slot,
                    snapshot_gen,
                }]))
            })
        })
    }

    // ---------------------------------------------------------------------------------------
    // I/O

    /// Writes `data` at `offset` within the chunk. Durable when this returns.
    ///
    /// Whole clusters are written to new space (redirect-on-write). Partial clusters already
    /// mapped in the top layer go through the journal and are applied in place; other partial
    /// clusters are read, merged and redirected.
    pub fn write(
        &mut self,
        key: ChunkKey,
        offset: u64,
        data: &[u8],
        write_seq: u64,
        epoch: u64,
    ) -> Result<(), StoreError> {
        let req = WriteReq {
            key,
            offset,
            data,
            write_seq,
            epoch,
        };
        self.write_batch(&[req])?
            .pop()
            .expect("one result per request")
    }

    /// Writes several requests with **group commit**: requests share the data flush, one
    /// journal batch and the journal flush, which is what makes durable writes fast at queue
    /// depths above one. All are durable when this returns.
    ///
    /// Requests are committed in order, in groups. A group ends early at a request that
    /// touches a cluster an earlier request of the group touches, so overlapping writes are
    /// applied in submission order. Each group is atomic.
    ///
    /// The outer `Err` means the store failed (and is poisoned). Otherwise there is one result
    /// per request: a request can fail on its own (bad range, stale epoch, out of space)
    /// without affecting the others.
    pub fn write_batch(
        &mut self,
        reqs: &[WriteReq<'_>],
    ) -> Result<Vec<Result<(), StoreError>>, StoreError> {
        if self.poisoned {
            return Err(StoreError::Poisoned);
        }
        let mut results: Vec<Option<Result<(), StoreError>>> = reqs.iter().map(|_| None).collect();
        let mut pending = Vec::with_capacity(reqs.len());
        for (i, r) in reqs.iter().enumerate() {
            match self.check_io(r.key, r.offset, r.data.len() as u64, r.epoch) {
                Ok(slot) => pending.push((i, slot)),
                Err(e) => results[i] = Some(Err(e)),
            }
        }
        let mut next = 0;
        let mut retries = 0;
        while next < pending.len() {
            let (consumed, failed) = self.guard(|s| s.commit_group(reqs, &pending[next..]))?;
            if consumed == 0 {
                // No journal room: commit_group checkpointed. Try once more.
                retries += 1;
                if retries > 1 {
                    return Err(StoreError::corrupt(
                        "journal has no room even after a checkpoint",
                    ));
                }
                continue;
            }
            retries = 0;
            for (i, e) in failed {
                results[i] = Some(Err(e));
            }
            for &(i, _) in &pending[next..next + consumed] {
                results[i].get_or_insert(Ok(()));
            }
            next += consumed;
        }
        Ok(results
            .into_iter()
            .map(|r| r.expect("every request has a result"))
            .collect())
    }

    /// Plans and commits the longest prefix of `pending` that forms a valid group. Returns how
    /// many requests it consumed (committed or failed individually) and the individual
    /// failures. Consumes nothing, after checkpointing, if the journal had no room.
    #[allow(clippy::type_complexity)]
    fn commit_group(
        &mut self,
        reqs: &[WriteReq<'_>],
        pending: &[(usize, u32)],
    ) -> Result<(usize, Vec<(usize, StoreError)>), StoreError> {
        let cs = self.state.cluster_size();
        let mut batch = BatchBuilder::new(self.config.max_batch_bytes);
        let mut group = Plan {
            records: Vec::new(),
            data_writes: Vec::new(),
            reserved: Vec::new(),
        };
        let mut touched: HashSet<(u32, u64)> = HashSet::new();
        let mut pool_budget = 0;
        let mut failed = Vec::new();
        let mut consumed = 0;
        for &(i, slot) in pending {
            let r = &reqs[i];
            let end = r.offset + r.data.len() as u64;
            let clusters = r.offset / cs..=(end - 1) / cs;
            if clusters.clone().any(|x| touched.contains(&(slot, x))) {
                break;
            }
            let needed = self.state.pool_needed(slot, clusters.clone());
            if self.state.pool.free() < pool_budget + needed {
                failed.push((i, StoreError::OutOfSpace("metadata page pool")));
                consumed += 1;
                continue;
            }
            let mut plan = Plan {
                records: Vec::new(),
                data_writes: Vec::new(),
                reserved: Vec::new(),
            };
            let range = (*clusters.start(), *clusters.end());
            if let Err(e) =
                self.fill_write_plan(&mut plan, slot, r.offset, r.data, r.write_seq, range)
            {
                self.release(&plan.reserved);
                if matches!(e, StoreError::Io(_) | StoreError::Corrupt(_)) {
                    self.release(&group.reserved);
                    return Err(e);
                }
                failed.push((i, e));
                consumed += 1;
                continue;
            }
            let size: usize = plan.records.iter().map(Record::encoded_len).sum();
            if !batch.would_fit(size) {
                self.release(&plan.reserved);
                if group.records.is_empty() {
                    failed.push((
                        i,
                        StoreError::invalid("request too large for one journal batch"),
                    ));
                    consumed += 1;
                    continue;
                }
                break;
            }
            for rec in &plan.records {
                batch.push(rec)?;
            }
            touched.extend(clusters.map(|x| (slot, x)));
            pool_budget += needed;
            group.records.extend(plan.records);
            group.data_writes.extend(plan.data_writes);
            group.reserved.extend(plan.reserved);
            consumed += 1;
        }
        if group.records.is_empty() {
            return Ok((consumed, failed));
        }
        if !self.execute(batch, group)? {
            self.checkpoint_inner()?;
            return Ok((0, Vec::new()));
        }
        Ok((consumed, failed))
    }

    fn fill_write_plan<'a>(
        &mut self,
        plan: &mut Plan<'a>,
        slot: u32,
        offset: u64,
        data: &'a [u8],
        write_seq: u64,
        (first_x, last_x): (u64, u64),
    ) -> Result<(), StoreError> {
        let cs = self.state.cluster_size();
        let cpc = self.state.cpc as u32;
        let end = offset + data.len() as u64;
        let full = |x: u64| offset <= x * cs && (x + 1) * cs <= end;
        let mut x = first_x;
        while x <= last_x {
            if full(x) {
                let mut n = 1;
                while x + n <= last_x && full(x + n) {
                    n += 1;
                }
                let mut done = 0;
                while done < n {
                    let (p, got) = self
                        .state
                        .data
                        .alloc_run(n - done)
                        .ok_or(StoreError::OutOfSpace("data clusters"))?;
                    plan.reserved.push((p, got));
                    let lo = ((x + done) * cs - offset) as usize;
                    let piece = &data[lo..lo + (got * cs) as usize];
                    plan.data_writes.push((
                        self.state.layout.data_cluster_offset(p),
                        Cow::Borrowed(piece),
                    ));
                    plan.records.push(Record::MapUpdate {
                        slot,
                        first_cluster: (x + done) as u32,
                        count: got as u32,
                        csums_per_cluster: cpc,
                        phys_first: p,
                        write_seq,
                        csums: Cow::Owned(block_checksums(piece)),
                    });
                    done += got;
                }
                x += n;
                continue;
            }
            let (a, b) = (offset.max(x * cs), end.min((x + 1) * cs));
            let piece = &data[(a - offset) as usize..(b - offset) as usize];
            if let Phys::Cluster(_) = self.state.chunk(slot).top_phys(self.state.epp, x) {
                plan.records.push(Record::DeferredWrite {
                    slot,
                    offset: a as u32,
                    write_seq,
                    csums: Cow::Owned(block_checksums(piece)),
                    data: piece,
                });
            } else {
                // Read-modify-write into a new cluster.
                let mut buf = self.read_cluster(slot, x)?;
                buf[(a - x * cs) as usize..(b - x * cs) as usize].copy_from_slice(piece);
                let (p, _) = self
                    .state
                    .data
                    .alloc_run(1)
                    .ok_or(StoreError::OutOfSpace("data clusters"))?;
                plan.reserved.push((p, 1));
                plan.records.push(Record::MapUpdate {
                    slot,
                    first_cluster: x as u32,
                    count: 1,
                    csums_per_cluster: cpc,
                    phys_first: p,
                    write_seq,
                    csums: Cow::Owned(block_checksums(&buf)),
                });
                plan.data_writes
                    .push((self.state.layout.data_cluster_offset(p), Cow::Owned(buf)));
            }
            x += 1;
        }
        Ok(())
    }

    /// The visible contents of cluster `x`, verified.
    fn read_cluster(&self, slot: u32, x: u64) -> Result<Vec<u8>, StoreError> {
        let mut buf = vec![0u8; self.state.cluster_size() as usize];
        self.read_range(slot, x * self.state.cluster_size(), &mut buf)?;
        Ok(buf)
    }

    /// Reads `buf.len()` bytes at `offset` within the chunk, verifying every 4 KiB block.
    /// Returns [`StoreError::Checksum`] if stored data is damaged; read another replica then.
    pub fn read(
        &self,
        key: ChunkKey,
        offset: u64,
        buf: &mut [u8],
        epoch: u64,
    ) -> Result<(), StoreError> {
        let mut reqs = [ReadReq {
            key,
            offset,
            buf,
            epoch,
        }];
        self.read_batch(&mut reqs)?
            .pop()
            .expect("one result per request")
    }

    /// Reads several requests with all device reads in flight at once. The outer `Err` is a
    /// device failure; otherwise there is one result per request.
    pub fn read_batch(
        &self,
        reqs: &mut [ReadReq<'_>],
    ) -> Result<Vec<Result<(), StoreError>>, StoreError> {
        if self.poisoned {
            return Err(StoreError::Poisoned);
        }
        let cs = self.state.cluster_size();
        let mut results: Vec<Result<(), StoreError>> = Vec::with_capacity(reqs.len());
        let mut reads: Vec<(u64, &mut [u8])> = Vec::new();
        // For each device read: request index, chunk key, chunk offset, expected checksums.
        let mut checks: Vec<(usize, ChunkKey, u64, Vec<u32>)> = Vec::new();
        for (i, r) in reqs.iter_mut().enumerate() {
            let slot = match self.check_io(r.key, r.offset, r.buf.len() as u64, r.epoch) {
                Ok(slot) => slot,
                Err(e) => {
                    results.push(Err(e));
                    continue;
                }
            };
            results.push(Ok(()));
            let chunk = self.state.chunk(slot);
            let mut rest: &mut [u8] = &mut *r.buf;
            let mut o = r.offset;
            while !rest.is_empty() {
                let (x, within) = (o / cs, o % cs);
                let n = ((cs - within) as usize).min(rest.len());
                let (piece, tail) = std::mem::take(&mut rest).split_at_mut(n);
                rest = tail;
                match chunk.resolve(self.state.epp, x) {
                    Resolved::Zeros => piece.fill(0),
                    Resolved::Data { phys, csums } => {
                        let first = (within / BLOCK_SIZE_U64) as usize;
                        let expected = csums[first..first + n / BLOCK_SIZE].to_vec();
                        reads.push((self.state.layout.data_cluster_offset(phys) + within, piece));
                        checks.push((i, chunk.key, o, expected));
                    }
                }
                o += n as u64;
            }
        }
        self.dev.read_many(&mut reads)?;
        for ((_, data), (i, key, offset, expected)) in reads.iter().zip(&checks) {
            if results[*i].is_err() {
                continue;
            }
            for (b, block) in data.chunks_exact(BLOCK_SIZE).enumerate() {
                if crc32c(block) != expected[b] {
                    results[*i] = Err(StoreError::Checksum {
                        key: *key,
                        offset: offset + (b * BLOCK_SIZE) as u64,
                    });
                    break;
                }
            }
        }
        Ok(results)
    }

    fn read_range(&self, slot: u32, offset: u64, buf: &mut [u8]) -> Result<(), StoreError> {
        let cs = self.state.cluster_size();
        let chunk = self.state.chunk(slot);
        let mut pos = 0usize;
        while pos < buf.len() {
            let o = offset + pos as u64;
            let (x, within) = (o / cs, o % cs);
            let n = ((cs - within) as usize).min(buf.len() - pos);
            let out = &mut buf[pos..pos + n];
            match chunk.resolve(self.state.epp, x) {
                Resolved::Zeros => out.fill(0),
                Resolved::Data { phys, csums } => {
                    self.dev
                        .read_at(self.state.layout.data_cluster_offset(phys) + within, out)?;
                    let first_block = (within / BLOCK_SIZE_U64) as usize;
                    for (i, block) in out.chunks_exact(BLOCK_SIZE).enumerate() {
                        if crc32c(block) != csums[first_block + i] {
                            return Err(StoreError::Checksum {
                                key: chunk.key,
                                offset: x * cs + ((first_block + i) * BLOCK_SIZE) as u64,
                            });
                        }
                    }
                }
            }
            pos += n;
        }
        Ok(())
    }

    /// Discards whole clusters inside `[offset, offset + len)`. Partial clusters at the edges
    /// are left alone (discard is advisory). Reads of discarded clusters return zeros.
    pub fn discard(
        &mut self,
        key: ChunkKey,
        offset: u64,
        len: u64,
        epoch: u64,
    ) -> Result<(), StoreError> {
        self.guard(|s| {
            let slot = s.check_io(key, offset, len, epoch)?;
            let cs = s.state.cluster_size();
            let (first, end) = (offset.div_ceil(cs), (offset + len) / cs);
            if first >= end {
                return Ok(());
            }
            s.run(|s| {
                if s.state.chunk(slot).layers.len() > 1
                    && s.state.pool.free() < s.state.pool_needed(slot, first..end)
                {
                    return Err(StoreError::OutOfSpace("metadata page pool"));
                }
                Ok(Plan::records(vec![Record::Discard {
                    slot,
                    first_cluster: first as u32,
                    count: (end - first) as u32,
                }]))
            })
        })
    }

    // ---------------------------------------------------------------------------------------
    // Checkpoints

    /// Writes every changed metadata page and moves the journal start (spec §5.1).
    pub fn checkpoint(&mut self) -> Result<(), StoreError> {
        self.guard(Self::checkpoint_inner)
    }

    fn checkpoint_inner(&mut self) -> Result<(), StoreError> {
        let k = self.state.checkpoint_seq + 1;
        let nonce = self.sb.format_nonce;
        let layout: Layout = self.state.layout;
        self.state.release_pending();

        let mut writes: Vec<(u64, Box<PageBuf>)> = Vec::new();
        let mut emit =
            |slot: &mut PageSlot, pair_offset: u64, encode: &dyn Fn(u64) -> Box<PageBuf>| {
                let w = PairChoice {
                    current: slot.current.map(|s| (s, 0)),
                }
                .write_slot();
                let offset = PairChoice::slot_offset(pair_offset, w);
                writes.push((offset, encode(offset)));
                *slot = PageSlot {
                    current: Some(w),
                    dirty: false,
                };
            };

        let State {
            chunks,
            headers,
            data,
            pool,
            data_pages,
            pool_pages,
            ..
        } = &mut self.state;
        for (s, header_slot) in headers.iter_mut().enumerate() {
            let chunk = chunks[s].as_mut();
            if header_slot.dirty {
                let header = chunk.as_ref().map_or_else(free_header, |c| c.header());
                emit(header_slot, layout.chunk_slot_offset(s as u64), &|o| {
                    header.encode(nonce, k, o)
                });
            }
            let Some(chunk) = chunk else { continue };
            for (li, layer) in chunk.layers.iter_mut().enumerate() {
                let Some(table) = layer.table.as_mut() else {
                    continue;
                };
                if table.slot.dirty {
                    let t = table.to_page_table(s as u32, li as u32);
                    emit(&mut table.slot, layout.pool_pair_offset(table.pair), &|o| {
                        t.encode(nonce, k, o)
                    });
                }
                for mp in table.map_pages.iter_mut().flatten() {
                    if mp.slot.dirty {
                        let page = &mp.page;
                        emit(&mut mp.slot, layout.pool_pair_offset(mp.pair), &|o| {
                            page.encode(nonce, k, o)
                        });
                    }
                }
            }
        }
        for p in data.take_dirty() {
            let page = data.to_page(p);
            emit(
                &mut data_pages[p as usize],
                layout.data_bitmap_pair_offset(p),
                &|o| page.encode(BitmapKind::Data, nonce, k, o),
            );
        }
        for p in pool.take_dirty() {
            let page = pool.to_page(p);
            emit(
                &mut pool_pages[p as usize],
                layout.pool_bitmap_pair_offset(p),
                &|o| page.encode(BitmapKind::Pool, nonce, k, o),
            );
        }
        for (o, page) in &writes {
            self.dev.write_at(*o, &page[..])?;
        }
        // One flush makes the new pages and every earlier data write (including in-place
        // deferred writes) durable. Only the header below makes checkpoint K+1 count, and only
        // it moves the journal start, so nothing depends on these writes before then.
        self.dev.flush()?;

        let header = CheckpointHeader {
            journal_start_offset: self.journal.head.offset,
            journal_start_seq: self.journal.head.seq,
            data_free_clusters: self.state.data.free(),
            pool_free_pairs: self.state.pool.free(),
            created_unix_secs: 0,
        };
        let w = PairChoice {
            current: self.state.checkpoint_slot.current.map(|s| (s, 0)),
        }
        .write_slot();
        let offset = PairChoice::slot_offset(layout.checkpoint_offset, w);
        self.dev
            .write_at(offset, &header.encode(nonce, k, offset)[..])?;
        self.dev.flush()?;
        self.state.checkpoint_slot.current = Some(w);
        self.state.checkpoint_seq = k;
        self.journal.start = self.journal.head;
        Ok(())
    }

    // ---------------------------------------------------------------------------------------
    // Introspection

    /// Information about one chunk.
    pub fn chunk_info(&self, key: ChunkKey) -> Option<ChunkInfo> {
        self.state
            .by_key
            .get(&key)
            .map(|&slot| info(self.state.chunk(slot)))
    }

    /// All chunks on the disk.
    pub fn chunks(&self) -> Vec<ChunkInfo> {
        self.state.chunks.iter().flatten().map(info).collect()
    }

    /// Space and journal usage.
    pub fn stats(&self) -> Stats {
        Stats {
            data_clusters: self.state.data.len(),
            data_used: self.state.data.ones(),
            pool_pairs: self.state.pool.len(),
            pool_used: self.state.pool.ones(),
            chunks: self.state.by_key.len(),
            checkpoint_seq: self.state.checkpoint_seq,
            journal_live_bytes: self.journal.live_bytes(),
        }
    }

    /// Checks that every allocated cluster and page is referenced exactly once (or pending
    /// free), and with `deep`, that every mapped block matches its checksum.
    pub fn fsck(&self, deep: bool) -> Result<FsckReport, StoreError> {
        let st = &self.state;
        let mut errors = Vec::new();
        let mut data_seen = Bitmap::new(st.data.len());
        let mut pool_seen = Bitmap::new(st.pool.len());
        let mark = |errors: &mut Vec<String>,
                    seen: &mut Bitmap,
                    allocated: &Bitmap,
                    i: u64,
                    what: &str| {
            if seen.get(i) {
                errors.push(format!("{what} {i} referenced twice"));
            }
            if !allocated.get(i) {
                errors.push(format!("{what} {i} referenced but free"));
            }
            seen.set(i, true);
        };
        let mut verified = 0;
        for chunk in st.chunks.iter().flatten() {
            for table in chunk.layers.iter().filter_map(|l| l.table.as_ref()) {
                mark(
                    &mut errors,
                    &mut pool_seen,
                    &st.pool,
                    table.pair,
                    "page-pool pair",
                );
                for mp in table.map_pages.iter().flatten() {
                    mark(
                        &mut errors,
                        &mut pool_seen,
                        &st.pool,
                        mp.pair,
                        "page-pool pair",
                    );
                    for e in 0..mp.page.len() {
                        let Phys::Cluster(q) = mp.page.phys(e) else {
                            continue;
                        };
                        mark(&mut errors, &mut data_seen, &st.data, q, "data cluster");
                        if deep {
                            let mut buf = vec![0u8; st.cluster_size() as usize];
                            self.dev
                                .read_at(st.layout.data_cluster_offset(q), &mut buf)?;
                            for (b, block) in buf.chunks_exact(BLOCK_SIZE).enumerate() {
                                if crc32c(block) != mp.page.csums(e)[b] {
                                    errors.push(format!(
                                        "chunk {} cluster {} block {b}: checksum mismatch",
                                        chunk.key,
                                        mp.page.first_cluster as usize + e
                                    ));
                                }
                                verified += 1;
                            }
                        }
                    }
                }
            }
        }
        let pending_data: HashSet<u64> = st.pending_data.iter().copied().collect();
        let pending_pool: HashSet<u64> = st.pending_pool.iter().copied().collect();
        for q in 0..st.data.len() {
            if st.data.get(q) && !data_seen.get(q) && !pending_data.contains(&q) {
                errors.push(format!("data cluster {q} leaked"));
            }
        }
        for p in 0..st.pool.len() {
            if st.pool.get(p) && !pool_seen.get(p) && !pending_pool.contains(&p) {
                errors.push(format!("page-pool pair {p} leaked"));
            }
        }
        if !errors.is_empty() {
            errors.truncate(20);
            return Err(StoreError::corrupt(errors.join("; ")));
        }
        Ok(FsckReport {
            referenced_clusters: data_seen.ones(),
            pending_clusters: pending_data.len() as u64,
            verified_blocks: verified,
        })
    }
}

fn info(c: &Chunk) -> ChunkInfo {
    ChunkInfo {
        key: c.key,
        chunk_size: c.chunk_size,
        volume_size: c.volume_size,
        fenced_epoch: c.fenced_epoch,
        last_write_seq: c.last_write_seq,
        layers: c.layers.len(),
        top_snapshot_gen: c.layers.last().expect("at least one layer").snapshot_gen,
    }
}

fn free_header() -> ChunkHeader {
    ChunkHeader {
        state: ChunkState::Free,
        volume_uuid: flaminio_format::Uuid::NIL,
        chunk_index: 0,
        chunk_size: 0,
        volume_size: 0,
        fenced_epoch: 0,
        last_write_seq: 0,
        layers: Vec::new(),
    }
}

/// Loads a chunk's page tables and map pages as of checkpoint `k`.
fn load_chunk<D: BlockDevice>(
    dev: &D,
    state: &State,
    slot: u32,
    header: ChunkHeader,
    nonce: u64,
    k: u64,
) -> Result<Chunk, StoreError> {
    let layout = &state.layout;
    let pages = state.map_pages_per_chunk(header.chunk_size);
    let mut layers = Vec::with_capacity(header.layers.len());
    for (li, lr) in header.layers.iter().enumerate() {
        let table = match lr.page_table {
            None => None,
            Some(pair) => {
                if !state.pool.get(pair) {
                    return Err(StoreError::corrupt(format!(
                        "slot {slot}: page table in free pool pair {pair}"
                    )));
                }
                let (cur, offset, page) = read_pair(
                    dev,
                    layout.pool_pair_offset(pair),
                    PageType::PageTable,
                    nonce,
                    k,
                )?
                .ok_or_else(|| {
                    StoreError::corrupt(format!("slot {slot} layer {li}: page table missing"))
                })?;
                let (_, pt) = PageTable::decode(&page[..], nonce, offset, layout, slot, li as u32)?;
                let mut map_pages: Vec<Option<MapPageState>> = (0..pages).map(|_| None).collect();
                for (pi, r) in pt.map_pages.iter().enumerate() {
                    let Some(mp_pair) = *r else { continue };
                    if pi >= pages || !state.pool.get(mp_pair) {
                        return Err(StoreError::corrupt(format!(
                            "slot {slot} layer {li}: bad map page reference {pi}"
                        )));
                    }
                    let (mcur, moff, mpage) = read_pair(
                        dev,
                        layout.pool_pair_offset(mp_pair),
                        PageType::Map,
                        nonce,
                        k,
                    )?
                    .ok_or_else(|| {
                        StoreError::corrupt(format!(
                            "slot {slot} layer {li}: map page {pi} missing"
                        ))
                    })?;
                    let (_, page) = MapPage::decode(
                        &mpage[..],
                        nonce,
                        moff,
                        layout,
                        slot,
                        (pi * state.epp) as u32,
                    )?;
                    for e in 0..page.len() {
                        if let Phys::Cluster(q) = page.phys(e)
                            && !state.data.get(q)
                        {
                            return Err(StoreError::corrupt(format!(
                                "slot {slot}: map entry points at free cluster {q}"
                            )));
                        }
                    }
                    map_pages[pi] = Some(MapPageState {
                        pair: mp_pair,
                        slot: PageSlot {
                            current: Some(mcur),
                            dirty: false,
                        },
                        page,
                    });
                }
                Some(PageTableState {
                    pair,
                    slot: PageSlot {
                        current: Some(cur),
                        dirty: false,
                    },
                    map_pages,
                })
            }
        };
        layers.push(Layer {
            snapshot_gen: lr.snapshot_gen,
            table,
        });
    }
    Ok(Chunk {
        key: ChunkKey {
            volume: header.volume_uuid,
            index: header.chunk_index,
        },
        state: header.state,
        chunk_size: header.chunk_size,
        volume_size: header.volume_size,
        fenced_epoch: header.fenced_epoch,
        last_write_seq: header.last_write_seq,
        layers,
    })
}
