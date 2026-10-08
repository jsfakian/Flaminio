//! Device layout: where every region lives. See spec §2.

use crate::codec::align_up;
use crate::consts::*;
use crate::error::FormatError;
use crate::page::PAGE_PAYLOAD_LEN;

/// Size of a page pair (two physical pages).
pub const PAIR_SIZE: u64 = 2 * BLOCK_SIZE_U64;
/// Bytes at the start of a map page payload before the entries (`slot`, `first_cluster`).
pub const MAP_PAGE_HEADER_LEN: usize = 8;

/// Parameters chosen at format time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutParams {
    /// Data allocation unit: a power of two in [`MIN_CLUSTER_SIZE`, `MAX_CLUSTER_SIZE`].
    pub cluster_size: u32,
    /// Journal size in bytes (a multiple of 1 MiB). `None` picks the default for the device size.
    pub journal_size: Option<u64>,
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            cluster_size: DEFAULT_CLUSTER_SIZE,
            journal_size: None,
        }
    }
}

/// The complete, validated placement of every region on a disk. Stored in the superblock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Device size at format time, rounded down to the block size.
    pub device_size: u64,
    /// Data allocation unit.
    pub cluster_size: u32,
    /// Byte offset of the journal.
    pub journal_offset: u64,
    /// Journal size in bytes.
    pub journal_size: u64,
    /// Byte offset of the metadata area.
    pub meta_offset: u64,
    /// Metadata area size in bytes.
    pub meta_size: u64,
    /// Byte offset of data cluster 0.
    pub data_offset: u64,
    /// Number of data clusters.
    pub data_clusters: u64,
    /// Byte offset of the checkpoint-header pair.
    pub checkpoint_offset: u64,
    /// Byte offset of the first data-bitmap pair.
    pub data_bitmap_offset: u64,
    /// Number of data-bitmap pairs.
    pub data_bitmap_pairs: u64,
    /// Byte offset of the first pool-bitmap pair.
    pub pool_bitmap_offset: u64,
    /// Number of pool-bitmap pairs.
    pub pool_bitmap_pairs: u64,
    /// Byte offset of chunk slot 0.
    pub chunk_slots_offset: u64,
    /// Number of chunk slots.
    pub chunk_slots: u64,
    /// Byte offset of page-pool pair 0.
    pub pool_offset: u64,
    /// Number of page-pool pairs.
    pub pool_pairs: u64,
    /// Maximum layers per chunk replica.
    pub max_layers: u32,
}

/// CRC-32C values stored per cluster (one per 4 KiB block).
pub fn csums_per_cluster(cluster_size: u32) -> usize {
    cluster_size as usize / BLOCK_SIZE
}

/// Size of one map entry for the given cluster size.
pub fn map_entry_size(cluster_size: u32) -> usize {
    8 + 4 * csums_per_cluster(cluster_size)
}

/// Map entries per map page for the given cluster size.
pub fn map_entries_per_page(cluster_size: u32) -> usize {
    (PAGE_PAYLOAD_LEN - MAP_PAGE_HEADER_LEN) / map_entry_size(cluster_size)
}

fn validate_cluster_size(cluster_size: u32) -> Result<(), FormatError> {
    if !cluster_size.is_power_of_two()
        || !(MIN_CLUSTER_SIZE..=MAX_CLUSTER_SIZE).contains(&cluster_size)
    {
        return Err(FormatError::invalid(
            "cluster_size",
            format!(
                "{cluster_size} is not a power of two between {MIN_CLUSTER_SIZE} and {MAX_CLUSTER_SIZE}"
            ),
        ));
    }
    Ok(())
}

struct MetaSizes {
    data_bitmap_pairs: u64,
    pool_bitmap_pairs: u64,
    chunk_slots: u64,
    pool_pairs: u64,
    meta_size: u64,
}

