//! Typed metadata pages (spec §5). Each type encodes to a sealed [`PageBuf`] and decodes from
//! untrusted bytes, checking every reference against the [`Layout`].

use crate::codec::{get_16, get_u32, get_u64, put_u32, put_u64};
use crate::consts::{BITS_PER_BITMAP_PAGE, MAX_LAYERS, PAGE_TABLE_ENTRIES};
use crate::error::FormatError;
use crate::layout::{Layout, MAP_PAGE_HEADER_LEN};
use crate::page::{
    PAGE_PAYLOAD_LEN, PageBuf, PageType, new_page, payload, payload_mut, seal, verify,
};
use crate::uuid::Uuid;

fn encode_page(
    page_type: PageType,
    nonce: u64,
    seq: u64,
    offset: u64,
    fill: impl FnOnce(&mut [u8]),
) -> Box<PageBuf> {
    let mut page = new_page();
    fill(payload_mut(&mut page));
    seal(&mut page, page_type, nonce, seq, offset);
    page
}

/// Verifies a page and returns its sequence number and payload.
fn decode_page(
    buf: &[u8],
    page_type: PageType,
    nonce: u64,
    offset: u64,
) -> Result<(u64, &[u8]), FormatError> {
    let header = verify(buf, page_type, nonce, offset)?;
    let page: &PageBuf = buf.try_into().expect("verify checked the length");
    Ok((header.seq, payload(page)))
}

// ---------------------------------------------------------------------------------------------
// Checkpoint header (§5.1)

/// Where recovery starts. The page's sequence number is the checkpoint number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointHeader {
    /// Journal offset (relative to the journal region) where replay starts.
    pub journal_start_offset: u64,
    /// Batch sequence number expected at `journal_start_offset`.
    pub journal_start_seq: u64,
    /// Free data clusters at checkpoint time (informational).
    pub data_free_clusters: u64,
    /// Free page-pool pairs at checkpoint time (informational).
    pub pool_free_pairs: u64,
    /// Wall-clock time of the checkpoint.
    pub created_unix_secs: u64,
}

impl CheckpointHeader {
    /// Encodes as checkpoint number `seq` for the slot at byte `offset`.
    pub fn encode(&self, nonce: u64, seq: u64, offset: u64) -> Box<PageBuf> {
        encode_page(PageType::Checkpoint, nonce, seq, offset, |p| {
            put_u64(p, 0, self.journal_start_offset);
            put_u64(p, 8, self.journal_start_seq);
            put_u64(p, 16, self.data_free_clusters);
            put_u64(p, 24, self.pool_free_pairs);
            put_u64(p, 32, self.created_unix_secs);
        })
    }

    /// Decodes and validates; returns the checkpoint number and the header.
    pub fn decode(
        buf: &[u8],
        nonce: u64,
        offset: u64,
        layout: &Layout,
    ) -> Result<(u64, Self), FormatError> {
        let (seq, p) = decode_page(buf, PageType::Checkpoint, nonce, offset)?;
        let h = Self {
            journal_start_offset: get_u64(p, 0),
            journal_start_seq: get_u64(p, 8),
            data_free_clusters: get_u64(p, 16),
            pool_free_pairs: get_u64(p, 24),
            created_unix_secs: get_u64(p, 32),
        };
        if h.journal_start_offset % crate::consts::BLOCK_SIZE_U64 != 0
            || h.journal_start_offset >= layout.journal_size
        {
            return Err(FormatError::invalid(
                "journal_start_offset",
                "misaligned or out of range",
            ));
        }
        if h.journal_start_seq == 0 {
            return Err(FormatError::invalid(
                "journal_start_seq",
                "must be at least 1",
            ));
        }
        Ok((seq, h))
    }
}

// ---------------------------------------------------------------------------------------------
// Bitmap pages (§5.2)

/// Which allocation bitmap a page belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitmapKind {
    /// Data clusters.
    Data,
    /// Page-pool pairs.
    Pool,
}

impl BitmapKind {
    fn page_type(self) -> PageType {
        match self {
            BitmapKind::Data => PageType::DataBitmap,
            BitmapKind::Pool => PageType::PoolBitmap,
        }
    }
}

