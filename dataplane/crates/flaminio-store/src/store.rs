//! The replica store: recovery, the write and read paths, and checkpoints.

use std::borrow::Cow;
use std::ops::Deref;

use bytes::Bytes;
use std::cell::Cell;
use std::collections::{HashSet, VecDeque};

use flaminio_format::FormatError;
use flaminio_format::checksum::{block_checksums, crc32c};
use flaminio_format::consts::{
    BITS_PER_BITMAP_PAGE, BLOCK_SIZE, BLOCK_SIZE_U64, FEATURE_INCOMPAT_PIPELINED_COMMITS, MIB,
    PIPELINE_DEPTH,
};
use flaminio_format::journal::{Batch, BatchBuilder, JournalPosition, JournalScanner, place_batch};
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
    /// Commit up to `PIPELINE_DEPTH` batches at once (spec §6.2). Opening a disk without the
    /// feature sets it, after recovery; without this, a disk that has it is still recovered by
    /// its rules, and commits one batch at a time.
    ///
    /// Off by default: a short validation that launched every staged group at once lost about
    /// 20% on 1M sequential (548 against ~700 MiB/s) by bypassing gathering. Opt in once the
    /// engine gathers before launching.
    pub pipelined_commits: bool,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            checkpoint_threshold: 0.5,
            pipelined_commits: false,
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
    /// Where the next journal batch goes (offset within the journal region).
    pub journal_head: u64,
    /// Freed data clusters held back from reuse while planned reads are in flight.
    pub data_quarantined: u64,
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
    /// Redirect-on-write data, written before the batch (shared with the device: no copy).
    data_writes: Vec<(u64, Bytes)>,
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

/// A group commit whose I/O is in flight (see [`Store::start_write_batch`]).
struct InFlight {
    /// The journal batch, applied once durable, as replay would apply it. Empty when
    /// [`device`](Self::device) is false.
    batch: Bytes,
    /// The results of the requests of the call that started it: `None` for the requests in
    /// the batch, which succeed once it is durable.
    results: Vec<Option<Result<(), StoreError>>>,
    /// The clusters its requests write, as (slot, chunk-relative cluster): a staged request
    /// may not touch them, as its plan would not see this batch's changes.
    touched: HashSet<(u32, u64)>,
    /// Metadata pages its records may take from the pool when applied.
    pool_budget: u64,
    /// False when every request failed on its own: no device commit. Still sits in
    /// `in_flight` so [`poll_write_batch`](Store::poll_write_batch) returns results in order.
    device: bool,
}

/// The next group, staged while a commit is in flight ([`Store::stage_writes`]): its records
/// gather in a journal batch, written once the batch in flight is durable; its
/// redirect-on-write data is written already.
struct Staged {
    batch: BatchBuilder,
    touched: HashSet<(u32, u64)>,
    pool_budget: u64,
    /// One per staged request: `None` for those in the batch, which succeed once it is durable.
    results: Vec<Option<Result<(), StoreError>>>,
}

/// What [`Store::start_write_batch`] did.
#[derive(Debug)]
#[must_use = "a commit in flight must be polled"]
pub enum Started {
    /// Every request is done: durable, or failed on its own. One result per request.
    Done(Vec<Result<(), StoreError>>),
    /// The last group's writes and flush are in flight: [`Store::poll_write_batch`] gives the
    /// results.
    InFlight,
}

/// A group whose batch is in flight, from [`Store::commit_group`].
struct Flying {
    bytes: Bytes,
    touched: HashSet<(u32, u64)>,
    pool_budget: u64,
}

/// How [`Store::execute_start`] went.
enum Exec {
    /// The journal has no room for the batch: nothing was written.
    NoRoom,
    /// Durable and applied.
    Done,
    /// Written and flushed asynchronously: apply this batch once the device reports it done.
    InFlight(Bytes),
}

/// The data of a [`WriteReq`].
#[derive(Debug, Clone)]
pub enum WriteData<'a> {
    /// Borrowed: the store copies what it writes asynchronously.
    Slice(&'a [u8]),
    /// Shared: the store hands slices of it to the device, without a copy. Best 4 KiB aligned
    /// (an aligned buffer is written as is with `O_DIRECT`).
    Shared(Bytes),
}

impl Deref for WriteData<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            WriteData::Slice(s) => s,
            WriteData::Shared(b) => b,
        }
    }
}

impl<'a> From<&'a [u8]> for WriteData<'a> {
    fn from(s: &'a [u8]) -> Self {
        WriteData::Slice(s)
    }
}