/// Metadata sizing for `n` data clusters (spec §2.1).
fn meta_sizes(n: u64, cluster_size: u32) -> MetaSizes {
    let entries = map_entries_per_page(cluster_size) as u64;
    let chunk_slots = (n * u64::from(cluster_size))
        .div_ceil(MIN_CHUNK_SIZE)
        .saturating_mul(CHUNK_SLOT_OVERCOMMIT)
        .max(MIN_CHUNK_SLOTS);
    let pool_pairs = n.div_ceil(entries) * MAP_SPARSITY_FACTOR + chunk_slots * PAGE_TABLES_PER_SLOT;
    let data_bitmap_pairs = n.div_ceil(BITS_PER_BITMAP_PAGE).max(1);
    let pool_bitmap_pairs = pool_pairs.div_ceil(BITS_PER_BITMAP_PAGE).max(1);
    let pairs = 1 + data_bitmap_pairs + pool_bitmap_pairs + chunk_slots + pool_pairs;
    MetaSizes {
        data_bitmap_pairs,
        pool_bitmap_pairs,
        chunk_slots,
        pool_pairs,
        meta_size: pairs * PAIR_SIZE,
    }
}

impl Layout {
    /// Computes the layout for a device of `device_size` bytes, maximizing data clusters.
    pub fn compute(device_size: u64, params: LayoutParams) -> Result<Layout, FormatError> {
        validate_cluster_size(params.cluster_size)?;
        let dev = device_size & !(BLOCK_SIZE_U64 - 1);
        let too_small = || FormatError::DeviceTooSmall {
            size: device_size,
            min: MIN_DEVICE_SIZE,
        };
        if dev < MIN_DEVICE_SIZE {
            return Err(too_small());
        }
        let journal_size = match params.journal_size {
            Some(j) if j % MIB != 0 || j < MIN_JOURNAL_SIZE => {
                return Err(FormatError::invalid(
                    "journal_size",
                    format!("{j} must be a multiple of 1 MiB and at least {MIN_JOURNAL_SIZE}"),
                ));
            }
            Some(j) => j,
            None => (dev / 512).clamp(DEFAULT_JOURNAL_MIN, DEFAULT_JOURNAL_MAX) / MIB * MIB,
        };

        let cluster = u64::from(params.cluster_size);
        let meta_offset = JOURNAL_OFFSET + journal_size;
        let sb_b = dev - BLOCK_SIZE_U64;
        let data_align = cluster.max(MIB);
        if meta_offset >= sb_b {
            return Err(too_small());
        }

        // For `n` clusters: the metadata sizes, the data offset, and how many clusters would fit.
        let plan = |n: u64| {
            let sizes = meta_sizes(n, params.cluster_size);
            let data_offset = align_up(meta_offset + sizes.meta_size, data_align);
            let fit = sb_b.saturating_sub(data_offset) / cluster;
            (sizes, data_offset, fit)
        };
        // Start from an upper bound and shrink until data + metadata fit. Metadata size is
        // monotonic in the cluster count, so this converges in a few iterations. Shrinking
        // also shrinks the metadata, so then grow back one cluster at a time while it fits.
        let mut n = (sb_b - meta_offset) / cluster;
        loop {
            let (_, _, fit) = plan(n);
            if fit >= n {
                break;
            }
            n = fit;
        }
        while plan(n + 1).2 > n {
            n += 1;
        }
        let (sizes, data_offset, _) = plan(n);
        if n == 0 {
            return Err(too_small());
        }

        let checkpoint_offset = meta_offset;
        let data_bitmap_offset = checkpoint_offset + PAIR_SIZE;
        let pool_bitmap_offset = data_bitmap_offset + sizes.data_bitmap_pairs * PAIR_SIZE;
        let chunk_slots_offset = pool_bitmap_offset + sizes.pool_bitmap_pairs * PAIR_SIZE;
        let pool_offset = chunk_slots_offset + sizes.chunk_slots * PAIR_SIZE;

        let layout = Layout {
            device_size: dev,
            cluster_size: params.cluster_size,
            journal_offset: JOURNAL_OFFSET,
            journal_size,
            meta_offset,
            meta_size: sizes.meta_size,
            data_offset,
            data_clusters: n,
            checkpoint_offset,
            data_bitmap_offset,
            data_bitmap_pairs: sizes.data_bitmap_pairs,
            pool_bitmap_offset,
            pool_bitmap_pairs: sizes.pool_bitmap_pairs,
            chunk_slots_offset,
            chunk_slots: sizes.chunk_slots,
            pool_offset,
            pool_pairs: sizes.pool_pairs,
            max_layers: MAX_LAYERS,
        };
        layout.validate()?;
        Ok(layout)
    }