const BITMAP_BYTES: usize = PAGE_PAYLOAD_LEN - 8;
const _: () = assert!(BITMAP_BYTES as u64 * 8 == BITS_PER_BITMAP_PAGE);

/// One page of an allocation bitmap. Bit `i` describes item `first_index + i`; 1 = allocated.
#[derive(Clone, PartialEq, Eq)]
pub struct BitmapPage {
    /// Index of the item described by bit 0.
    pub first_index: u64,
    bits: Vec<u8>,
}

impl std::fmt::Debug for BitmapPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitmapPage")
            .field("first_index", &self.first_index)
            .field("ones", &self.count_ones())
            .finish()
    }
}

impl BitmapPage {
    /// An all-free bitmap page.
    pub fn new(first_index: u64) -> Self {
        Self {
            first_index,
            bits: vec![0; BITMAP_BYTES],
        }
    }

    /// Whether bit `i` (relative to `first_index`) is set.
    pub fn get(&self, i: u64) -> bool {
        assert!(i < BITS_PER_BITMAP_PAGE, "bit out of range");
        self.bits[(i / 8) as usize] & (1 << (i % 8)) != 0
    }

    /// Sets or clears bit `i` (relative to `first_index`).
    pub fn set(&mut self, i: u64, value: bool) {
        assert!(i < BITS_PER_BITMAP_PAGE, "bit out of range");
        let byte = &mut self.bits[(i / 8) as usize];
        if value {
            *byte |= 1 << (i % 8);
        } else {
            *byte &= !(1 << (i % 8));
        }
    }

    /// Number of set bits.
    pub fn count_ones(&self) -> u64 {
        self.bits.iter().map(|b| u64::from(b.count_ones())).sum()
    }

    /// Encodes for the slot at byte `offset`.
    pub fn encode(&self, kind: BitmapKind, nonce: u64, seq: u64, offset: u64) -> Box<PageBuf> {
        encode_page(kind.page_type(), nonce, seq, offset, |p| {
            put_u64(p, 0, self.first_index);
            p[8..].copy_from_slice(&self.bits);
        })
    }

    /// Decodes and checks that the page covers `expected_first_index`.
    pub fn decode(
        buf: &[u8],
        kind: BitmapKind,
        nonce: u64,
        offset: u64,
        expected_first_index: u64,
    ) -> Result<(u64, Self), FormatError> {
        let (seq, p) = decode_page(buf, kind.page_type(), nonce, offset)?;
        let first_index = get_u64(p, 0);
        if first_index != expected_first_index {
            return Err(FormatError::invalid(
                "first_index",
                "bitmap page covers the wrong range",
            ));
        }
        Ok((
            seq,
            Self {
                first_index,
                bits: p[8..].to_vec(),
            },
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// Chunk header (§5.3)

/// Lifecycle state of a chunk slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ChunkState {
    /// Slot unused.
    Free = 0,
    /// In service.
    Active = 1,
    /// Being filled by a rebuild or migration.
    Rebuilding = 2,
    /// Being deleted.
    Deleting = 3,
}

impl ChunkState {
    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::Free,
            1 => Self::Active,
            2 => Self::Rebuilding,
            3 => Self::Deleting,
            _ => return None,
        })
    }
}

/// One layer of a chunk replica's block map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerRef {
    /// Snapshot generation that created the layer.
    pub snapshot_gen: u64,
    /// Page-pool pair holding the layer's page table, if any cluster has been mapped yet.
    pub page_table: Option<u64>,
}

const LAYERS_OFFSET: usize = 72;
const LAYER_REF_LEN: usize = 16;

/// The root of one chunk replica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkHeader {
    /// Slot state. A free slot carries no other meaningful fields.
    pub state: ChunkState,
    /// Volume the chunk belongs to.
    pub volume_uuid: Uuid,
    /// Index of the chunk within the volume.
    pub chunk_index: u64,
    /// Chunk size in bytes.
    pub chunk_size: u64,
    /// Volume size in bytes (self-description).
    pub volume_size: u64,
    /// Requests with a lower epoch are rejected.
    pub fenced_epoch: u64,
    /// Highest applied target write sequence number.
    pub last_write_seq: u64,
    /// Layers, oldest first. The last one is the writable top layer.
    pub layers: Vec<LayerRef>,
}