impl<'a> From<&'a Vec<u8>> for WriteData<'a> {
    fn from(v: &'a Vec<u8>) -> Self {
        WriteData::Slice(v)
    }
}

impl From<Bytes> for WriteData<'_> {
    fn from(b: Bytes) -> Self {
        WriteData::Shared(b)
    }
}

impl WriteData<'_> {
    /// `range` of the data, shared: a slice of the buffer, or a copy of borrowed data.
    fn shared(&self, range: std::ops::Range<usize>) -> Bytes {
        match self {
            WriteData::Slice(s) => Bytes::copy_from_slice(&s[range]),
            WriteData::Shared(b) => b.slice(range),
        }
    }
}

/// One write of a [`Store::write_batch`].
#[derive(Debug, Clone)]
pub struct WriteReq<'a> {
    /// Chunk to write.
    pub key: ChunkKey,
    /// Byte offset within the chunk (4 KiB aligned).
    pub offset: u64,
    /// Data (a multiple of 4 KiB).
    pub data: WriteData<'a>,
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

/// One device read of a [`ReadPlan`].
#[derive(Debug, Clone)]
pub struct PlannedRead {
    /// Byte offset on the device.
    pub device_offset: u64,
    /// Where the data goes, as an offset into the request buffer.
    pub buf_offset: usize,
    /// Length in bytes.
    pub len: usize,
    csums: Vec<u32>,
}

/// A read planned by [`Store::plan_read`]: the device reads the caller must perform, and the
/// checksums to verify them against. Unmapped ranges were already zero-filled in the buffer.
/// While a plan is outstanding, the clusters it reads cannot be reused; release it with
/// [`Store::finish_read`].
#[derive(Debug)]
#[must_use = "pass the plan to Store::finish_read when its reads are done"]
pub struct ReadPlan {
    key: ChunkKey,
    offset: u64,
    reads: Vec<PlannedRead>,
}

impl ReadPlan {
    /// The device reads to perform. They do not overlap, on the device or in the buffer.
    pub fn reads(&self) -> &[PlannedRead] {
        &self.reads
    }

    /// Checks the request buffer once every read has completed.
    pub fn verify(&self, buf: &[u8]) -> Result<(), StoreError> {
        for r in &self.reads {
            let data = &buf[r.buf_offset..r.buf_offset + r.len];
            for (b, block) in data.chunks_exact(BLOCK_SIZE).enumerate() {
                if crc32c(block) != r.csums[b] {
                    return Err(StoreError::Checksum {
                        key: self.key,
                        offset: self.offset + (r.buf_offset + b * BLOCK_SIZE) as u64,
                    });
                }
            }
        }
        Ok(())
    }
}