    /// Checks every invariant of spec §2. Used on layouts read from untrusted superblocks, so all
    /// arithmetic is overflow-checked.
    pub fn validate(&self) -> Result<(), FormatError> {
        validate_cluster_size(self.cluster_size)?;
        let inv = |field: &'static str, reason: &str| FormatError::invalid(field, reason);
        let add =
            |a: u64, b: u64, f: &'static str| a.checked_add(b).ok_or_else(|| inv(f, "overflow"));
        let pairs =
            |n: u64, f: &'static str| n.checked_mul(PAIR_SIZE).ok_or_else(|| inv(f, "overflow"));

        if self.device_size % BLOCK_SIZE_U64 != 0 || self.device_size < MIN_DEVICE_SIZE {
            return Err(inv("device_size", "not block aligned or below minimum"));
        }
        if self.journal_offset != JOURNAL_OFFSET {
            return Err(inv("journal_offset", "must be 1 MiB"));
        }
        if self.journal_size % MIB != 0 || self.journal_size < MIN_JOURNAL_SIZE {
            return Err(inv(
                "journal_size",
                "must be a multiple of 1 MiB and >= 4 MiB",
            ));
        }
        if self.meta_offset != add(self.journal_offset, self.journal_size, "meta_offset")? {
            return Err(inv("meta_offset", "must directly follow the journal"));
        }
        if self.max_layers == 0 || self.max_layers > MAX_LAYERS {
            return Err(inv("max_layers", "out of range"));
        }
        if self.data_clusters == 0 || self.chunk_slots == 0 || self.pool_pairs == 0 {
            return Err(inv("layout", "empty region"));
        }
        if self.checkpoint_offset != self.meta_offset {
            return Err(inv("checkpoint_offset", "must start the metadata area"));
        }
        let expect = [
            (
                "data_bitmap_offset",
                self.data_bitmap_offset,
                add(self.checkpoint_offset, PAIR_SIZE, "data_bitmap_offset")?,
            ),
            (
                "pool_bitmap_offset",
                self.pool_bitmap_offset,
                add(
                    self.data_bitmap_offset,
                    pairs(self.data_bitmap_pairs, "data_bitmap_pairs")?,
                    "pool_bitmap_offset",
                )?,
            ),
            (
                "chunk_slots_offset",
                self.chunk_slots_offset,
                add(
                    self.pool_bitmap_offset,
                    pairs(self.pool_bitmap_pairs, "pool_bitmap_pairs")?,
                    "chunk_slots_offset",
                )?,
            ),
            (
                "pool_offset",
                self.pool_offset,
                add(
                    self.chunk_slots_offset,
                    pairs(self.chunk_slots, "chunk_slots")?,
                    "pool_offset",
                )?,
            ),
        ];
        for (field, actual, expected) in expect {
            if actual != expected {
                return Err(inv(field, "metadata sub-regions must be contiguous"));
            }
        }
        let meta_end = add(
            self.pool_offset,
            pairs(self.pool_pairs, "pool_pairs")?,
            "meta_size",
        )?;
        if add(self.meta_offset, self.meta_size, "meta_size")? != meta_end {
            return Err(inv("meta_size", "does not match the sub-regions"));
        }
        let data_align = u64::from(self.cluster_size).max(MIB);
        if self.data_offset % data_align != 0 || self.data_offset < meta_end {
            return Err(inv("data_offset", "misaligned or overlaps metadata"));
        }
        let data_len = self
            .data_clusters
            .checked_mul(u64::from(self.cluster_size))
            .ok_or_else(|| inv("data_clusters", "overflow"))?;
        if add(self.data_offset, data_len, "data_clusters")? > self.device_size - BLOCK_SIZE_U64 {
            return Err(inv("data_clusters", "data area overlaps superblock B"));
        }
        let bits = |n: u64| n.saturating_mul(BITS_PER_BITMAP_PAGE);
        if bits(self.data_bitmap_pairs) < self.data_clusters {
            return Err(inv(
                "data_bitmap_pairs",
                "too few bits for the data clusters",
            ));
        }
        if bits(self.pool_bitmap_pairs) < self.pool_pairs {
            return Err(inv("pool_bitmap_pairs", "too few bits for the page pool"));
        }
        Ok(())
    }