impl ChunkHeader {
    /// Checks the header against the layout.
    pub fn validate(&self, layout: &Layout) -> Result<(), FormatError> {
        if self.state == ChunkState::Free {
            return Ok(());
        }
        let cluster = u64::from(layout.cluster_size);
        if self.chunk_size == 0 || self.chunk_size % cluster != 0 {
            return Err(FormatError::invalid(
                "chunk_size",
                "must be a non-zero multiple of the cluster size",
            ));
        }
        if self.chunk_size / cluster > layout.max_clusters_per_chunk() {
            return Err(FormatError::invalid(
                "chunk_size",
                "too many clusters for one page table",
            ));
        }
        if self.layers.is_empty() || self.layers.len() > layout.max_layers as usize {
            return Err(FormatError::invalid("layer_count", "out of range"));
        }
        if self
            .layers
            .iter()
            .any(|l| l.page_table.is_some_and(|p| p >= layout.pool_pairs))
        {
            return Err(FormatError::invalid(
                "page_table_ref",
                "points outside the page pool",
            ));
        }
        Ok(())
    }

    /// Encodes for the slot at byte `offset`.
    ///
    /// # Panics
    ///
    /// If there are more than [`MAX_LAYERS`] layers.
    pub fn encode(&self, nonce: u64, seq: u64, offset: u64) -> Box<PageBuf> {
        assert!(self.layers.len() <= MAX_LAYERS as usize, "too many layers");
        encode_page(PageType::ChunkHeader, nonce, seq, offset, |p| {
            p[0] = self.state as u8;
            p[8..24].copy_from_slice(&self.volume_uuid.0);
            put_u64(p, 24, self.chunk_index);
            put_u64(p, 32, self.chunk_size);
            put_u64(p, 40, self.volume_size);
            put_u64(p, 48, self.fenced_epoch);
            put_u64(p, 56, self.last_write_seq);
            put_u32(p, 64, self.layers.len() as u32);
            for (i, l) in self.layers.iter().enumerate() {
                let o = LAYERS_OFFSET + i * LAYER_REF_LEN;
                put_u64(p, o, l.snapshot_gen);
                put_u64(p, o + 8, l.page_table.map_or(0, |r| r + 1));
            }
        })
    }

    /// Decodes and validates against the layout.
    pub fn decode(
        buf: &[u8],
        nonce: u64,
        offset: u64,
        layout: &Layout,
    ) -> Result<(u64, Self), FormatError> {
        let (seq, p) = decode_page(buf, PageType::ChunkHeader, nonce, offset)?;
        let state = ChunkState::from_u8(p[0])
            .ok_or_else(|| FormatError::invalid("state", format!("unknown value {}", p[0])))?;
        let layer_count = get_u32(p, 64);
        if layer_count > MAX_LAYERS {
            return Err(FormatError::invalid("layer_count", "out of range"));
        }
        let layers = (0..layer_count as usize)
            .map(|i| {
                let o = LAYERS_OFFSET + i * LAYER_REF_LEN;
                LayerRef {
                    snapshot_gen: get_u64(p, o),
                    page_table: get_u64(p, o + 8).checked_sub(1),
                }
            })
            .collect();
        let h = Self {
            state,
            volume_uuid: Uuid(get_16(p, 8)),
            chunk_index: get_u64(p, 24),
            chunk_size: get_u64(p, 32),
            volume_size: get_u64(p, 40),
            fenced_epoch: get_u64(p, 48),
            last_write_seq: get_u64(p, 56),
            layers,
        };
        h.validate(layout)?;
        Ok((seq, h))
    }
}

// ---------------------------------------------------------------------------------------------
// Page table (§5.4)

/// Maps map-page index → page-pool pair for one (slot, layer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageTable {
    /// Owning chunk slot.
    pub slot: u32,
    /// Owning layer index within the slot.
    pub layer_index: u32,
    /// `map_pages[i]` covers clusters `i × E .. (i + 1) × E`. Always [`PAGE_TABLE_ENTRIES`] long.
    pub map_pages: Vec<Option<u64>>,
}

