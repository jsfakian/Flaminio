//! A node holding chunk replicas (protocol spec §3, §6, §7).
//!
//! [`ReplicaCore`] holds the protocol rules without storage: fencing, sessions, uncertain sets
//! and resync filters. For each request it returns an [`Action`] for its caller to carry out:
//! the data plane against the replica store, the simulator against the in-memory
//! [`Replica`].
//!
//! Durable state is the data and the fenced epochs. Sessions, filters and uncertain sets are
//! volatile. After a restart a replica cannot tell which of its recent writes completed, so it
//! reports every block of its chunks as uncertain. That lasts until a target of a newer epoch
//! than any it had seen prunes the set: such a target has reconciled the chunk.

use std::collections::{BTreeMap, BTreeSet};

use crate::extent::{Extent, ExtentSet};
use crate::msg::{FromReplica, Request, Response, ToReplica};
use crate::types::{Block, ChunkIdx, Epoch, Value, Version, VolumeId};

/// What the caller does with a request, in the order the requests arrived on their session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action<P> {
    /// Send this reply.
    Reply(Response<P>),
    /// Make the volume's fenced epoch durable, then send the reply.
    Fence { epoch: Epoch, reply: Response<P> },
    /// Ignore the request: it belongs to a session that is no longer current.
    Drop,
    /// Write the extent, then reply `Ok`.
    Write {
        chunk: ChunkIdx,
        ext: Extent,
        data: P,
    },
    /// Read the extent, then reply `Data`.
    Read { chunk: ChunkIdx, ext: Extent },
    /// A `BeginResync` for a chunk this node does not hold: a replica moving here. Create the
    /// chunk, empty ([`ReplicaCore::add_chunk`]), then handle the request again.
    CreateChunk { chunk: ChunkIdx },
    /// Write only `parts` of the extent (the blocks without live writes since
    /// `BeginResync`), taking each part's data from its offset in `data`, then reply `Ok`.
    Copy {
        chunk: ChunkIdx,
        ext: Extent,
        data: P,
        parts: Vec<Extent>,
    },
    /// Return digests for the extent (store metadata or simulator block values).
    Digest { chunk: ChunkIdx, ext: Extent },
    /// Return the sub-extents of `ext` that hold data.
    Mapped { chunk: ChunkIdx, ext: Extent },
    /// Discard `parts` of the extent (blocks without live writes since `BeginResync`), then
    /// reply `Ok`.
    Discard {
        chunk: ChunkIdx,
        ext: Extent,
        parts: Vec<Extent>,
    },
}

/// Per-volume slots for held chunks and precise uncertainty records together.
pub const UNCERTAINTY_BUDGET: u32 = 65_536;

const ARENA_PAGE_SLOTS: usize = 256;

/// Refusing an add never changes an existing chunk or its uncertainty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkError {
    Exists,
    TooManyChunks,
}

/// Read-only accounting for the bounded uncertainty representation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UncertaintyStats {
    pub chunks: usize,
    pub records: usize,
    pub allocated_slots: usize,
}

#[derive(Debug, Clone, Copy)]
struct PreciseRecord {
    ext: Extent,
    version: Version,
    next: Option<usize>,
}

#[derive(Debug, Clone)]
enum RecordSlot {
    Record(PreciseRecord),
    Free(Option<usize>),
}

/// One arena per volume. Free-list links share slots with records, so pruning cannot leave
/// a growing allocation behind in every chunk that once held uncertainty.
#[derive(Debug, Clone, Default)]
struct RecordArena {
    pages: Vec<Box<[RecordSlot]>>,
    allocated: usize,
    free: Option<usize>,
}

impl RecordArena {
    fn slot(&self, index: usize) -> &RecordSlot {
        &self.pages[index / ARENA_PAGE_SLOTS][index % ARENA_PAGE_SLOTS]
    }

    fn slot_mut(&mut self, index: usize) -> &mut RecordSlot {
        &mut self.pages[index / ARENA_PAGE_SLOTS][index % ARENA_PAGE_SLOTS]
    }

    fn record(&self, index: usize) -> &PreciseRecord {
        match self.slot(index) {
            RecordSlot::Record(record) => record,
            RecordSlot::Free(_) => unreachable!("a chunk links only live records"),
        }
    }

    fn record_mut(&mut self, index: usize) -> &mut PreciseRecord {
        match self.slot_mut(index) {
            RecordSlot::Record(record) => record,
            RecordSlot::Free(_) => unreachable!("a chunk links only live records"),
        }
    }

    fn insert(&mut self, record: PreciseRecord, limit: usize) -> usize {
        if self.free.is_none() {
            let count = ARENA_PAGE_SLOTS.min(limit - self.allocated);
            assert!(count > 0, "precise record demand is within the budget");
            let start = self.pages.len() * ARENA_PAGE_SLOTS;
            let page = (0..count)
                .map(|offset| RecordSlot::Free((offset + 1 < count).then_some(start + offset + 1)))
                .collect::<Vec<_>>()
                .into_boxed_slice();
            self.pages.push(page);
            self.allocated += count;
            self.free = Some(start);
        }
        let index = self.free.expect("allocated a free slot");
        self.free = match self.slot(index) {
            RecordSlot::Free(next) => *next,
            RecordSlot::Record(_) => unreachable!("the free list contains only free slots"),
        };
        *self.slot_mut(index) = RecordSlot::Record(record);
        index
    }

    fn remove(&mut self, index: usize) -> PreciseRecord {
        let free = self.free;
        let old = std::mem::replace(self.slot_mut(index), RecordSlot::Free(free));
        self.free = Some(index);
        match old {
            RecordSlot::Record(record) => record,
            RecordSlot::Free(_) => unreachable!("a record is freed only once"),
        }
    }
}

#[derive(Debug, Clone)]
struct CoreChunk {
    blocks: Block,
    /// Head of this chunk's precise records in its volume's arena.
    head: Option<usize>,
    /// Whole-chunk uncertainty, retaining the greatest version it represents.
    cover: Option<Version>,
    /// After a restart: every block may hold an unfinished write of an epoch up to this one.
    restarted_at: Option<Epoch>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct UncertaintyWork {
    retained_writes: usize,
    coarse_updates: usize,
    cleared_records: usize,
    pruned_records: usize,
    report_chunks: usize,
    report_records: usize,
}

#[derive(Debug, Clone, Default)]
struct CoreVol {
    fenced: Epoch,
    chunks: BTreeMap<ChunkIdx, CoreChunk>,
    session: Option<(Epoch, u64)>,
    filters: BTreeMap<ChunkIdx, ExtentSet>,
    arena: RecordArena,
    records: usize,
    record_chunks: BTreeSet<ChunkIdx>,
    #[cfg(test)]
    work: std::cell::Cell<UncertaintyWork>,
}

impl CoreVol {
    fn clear_records(&mut self, chunk: ChunkIdx) -> Option<Version> {
        #[cfg(test)]
        let before = self.records;
        let mut next = self.chunks.get_mut(&chunk).expect("held chunk").head.take();
        let had_records = next.is_some();
        let mut maximum = None;
        while let Some(index) = next {
            let record = self.arena.remove(index);
            next = record.next;
            maximum = Some(maximum.map_or(record.version, |m: Version| m.max(record.version)));
            self.records -= 1;
        }
        if had_records {
            self.record_chunks.remove(&chunk);
        }
        #[cfg(test)]
        {
            let mut work = self.work.get();
            work.cleared_records += before - self.records;
            self.work.set(work);
        }
        maximum
    }

