//! In-memory state of a disk and the single function that changes it: [`State::apply`].
//!
//! Live operations and journal replay both go through `apply`, so replay reproduces exactly
//! what the live path did.

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use flaminio_format::Uuid;
use flaminio_format::consts::{MAX_CHUNK_SIZE, PAGE_TABLE_ENTRIES};
use flaminio_format::layout::Layout;
use flaminio_format::meta::{ChunkHeader, ChunkState, LayerRef, MapPage, PageTable, Phys};
use flaminio_format::record::Record;

use crate::bitmap::Bitmap;
use crate::error::StoreError;

/// Identifies a chunk replica: which volume, which chunk of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkKey {
    /// Volume UUID.
    pub volume: Uuid,
    /// Chunk index within the volume.
    pub index: u64,
}

impl fmt::Display for ChunkKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.volume, self.index)
    }
}

/// Persistence state of one logical page (a page pair on disk).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PageSlot {
    /// The slot holding the last checkpointed version, if any.
    pub current: Option<u8>,
    /// Changed since the last checkpoint.
    pub dirty: bool,
}

impl PageSlot {
    pub(crate) fn new_dirty() -> Self {
        Self {
            current: None,
            dirty: true,
        }
    }
}

pub(crate) struct MapPageState {
    pub pair: u64,
    pub slot: PageSlot,
    pub page: MapPage,
}

pub(crate) struct PageTableState {
    pub pair: u64,
    pub slot: PageSlot,
    pub map_pages: Vec<Option<MapPageState>>,
}

impl PageTableState {
    pub(crate) fn to_page_table(&self, slot: u32, layer_index: u32) -> PageTable {
        let mut t = PageTable::new(slot, layer_index);
        for (i, mp) in self.map_pages.iter().enumerate() {
            t.map_pages[i] = mp.as_ref().map(|m| m.pair);
        }
        t
    }
}

pub(crate) struct Layer {
    pub snapshot_gen: u64,
    pub table: Option<PageTableState>,
}

pub(crate) struct Chunk {
    pub key: ChunkKey,
    pub state: ChunkState,
    pub chunk_size: u64,
    pub volume_size: u64,
    pub fenced_epoch: u64,
    pub last_write_seq: u64,
    pub layers: Vec<Layer>,
}

/// Where a cluster's visible contents come from, after looking through all layers.
pub(crate) enum Resolved<'a> {
    Zeros,
    Data { phys: u64, csums: &'a [u32] },
}

fn layer_entry(layer: &Layer, epp: usize, x: u64) -> (Phys, Option<&[u32]>) {
    let x = x as usize;
    match layer
        .table
        .as_ref()
        .and_then(|t| t.map_pages[x / epp].as_ref())
    {
        Some(mp) => (mp.page.phys(x % epp), Some(mp.page.csums(x % epp))),
        None => (Phys::Unmapped, None),
    }
}

impl Chunk {
    pub(crate) fn header(&self) -> ChunkHeader {
        ChunkHeader {
            state: self.state,
            volume_uuid: self.key.volume,
            chunk_index: self.key.index,
            chunk_size: self.chunk_size,
            volume_size: self.volume_size,
            fenced_epoch: self.fenced_epoch,
            last_write_seq: self.last_write_seq,
            layers: self
                .layers
                .iter()
                .map(|l| LayerRef {
                    snapshot_gen: l.snapshot_gen,
                    page_table: l.table.as_ref().map(|t| t.pair),
                })
                .collect(),
        }
    }

    pub(crate) fn top_phys(&self, epp: usize, x: u64) -> Phys {
        layer_entry(self.layers.last().expect("at least one layer"), epp, x).0
    }

    pub(crate) fn resolve(&self, epp: usize, x: u64) -> Resolved<'_> {
        for layer in self.layers.iter().rev() {
            match layer_entry(layer, epp, x) {
                (Phys::Unmapped, _) => continue,
                (Phys::Zero, _) => return Resolved::Zeros,
                (Phys::Cluster(phys), csums) => {
                    return Resolved::Data {
                        phys,
                        csums: csums.expect("mapped entry has checksums"),
                    };
                }
            }
        }
        Resolved::Zeros
    }
}