impl PageTable {
    /// An empty page table.
    pub fn new(slot: u32, layer_index: u32) -> Self {
        Self {
            slot,
            layer_index,
            map_pages: vec![None; PAGE_TABLE_ENTRIES],
        }
    }

    /// Encodes for the slot at byte `offset`.
    pub fn encode(&self, nonce: u64, seq: u64, offset: u64) -> Box<PageBuf> {
        assert_eq!(
            self.map_pages.len(),
            PAGE_TABLE_ENTRIES,
            "page table length"
        );
        encode_page(PageType::PageTable, nonce, seq, offset, |p| {
            put_u32(p, 0, self.slot);
            put_u32(p, 4, self.layer_index);
            for (i, r) in self.map_pages.iter().enumerate() {
                put_u64(p, 8 + i * 8, r.map_or(0, |r| r + 1));
            }
        })
    }

    /// Decodes, checking ownership and that every reference is inside the page pool.
    pub fn decode(
        buf: &[u8],
        nonce: u64,
        offset: u64,
        layout: &Layout,
        expected_slot: u32,
        expected_layer: u32,
    ) -> Result<(u64, Self), FormatError> {
        let (seq, p) = decode_page(buf, PageType::PageTable, nonce, offset)?;
        let (slot, layer_index) = (get_u32(p, 0), get_u32(p, 4));
        if slot != expected_slot || layer_index != expected_layer {
            return Err(FormatError::invalid(
                "owner",
                "page table belongs to another chunk or layer",
            ));
        }
        let map_pages = (0..PAGE_TABLE_ENTRIES)
            .map(|i| match get_u64(p, 8 + i * 8).checked_sub(1) {
                Some(r) if r >= layout.pool_pairs => Err(FormatError::invalid(
                    "map_page_ref",
                    "points outside the page pool",
                )),
                r => Ok(r),
            })
            .collect::<Result<_, _>>()?;
        Ok((
            seq,
            Self {
                slot,
                layer_index,
                map_pages,
            },
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// Map page (§5.5)

/// Where a cluster's data lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phys {
    /// Not mapped in this layer: read through to the layer below (zeros at the bottom).
    Unmapped,
    /// Explicitly zero (discarded); does not read through.
    Zero,
    /// Stored in this data cluster.
    Cluster(u64),
}

impl Phys {
    fn to_raw(self) -> u64 {
        match self {
            Phys::Unmapped => 0,
            Phys::Zero => u64::MAX,
            Phys::Cluster(c) => {
                assert!(c < u64::MAX - 1, "cluster index out of range");
                c + 1
            }
        }
    }

    fn from_raw(raw: u64, data_clusters: u64) -> Result<Self, FormatError> {
        match raw {
            0 => Ok(Phys::Unmapped),
            u64::MAX => Ok(Phys::Zero),
            r if r - 1 < data_clusters => Ok(Phys::Cluster(r - 1)),
            _ => Err(FormatError::invalid("phys", "data cluster out of range")),
        }
    }
}

/// A run of `E` consecutive map entries of one chunk layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapPage {
    /// Owning chunk slot.
    pub slot: u32,
    /// Chunk-relative cluster index of entry 0.
    pub first_cluster: u32,
    csums_per_cluster: usize,
    phys: Vec<Phys>,
    csums: Vec<u32>,
}

impl MapPage {
    /// A map page with every entry unmapped.
    pub fn new(layout: &Layout, slot: u32, first_cluster: u32) -> Self {
        let n = layout.map_entries_per_page();
        let cpc = layout.csums_per_cluster();
        Self {
            slot,
            first_cluster,
            csums_per_cluster: cpc,
            phys: vec![Phys::Unmapped; n],
            csums: vec![0; n * cpc],
        }
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.phys.len()
    }

    /// Always false: a map page has a fixed, non-zero number of entries.
    pub fn is_empty(&self) -> bool {
        self.phys.is_empty()
    }

    /// Location of entry `i`.
    pub fn phys(&self, i: usize) -> Phys {
        self.phys[i]
    }

    /// Per-4 KiB checksums of entry `i`.
    pub fn csums(&self, i: usize) -> &[u32] {
        &self.csums[i * self.csums_per_cluster..(i + 1) * self.csums_per_cluster]
    }

    /// Sets entry `i`.
    pub fn set(&mut self, i: usize, phys: Phys, csums: &[u32]) {
        assert_eq!(csums.len(), self.csums_per_cluster, "checksum count");
        self.phys[i] = phys;
        self.csums[i * self.csums_per_cluster..(i + 1) * self.csums_per_cluster]
            .copy_from_slice(csums);
    }

    /// Encodes for the slot at byte `offset`.
    pub fn encode(&self, nonce: u64, seq: u64, offset: u64) -> Box<PageBuf> {
        let entry = 8 + 4 * self.csums_per_cluster;
        encode_page(PageType::Map, nonce, seq, offset, |p| {
            put_u32(p, 0, self.slot);
            put_u32(p, 4, self.first_cluster);
            for (i, phys) in self.phys.iter().enumerate() {
                let o = MAP_PAGE_HEADER_LEN + i * entry;
                put_u64(p, o, phys.to_raw());
                for (j, c) in self.csums(i).iter().enumerate() {
                    put_u32(p, o + 8 + j * 4, *c);
                }
            }
        })
    }

    /// Decodes, checking ownership and that every cluster reference is inside the data area.
    pub fn decode(
        buf: &[u8],
        nonce: u64,
        offset: u64,
        layout: &Layout,
        expected_slot: u32,
        expected_first_cluster: u32,
    ) -> Result<(u64, Self), FormatError> {
        let (seq, p) = decode_page(buf, PageType::Map, nonce, offset)?;
        let (slot, first_cluster) = (get_u32(p, 0), get_u32(p, 4));
        if slot != expected_slot || first_cluster != expected_first_cluster {
            return Err(FormatError::invalid(
                "owner",
                "map page belongs to another chunk or range",
            ));
        }
        let mut page = Self::new(layout, slot, first_cluster);
        let entry = layout.map_entry_size_bytes();
        for i in 0..page.len() {
            let o = MAP_PAGE_HEADER_LEN + i * entry;
            page.phys[i] = Phys::from_raw(get_u64(p, o), layout.data_clusters)?;
            for j in 0..page.csums_per_cluster {
                page.csums[i * page.csums_per_cluster + j] = get_u32(p, o + 8 + j * 4);
            }
        }
        Ok((seq, page))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::GIB;
    use crate::layout::LayoutParams;

    const NONCE: u64 = 42;

    fn layout() -> Layout {
        Layout::compute(4 * GIB, LayoutParams::default()).unwrap()
    }

    #[test]
    fn checkpoint_roundtrip_and_validation() {
        let l = layout();
        let h = CheckpointHeader {
            journal_start_offset: 8192,
            journal_start_seq: 17,
            data_free_clusters: 5,
            pool_free_pairs: 6,
            created_unix_secs: 7,
        };
        let off = l.checkpoint_offset;
        assert_eq!(
            CheckpointHeader::decode(&h.encode(NONCE, 3, off)[..], NONCE, off, &l).unwrap(),
            (3, h)
        );
        let bad = CheckpointHeader {
            journal_start_offset: l.journal_size,
            ..h
        };
        assert!(CheckpointHeader::decode(&bad.encode(NONCE, 3, off)[..], NONCE, off, &l).is_err());
    }

    #[test]
    fn bitmap_roundtrip() {
        let mut b = BitmapPage::new(BITS_PER_BITMAP_PAGE);
        for i in [0, 7, 8, 1000, BITS_PER_BITMAP_PAGE - 1] {
            b.set(i, true);
        }
        b.set(1000, false);
        assert_eq!(b.count_ones(), 4);
        let off = 1 << 30;
        let page = b.encode(BitmapKind::Data, NONCE, 1, off);
        let (_, back) = BitmapPage::decode(
            &page[..],
            BitmapKind::Data,
            NONCE,
            off,
            BITS_PER_BITMAP_PAGE,
        )
        .unwrap();
        assert_eq!(back, b);
        assert!(back.get(7) && back.get(BITS_PER_BITMAP_PAGE - 1) && !back.get(1000));
        assert!(
            BitmapPage::decode(
                &page[..],
                BitmapKind::Pool,
                NONCE,
                off,
                BITS_PER_BITMAP_PAGE
            )
            .is_err()
        );
        assert!(BitmapPage::decode(&page[..], BitmapKind::Data, NONCE, off, 0).is_err());
    }

    fn chunk() -> ChunkHeader {
        ChunkHeader {
            state: ChunkState::Active,
            volume_uuid: Uuid([9; 16]),
            chunk_index: 12,
            chunk_size: 256 << 20,
            volume_size: 10 * GIB,
            fenced_epoch: 4,
            last_write_seq: 99,
            layers: vec![
                LayerRef {
                    snapshot_gen: 0,
                    page_table: Some(3),
                },
                LayerRef {
                    snapshot_gen: 1,
                    page_table: None,
                },
            ],
        }
    }

    #[test]
    fn chunk_header_roundtrip() {
        let l = layout();
        let off = l.chunk_slot_offset(5);
        let c = chunk();
        assert_eq!(
            ChunkHeader::decode(&c.encode(NONCE, 2, off)[..], NONCE, off, &l).unwrap(),
            (2, c)
        );
    }

    #[test]
    fn chunk_header_rejects_bad_references() {
        let l = layout();
        let off = l.chunk_slot_offset(0);
        let cases = [
            ChunkHeader {
                chunk_size: 100,
                ..chunk()
            },
            ChunkHeader {
                chunk_size: 64 << 30,
                ..chunk()
            },
            ChunkHeader {
                layers: vec![],
                ..chunk()
            },
            ChunkHeader {
                layers: vec![LayerRef {
                    snapshot_gen: 0,
                    page_table: Some(l.pool_pairs),
                }],
                ..chunk()
            },
        ];
        for c in cases {
            assert!(
                ChunkHeader::decode(&c.encode(NONCE, 1, off)[..], NONCE, off, &l).is_err(),
                "{c:?}"
            );
        }
    }

    #[test]
    fn page_table_roundtrip_and_ownership() {
        let l = layout();
        let off = l.pool_pair_offset(1);
        let mut t = PageTable::new(5, 1);
        t.map_pages[0] = Some(0);
        t.map_pages[505] = Some(l.pool_pairs - 1);
        let page = t.encode(NONCE, 4, off);
        assert_eq!(
            PageTable::decode(&page[..], NONCE, off, &l, 5, 1).unwrap(),
            (4, t.clone())
        );
        assert!(PageTable::decode(&page[..], NONCE, off, &l, 6, 1).is_err());
        t.map_pages[1] = Some(l.pool_pairs);
        assert!(PageTable::decode(&t.encode(NONCE, 4, off)[..], NONCE, off, &l, 5, 1).is_err());
    }

    #[test]
    fn map_page_roundtrip_all_cluster_sizes() {
        for cs in [16 * 1024, 64 * 1024, 256 * 1024] {
            let l = Layout::compute(
                4 * GIB,
                LayoutParams {
                    cluster_size: cs,
                    journal_size: None,
                },
            )
            .unwrap();
            let off = l.pool_pair_offset(0);
            let first = l.map_entries_per_page() as u32;
            let mut m = MapPage::new(&l, 3, first);
            let cpc = l.csums_per_cluster();
            let sums: Vec<u32> = (0..cpc as u32).collect();
            m.set(0, Phys::Cluster(0), &sums);
            m.set(1, Phys::Zero, &vec![0; cpc]);
            m.set(m.len() - 1, Phys::Cluster(l.data_clusters - 1), &sums);
            let page = m.encode(NONCE, 9, off);
            let (seq, back) = MapPage::decode(&page[..], NONCE, off, &l, 3, first).unwrap();
            assert_eq!((seq, &back), (9, &m));
            assert_eq!(back.phys(2), Phys::Unmapped);
            assert_eq!(back.csums(0), &sums[..]);
        }
    }

    #[test]
    fn map_page_rejects_cluster_outside_data_area() {
        let l = layout();
        let off = l.pool_pair_offset(0);
        let mut m = MapPage::new(&l, 0, 0);
        m.set(
            0,
            Phys::Cluster(l.data_clusters),
            &vec![0; l.csums_per_cluster()],
        );
        assert!(MapPage::decode(&m.encode(NONCE, 1, off)[..], NONCE, off, &l, 0, 0).is_err());
    }
}