    fn coarsen(&mut self, chunk: ChunkIdx, incoming: Option<Version>, drop_cover: bool) {
        let retained = self.clear_records(chunk);
        let maximum = match (retained, incoming) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.chunks.get_mut(&chunk).expect("held chunk").cover =
            if drop_cover { None } else { maximum };
    }

    fn prune(&mut self, chunk: ChunkIdx, below: Version, coarse_prune_early: bool) {
        #[cfg(test)]
        let mut visits = 0;
        let c = self.chunks.get_mut(&chunk).expect("held chunk");
        if c.cover
            .is_some_and(|maximum| coarse_prune_early || maximum < below)
        {
            c.cover = None;
        }
        if c.restarted_at.is_some_and(|epoch| epoch < below.epoch) {
            c.restarted_at = None;
        }
        let mut next = c.head.take();
        let had_records = next.is_some();
        let mut head = None;
        let mut tail = None;
        while let Some(index) = next {
            #[cfg(test)]
            {
                visits += 1;
            }
            let record = *self.arena.record(index);
            next = record.next;
            if record.version < below {
                self.arena.remove(index);
                self.records -= 1;
            } else {
                if let Some(previous) = tail {
                    self.arena.record_mut(previous).next = Some(index);
                } else {
                    head = Some(index);
                }
                tail = Some(index);
            }
        }
        if let Some(last) = tail {
            self.arena.record_mut(last).next = None;
        }
        c.head = head;
        if had_records && head.is_none() {
            self.record_chunks.remove(&chunk);
        }
        #[cfg(test)]
        {
            let mut work = self.work.get();
            work.pruned_records += visits;
            self.work.set(work);
        }
    }

    fn retain_write(
        &mut self,
        chunk: ChunkIdx,
        ext: Extent,
        version: Version,
        budget: usize,
        drop_cover: bool,
        bypass_budget: bool,
    ) {
        #[cfg(test)]
        {
            let mut work = self.work.get();
            work.retained_writes += 1;
            self.work.set(work);
        }
        let c = self.chunks.get_mut(&chunk).expect("held chunk");
        if let Some(maximum) = &mut c.cover {
            *maximum = (*maximum).max(version);
            #[cfg(test)]
            {
                let mut work = self.work.get();
                work.coarse_updates += 1;
                self.work.set(work);
            }
            return;
        }
        if !bypass_budget && self.chunks.len() + self.records >= budget {
            self.coarsen(chunk, Some(version), drop_cover);
            return;
        }
        let next = self.chunks[&chunk].head;
        // The mutation may exceed demand; keep the allocation well-defined so the simulator's
        // independent checker, rather than an allocation assertion, catches the violation.
        let limit = if bypass_budget {
            budget.max(self.arena.allocated.saturating_add(1))
        } else {
            budget
        };
        let index = self
            .arena
            .insert(PreciseRecord { ext, version, next }, limit);
        self.chunks.get_mut(&chunk).expect("held chunk").head = Some(index);
        self.records += 1;
        if next.is_none() {
            self.record_chunks.insert(chunk);
        }
    }

    fn report(&self) -> Vec<(ChunkIdx, Vec<Extent>)> {
        #[cfg(test)]
        let mut visits = 0;
        let rows: Vec<_> = self
            .chunks
            .iter()
            .map(|(&chunk, c)| {
                let extents = if c.restarted_at.is_some() || c.cover.is_some() {
                    vec![Extent::new(0, c.blocks)]
                } else {
                    let mut set = ExtentSet::new();
                    let mut next = c.head;
                    while let Some(index) = next {
                        #[cfg(test)]
                        {
                            visits += 1;
                        }
                        let record = self.arena.record(index);
                        next = record.next;
                        set.insert(record.ext);
                    }
                    set.iter().collect()
                };
                (chunk, extents)
            })
            .collect();
        #[cfg(test)]
        {
            let mut work = self.work.get();
            work.report_chunks += rows.len();
            work.report_records += visits;
            self.work.set(work);
        }
        rows
    }
}

/// Deliberate protocol violations, for checking that the simulator catches them.
#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Bugs {
    /// Accept requests from any session of the current epoch.
    pub any_session: bool,
    /// Apply resync copies even over newer live writes.
    pub no_resync_filter: bool,
    /// Lose uncertainty when precise records become a whole-chunk cover.
    pub drop_on_coarsen: bool,
    /// Clear a cover even when the prune bound does not exceed its maximum version.
    pub coarse_prune_early: bool,
    /// Retain precise records beyond the volume's uncertainty budget.
    pub bypass_uncertainty_budget: bool,
    /// Trust write/prune/Settle versions from a different epoch than their session.
    pub accept_foreign_epoch: bool,
}

/// The protocol rules of a replica node.
#[derive(Debug, Clone)]
pub struct ReplicaCore {
    vols: BTreeMap<VolumeId, CoreVol>,
    budget: u32,
    #[cfg(test)]
    pub bugs: Bugs,
}

impl Default for ReplicaCore {
    fn default() -> Self {
        Self::with_budget(UNCERTAINTY_BUDGET)
    }
}

impl ReplicaCore {
    pub fn with_budget(budget: u32) -> Self {
        Self {
            vols: BTreeMap::new(),
            budget,
            #[cfg(test)]
            bugs: Bugs::default(),
        }
    }

    pub fn budget(&self) -> u32 {
        self.budget
    }

    pub fn uncertainty_stats(&self, vol: VolumeId) -> UncertaintyStats {
        self.vols
            .get(&vol)
            .map_or_else(UncertaintyStats::default, |v| UncertaintyStats {
                chunks: v.chunks.len(),
                records: v.records,
                allocated_slots: v.arena.allocated,
            })
    }

    /// A complete report, including explicit empty rows for every held chunk.
    pub fn report(&self, vol: VolumeId) -> Vec<(ChunkIdx, Vec<Extent>)> {
        self.vols.get(&vol).map_or_else(Vec::new, CoreVol::report)
    }

    pub fn can_add_chunk(&self, vol: VolumeId, chunk: ChunkIdx) -> Result<(), ChunkError> {
        let held = self.vols.get(&vol);
        if held.is_some_and(|v| v.chunks.contains_key(&chunk)) {
            return Err(ChunkError::Exists);
        }
        if held.map_or(0, |v| v.chunks.len()) >= self.budget as usize {
            return Err(ChunkError::TooManyChunks);
        }
        Ok(())
    }

    /// Adds a new, empty chunk replica.
    pub fn add_chunk(
        &mut self,
        vol: VolumeId,
        chunk: ChunkIdx,
        blocks: Block,
    ) -> Result<(), ChunkError> {
        self.insert_chunk(vol, chunk, blocks, None)
    }