pub(crate) struct State {
    pub layout: Layout,
    /// Map entries per map page.
    pub epp: usize,
    /// Checksums per cluster.
    pub cpc: usize,
    pub chunks: Vec<Option<Chunk>>,
    pub headers: Vec<PageSlot>,
    pub by_key: HashMap<ChunkKey, u32>,
    pub data: Bitmap,
    pub pool: Bitmap,
    pub data_pages: Vec<PageSlot>,
    pub pool_pages: Vec<PageSlot>,
    /// Freed since the last checkpoint. Still marked allocated, and not reusable until the next
    /// checkpoint completes: replaying the journal may still write to them in place.
    pub pending_data: Vec<u64>,
    pub pending_pool: Vec<u64>,
    pub checkpoint_seq: u64,
    pub checkpoint_slot: PageSlot,
}

impl State {
    pub(crate) fn new(layout: Layout) -> Self {
        Self {
            epp: layout.map_entries_per_page(),
            cpc: layout.csums_per_cluster(),
            chunks: (0..layout.chunk_slots).map(|_| None).collect(),
            headers: vec![PageSlot::default(); layout.chunk_slots as usize],
            by_key: HashMap::new(),
            data: Bitmap::new(layout.data_clusters),
            pool: Bitmap::new(layout.pool_pairs),
            data_pages: vec![PageSlot::default(); layout.data_bitmap_pairs as usize],
            pool_pages: vec![PageSlot::default(); layout.pool_bitmap_pairs as usize],
            pending_data: Vec::new(),
            pending_pool: Vec::new(),
            checkpoint_seq: 0,
            checkpoint_slot: PageSlot::default(),
            layout,
        }
    }

    pub(crate) fn cluster_size(&self) -> u64 {
        u64::from(self.layout.cluster_size)
    }

    pub(crate) fn slot_of(&self, key: ChunkKey) -> Result<u32, StoreError> {
        self.by_key
            .get(&key)
            .copied()
            .ok_or(StoreError::NoSuchChunk(key))
    }

    pub(crate) fn chunk(&self, slot: u32) -> &Chunk {
        self.chunks[slot as usize]
            .as_ref()
            .expect("slot holds a chunk")
    }

    pub(crate) fn map_pages_per_chunk(&self, chunk_size: u64) -> usize {
        (chunk_size / self.cluster_size()).div_ceil(self.epp as u64) as usize
    }

    pub(crate) fn validate_chunk_size(&self, chunk_size: u64) -> Result<(), StoreError> {
        let cs = self.cluster_size();
        if chunk_size == 0 || chunk_size % cs != 0 || chunk_size > MAX_CHUNK_SIZE {
            return Err(StoreError::invalid(format!(
                "chunk size {chunk_size} must be a non-zero multiple of {cs} no larger than {MAX_CHUNK_SIZE}"
            )));
        }
        if chunk_size / cs > self.layout.max_clusters_per_chunk() {
            return Err(StoreError::invalid(
                "chunk has too many clusters for one page table",
            ));
        }
        Ok(())
    }

    /// Page-pool pairs that mapping clusters `xs` in the top layer of `slot` would allocate.
    pub(crate) fn pool_needed(&self, slot: u32, xs: impl Iterator<Item = u64>) -> u64 {
        let top = self.chunk(slot).layers.last().expect("at least one layer");
        let pages: BTreeSet<usize> = xs.map(|x| x as usize / self.epp).collect();
        match &top.table {
            None => 1 + pages.len() as u64,
            Some(t) => pages.iter().filter(|&&p| t.map_pages[p].is_none()).count() as u64,
        }
    }

    /// Makes clusters and pages freed since the last checkpoint allocatable again. Called at the
    /// start of a checkpoint, so the bitmaps it writes already show them as free.
    pub(crate) fn release_pending(&mut self) {
        for q in std::mem::take(&mut self.pending_data) {
            self.data.set(q, false);
        }
        for p in std::mem::take(&mut self.pending_pool) {
            self.pool.set(p, false);
        }
    }