    /// Byte offset of superblock B.
    pub fn superblock_b_offset(&self) -> u64 {
        self.device_size - BLOCK_SIZE_U64
    }

    /// CRC-32C values per cluster.
    pub fn csums_per_cluster(&self) -> usize {
        csums_per_cluster(self.cluster_size)
    }

    /// Size of one map entry in bytes.
    pub fn map_entry_size_bytes(&self) -> usize {
        map_entry_size(self.cluster_size)
    }

    /// Map entries per map page.
    pub fn map_entries_per_page(&self) -> usize {
        map_entries_per_page(self.cluster_size)
    }

    /// Largest number of clusters one chunk can address through its page table.
    pub fn max_clusters_per_chunk(&self) -> u64 {
        (crate::consts::PAGE_TABLE_ENTRIES * self.map_entries_per_page()) as u64
    }

    /// Byte offset of chunk slot `slot`'s page pair.
    pub fn chunk_slot_offset(&self, slot: u64) -> u64 {
        assert!(slot < self.chunk_slots, "chunk slot out of range");
        self.chunk_slots_offset + slot * PAIR_SIZE
    }

    /// Byte offset of page-pool pair `pair`.
    pub fn pool_pair_offset(&self, pair: u64) -> u64 {
        assert!(pair < self.pool_pairs, "pool pair out of range");
        self.pool_offset + pair * PAIR_SIZE
    }

    /// Byte offset of data-bitmap pair `pair`.
    pub fn data_bitmap_pair_offset(&self, pair: u64) -> u64 {
        assert!(
            pair < self.data_bitmap_pairs,
            "data bitmap pair out of range"
        );
        self.data_bitmap_offset + pair * PAIR_SIZE
    }

    /// Byte offset of pool-bitmap pair `pair`.
    pub fn pool_bitmap_pair_offset(&self, pair: u64) -> u64 {
        assert!(
            pair < self.pool_bitmap_pairs,
            "pool bitmap pair out of range"
        );
        self.pool_bitmap_offset + pair * PAIR_SIZE
    }

    /// Byte offset of data cluster `cluster`.
    pub fn data_cluster_offset(&self, cluster: u64) -> u64 {
        assert!(cluster < self.data_clusters, "data cluster out of range");
        self.data_offset + cluster * u64::from(self.cluster_size)
    }