    fn insert_chunk(
        &mut self,
        vol: VolumeId,
        chunk: ChunkIdx,
        blocks: Block,
        restarted_at: Option<Epoch>,
    ) -> Result<(), ChunkError> {
        self.can_add_chunk(vol, chunk)?;
        #[cfg(not(test))]
        let drop_cover = false;
        #[cfg(test)]
        let drop_cover = self.bugs.drop_on_coarsen;
        let v = self.vols.entry(vol).or_default();
        if v.chunks.len() + v.records >= self.budget as usize {
            let oldest_chunk = *v
                .record_chunks
                .first()
                .expect("records consume the remaining budget");
            v.coarsen(oldest_chunk, None, drop_cover);
        }
        v.chunks.insert(
            chunk,
            CoreChunk {
                blocks,
                head: None,
                cover: None,
                restarted_at,
            },
        );
        Ok(())
    }

    /// Adds a chunk replica found on disk at startup, with its volume's fenced epoch.
    pub fn recover_chunk(
        &mut self,
        vol: VolumeId,
        fenced: Epoch,
        chunk: ChunkIdx,
        blocks: Block,
    ) -> Result<(), ChunkError> {
        self.insert_chunk(vol, chunk, blocks, Some(fenced))?;
        let v = self.vols.get_mut(&vol).expect("inserted chunk");
        v.fenced = v.fenced.max(fenced);
        Ok(())
    }

    /// Loses the volatile state, as after a process or node crash.
    pub fn restart(&mut self) {
        for v in self.vols.values_mut() {
            v.session = None;
            v.filters.clear();
            while let Some(&chunk) = v.record_chunks.first() {
                v.clear_records(chunk);
            }
            for c in v.chunks.values_mut() {
                c.cover = None;
                c.restarted_at = Some(v.fenced);
            }
        }
    }

    /// Forgets a chunk replica that moved away. The volume's fenced epoch stays.
    pub fn remove_chunk(&mut self, vol: VolumeId, chunk: ChunkIdx) {
        if let Some(v) = self.vols.get_mut(&vol) {
            if v.chunks.contains_key(&chunk) {
                v.clear_records(chunk);
            }
            v.chunks.remove(&chunk);
            v.filters.remove(&chunk);
        }
    }

    /// Forgets a deleted volume entirely, its fenced epoch included: for a volume that no
    /// longer exists, so that a new volume of the same name starts afresh. Requests of its
    /// sessions are not valid after this.
    pub fn remove_volume(&mut self, vol: VolumeId) {
        self.vols.remove(&vol);
    }

    pub fn fenced(&self, vol: VolumeId) -> Epoch {
        self.vols.get(&vol).map_or(0, |v| v.fenced)
    }

    /// Decides what to do with one request. Requests must be handled in the order they arrive
    /// on their connection, and their actions carried out in that order.
    pub fn handle<P: Clone>(&mut self, m: &ToReplica<P>) -> Action<P> {
        #[cfg(not(test))]
        let (drop_cover, coarse_prune_early, bypass_budget, accept_foreign_epoch) =
            (false, false, false, false);
        #[cfg(test)]
        let (drop_cover, coarse_prune_early, bypass_budget, accept_foreign_epoch) = (
            self.bugs.drop_on_coarsen,
            self.bugs.coarse_prune_early,
            self.bugs.bypass_uncertainty_budget,
            self.bugs.accept_foreign_epoch,
        );
        let v = self.vols.entry(m.vol).or_default();
        if m.epoch < v.fenced {
            return Action::Reply(Response::Stale { fenced: v.fenced });
        }
        if let Request::Hello { .. } = m.body {
            v.fenced = m.epoch;
            if v.session.is_none_or(|s| s < (m.epoch, m.session)) {
                v.session = Some((m.epoch, m.session));
                v.filters.clear();
            }
            let uncertain = v.report();
            return Action::Fence {
                epoch: m.epoch,
                reply: Response::Fenced { uncertain },
            };
        }
        let current = v.session == Some((m.epoch, m.session));
        #[cfg(test)]
        let current = current || (self.bugs.any_session && m.epoch == v.fenced);
        if !current {
            return Action::Drop;
        }
        let in_range = |chunk: &ChunkIdx, ext: &Extent| {
            v.chunks
                .get(chunk)
                .is_some_and(|c| ext.len > 0 && ext.end() <= c.blocks)
        };
        match &m.body {
            Request::Hello { .. } => unreachable!(),
            Request::Heartbeat => Action::Reply(Response::Ok),
            Request::Read { chunk, ext } if in_range(chunk, ext) => Action::Read {
                chunk: *chunk,
                ext: *ext,
            },
            Request::Digest { chunk, ext } if in_range(chunk, ext) => Action::Digest {
                chunk: *chunk,
                ext: *ext,
            },
            Request::Mapped { chunk, ext } if in_range(chunk, ext) => Action::Mapped {
                chunk: *chunk,
                ext: *ext,
            },
            Request::Discard { chunk, ext } if in_range(chunk, ext) => {
                // Same filter as Copy: live writes since BeginResync win over a discard of a
                // hole observed before those writes.
                let parts = match v.filters.get(chunk) {
                    Some(f) => f.missing_from(ext),
                    None => vec![*ext],
                };
                Action::Discard {
                    chunk: *chunk,
                    ext: *ext,
                    parts,
                }
            }
            Request::Write {
                chunk,
                ext,
                data,
                version,
                prune,
            } if in_range(chunk, ext) => {
                if !accept_foreign_epoch
                    && (version.epoch != m.epoch
                        || prune.is_some_and(|bound| bound.epoch != m.epoch))
                {
                    return Action::Drop;
                }
                if let Some(p) = prune {
                    v.prune(*chunk, *p, coarse_prune_early);
                }
                if prune.is_none_or(|bound| *version >= bound) {
                    v.retain_write(
                        *chunk,
                        *ext,
                        *version,
                        self.budget as usize,
                        drop_cover,
                        bypass_budget,
                    );
                }
                if let Some(f) = v.filters.get_mut(chunk) {
                    f.insert(*ext);
                }
                Action::Write {
                    chunk: *chunk,
                    ext: *ext,
                    data: data.clone(),
                }
            }
            Request::Settle { chunk, below } if v.chunks.contains_key(chunk) => {
                if !accept_foreign_epoch && below.epoch != m.epoch {
                    return Action::Drop;
                }
                v.prune(*chunk, *below, coarse_prune_early);
                Action::Reply(Response::Ok)
            }
            Request::BeginResync { chunk } if v.chunks.contains_key(chunk) => {
                v.filters.insert(*chunk, ExtentSet::new());
                Action::Reply(Response::Ok)
            }
            // The target resyncs only replicas that the metadata service places here.
            Request::BeginResync { chunk } => Action::CreateChunk { chunk: *chunk },
            Request::Copy { chunk, ext, data } if in_range(chunk, ext) => {
                let parts = match v.filters.get(chunk) {
                    Some(f) => f.missing_from(ext),
                    None => vec![*ext],
                };
                #[cfg(test)]
                let parts = if self.bugs.no_resync_filter {
                    vec![*ext]
                } else {
                    parts
                };
                Action::Copy {
                    chunk: *chunk,
                    ext: *ext,
                    data: data.clone(),
                    parts,
                }
            }
            // A request for a chunk this node does not hold, or out of its range: a protocol
            // error by the target. Dropping it makes the target's request time out.
            _ => Action::Drop,
        }
    }
}