    /// Applies one committed record. Returns the in-place data writes it implies (deferred
    /// writes), which the caller must perform.
    pub(crate) fn apply<'r>(
        &mut self,
        rec: &Record<'r>,
    ) -> Result<Vec<(u64, &'r [u8])>, StoreError> {
        let State {
            layout,
            epp,
            cpc,
            chunks,
            headers,
            by_key,
            data,
            pool,
            pending_data,
            pending_pool,
            ..
        } = self;
        let (epp, cpc, cs) = (*epp, *cpc, u64::from(layout.cluster_size));
        let corrupt = |what: &str| {
            StoreError::corrupt(format!("journal record type {}: {what}", rec.record_type()))
        };
        let slot_index = |slot: u32| -> Result<usize, StoreError> {
            if u64::from(slot) < layout.chunk_slots {
                Ok(slot as usize)
            } else {
                Err(corrupt("slot out of range"))
            }
        };
        let mut in_place = Vec::new();

        match rec {
            Record::ChunkCreate {
                slot,
                volume_uuid,
                chunk_index,
                chunk_size,
                volume_size,
                fenced_epoch,
            } => {
                let s = slot_index(*slot)?;
                let key = ChunkKey {
                    volume: *volume_uuid,
                    index: *chunk_index,
                };
                if chunks[s].is_some() || by_key.contains_key(&key) {
                    return Err(corrupt("slot or chunk already in use"));
                }
                if *chunk_size == 0
                    || chunk_size % cs != 0
                    || *chunk_size > MAX_CHUNK_SIZE
                    || chunk_size / cs > layout.max_clusters_per_chunk()
                {
                    return Err(corrupt("invalid chunk size"));
                }
                chunks[s] = Some(Chunk {
                    key,
                    state: ChunkState::Active,
                    chunk_size: *chunk_size,
                    volume_size: *volume_size,
                    fenced_epoch: *fenced_epoch,
                    last_write_seq: 0,
                    layers: vec![Layer {
                        snapshot_gen: 0,
                        table: None,
                    }],
                });
                by_key.insert(key, *slot);
                headers[s].dirty = true;
            }
            Record::ChunkDelete { slot } => {
                let s = slot_index(*slot)?;
                let chunk = chunks[s]
                    .take()
                    .ok_or_else(|| corrupt("no chunk in slot"))?;
                by_key.remove(&chunk.key);
                for table in chunk.layers.into_iter().filter_map(|l| l.table) {
                    for mp in table.map_pages.into_iter().flatten() {
                        for i in 0..mp.page.len() {
                            if let Phys::Cluster(q) = mp.page.phys(i) {
                                pending_data.push(q);
                            }
                        }
                        pending_pool.push(mp.pair);
                    }
                    pending_pool.push(table.pair);
                }
                headers[s].dirty = true;
            }
            Record::Fence { volume_uuid, epoch } => {
                for (s, chunk) in chunks.iter_mut().enumerate() {
                    if let Some(c) = chunk
                        && c.key.volume == *volume_uuid
                        && *epoch > c.fenced_epoch
                    {
                        c.fenced_epoch = *epoch;
                        headers[s].dirty = true;
                    }
                }
            }
            Record::LayerCreate { slot, snapshot_gen } => {
                let s = slot_index(*slot)?;
                let chunk = chunks[s]
                    .as_mut()
                    .ok_or_else(|| corrupt("no chunk in slot"))?;
                if chunk.layers.len() >= layout.max_layers as usize {
                    return Err(corrupt("too many layers"));
                }
                chunk.layers.push(Layer {
                    snapshot_gen: *snapshot_gen,
                    table: None,
                });
                headers[s].dirty = true;
            }
            Record::MapUpdate {
                slot,
                first_cluster,
                count,
                csums_per_cluster,
                phys_first,
                write_seq,
                csums,
            } => {
                let s = slot_index(*slot)?;
                let clusters = chunks[s]
                    .as_ref()
                    .ok_or_else(|| corrupt("no chunk in slot"))?
                    .chunk_size
                    / cs;
                let (first, count) = (u64::from(*first_cluster), u64::from(*count));
                if *csums_per_cluster as usize != cpc || first + count > clusters {
                    return Err(corrupt(
                        "cluster range or checksum count does not fit the chunk",
                    ));
                }
                if phys_first
                    .checked_add(count)
                    .is_none_or(|end| end > layout.data_clusters)
                {
                    return Err(corrupt("physical clusters out of range"));
                }
                for i in 0..count {
                    let p = phys_first + i;
                    data.set(p, true);
                    let (mp, e) =
                        top_entry_mut(chunks, pool, headers, layout, epp, *slot, first + i)?;
                    if let Phys::Cluster(old) = mp.page.phys(e) {
                        pending_data.push(old);
                    }
                    let c = i as usize * cpc;
                    mp.page.set(e, Phys::Cluster(p), &csums[c..c + cpc]);
                }
                bump_seq(
                    chunks[s].as_mut().expect("checked"),
                    *write_seq,
                    &mut headers[s],
                );
            }
            Record::DeferredWrite {
                slot,
                offset,
                write_seq,
                csums,
                data: bytes,
            } => {
                let s = slot_index(*slot)?;
                let chunk = chunks[s]
                    .as_ref()
                    .ok_or_else(|| corrupt("no chunk in slot"))?;
                let (off, len) = (u64::from(*offset), bytes.len() as u64);
                if off + len > chunk.chunk_size || off / cs != (off + len - 1) / cs {
                    return Err(corrupt("write crosses a cluster or the chunk end"));
                }
                let x = off / cs;
                let Phys::Cluster(q) = chunk.top_phys(epp, x) else {
                    return Err(corrupt(
                        "deferred write to a cluster not mapped in the top layer",
                    ));
                };
                let (mp, e) = top_entry_mut(chunks, pool, headers, layout, epp, *slot, x)?;
                let first_block = ((off % cs) / 4096) as usize;
                mp.page.csums_mut(e)[first_block..first_block + csums.len()].copy_from_slice(csums);
                in_place.push((layout.data_cluster_offset(q) + off % cs, *bytes));
                bump_seq(
                    chunks[s].as_mut().expect("checked"),
                    *write_seq,
                    &mut headers[s],
                );
            }
            Record::Discard {
                slot,
                first_cluster,
                count,
            } => {
                let s = slot_index(*slot)?;
                let chunk = chunks[s]
                    .as_ref()
                    .ok_or_else(|| corrupt("no chunk in slot"))?;
                let (first, count) = (u64::from(*first_cluster), u64::from(*count));
                if first + count > chunk.chunk_size / cs {
                    return Err(corrupt("cluster range does not fit the chunk"));
                }
                let new = if chunk.layers.len() > 1 {
                    Phys::Zero
                } else {
                    Phys::Unmapped
                };
                for x in first..first + count {
                    let current = chunks[s].as_ref().expect("checked").top_phys(epp, x);
                    if current == new {
                        continue;
                    }
                    let (mp, e) = top_entry_mut(chunks, pool, headers, layout, epp, *slot, x)?;
                    if let Phys::Cluster(q) = current {
                        pending_data.push(q);
                    }
                    mp.page.set(e, new, &vec![0; cpc]);
                }
            }
            Record::Unknown { .. } => {}
        }
        Ok(in_place)
    }
}