/// Chunk replicas on one device. See the crate documentation.
pub struct Store<D: BlockDevice> {
    dev: D,
    sb: Superblock,
    state: State,
    journal: Journal,
    config: StoreConfig,
    poisoned: bool,
    /// Plans handed out by plan_read and not yet finished.
    reads_in_flight: Cell<u64>,
    /// The group commits in flight, oldest first: one at a time, or with pipelined commits up
    /// to `PIPELINE_DEPTH` (spec §6.2). Recovery relies on every batch but the last of those
    /// being durable, data included.
    in_flight: VecDeque<InFlight>,
    /// The next group, staged behind the commit in flight.
    staged: Option<Staged>,
    /// When true, [`Self::poll_write_batch`] does not start the staged group's commit: the
    /// engine grows it toward a byte floor, then calls [`Self::launch_staged`].
    hold_staged: bool,
    /// The results of commits that another change waited for (see [`Store::guard`]), in order,
    /// until [`Store::poll_write_batch`] hands them out.
    settled: VecDeque<Vec<Result<(), StoreError>>>,
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
        // A batch is written together with its redirect-on-write data, followed by one flush
        // (spec §6.2). Only the last batch of the log can therefore be missing data: every
        // later batch was written after that flush completed. So batches are applied one
        // behind, and the last is applied only if its data matches the checksums it carries.
        // With pipelined commits, the same holds for the last PIPELINE_DEPTH batches: they are
        // applied in order while their data matches, and the log ends at the first that does
        // not.
        let apply_batch = |state: &mut State, batch: &Batch| -> Result<(), StoreError> {
            for record in batch.records() {
                for (o, d) in state.apply(&record?)? {
                    dev.write_at(o, d)?;
                }
            }
            Ok(())
        };
        let pipelined = sb.features_incompat & FEATURE_INCOMPAT_PIPELINED_COMMITS != 0;
        let window = if pipelined { PIPELINE_DEPTH } else { 1 };
        let reader = Reader(&dev);
        let mut scan = JournalScanner::new(&reader, &layout, nonce, start);
        let mut tail: VecDeque<Batch> = VecDeque::with_capacity(window + 1);
        for batch in scan.by_ref() {
            tail.push_back(batch?);
            if tail.len() > window {
                apply_batch(&mut state, &tail.pop_front().expect("over the window"))?;
            }
        }
        let mut head = scan.position();
        while let Some(batch) = tail.pop_front() {
            // Clusters a later batch rewrites in place: not checked (spec §6.2).
            let mut rewritten = HashSet::new();
            for later in &tail {
                for record in later.records() {
                    if let Record::DeferredWrite { slot, offset, .. } = record? {
                        rewritten
                            .insert((slot, u64::from(offset) / u64::from(layout.cluster_size)));
                    }
                }
            }
            if batch_data_is_durable(&dev, &layout, &batch, &rewritten)? {
                apply_batch(&mut state, &batch)?;
                continue;
            }
            // Its flush never completed, so nothing in it, or after it, was acknowledged.
            let h = *batch.header();
            head = JournalPosition {
                offset: h.offset,
                seq: h.batch_seq,
            };
            if !pipelined {
                // Erase its header so it cannot be mistaken for a later batch with the same
                // sequence number.
                dev.write_at(layout.journal_offset + h.offset, &[0u8; BLOCK_SIZE])?;
                dev.flush()?;
            }
            break;
        }
        if pipelined {
            // Stale batches (rejected ones, and any beyond a lost batch) all have sequence
            // numbers below the next one replay will expect.
            head.seq += PIPELINE_DEPTH as u64;
        }
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
            reads_in_flight: Cell::new(0),
            in_flight: VecDeque::new(),
            staged: None,
            hold_staged: false,
            settled: VecDeque::new(),
        };
        store.checkpoint_inner()?;
        if store.config.pipelined_commits && !pipelined {
            store.set_pipelined_commits()?;
        }
        Ok(store)
    }

    /// Sets the pipelined-commits feature (spec §6.2): both superblock copies, the next
    /// generation, each flushed. Only right after a checkpoint, with nothing in flight: no batch
    /// written before is replayed by the new rules.
    fn set_pipelined_commits(&mut self) -> Result<(), StoreError> {
        let mut sb = self.sb.clone();
        sb.features_incompat |= FEATURE_INCOMPAT_PIPELINED_COMMITS;
        sb.sb_generation += 1;
        let page = sb.encode();
        for offset in [0, sb.layout.superblock_b_offset()] {
            self.dev.write_at(offset, &page[..])?;
            self.dev.flush()?;
        }
        self.sb = sb;
        Ok(())
    }

    /// How many batches may commit at once on this disk.
    fn pipeline_depth(&self) -> usize {
        if self.config.pipelined_commits
            && self.sb.features_incompat & FEATURE_INCOMPAT_PIPELINED_COMMITS != 0
        {
            PIPELINE_DEPTH
        } else {
            1
        }
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

    /// Runs a mutating operation, after the commit in flight completes. Device and consistency
    /// failures poison the store: in-memory state no longer matches the disk, and only a
    /// reopen (recovery) can fix that.
    fn guard<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        if self.poisoned {
            return Err(StoreError::Poisoned);
        }
        let result = self.settle().and_then(|()| {
            if self.reads_in_flight.get() == 0 {
                self.state.data.unblock_all();
            }
            f(self)
        });
        self.poison_on(result)
    }

    /// Runs a step of the commit in flight: like [`Self::guard`], without waiting for it.
    fn poisoning<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        if self.poisoned {
            return Err(StoreError::Poisoned);
        }
        let result = f(self);
        self.poison_on(result)
    }

    fn poison_on<T>(&mut self, result: Result<T, StoreError>) -> Result<T, StoreError> {
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

    /// The commit protocol (format spec §6.2): redirect-on-write data and the journal batch
    /// together, one flush, then apply (including in-place deferred writes). Recovery verifies
    /// the data of the last batch against the checksums it carries, so the data needs no flush
    /// of its own. Returns false, after releasing the plan's reservations, if the journal has
    /// no room for the batch.
    fn execute(&mut self, batch: BatchBuilder, plan: Plan<'_>) -> Result<bool, StoreError> {
        match self.execute_start(batch, plan)? {
            Exec::NoRoom => Ok(false),
            Exec::Done => Ok(true),
            Exec::InFlight(bytes) => {
                self.dev.poll_commit(true).expect("waited for the commit")?;
                self.execute_finish(bytes)?;
                Ok(true)
            }
        }
    }

    /// Starts the commit protocol: the data and the batch written, then a flush, without
    /// waiting if the device can. Nothing is applied until the batch is durable, so reads see
    /// the state before it meanwhile.
    fn execute_start(&mut self, batch: BatchBuilder, plan: Plan<'_>) -> Result<Exec, StoreError> {
        let len = batch.total_len();
        let Some(at) = self.journal.place(len) else {
            self.release(&plan.reserved);
            return Ok(Exec::NoRoom);
        };
        let bytes = Bytes::from(batch.finish(self.sb.format_nonce, self.journal.head.seq, at));
        let mut writes = plan.data_writes;
        writes.push((self.journal.offset + at, bytes.clone()));
        let done = self.dev.start_commit(&writes)?;
        drop(writes);
        self.journal.advance(at, len);
        if done {
            self.execute_finish(bytes)?;
            return Ok(Exec::Done);
        }
        Ok(Exec::InFlight(bytes))
    }

    /// Ends the commit protocol once the batch is durable: applies its records, decoded from
    /// the batch as replay decodes them, writes deferred writes in place, and checkpoints if
    /// the journal is full enough.
    fn execute_finish(&mut self, bytes: Bytes) -> Result<(), StoreError> {
        // The device has let go of its reference: no copy when this is the last one.
        let batch = Batch::from_bytes(Vec::from(bytes), self.sb.format_nonce)?;
        let mut in_place = Vec::new();
        for r in batch.records() {
            in_place.extend(self.state.apply(&r?)?);
        }
        if !in_place.is_empty() {
            self.dev.write_many(&in_place)?;
        }
        // Not while a group is staged or another commit is in flight: their reserved clusters
        // would reach the bitmaps on disk before their batches do, and leak in a crash, and the
        // journal start would move past batches not applied yet. A later commit checkpoints
        // instead (staging stops when the journal runs out of room).
        if self.staged.is_none()
            && self.in_flight.is_empty()
            && self.journal.live_bytes() as f64
                > self.journal.size as f64 * self.config.checkpoint_threshold
        {
            self.checkpoint_inner()?;
        }
        Ok(())
    }

    /// Waits for the commits in flight, and the staged one after them, and applies them in
    /// order, keeping their results for [`Self::poll_write_batch`]. Every other change waits
    /// for them this way (from [`Self::guard`]), so batches stay durable in order.
    fn settle(&mut self) -> Result<(), StoreError> {
        loop {
            while let Some(f) = self.in_flight.pop_front() {
                if f.device {
                    self.dev.poll_commit(true).expect("waited for the commit")?;
                }
                let results = self.finish_in_flight(f)?;
                self.settled.push_back(results);
            }
            if self.staged.is_none() {
                return Ok(());
            }
            self.promote_staged()?;
        }
    }

    fn finish_in_flight(&mut self, f: InFlight) -> Result<Vec<Result<(), StoreError>>, StoreError> {
        if f.device {
            self.execute_finish(f.batch)?;
        }
        Ok(f.results.into_iter().map(|r| r.unwrap_or(Ok(()))).collect())
    }

    /// Starts the staged group's commit: its journal batch, then a flush that also covers its
    /// data, written already. Once the batch before it is durable and applied, or, with
    /// pipelined commits, behind fewer than `PIPELINE_DEPTH` batches in flight.
    fn promote_staged(&mut self) -> Result<(), StoreError> {
        let Some(st) = self.staged.take() else {
            return Ok(());
        };
        if st.batch.record_count() == 0 {
            // Every staged request failed on its own: no device commit, but keep the slot in
            // `in_flight` so results stay ordered with the commits ahead of it.
            self.in_flight.push_back(InFlight {
                batch: Bytes::new(),
                results: st.results,
                touched: st.touched,
                pool_budget: st.pool_budget,
                device: false,
            });
            return Ok(());
        }
        let len = st.batch.total_len();
        let at = self
            .journal
            .place(len)
            .ok_or_else(|| StoreError::corrupt("a staged batch lost its room in the journal"))?;
        let bytes = Bytes::from(
            st.batch
                .finish(self.sb.format_nonce, self.journal.head.seq, at),
        );
        let done = self
            .dev
            .start_commit(&[(self.journal.offset + at, bytes.clone())])?;
        self.journal.advance(at, len);
        let f = InFlight {
            batch: bytes,
            results: st.results,
            touched: st.touched,
            pool_budget: st.pool_budget,
            device: true,
        };
        if done {
            // Only a device with nothing in flight completes a commit at once.
            debug_assert!(self.in_flight.is_empty());
            let results = self.finish_in_flight(f)?;
            self.settled.push_back(results);
        } else {
            self.in_flight.push_back(f);
        }
        Ok(())
    }

    /// When true, landing a commit does not start the staged group ([`Self::poll_write_batch`]);
    /// the caller grows it and starts it with [`Self::launch_staged`].
    pub fn set_hold_staged(&mut self, hold: bool) {
        self.hold_staged = hold;
    }

    /// Starts the staged group's commit now. With pipelined commits, behind those in flight if
    /// fewer than `PIPELINE_DEPTH` are already out (spec §6.2). With [`Self::set_hold_staged`],
    /// also when nothing is in flight (the previous commit landed and promotion was deferred).
    /// Returns whether it did.
    pub fn launch_staged(&mut self) -> Result<bool, StoreError> {
        if self.staged.is_none() {
            return Ok(false);
        }
        let n = self.in_flight.len();
        let depth = self.pipeline_depth();
        if n >= depth || (n > 0 && depth == 1) {
            return Ok(false);
        }
        // n == 0: only when holding a staged group after the previous commit landed.
        if n == 0 && !self.hold_staged {
            return Ok(false);
        }
        self.poisoning(Self::promote_staged)?;
        Ok(true)
    }

    /// True if [`Self::stage_writes`] would take this write now: a commit is in flight, the
    /// write covers at least one whole cluster, touches none of the in-flight commit's clusters
    /// nor those of the requests staged already, and the metadata pool and the journal have room
    /// for it.
    ///
    /// Only writes with redirect-on-write data are staged: that data is what goes to the disk
    /// early. A small write into a mapped cluster is all journal, so staging it gains nothing,
    /// and taking it at once would split a client's writes between two commits that gathering
    /// would have committed together (4K QD32: 6 700 against 10 200 IOPS).
    pub fn can_stage(&self, key: ChunkKey, offset: u64, len: u64) -> bool {
        self.stage_check(key, offset, len).is_some()
    }

    /// The write's slot, clusters and metadata pages, if it can be staged.
    fn stage_check(
        &self,
        key: ChunkKey,
        offset: u64,
        len: u64,
    ) -> Option<(u32, std::ops::RangeInclusive<u64>, u64)> {
        if self.poisoned || len == 0 {
            return None;
        }
        // Normally a commit must be in flight. With hold_staged, append to an existing staged
        // group after that commit landed (growing toward a byte floor).
        if self.in_flight.is_empty() && !(self.hold_staged && self.staged.is_some()) {
            return None;
        }
        let slot = self.state.slot_of(key).ok()?;
        let cs = self.state.cluster_size();
        let clusters = offset / cs..=(offset + len - 1) / cs;
        if offset.div_ceil(cs) * cs + cs > offset + len {
            return None; // no whole cluster
        }
        let staged = self.staged.as_ref();
        let busy = |x: u64| {
            self.in_flight
                .iter()
                .any(|f| f.touched.contains(&(slot, x)))
                || staged.is_some_and(|st| st.touched.contains(&(slot, x)))
        };
        if clusters.clone().any(busy) {
            return None;
        }
        let needed = self.state.pool_needed(slot, clusters.clone());
        let budget = self.in_flight.iter().map(|f| f.pool_budget).sum::<u64>()
            + staged.map_or(0, |st| st.pool_budget);
        if self.state.pool.free() < budget + needed {
            return None;
        }
        // Records of a write: at most one per cluster, each with a checksum per block, and the
        // data itself for a small write. A conservative bound for the journal checks.
        let bound = (clusters.end() - clusters.start() + 1) as usize * 128
            + len as usize / 1024
            + if len < cs { len as usize } else { 0 };
        let total = staged.map_or_else(
            || BatchBuilder::new(self.config.max_batch_bytes).total_len_with(bound),
            |st| st.batch.total_len_with(bound),
        );
        if total > self.config.max_batch_bytes as u64 || self.journal.place(total).is_none() {
            return None;
        }
        Some((slot, clusters, needed))
    }

    /// Stages writes behind the commit in flight: takes the longest prefix of `reqs` that
    /// [`Self::can_stage`] accepts, starts writing their redirect-on-write data, and adds their
    /// records to the next journal batch. That batch is written, and flushed, as soon as the
    /// batch in flight is durable; [`Self::poll_write_batch`] then returns the staged requests'
    /// results with the next commit's. Returns how many requests were taken.
    ///
    /// Recovery is unchanged: batches still become durable one at a time and in order, and the
    /// data written early goes to clusters no durable batch references until its batch is.
    pub fn stage_writes(&mut self, reqs: &[WriteReq<'_>]) -> Result<usize, StoreError> {
        if self.poisoned {
            return Err(StoreError::Poisoned);
        }
        let mut taken = 0;
        for r in reqs {
            let Some((slot, clusters, needed)) =
                self.stage_check(r.key, r.offset, r.data.len() as u64)
            else {
                break;
            };
            self.staged.get_or_insert_with(|| Staged {
                batch: BatchBuilder::new(self.config.max_batch_bytes),
                touched: HashSet::new(),
                pool_budget: 0,
                results: Vec::new(),
            });
            if let Err(e) = self.check_io(r.key, r.offset, r.data.len() as u64, r.epoch) {
                self.staged
                    .as_mut()
                    .expect("staged")
                    .results
                    .push(Some(Err(e)));
                taken += 1;
                continue;
            }
            let mut plan = Plan {
                records: Vec::new(),
                data_writes: Vec::new(),
                reserved: Vec::new(),
            };
            let range = (*clusters.start(), *clusters.end());
            let planned = self.poisoning(|s| {
                match s.fill_write_plan(&mut plan, slot, r.offset, &r.data, r.write_seq, range) {
                    Ok(()) => Ok(None),
                    Err(e @ (StoreError::Io(_) | StoreError::Corrupt(_))) => Err(e),
                    Err(e) => Ok(Some(e)),
                }
            });
            match planned {
                Err(e) => {
                    self.release(&plan.reserved);
                    return Err(e);
                }
                Ok(Some(e)) => {
                    self.release(&plan.reserved);
                    self.staged
                        .as_mut()
                        .expect("staged")
                        .results
                        .push(Some(Err(e)));
                    taken += 1;
                    continue;
                }
                Ok(None) => {}
            }
            if !plan.data_writes.is_empty() {
                let started = self.dev.start_writes(&plan.data_writes);
                self.poisoning(|_| started.map_err(StoreError::from))?;
            }
            let st = self.staged.as_mut().expect("staged");
            for rec in &plan.records {
                if let Err(e) = st.batch.push(rec) {
                    // can_stage bounded the records' size: this is a bug, not a full batch.
                    self.poisoned = true;
                    return Err(StoreError::Format(e));
                }
            }
            st.touched.extend(clusters.map(|x| (slot, x)));
            st.pool_budget += needed;
            st.results.push(None);
            taken += 1;
        }
        Ok(taken)
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
            data: data.into(),
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
        match self.start_write_batch(reqs)? {
            Started::Done(results) => Ok(results),
            Started::InFlight => Ok(self.poll_write_batch(true)?.expect("waited for the commit")),
        }
    }

    /// [`Self::write_batch`], without waiting for the last group's I/O when the device can do
    /// it asynchronously ([`BlockDevice::start_commit`]). The data is copied: `reqs` may go as
    /// soon as this returns. Until [`Self::poll_write_batch`] has returned the results, reads
    /// see the state before the batch, and no other write batch may start; any other change
    /// waits for the commit first.
    pub fn start_write_batch(&mut self, reqs: &[WriteReq<'_>]) -> Result<Started, StoreError> {
        if self.poisoned {
            return Err(StoreError::Poisoned);
        }
        if self.commit_in_flight() {
            return Err(StoreError::invalid(
                "a write batch is in flight: poll it before starting another",
            ));
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
            let (consumed, failed, flying) =
                self.guard(|s| s.commit_group(reqs, &pending[next..]))?;
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
            let group = next..next + consumed;
            next += consumed;
            if let Some(f) = flying {
                if next == pending.len() {
                    self.in_flight.push_back(InFlight {
                        batch: f.bytes,
                        results,
                        touched: f.touched,
                        pool_budget: f.pool_budget,
                        device: true,
                    });
                    return Ok(Started::InFlight);
                }
                // Groups commit one after the other: only the last stays in flight.
                self.guard(|s| {
                    s.dev.poll_commit(true).expect("waited for the commit")?;
                    s.execute_finish(f.bytes)
                })?;
            }
            for &(i, _) in &pending[group] {
                results[i].get_or_insert(Ok(()));
            }
        }
        Ok(Started::Done(
            results
                .into_iter()
                .map(|r| r.expect("every request has a result"))
                .collect(),
        ))
    }

    /// The results of the write batch in flight, once durable (with `wait`, waits for it):
    /// one per request of [`Self::start_write_batch`]. `None` while still in flight.
    pub fn poll_write_batch(
        &mut self,
        wait: bool,
    ) -> Result<Option<Vec<Result<(), StoreError>>>, StoreError> {
        if let Some(results) = self.settled.pop_front() {
            return Ok(Some(results));
        }
        if self.poisoned {
            return Err(StoreError::Poisoned);
        }
        if self.in_flight.is_empty() {
            return Err(StoreError::invalid("no write batch in flight"));
        }
        // A front entry with no device I/O completes without waiting.
        if !self.in_flight.front().expect("in flight").device {
            let f = self.in_flight.pop_front().expect("in flight");
            return self
                .poisoning(|s| {
                    let results = s.finish_in_flight(f)?;
                    if !s.hold_staged {
                        s.promote_staged()?;
                    }
                    Ok(results)
                })
                .map(Some);
        }
        let Some(outcome) = self.dev.poll_commit(wait) else {
            return Ok(None);
        };
        // The device reports the oldest commit first.
        let f = self.in_flight.pop_front().expect("in flight");
        debug_assert!(f.device);
        // Not `guard`: it would wait for the staged group too.
        self.poisoning(|s| {
            outcome?;
            let results = s.finish_in_flight(f)?;
            if !s.hold_staged {
                s.promote_staged()?;
            }
            Ok(results)
        })
        .map(Some)
    }

    /// True from [`Self::start_write_batch`] returning [`Started::InFlight`] until
    /// [`Self::poll_write_batch`] has returned the results of every batch in flight or staged.
    pub fn commit_in_flight(&self) -> bool {
        !self.in_flight.is_empty() || self.staged.is_some() || !self.settled.is_empty()
    }

    /// Plans and commits the longest prefix of `pending` that forms a valid group. Returns how
    /// many requests it consumed (committed or failed individually), the individual failures,
    /// and the group's batch if its I/O is still in flight. Consumes nothing, after
    /// checkpointing, if the journal had no room.
    #[allow(clippy::type_complexity)]
    fn commit_group(
        &mut self,
        reqs: &[WriteReq<'_>],
        pending: &[(usize, u32)],
    ) -> Result<(usize, Vec<(usize, StoreError)>, Option<Flying>), StoreError> {
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
                self.fill_write_plan(&mut plan, slot, r.offset, &r.data, r.write_seq, range)
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
            return Ok((consumed, failed, None));
        }
        match self.execute_start(batch, group)? {
            Exec::NoRoom => {
                self.checkpoint_inner()?;
                Ok((0, Vec::new(), None))
            }
            Exec::Done => Ok((consumed, failed, None)),
            Exec::InFlight(bytes) => Ok((
                consumed,
                failed,
                Some(Flying {
                    bytes,
                    touched,
                    pool_budget,
                }),
            )),
        }
    }

    fn fill_write_plan<'a>(
        &mut self,
        plan: &mut Plan<'a>,
        slot: u32,
        offset: u64,
        write: &'a WriteData<'a>,
        write_seq: u64,
        (first_x, last_x): (u64, u64),
    ) -> Result<(), StoreError> {
        let data: &'a [u8] = write;
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
                    let range = lo..lo + (got * cs) as usize;
                    let piece = &data[range.clone()];
                    plan.data_writes.push((
                        self.state.layout.data_cluster_offset(p),
                        write.shared(range),
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
                    .push((self.state.layout.data_cluster_offset(p), Bytes::from(buf)));
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

    /// Resolves a read of `buf.len()` bytes at `offset` in chunk `slot` into device reads,
    /// zero-filling unmapped ranges of `buf`. Neighbouring clusters that are also neighbours on
    /// disk become one device read.
    fn plan_segments(&self, slot: u32, offset: u64, buf: &mut [u8]) -> ReadPlan {
        let cs = self.state.cluster_size();
        let chunk = self.state.chunk(slot);
        let mut reads: Vec<PlannedRead> = Vec::new();
        let mut pos = 0usize;
        while pos < buf.len() {
            let o = offset + pos as u64;
            let (x, within) = (o / cs, o % cs);
            let n = ((cs - within) as usize).min(buf.len() - pos);
            match chunk.resolve(self.state.epp, x) {
                Resolved::Zeros => buf[pos..pos + n].fill(0),
                Resolved::Data { phys, csums } => {
                    let dev = self.state.layout.data_cluster_offset(phys) + within;
                    let first = (within / BLOCK_SIZE_U64) as usize;
                    let sums = &csums[first..first + n / BLOCK_SIZE];
                    match reads.last_mut() {
                        Some(r)
                            if r.device_offset + r.len as u64 == dev
                                && r.buf_offset + r.len == pos =>
                        {
                            r.len += n;
                            r.csums.extend_from_slice(sums);
                        }
                        _ => reads.push(PlannedRead {
                            device_offset: dev,
                            buf_offset: pos,
                            len: n,
                            csums: sums.to_vec(),
                        }),
                    }
                }
            }
            pos += n;
        }
        ReadPlan {
            key: chunk.key,
            offset,
            reads,
        }
    }

    /// Plans an asynchronous read: returns the device reads to perform into `buf` (unmapped
    /// ranges are zero-filled now) and the checksums to verify them against. The caller performs
    /// the reads, calls [`ReadPlan::verify`], and must then call [`finish_read`](Self::finish_read).
    /// Until then, clusters freed by later writes are kept from reuse. A concurrent write to the
    /// same range can make `verify` fail; plan and read again in that case.
    pub fn plan_read(
        &self,
        key: ChunkKey,
        offset: u64,
        buf: &mut [u8],
        epoch: u64,
    ) -> Result<ReadPlan, StoreError> {
        if self.poisoned {
            return Err(StoreError::Poisoned);
        }
        let slot = self.check_io(key, offset, buf.len() as u64, epoch)?;
        let plan = self.plan_segments(slot, offset, buf);
        self.reads_in_flight.set(self.reads_in_flight.get() + 1);
        Ok(plan)
    }

    /// Ends a plan from [`plan_read`](Self::plan_read).
    pub fn finish_read(&self, plan: ReadPlan) {
        drop(plan);
        self.reads_in_flight
            .set(self.reads_in_flight.get().saturating_sub(1));
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
        let mut results: Vec<Result<(), StoreError>> = Vec::with_capacity(reqs.len());
        let mut plans: Vec<(usize, ReadPlan)> = Vec::new();
        for (i, r) in reqs.iter_mut().enumerate() {
            match self.check_io(r.key, r.offset, r.buf.len() as u64, r.epoch) {
                Ok(slot) => {
                    plans.push((i, self.plan_segments(slot, r.offset, r.buf)));
                    results.push(Ok(()));
                }
                Err(e) => results.push(Err(e)),
            }
        }
        // Carve each request buffer into the planned pieces; all go to the device together.
        {
            let mut reads: Vec<(u64, &mut [u8])> = Vec::new();
            let mut by_request: Vec<Option<&mut [u8]>> =
                reqs.iter_mut().map(|r| Some(&mut *r.buf)).collect();
            for (i, plan) in &plans {
                let mut rest = by_request[*i].take().expect("one plan per request");
                let mut at = 0;
                for p in plan.reads() {
                    let (_, tail) = std::mem::take(&mut rest).split_at_mut(p.buf_offset - at);
                    let (piece, tail) = tail.split_at_mut(p.len);
                    reads.push((p.device_offset, piece));
                    rest = tail;
                    at = p.buf_offset + p.len;
                }
            }
            self.dev.read_many(&mut reads)?;
        }
        for (i, plan) in &plans {
            if let Err(e) = plan.verify(reqs[*i].buf) {
                results[*i] = Err(e);
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
        self.state.release_pending(self.reads_in_flight.get() > 0);

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
        let batch: Vec<(u64, &[u8])> = writes.iter().map(|(o, p)| (*o, &p[..])).collect();
        self.dev.write_many(&batch)?;
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
            journal_head: self.journal.head.offset,
            data_quarantined: self.state.data.blocked_count(),
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

/// Whether the redirect-on-write data of every `MapUpdate` in `batch` is on disk, according to
/// the checksums the records carry.
/// Whether the redirect-on-write data of `batch` is on the disk, except for the clusters in
/// `rewritten` ((slot, chunk-relative cluster)), which are not checked.
fn batch_data_is_durable<D: BlockDevice>(
    dev: &D,
    layout: &Layout,
    batch: &Batch,
    rewritten: &HashSet<(u32, u64)>,
) -> Result<bool, StoreError> {
    let mut buf = vec![0u8; layout.cluster_size as usize];
    for record in batch.records() {
        let Record::MapUpdate {
            slot,
            first_cluster,
            count,
            csums_per_cluster,
            phys_first,
            csums,
            ..
        } = record?
        else {
            continue;
        };
        let cpc = csums_per_cluster as usize;
        if phys_first
            .checked_add(u64::from(count))
            .is_none_or(|end| end > layout.data_clusters)
        {
            return Err(StoreError::corrupt(
                "journal MapUpdate points outside the data area",
            ));
        }
        for i in 0..count as usize {
            if rewritten.contains(&(slot, u64::from(first_cluster) + i as u64)) {
                continue;
            }
            dev.read_at(layout.data_cluster_offset(phys_first + i as u64), &mut buf)?;
            let expected = &csums[i * cpc..(i + 1) * cpc];
            if buf
                .chunks_exact(BLOCK_SIZE)
                .zip(expected)
                .any(|(b, &c)| crc32c(b) != c)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
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