/// The simulator's replica node: the core over in-memory blocks. Payloads are one value per
/// block of the extent.
#[derive(Debug, Clone, Default)]
pub struct Replica {
    pub core: ReplicaCore,
    data: BTreeMap<(VolumeId, ChunkIdx), BTreeMap<Block, Value>>,
    /// Size of the chunks it creates for replicas moving here (one volume shape per run).
    pub blocks_per_chunk: Block,
}

impl Replica {
    /// A node whose chunks have `blocks` blocks each.
    pub fn with_chunk_size(blocks: Block) -> Self {
        Self::with_chunk_size_and_budget(blocks, UNCERTAINTY_BUDGET)
    }

    pub fn with_chunk_size_and_budget(blocks: Block, budget: u32) -> Self {
        Self {
            core: ReplicaCore::with_budget(budget),
            blocks_per_chunk: blocks,
            ..Self::default()
        }
    }

    pub fn create_chunk(
        &mut self,
        vol: VolumeId,
        chunk: ChunkIdx,
        blocks: Block,
    ) -> Result<(), ChunkError> {
        self.core.add_chunk(vol, chunk, blocks)?;
        self.data.entry((vol, chunk)).or_default();
        Ok(())
    }

    pub fn restart(&mut self) {
        self.core.restart();
    }

    /// Deletes a chunk replica that moved away, data included.
    pub fn remove_chunk(&mut self, vol: VolumeId, chunk: ChunkIdx) {
        self.core.remove_chunk(vol, chunk);
        self.data.remove(&(vol, chunk));
    }

    /// Contents of a block, for checks.
    pub fn block(&self, vol: VolumeId, chunk: ChunkIdx, block: Block) -> Option<Value> {
        let c = self.data.get(&(vol, chunk))?;
        Some(c.get(&block).copied().unwrap_or(0))
    }

    /// Sets a block on this replica only, as a write that reached no other replica would: for
    /// tests whose replicas must disagree.
    #[cfg(test)]
    pub fn set_block(&mut self, vol: VolumeId, chunk: ChunkIdx, block: Block, v: Value) {
        self.data
            .get_mut(&(vol, chunk))
            .expect("chunk")
            .insert(block, v);
    }

