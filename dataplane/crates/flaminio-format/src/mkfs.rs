//! Planning a fresh format (spec §2, §5.1). Produces the exact writes; the caller performs them.

use crate::consts::BITS_PER_BITMAP_PAGE;
use crate::error::FormatError;
use crate::layout::{Layout, LayoutParams};
use crate::meta::{BitmapKind, BitmapPage, CheckpointHeader};
use crate::page::PageBuf;
use crate::superblock::{LABEL_LEN, Superblock};
use crate::uuid::Uuid;

/// Everything chosen when formatting a disk. Randomness (UUIDs, nonce) comes from the caller.
#[derive(Debug, Clone)]
pub struct FormatOptions {
    /// New disk identity. Must not be nil.
    pub disk_uuid: Uuid,
    /// Cluster the disk joins.
    pub cluster_uuid: Uuid,
    /// Random, non-zero format nonce.
    pub format_nonce: u64,
    /// Format time.
    pub created_unix_secs: u64,
    /// Layout parameters.
    pub layout: LayoutParams,
    /// Optional label (at most 64 bytes).
    pub label: String,
}

/// The writes that format a disk, in three ordered phases. Each phase must be durable (flushed)
/// before the next starts, so that a crash at any point leaves either the old format, no
/// format, or the new format, never a mix.
pub struct FormatPlan {
    /// The superblock that will be written.
    pub superblock: Superblock,
    /// Phase 1: zero these blocks (the superblock locations), so a crash during phase 2 cannot
    /// leave an old superblock pointing at half-overwritten metadata.
    pub wipe: Vec<u64>,
    /// Phase 2: initial metadata pages (checkpoint 1 and empty bitmaps).
    pub metadata: Vec<(u64, Box<PageBuf>)>,
    /// Phase 3: both superblock copies.
    pub superblocks: Vec<(u64, Box<PageBuf>)>,
}

/// Plans the format of a device of `device_size` bytes.
///
/// The journal, chunk slots and page pool are not written. Anything already there carries a
/// different format nonce and is ignored.
pub fn plan(device_size: u64, opts: &FormatOptions) -> Result<FormatPlan, FormatError> {
    if opts.disk_uuid.is_nil() {
        return Err(FormatError::invalid("disk_uuid", "must not be nil"));
    }
    if opts.format_nonce == 0 {
        return Err(FormatError::invalid(
            "format_nonce",
            "must be random and non-zero",
        ));
    }
    if opts.label.len() > LABEL_LEN {
        return Err(FormatError::invalid(
            "label",
            format!("longer than {LABEL_LEN} bytes"),
        ));
    }
    let layout = Layout::compute(device_size, opts.layout)?;
    let nonce = opts.format_nonce;
    let seq = 1;

    let mut metadata = Vec::new();
    let checkpoint = CheckpointHeader {
        journal_start_offset: 0,
        journal_start_seq: 1,
        data_free_clusters: layout.data_clusters,
        pool_free_pairs: layout.pool_pairs,
        created_unix_secs: opts.created_unix_secs,
    };
    metadata.push((
        layout.checkpoint_offset,
        checkpoint.encode(nonce, seq, layout.checkpoint_offset),
    ));
    for i in 0..layout.data_bitmap_pairs {
        let off = layout.data_bitmap_pair_offset(i);
        let page =
            BitmapPage::new(i * BITS_PER_BITMAP_PAGE).encode(BitmapKind::Data, nonce, seq, off);
        metadata.push((off, page));
    }
    for i in 0..layout.pool_bitmap_pairs {
        let off = layout.pool_bitmap_pair_offset(i);
        let page =
            BitmapPage::new(i * BITS_PER_BITMAP_PAGE).encode(BitmapKind::Pool, nonce, seq, off);
        metadata.push((off, page));
    }

    let superblock = Superblock {
        features_compat: 0,
        features_ro_compat: 0,
        features_incompat: 0,
        disk_uuid: opts.disk_uuid,
        cluster_uuid: opts.cluster_uuid,
        format_nonce: nonce,
        created_unix_secs: opts.created_unix_secs,
        sb_generation: 1,
        label: opts.label.clone(),
        layout,
    };
    let sb_offsets = [0, layout.superblock_b_offset()];
    let encoded = superblock.encode();
    Ok(FormatPlan {
        wipe: sb_offsets.to_vec(),
        metadata,
        superblocks: sb_offsets.iter().map(|&o| (o, encoded.clone())).collect(),
        superblock,
    })
}