    /// Fraction of the device used by everything other than data clusters.
    pub fn overhead_ratio(&self) -> f64 {
        let data = self.data_clusters * u64::from(self.cluster_size);
        (self.device_size - data) as f64 / self.device_size as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIB: u64 = 1 << 40;

    fn regions(l: &Layout) -> Vec<(u64, u64)> {
        vec![
            (0, BLOCK_SIZE_U64),
            (l.journal_offset, l.journal_size),
            (l.meta_offset, l.meta_size),
            (l.data_offset, l.data_clusters * u64::from(l.cluster_size)),
            (l.superblock_b_offset(), BLOCK_SIZE_U64),
        ]
    }

    #[test]
    fn layouts_are_valid_and_dense_across_sizes() {
        for size in [MIN_DEVICE_SIZE, GIB, 100 * GIB, TIB, 4 * TIB, 30 * TIB] {
            for cluster in [MIN_CLUSTER_SIZE, DEFAULT_CLUSTER_SIZE, MAX_CLUSTER_SIZE] {
                let l = Layout::compute(
                    size,
                    LayoutParams {
                        cluster_size: cluster,
                        journal_size: None,
                    },
                )
                .unwrap_or_else(|e| panic!("size {size} cluster {cluster}: {e}"));
                l.validate().unwrap();
                let r = regions(&l);
                for w in r.windows(2) {
                    assert!(w[0].0 + w[0].1 <= w[1].0, "overlap in {l:?}");
                }
                // Little slack: no more than one alignment unit plus one cluster wasted.
                let data_end = l.data_offset + l.data_clusters * u64::from(cluster);
                let slack = l.superblock_b_offset() - data_end;
                assert!(
                    slack < u64::from(cluster),
                    "wasted {slack} bytes for size {size}"
                );
                if size >= 100 * GIB {
                    assert!(
                        l.overhead_ratio() < 0.01,
                        "overhead {} at {size}",
                        l.overhead_ratio()
                    );
                }
            }
        }
    }

    #[test]
    fn default_journal_is_clamped() {
        let small = Layout::compute(GIB, LayoutParams::default()).unwrap();
        assert_eq!(small.journal_size, DEFAULT_JOURNAL_MIN);
        let big = Layout::compute(4 * TIB, LayoutParams::default()).unwrap();
        assert_eq!(big.journal_size, DEFAULT_JOURNAL_MAX);
    }

    #[test]
    fn rejects_bad_parameters() {
        assert!(matches!(
            Layout::compute(MIN_DEVICE_SIZE - 1, LayoutParams::default()),
            Err(FormatError::DeviceTooSmall { .. })
        ));
        for cs in [0, 4096, 48 * 1024, 512 * 1024] {
            let p = LayoutParams {
                cluster_size: cs,
                journal_size: None,
            };
            assert!(
                Layout::compute(GIB, p).is_err(),
                "cluster size {cs} accepted"
            );
        }
        let p = LayoutParams {
            cluster_size: DEFAULT_CLUSTER_SIZE,
            journal_size: Some(MIB + 1),
        };
        assert!(Layout::compute(GIB, p).is_err());
    }

    #[test]
    fn validate_catches_tampering() {
        let good = Layout::compute(TIB, LayoutParams::default()).unwrap();
        let mut bad = good;
        bad.data_clusters += 1_000_000;
        assert!(bad.validate().is_err());
        let mut bad = good;
        bad.pool_offset += BLOCK_SIZE_U64;
        assert!(bad.validate().is_err());
        let mut bad = good;
        bad.pool_pairs = u64::MAX;
        assert!(bad.validate().is_err());
        let mut bad = good;
        bad.data_offset += 4096;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn max_chunk_fits_page_table_for_all_cluster_sizes() {
        for cs in [
            MIN_CLUSTER_SIZE,
            32 * 1024,
            DEFAULT_CLUSTER_SIZE,
            128 * 1024,
            MAX_CLUSTER_SIZE,
        ] {
            let l = Layout::compute(
                GIB,
                LayoutParams {
                    cluster_size: cs,
                    journal_size: None,
                },
            )
            .unwrap();
            assert!(
                l.max_clusters_per_chunk() * u64::from(cs) >= MAX_CHUNK_SIZE,
                "cluster {cs}"
            );
        }
    }
}