    /// Handles one request. Returns the reply, or `None` if the request is dropped.
    pub fn handle(&mut self, m: &ToReplica<Vec<Value>>) -> Option<FromReplica<Vec<Value>>> {
        let mut action = self.core.handle(m);
        if matches!(action, Action::Fence { .. })
            && let Request::Hello { chunks } = &m.body
        {
            // The packet carries the send-time placement, even if its target is now gone.
            // The core's stale check runs before creating anything, as on the real node.
            for &chunk in chunks {
                if !self.data.contains_key(&(m.vol, chunk)) {
                    self.create_chunk(m.vol, chunk, self.blocks_per_chunk)
                        .ok()?;
                }
            }
            action = Action::Fence {
                epoch: m.epoch,
                reply: Response::Fenced {
                    uncertain: self.core.report(m.vol),
                },
            };
        }
        if let Action::CreateChunk { chunk } = action {
            let blocks = self.blocks_per_chunk;
            self.core.can_add_chunk(m.vol, chunk).ok()?;
            self.create_chunk(m.vol, chunk, blocks).ok()?;
            action = self.core.handle(m);
        }
        let body = match action {
            Action::CreateChunk { .. } => unreachable!("created above"),
            Action::Reply(r) | Action::Fence { reply: r, .. } => r,
            Action::Drop => return None,
            Action::Write { chunk, ext, data } => {
                assert_eq!(data.len(), ext.len as usize, "payload size");
                let c = self.data.get_mut(&(m.vol, chunk)).expect("chunk");
                for (b, v) in ext.blocks().zip(data) {
                    c.insert(b, v);
                }
                Response::Ok
            }
            Action::Read { chunk, ext } => {
                let c = &self.data[&(m.vol, chunk)];
                Response::Data(
                    ext.blocks()
                        .map(|b| c.get(&b).copied().unwrap_or(0))
                        .collect(),
                )
            }
            Action::Digest { chunk, ext } => {
                // Simulator digests are the block values (follow-ups §2). Never-written = 0.
                let c = self.data.get(&(m.vol, chunk));
                Response::Digests(
                    ext.blocks()
                        .map(|b| {
                            let v = c.and_then(|m| m.get(&b).copied()).unwrap_or(0);
                            Some(v as u32)
                        })
                        .collect(),
                )
            }
            Action::Mapped { chunk, ext } => {
                // Written blocks only (follow-ups §3). Never-written keys are absent.
                let mut mapped = ExtentSet::new();
                if let Some(c) = self.data.get(&(m.vol, chunk)) {
                    for b in ext.blocks() {
                        if c.contains_key(&b) {
                            mapped.insert(Extent::new(b, 1));
                        }
                    }
                }
                Response::Mapped(mapped.iter().collect())
            }
            Action::Discard { chunk, parts, .. } => {
                if let Some(c) = self.data.get_mut(&(m.vol, chunk)) {
                    for p in parts {
                        for b in p.blocks() {
                            c.remove(&b);
                        }
                    }
                }
                Response::Ok
            }
            Action::Copy {
                chunk,
                ext,
                data,
                parts,
            } => {
                assert_eq!(data.len(), ext.len as usize, "payload size");
                let c = self.data.get_mut(&(m.vol, chunk)).expect("chunk");
                for p in parts {
                    for b in p.blocks() {
                        c.insert(b, data[(b - ext.start) as usize]);
                    }
                }
                Response::Ok
            }
        };
        Some(FromReplica {
            vol: m.vol,
            epoch: m.epoch,
            session: m.session,
            req: m.req,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Msg = ToReplica<Vec<Value>>;

    fn msg(epoch: Epoch, session: u64, body: Request<Vec<Value>>) -> Msg {
        ToReplica {
            vol: 1,
            epoch,
            session,
            req: 0,
            body,
        }
    }

    fn write(start: Block, values: &[Value], seq: u64, prune: Option<u64>) -> Request<Vec<Value>> {
        Request::Write {
            chunk: 0,
            ext: Extent::new(start, values.len() as Block),
            data: values.to_vec(),
            version: Version { epoch: 1, seq },
            prune: prune.map(|seq| Version { epoch: 1, seq }),
        }
    }

    fn replica() -> Replica {
        let mut r = Replica::default();
        r.create_chunk(1, 0, 8).unwrap();
        r
    }

    fn body(r: Option<FromReplica<Vec<Value>>>) -> Option<Response<Vec<Value>>> {
        r.map(|r| r.body)
    }

    fn uncertain(r: Option<FromReplica<Vec<Value>>>) -> Vec<Extent> {
        match body(r) {
            Some(Response::Fenced { uncertain }) => uncertain[0].1.clone(),
            other => panic!("{other:?}"),
        }
    }

    struct UncertaintyHarness {
        core: ReplicaCore,
        epoch: Epoch,
        session: u64,
        blocks: Block,
    }

    impl UncertaintyHarness {
        fn new(budget: u32, chunks: &[ChunkIdx], blocks: Block) -> Self {
            let mut core = ReplicaCore::with_budget(budget);
            for &chunk in chunks {
                core.add_chunk(1, chunk, blocks).unwrap();
            }
            let mut h = Self {
                core,
                epoch: 0,
                session: 0,
                blocks,
            };
            h.hello(1);
            h
        }

        fn send(&mut self, body: Request<()>) -> Action<()> {
            self.core.handle(&ToReplica {
                vol: 1,
                epoch: self.epoch,
                session: self.session,
                req: 0,
                body,
            })
        }

        fn hello(&mut self, epoch: Epoch) {
            self.epoch = epoch;
            self.session += 1;
            assert!(
                matches!(self.send(Request::Hello { chunks: vec![] }), Action::Fence { epoch: e, .. } if e == epoch)
            );
        }

        fn write(&mut self, chunk: ChunkIdx, ext: Extent, seq: u64, prune: Option<Version>) {
            assert_eq!(
                self.send(Request::Write {
                    chunk,
                    ext,
                    data: (),
                    version: Version {
                        epoch: self.epoch,
                        seq
                    },
                    prune,
                }),
                Action::Write {
                    chunk,
                    ext,
                    data: ()
                }
            );
        }

        fn settle(&mut self, chunk: ChunkIdx, seq: u64) {
            assert_eq!(
                self.send(Request::Settle {
                    chunk,
                    below: Version {
                        epoch: self.epoch,
                        seq
                    },
                }),
                Action::Reply(Response::Ok)
            );
        }

        fn row(&self, chunk: ChunkIdx) -> Vec<Extent> {
            self.core
                .report(1)
                .into_iter()
                .find(|(id, _)| *id == chunk)
                .unwrap()
                .1
        }

        fn whole(&self) -> Vec<Extent> {
            vec![Extent::new(0, self.blocks)]
        }
    }

    /// Check actual allocated slots and free/live links, not only the cached record count.
    fn assert_arena_bound(core: &ReplicaCore, vol: VolumeId) {
        let stats = core.uncertainty_stats(vol);
        assert!(
            stats.chunks + stats.records <= core.budget() as usize,
            "{stats:?}"
        );
        assert!(stats.allocated_slots <= core.budget() as usize, "{stats:?}");
        let Some(v) = core.vols.get(&vol) else {
            assert_eq!(stats, UncertaintyStats::default());
            return;
        };
        let allocated: usize = v.arena.pages.iter().map(|p| p.len()).sum();
        assert_eq!(allocated, stats.allocated_slots);
        assert_eq!(allocated, v.arena.allocated);
        for (page, slots) in v.arena.pages.iter().enumerate() {
            assert!(!slots.is_empty() && slots.len() <= ARENA_PAGE_SLOTS);
            if page + 1 < v.arena.pages.len() {
                assert_eq!(slots.len(), ARENA_PAGE_SLOTS);
            }
        }
        let mut live = BTreeSet::new();
        let mut record_chunks = BTreeSet::new();
        for (&chunk, state) in &v.chunks {
            if state.head.is_some() {
                record_chunks.insert(chunk);
                assert!(state.cover.is_none());
            }
            let mut next = state.head;
            while let Some(index) = next {
                assert!(index < allocated);
                assert!(live.insert(index), "record linked more than once");
                next = v.arena.record(index).next;
            }
        }
        assert_eq!(record_chunks, v.record_chunks);
        assert_eq!(live.len(), stats.records);
        let actual_records = v
            .arena
            .pages
            .iter()
            .flatten()
            .filter(|s| matches!(s, RecordSlot::Record(_)))
            .count();
        assert_eq!(actual_records, live.len());
        let mut free = BTreeSet::new();
        let mut next = v.arena.free;
        while let Some(index) = next {
            assert!(index < allocated && !live.contains(&index));
            assert!(free.insert(index), "free-list cycle");
            next = match v.arena.slot(index) {
                RecordSlot::Free(next) => *next,
                RecordSlot::Record(_) => panic!("live record in free list"),
            };
        }
        assert_eq!(live.len() + free.len(), allocated);
        let rows = core.report(vol);
        assert_eq!(rows.len(), stats.chunks);
        assert!(rows.iter().map(|(_, xs)| xs.len()).sum::<usize>() <= core.budget() as usize);
    }

    #[test]
    fn uncertainty_budget_boundaries_count_duplicate_records() {
        assert_eq!(ReplicaCore::default().budget(), UNCERTAINTY_BUDGET);
        let mut zero = ReplicaCore::with_budget(0);
        assert_eq!(zero.add_chunk(1, 0, 16), Err(ChunkError::TooManyChunks));
        assert_arena_bound(&zero, 1);
        let mut h = UncertaintyHarness::new(8, &[0, 1], 32);
        for seq in 1..=5 {
            h.write(0, Extent::new(2, 1), seq, None);
        }
        assert_eq!(h.core.uncertainty_stats(1).records, 5); // N+R=B-1, despite one union extent.
        h.write(0, Extent::new(4, 1), 6, None);
        assert_eq!(h.core.uncertainty_stats(1).records, 6); // N+R=B.
        assert_eq!(h.row(0), vec![Extent::new(2, 1), Extent::new(4, 1)]);
        h.write(0, Extent::new(6, 1), 7, None); // B+1 demand coarsens instead of allocating.
        assert_eq!(h.row(0), h.whole());
        assert!(h.row(1).is_empty());
        assert_eq!(h.core.uncertainty_stats(1).records, 0);
        assert_arena_bound(&h.core, 1);
    }

    #[test]
    fn pruning_precedes_full_budget_retention_without_unnecessary_coarsening() {
        let mut h = UncertaintyHarness::new(4, &[0], 16);
        for seq in 1..=3 {
            h.write(0, Extent::new((seq * 2) as Block, 1), seq, None);
        }
        let allocated = h.core.uncertainty_stats(1).allocated_slots;
        assert_eq!(h.core.uncertainty_stats(1).records, 3);
        h.core.vols[&1].work.set(UncertaintyWork::default());
        h.write(0, Extent::new(8, 1), 4, Some(Version { epoch: 1, seq: 4 }));
        assert_eq!(h.row(0), vec![Extent::new(8, 1)]);
        assert_eq!(h.core.uncertainty_stats(1).records, 1);
        // A bound of the session's own epoch stays the target's completion claim.
        h.write(0, Extent::new(10, 1), 5, Some(Version { epoch: 1, seq: 6 }));
        assert!(h.row(0).is_empty());
        assert_eq!(h.core.uncertainty_stats(1).records, 0);
        h.write(0, Extent::new(12, 1), 7, None);
        assert_eq!(h.row(0), vec![Extent::new(12, 1)]);
        let work = h.core.vols[&1].work.get();
        assert_eq!(work.cleared_records, 0, "no coarsening was needed");
        assert_eq!(work.pruned_records, 4);
        assert_eq!(work.retained_writes, 2);
        assert_eq!(h.core.uncertainty_stats(1).allocated_slots, allocated);
        assert_arena_bound(&h.core, 1);
    }

    #[test]
    fn coarsening_changes_only_the_written_chunk_and_keeps_the_maximum_version() {
        let mut h = UncertaintyHarness::new(8, &[0, 1, 2], 64);
        for (chunk, start, seq) in [(0, 1, 100), (0, 3, 4), (1, 7, 5), (1, 9, 6), (2, 11, 7)] {
            h.write(chunk, Extent::new(start, 1), seq, None);
        }
        let other_rows = (h.row(1), h.row(2));
        h.write(0, Extent::new(5, 1), 3, None);
        assert_eq!(h.row(0), h.whole());
        assert_eq!((h.row(1), h.row(2)), other_rows);
        h.settle(0, 100);
        assert_eq!(
            h.row(0),
            h.whole(),
            "a bound equal to the maximum does not clear it"
        );
        h.write(0, Extent::new(15, 1), 2, None);
        h.settle(0, 99);
        assert_eq!(
            h.row(0),
            h.whole(),
            "an older incoming write cannot lower the cover"
        );
        h.hello(2);
        h.write(0, Extent::new(17, 1), 1, None);
        h.settle(0, 1);
        assert_eq!(
            h.row(0),
            h.whole(),
            "the maximum compares the full epoch/sequence tuple"
        );
        h.settle(0, 2);
        assert!(h.row(0).is_empty());
        h.write(0, Extent::new(19, 1), 3, None);
        assert_eq!(
            h.row(0),
            vec![Extent::new(19, 1)],
            "a cleared cover returns to precise mode"
        );
        assert_eq!((h.row(1), h.row(2)), other_rows);
        assert_arena_bound(&h.core, 1);
    }

    #[test]
    fn adding_at_budget_coarsens_the_lowest_record_chunk_and_preserves_duplicates() {
        let mut h = UncertaintyHarness::new(4, &[5, 9], 16);
        h.write(9, Extent::new(9, 1), 1, None);
        h.write(5, Extent::new(5, 1), 2, None);
        h.core.add_chunk(1, 7, 16).unwrap();
        assert_eq!(h.row(5), h.whole());
        assert!(h.row(7).is_empty());
        assert_eq!(h.row(9), vec![Extent::new(9, 1)]);
        let before = h.core.report(1);
        let stats = h.core.uncertainty_stats(1);
        assert_eq!(h.core.can_add_chunk(1, 5), Err(ChunkError::Exists));
        assert_eq!(h.core.add_chunk(1, 5, 8), Err(ChunkError::Exists));
        assert_eq!(h.core.recover_chunk(1, 90, 9, 8), Err(ChunkError::Exists));
        assert_eq!(h.core.report(1), before);
        assert_eq!(h.core.uncertainty_stats(1), stats);
        assert_eq!(
            h.core.fenced(1),
            1,
            "failed recovery cannot change the fence"
        );
        h.core.recover_chunk(1, 2, 11, 16).unwrap();
        assert_eq!(h.row(11), h.whole());
        let full = h.core.report(1);
        assert_eq!(h.core.add_chunk(1, 12, 16), Err(ChunkError::TooManyChunks));
        assert_eq!(
            h.core.recover_chunk(1, 99, 12, 16),
            Err(ChunkError::TooManyChunks)
        );
        assert_eq!(h.core.report(1), full);
        assert_eq!(h.core.fenced(1), 2);
        h.core.remove_chunk(1, 5);
        assert_eq!(h.core.fenced(1), 2);
        h.core.add_chunk(1, 12, 16).unwrap();
        assert!(h.row(12).is_empty());
        assert_eq!(h.row(11), h.whole());
        assert_arena_bound(&h.core, 1);
        h.core.remove_volume(1);
        assert_eq!(h.core.fenced(1), 0);
        assert_arena_bound(&h.core, 1);
    }

    #[test]
    fn restart_and_coarse_cover_are_cleared_by_independent_proofs() {
        let mut h = UncertaintyHarness::new(4, &[0, 1], 16);
        for seq in 1..=3 {
            h.write(0, Extent::new(seq as Block, 1), seq, None);
        }
        assert!(h.core.vols[&1].chunks[&0].cover.is_some());
        h.write(1, Extent::new(8, 1), 4, None);
        h.core.restart();
        assert_eq!(h.core.uncertainty_stats(1).records, 0);
        for c in h.core.vols[&1].chunks.values() {
            assert_eq!(c.restarted_at, Some(1));
            assert!(c.cover.is_none() && c.head.is_none());
        }
        h.hello(1);
        h.write(0, Extent::new(2, 1), 10, None);
        assert!(
            h.core.vols[&1].chunks[&0].head.is_some(),
            "restart returns to precise retention"
        );
        h.write(0, Extent::new(4, 1), 11, None);
        h.write(0, Extent::new(6, 1), 12, None);
        assert!(h.core.vols[&1].chunks[&0].cover.is_some());
        h.settle(0, 13);
        assert!(h.core.vols[&1].chunks[&0].cover.is_none());
        assert_eq!(
            h.row(0),
            h.whole(),
            "same-epoch completion does not clear restart evidence"
        );
        h.hello(2);
        h.write(0, Extent::new(9, 1), 1, Some(Version { epoch: 2, seq: 2 }));
        assert!(
            h.row(0).is_empty(),
            "newer-epoch proof clears restart cover"
        );
        assert_eq!(h.row(1), h.whole(), "proof applies only to its chunk");
        h.settle(1, 0);
        assert!(h.row(1).is_empty());
        assert_arena_bound(&h.core, 1);
    }

    #[test]
    fn foreign_write_prune_and_settle_epochs_leave_all_uncertainty_unchanged() {
        for restarted in [false, true] {
            for foreign_epoch in [0, 2] {
                for field in 0..3 {
                    let mut h = UncertaintyHarness::new(4, &[0], 16);
                    h.write(0, Extent::new(3, 1), 7, None);
                    if restarted {
                        h.core.restart();
                        h.hello(1);
                    }
                    assert_eq!(
                        h.send(Request::BeginResync { chunk: 0 }),
                        Action::Reply(Response::Ok)
                    );
                    let before = h.core.report(1);
                    let stats = h.core.uncertainty_stats(1);
                    let bad = match field {
                        0 => Request::Write {
                            chunk: 0,
                            ext: Extent::new(5, 1),
                            data: (),
                            version: Version {
                                epoch: foreign_epoch,
                                seq: 99,
                            },
                            prune: None,
                        },
                        1 => Request::Write {
                            chunk: 0,
                            ext: Extent::new(5, 1),
                            data: (),
                            version: Version { epoch: 1, seq: 8 },
                            prune: Some(Version {
                                epoch: foreign_epoch,
                                seq: 99,
                            }),
                        },
                        _ => Request::Settle {
                            chunk: 0,
                            below: Version {
                                epoch: foreign_epoch,
                                seq: 99,
                            },
                        },
                    };
                    assert_eq!(
                        h.send(bad),
                        Action::Drop,
                        "restart={restarted}, epoch={foreign_epoch}, field={field}"
                    );
                    assert_eq!(h.core.report(1), before);
                    assert_eq!(h.core.uncertainty_stats(1), stats);
                    assert!(
                        h.core.vols[&1].filters[&0].is_empty(),
                        "rejected write cannot enter the live-write filter"
                    );
                    assert_eq!(h.core.fenced(1), 1);
                    assert_arena_bound(&h.core, 1);
                }
            }
        }
    }

    #[test]
    fn unanswered_writes_and_repairs_remain_covered_until_settle() {
        let mut h = UncertaintyHarness::new(4, &[0], 16);
        for seq in 1..=4 {
            // The core action is deliberately never executed or acknowledged by a store.
            h.write(0, Extent::new((seq * 2) as Block, 1), seq, None);
        }
        assert_eq!(h.row(0), h.whole());
        h.hello(1);
        assert_eq!(
            h.row(0),
            h.whole(),
            "a reconnect is not completion evidence"
        );
        h.hello(2);
        assert_eq!(
            h.row(0),
            h.whole(),
            "fencing alone does not clear the cover"
        );
        h.write(0, Extent::new(11, 1), 5, None); // Same representation as a repair Write.
        h.settle(0, 5);
        assert_eq!(h.row(0), h.whole());
        h.settle(0, 6);
        assert!(h.row(0).is_empty());
        h.write(0, Extent::new(13, 1), 7, None);
        assert_eq!(h.row(0), vec![Extent::new(13, 1)]);
        assert_arena_bound(&h.core, 1);
    }

    #[test]
    fn arena_slots_are_reused_across_chunks_and_partial_pages() {
        for budget in [4, 259, 515] {
            let mut h = UncertaintyHarness::new(budget, &[0, 1], 2048);
            let records = (budget - 2) as usize;
            for n in 0..records {
                h.write(0, Extent::new((n * 2) as Block, 1), n as u64, None);
            }
            let high_water = h.core.uncertainty_stats(1).allocated_slots;
            assert_eq!(
                high_water, budget as usize,
                "the capped final page is allocated"
            );
            for round in 0..32 {
                let chunk = round % 2;
                h.settle(0, u64::MAX);
                h.settle(1, u64::MAX);
                for n in 0..records {
                    h.write(chunk, Extent::new((n * 2) as Block, 1), n as u64, None);
                }
                assert_eq!(h.core.uncertainty_stats(1).allocated_slots, high_water);
                assert_arena_bound(&h.core, 1);
            }
            h.core.remove_chunk(1, 0);
            h.core.remove_chunk(1, 1);
            assert_eq!(h.core.uncertainty_stats(1).records, 0);
            assert_eq!(h.core.uncertainty_stats(1).allocated_slots, high_water);
            h.core.add_chunk(1, 2, 2048).unwrap();
            h.write(2, Extent::new(5, 1), 99, None);
            assert_eq!(h.core.uncertainty_stats(1).allocated_slots, high_water);
            assert_arena_bound(&h.core, 1);
        }
    }

    #[test]
    fn coarse_writes_and_per_chunk_pruning_have_bounded_work() {
        let mut h = UncertaintyHarness::new(16, &[0, 1], 64);
        for seq in 1..=14 {
            h.write(0, Extent::new((seq * 2) as Block, 1), seq, None);
        }
        h.core.vols[&1].work.set(UncertaintyWork::default());
        h.write(0, Extent::new(31, 1), 15, None);
        assert_eq!(h.core.vols[&1].work.get().cleared_records, 14);
        let allocated = h.core.uncertainty_stats(1).allocated_slots;
        h.core.vols[&1].work.set(UncertaintyWork::default());
        for seq in 16..4112 {
            h.write(0, Extent::new(33, 1), seq, None);
        }
        assert_eq!(
            h.core.vols[&1].work.get(),
            UncertaintyWork {
                retained_writes: 4096,
                coarse_updates: 4096,
                ..UncertaintyWork::default()
            }
        );
        assert_eq!(h.core.uncertainty_stats(1).allocated_slots, allocated);
        h.settle(0, 4112);
        for seq in 1..=3 {
            h.write(0, Extent::new(seq as Block, 1), seq, None);
        }
        for seq in 1..=4 {
            h.write(1, Extent::new((seq * 3) as Block, 1), seq, None);
        }
        h.core.vols[&1].work.set(UncertaintyWork::default());
        h.settle(0, 3);
        assert_eq!(
            h.core.vols[&1].work.get().pruned_records,
            3,
            "prune visits only its chunk's records"
        );
        h.core.vols[&1].work.set(UncertaintyWork::default());
        h.core.report(1);
        assert_eq!(
            h.core.vols[&1].work.get(),
            UncertaintyWork {
                report_chunks: 2,
                report_records: 5,
                ..UncertaintyWork::default()
            }
        );
        assert_arena_bound(&h.core, 1);
    }

    #[derive(Default)]
    struct ExactChunk {
        records: Vec<(Extent, Version)>,
        restarted_at: Option<Epoch>,
    }

    #[derive(Default)]
    struct ExactUncertainty {
        chunks: BTreeMap<ChunkIdx, ExactChunk>,
        fenced: Epoch,
    }

    impl ExactUncertainty {
        fn write(
            &mut self,
            chunk: ChunkIdx,
            ext: Extent,
            version: Version,
            prune: Option<Version>,
        ) {
            self.chunks
                .get_mut(&chunk)
                .unwrap()
                .records
                .push((ext, version));
            if let Some(below) = prune {
                self.settle(chunk, below);
            }
        }

        fn settle(&mut self, chunk: ChunkIdx, below: Version) {
            let c = self.chunks.get_mut(&chunk).unwrap();
            c.records.retain(|(_, version)| *version >= below);
            if c.restarted_at.is_some_and(|epoch| epoch < below.epoch) {
                c.restarted_at = None;
            }
        }

        fn restart(&mut self) {
            for c in self.chunks.values_mut() {
                c.records.clear();
                c.restarted_at = Some(self.fenced);
            }
        }

        fn assert_covered(&self, h: &UncertaintyHarness) {
            let report: BTreeMap<_, _> = h.core.report(1).into_iter().collect();
            assert_eq!(
                report.keys().copied().collect::<Vec<_>>(),
                self.chunks.keys().copied().collect::<Vec<_>>()
            );
            for (&chunk, exact) in &self.chunks {
                let extents = &report[&chunk];
                if exact.restarted_at.is_some() {
                    for block in 0..h.blocks {
                        assert!(
                            extents.iter().any(|e| e.contains(block)),
                            "restart uncertainty omitted: chunk={chunk}, block={block}"
                        );
                    }
                }
                for (ext, version) in &exact.records {
                    for block in ext.blocks() {
                        assert!(
                            extents.iter().any(|e| e.contains(block)),
                            "write uncertainty omitted: chunk={chunk}, block={block}, version={version:?}"
                        );
                    }
                }
            }
            assert_arena_bound(&h.core, 1);
        }
    }

    #[test]
    fn exact_reference_uncertainty_is_always_covered_through_mixed_operations() {
        for budget in [4, 8, 16] {
            for seed in 0..8u64 {
                let mut h = UncertaintyHarness::new(budget, &[0, 1], 16);
                let mut exact = ExactUncertainty {
                    fenced: 1,
                    ..ExactUncertainty::default()
                };
                exact.chunks.insert(0, ExactChunk::default());
                exact.chunks.insert(1, ExactChunk::default());
                let mut state = seed;
                for _step in 0..1200 {
                    state = state
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let random = state >> 16;
                    let chosen = (random % 8) as ChunkIdx;
                    match random % 16 {
                        0 => {
                            h.core.restart();
                            exact.restart();
                            h.hello(h.epoch + ((random >> 8) & 1));
                            exact.fenced = h.epoch;
                        }
                        1 => {
                            h.hello(h.epoch + 1);
                            exact.fenced = h.epoch;
                        }
                        2 => {
                            h.core.remove_chunk(1, chosen);
                            exact.chunks.remove(&chosen);
                        }
                        3 | 4 => {
                            let expected = if exact.chunks.contains_key(&chosen) {
                                Err(ChunkError::Exists)
                            } else if exact.chunks.len() == budget as usize {
                                Err(ChunkError::TooManyChunks)
                            } else {
                                Ok(())
                            };
                            let result = if random % 16 == 3 {
                                h.core.add_chunk(1, chosen, h.blocks)
                            } else {
                                h.core.recover_chunk(1, h.epoch, chosen, h.blocks)
                            };
                            assert_eq!(result, expected);
                            if result.is_ok() {
                                exact.chunks.insert(
                                    chosen,
                                    ExactChunk {
                                        records: Vec::new(),
                                        restarted_at: (random % 16 == 4).then_some(h.epoch),
                                    },
                                );
                            }
                        }
                        _ if !exact.chunks.is_empty() => {
                            let chunk = *exact
                                .chunks
                                .keys()
                                .nth((random as usize) % exact.chunks.len())
                                .unwrap();
                            let seq = (random >> 9) % 400;
                            if random % 16 == 5 {
                                h.settle(chunk, seq);
                                exact.settle(
                                    chunk,
                                    Version {
                                        epoch: h.epoch,
                                        seq,
                                    },
                                );
                            } else {
                                let len = ((random >> 20) % 3 + 1) as Block;
                                let start =
                                    ((random >> 30) % u64::from(h.blocks - len + 1)) as Block;
                                let prune = ((random >> 5) & 1 != 0).then_some(Version {
                                    epoch: h.epoch,
                                    seq: (random >> 38) % 400,
                                });
                                let ext = Extent::new(start, len);
                                h.write(chunk, ext, seq, prune);
                                exact.write(
                                    chunk,
                                    ext,
                                    Version {
                                        epoch: h.epoch,
                                        seq,
                                    },
                                    prune,
                                );
                            }
                        }
                        _ => {}
                    }
                    exact.assert_covered(&h);
                }
            }
        }
    }

    #[test]
    fn default_sequential_fill_coarsens_only_the_last_few_chunks() {
        let chunks: Vec<_> = (0..512).collect();
        let mut h = UncertaintyHarness::new(UNCERTAINTY_BUDGET, &chunks, 65_536);
        let mut seq = 0u64;
        for chunk in chunks {
            for offset in 0..128 {
                seq += 1;
                h.write(
                    chunk,
                    Extent::new(offset * 2, 1),
                    seq,
                    Some(Version {
                        epoch: 1,
                        seq: seq.saturating_sub(127),
                    }),
                );
            }
        }
        let rows = h.core.report(1);
        let whole = rows.iter().filter(|(_, xs)| *xs == h.whole()).count();
        assert_eq!(whole, 4);
        assert!(whole <= 8);
        for (_, xs) in rows.iter().filter(|(_, xs)| *xs != h.whole()) {
            assert_eq!(xs.len(), 128);
        }
        assert_eq!(h.core.uncertainty_stats(1).records, 508 * 128);
        assert_arena_bound(&h.core, 1);
    }

    #[test]
    fn fencing_refuses_older_epochs_and_survives_restart() {
        let mut r = replica();
        r.handle(&msg(3, 1, Request::Hello { chunks: vec![] }))
            .unwrap();
        r.restart();
        assert_eq!(
            body(r.handle(&msg(2, 9, Request::Hello { chunks: vec![] }))),
            Some(Response::Stale { fenced: 3 })
        );
        assert_eq!(
            body(r.handle(&msg(2, 1, Request::Heartbeat))),
            Some(Response::Stale { fenced: 3 })
        );
    }

    #[test]
    fn only_the_current_session_is_served() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello { chunks: vec![] }))
            .unwrap();
        r.handle(&msg(1, 2, Request::Hello { chunks: vec![] }))
            .unwrap();
        assert_eq!(r.handle(&msg(1, 1, write(0, &[5], 0, None))), None);
        assert_eq!(r.block(1, 0, 0), Some(0));
        assert_eq!(
            body(r.handle(&msg(1, 2, write(0, &[5], 0, None)))),
            Some(Response::Ok)
        );
        // An older session's late Hello does not take over.
        r.handle(&msg(1, 1, Request::Hello { chunks: vec![] }))
            .unwrap();
        assert_eq!(
            body(r.handle(&msg(1, 2, Request::Heartbeat))),
            Some(Response::Ok)
        );
        // After a restart, nothing is current until a Hello.
        r.restart();
        assert_eq!(r.handle(&msg(1, 2, Request::Heartbeat)), None);
    }