fn bump_seq(chunk: &mut Chunk, write_seq: u64, header: &mut PageSlot) {
    if write_seq > chunk.last_write_seq {
        chunk.last_write_seq = write_seq;
        header.dirty = true;
    }
}

/// The top-layer map entry for cluster `x`, allocating the page table and map page if needed.
/// The returned page is marked dirty.
fn top_entry_mut<'s>(
    chunks: &'s mut [Option<Chunk>],
    pool: &mut Bitmap,
    headers: &mut [PageSlot],
    layout: &Layout,
    epp: usize,
    slot: u32,
    x: u64,
) -> Result<(&'s mut MapPageState, usize), StoreError> {
    let chunk = chunks[slot as usize]
        .as_mut()
        .ok_or_else(|| StoreError::corrupt("no chunk in slot"))?;
    let pages = (chunk.chunk_size / u64::from(layout.cluster_size)).div_ceil(epp as u64) as usize;
    debug_assert!(pages <= PAGE_TABLE_ENTRIES);
    let layer = chunk.layers.last_mut().expect("at least one layer");
    if layer.table.is_none() {
        let (pair, _) = pool
            .alloc_run(1)
            .ok_or(StoreError::OutOfSpace("metadata page pool"))?;
        layer.table = Some(PageTableState {
            pair,
            slot: PageSlot::new_dirty(),
            map_pages: (0..pages).map(|_| None).collect(),
        });
        headers[slot as usize].dirty = true;
    }
    let table = layer.table.as_mut().expect("just ensured");
    let pi = x as usize / epp;
    if table.map_pages[pi].is_none() {
        let (pair, _) = pool
            .alloc_run(1)
            .ok_or(StoreError::OutOfSpace("metadata page pool"))?;
        table.map_pages[pi] = Some(MapPageState {
            pair,
            slot: PageSlot::new_dirty(),
            page: MapPage::new(layout, slot, (pi * epp) as u32),
        });
        table.slot.dirty = true;
    }
    let mp = table.map_pages[pi].as_mut().expect("just ensured");
    mp.slot.dirty = true;
    Ok((mp, x as usize % epp))
}