    #[test]
    fn out_of_range_requests_are_dropped() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello { chunks: vec![] }))
            .unwrap();
        assert_eq!(r.handle(&msg(1, 1, write(7, &[1, 2], 0, None))), None);
        let read = Request::Read {
            chunk: 3,
            ext: Extent::new(0, 1),
        };
        assert_eq!(r.handle(&msg(1, 1, read)), None);
    }

    #[test]
    fn uncertain_set_is_pruned_by_completed_writes() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello { chunks: vec![] }))
            .unwrap();
        r.handle(&msg(1, 1, write(0, &[10, 11], 0, None))).unwrap();
        r.handle(&msg(1, 1, write(2, &[12], 1, Some(0)))).unwrap();
        r.handle(&msg(1, 1, write(5, &[13, 14], 2, Some(2))))
            .unwrap();
        assert_eq!(
            uncertain(r.handle(&msg(2, 1, Request::Hello { chunks: vec![] }))),
            vec![Extent::new(5, 2)]
        );
    }

    #[test]
    fn after_a_restart_everything_is_uncertain_until_a_newer_epoch_prunes() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello { chunks: vec![] }))
            .unwrap();
        r.restart();
        assert_eq!(
            uncertain(r.handle(&msg(1, 2, Request::Hello { chunks: vec![] }))),
            vec![Extent::new(0, 8)]
        );
        // Pruning by the same epoch is not enough: its unfinished writes may be anywhere.
        r.handle(&msg(1, 2, write(0, &[1], 5, Some(5)))).unwrap();
        assert_eq!(
            uncertain(r.handle(&msg(2, 1, Request::Hello { chunks: vec![] }))),
            vec![Extent::new(0, 8)]
        );
        let newer = Request::Write {
            chunk: 0,
            ext: Extent::new(3, 1),
            data: vec![9],
            version: Version { epoch: 2, seq: 0 },
            prune: Some(Version { epoch: 2, seq: 1 }),
        };
        r.handle(&msg(2, 1, newer)).unwrap();
        assert_eq!(
            uncertain(r.handle(&msg(3, 1, Request::Hello { chunks: vec![] }))),
            vec![]
        );
    }

    #[test]
    fn copies_skip_blocks_with_live_writes() {
        let mut r = replica();
        r.handle(&msg(1, 1, Request::Hello { chunks: vec![] }))
            .unwrap();
        r.handle(&msg(1, 1, Request::BeginResync { chunk: 0 }))
            .unwrap();
        r.handle(&msg(1, 1, write(1, &[20, 21], 0, None))).unwrap();
        let copy = Request::Copy {
            chunk: 0,
            ext: Extent::new(0, 4),
            data: vec![1, 2, 3, 4],
        };
        r.handle(&msg(1, 1, copy)).unwrap();
        let blocks: Vec<_> = (0..4).map(|b| r.block(1, 0, b).unwrap()).collect();
        assert_eq!(blocks, vec![1, 20, 21, 4]);
    }
}
