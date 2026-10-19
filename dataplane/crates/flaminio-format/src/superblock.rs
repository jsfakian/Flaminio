//! The superblock (spec §3).

use crate::checksum::crc32c;
use crate::codec::{get_16, get_u16, get_u32, get_u64, put_u16, put_u32, put_u64};
use crate::consts::*;
use crate::error::FormatError;
use crate::layout::Layout;
use crate::page::{PageBuf, new_page};
use crate::uuid::Uuid;

const CRC_OFFSET: usize = BLOCK_SIZE - 4;
const LABEL_OFFSET: usize = 240;
/// Maximum label length in bytes.
pub const LABEL_LEN: usize = 64;

/// Identity, feature flags and layout of a formatted disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Superblock {
    /// Features that older readers may safely ignore.
    pub features_compat: u64,
    /// Features that older readers may only read.
    pub features_ro_compat: u64,
    /// Features that older readers must not touch.
    pub features_incompat: u64,
    /// This disk's identity.
    pub disk_uuid: Uuid,
    /// The Flaminio cluster the disk belongs to.
    pub cluster_uuid: Uuid,
    /// Random value that marks every page and batch of this format.
    pub format_nonce: u64,
    /// Format time.
    pub created_unix_secs: u64,
    /// Incremented whenever the superblock is rewritten.
    pub sb_generation: u64,
    /// Optional human-readable label (at most 64 bytes of UTF-8).
    pub label: String,
    /// Where everything lives.
    pub layout: Layout,
}

/// Which superblock copy was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuperblockCopy {
    /// The copy at offset 0.
    A,
    /// The copy in the last block.
    B,
}

impl Superblock {
    /// Encodes the superblock. Both copies are identical.
    ///
    /// # Panics
    ///
    /// If the label is longer than [`LABEL_LEN`] bytes.
    pub fn encode(&self) -> Box<PageBuf> {
        assert!(self.label.len() <= LABEL_LEN, "label too long");
        let l = &self.layout;
        let mut b = new_page();
        b[0..8].copy_from_slice(&SUPERBLOCK_MAGIC);
        put_u16(&mut b[..], 8, VERSION_MAJOR);
        put_u16(&mut b[..], 10, VERSION_MINOR);
        put_u32(&mut b[..], 12, BLOCK_SIZE as u32);
        put_u64(&mut b[..], 16, self.features_compat);
        put_u64(&mut b[..], 24, self.features_ro_compat);
        put_u64(&mut b[..], 32, self.features_incompat);
        b[40..56].copy_from_slice(&self.disk_uuid.0);
        b[56..72].copy_from_slice(&self.cluster_uuid.0);
        let fields: [(usize, u64); 19] = [
            (72, self.format_nonce),
            (80, self.created_unix_secs),
            (88, l.device_size),
            (104, l.journal_offset),
            (112, l.journal_size),
            (120, l.meta_offset),
            (128, l.meta_size),
            (136, l.data_offset),
            (144, l.data_clusters),
            (152, l.checkpoint_offset),
            (160, l.data_bitmap_offset),
            (168, l.data_bitmap_pairs),
            (176, l.pool_bitmap_offset),
            (184, l.pool_bitmap_pairs),
            (192, l.chunk_slots_offset),
            (200, l.chunk_slots),
            (208, l.pool_offset),
            (216, l.pool_pairs),
            (232, self.sb_generation),
        ];
        for (off, v) in fields {
            put_u64(&mut b[..], off, v);
        }
        put_u32(&mut b[..], 96, BLOCK_SIZE as u32);
        put_u32(&mut b[..], 100, l.cluster_size);
        put_u32(&mut b[..], 224, l.max_layers);
        b[LABEL_OFFSET..LABEL_OFFSET + self.label.len()].copy_from_slice(self.label.as_bytes());
        let crc = crc32c(&b[..CRC_OFFSET]);
        put_u32(&mut b[..], CRC_OFFSET, crc);
        b
    }

    /// Decodes and fully validates a superblock copy.
    pub fn decode(b: &[u8]) -> Result<Self, FormatError> {
        if b.len() != BLOCK_SIZE {
            return Err(FormatError::Truncated {
                need: BLOCK_SIZE,
                have: b.len(),
            });
        }
        if b[0..8] != SUPERBLOCK_MAGIC {
            return Err(FormatError::BadMagic);
        }
        if crc32c(&b[..CRC_OFFSET]) != get_u32(b, CRC_OFFSET) {
            return Err(FormatError::Checksum);
        }
        let (major, minor) = (get_u16(b, 8), get_u16(b, 10));
        if major != VERSION_MAJOR {
            return Err(FormatError::UnsupportedVersion { major, minor });
        }
        if get_u32(b, 12) != BLOCK_SIZE as u32 || get_u32(b, 96) != BLOCK_SIZE as u32 {
            return Err(FormatError::invalid("block_size", "must be 4096"));
        }
        let features_incompat = get_u64(b, 32);
        let unknown = features_incompat & !KNOWN_FEATURES_INCOMPAT;
        if unknown != 0 {
            return Err(FormatError::IncompatFeatures(unknown));
        }
        let label_bytes = &b[LABEL_OFFSET..LABEL_OFFSET + LABEL_LEN];
        let label_len = label_bytes
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(LABEL_LEN);
        let label = std::str::from_utf8(&label_bytes[..label_len])
            .map_err(|_| FormatError::invalid("label", "not UTF-8"))?
            .to_owned();
        let layout = Layout {
            device_size: get_u64(b, 88),
            cluster_size: get_u32(b, 100),
            journal_offset: get_u64(b, 104),
            journal_size: get_u64(b, 112),
            meta_offset: get_u64(b, 120),
            meta_size: get_u64(b, 128),
            data_offset: get_u64(b, 136),
            data_clusters: get_u64(b, 144),
            checkpoint_offset: get_u64(b, 152),
            data_bitmap_offset: get_u64(b, 160),
            data_bitmap_pairs: get_u64(b, 168),
            pool_bitmap_offset: get_u64(b, 176),
            pool_bitmap_pairs: get_u64(b, 184),
            chunk_slots_offset: get_u64(b, 192),
            chunk_slots: get_u64(b, 200),
            pool_offset: get_u64(b, 208),
            pool_pairs: get_u64(b, 216),
            max_layers: get_u32(b, 224),
        };
        layout.validate()?;
        let sb = Self {
            features_compat: get_u64(b, 16),
            features_ro_compat: get_u64(b, 24),
            features_incompat,
            disk_uuid: Uuid(get_16(b, 40)),
            cluster_uuid: Uuid(get_16(b, 56)),
            format_nonce: get_u64(b, 72),
            created_unix_secs: get_u64(b, 80),
            sb_generation: get_u64(b, 232),
            label,
            layout,
        };
        if sb.sb_generation == 0 {
            return Err(FormatError::invalid("sb_generation", "must be at least 1"));
        }
        Ok(sb)
    }

    /// True if the disk uses read-only-compatible features this implementation does not know.
    pub fn requires_read_only(&self) -> bool {
        self.features_ro_compat & !KNOWN_FEATURES_RO_COMPAT != 0
    }
}

/// Chooses between the two copies (spec §3): the valid one with the higher generation, A on a
/// tie. Returns copy A's error if neither is valid.
pub fn select_superblock(a: &[u8], b: &[u8]) -> Result<(Superblock, SuperblockCopy), FormatError> {
    match (Superblock::decode(a), Superblock::decode(b)) {
        (Ok(a), Ok(b)) if b.sb_generation > a.sb_generation => Ok((b, SuperblockCopy::B)),
        (Ok(a), _) => Ok((a, SuperblockCopy::A)),
        (Err(_), Ok(b)) => Ok((b, SuperblockCopy::B)),
        (Err(e), Err(_)) => Err(e),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::layout::LayoutParams;

    pub(crate) fn sample() -> Superblock {
        Superblock {
            features_compat: 0,
            features_ro_compat: 0,
            features_incompat: 0,
            disk_uuid: Uuid([1; 16]),
            cluster_uuid: Uuid([2; 16]),
            format_nonce: 0x0123_4567_89AB_CDEF,
            created_unix_secs: 1_790_000_000,
            sb_generation: 1,
            label: "node-a/nvme0".into(),
            layout: Layout::compute(1 << 40, LayoutParams::default()).unwrap(),
        }
    }

    #[test]
    fn roundtrip() {
        let sb = sample();
        assert_eq!(Superblock::decode(&sb.encode()[..]).unwrap(), sb);
    }

    #[test]
    fn rejects_damage_and_unknown_features() {
        let sb = sample();
        let mut torn = sb.encode();
        torn[500] ^= 1;
        assert_eq!(Superblock::decode(&torn[..]), Err(FormatError::Checksum));

        // Bit 0 (pipelined commits) is known; bit 1 is not.
        let newer = Superblock {
            features_incompat: FEATURE_INCOMPAT_PIPELINED_COMMITS | 2,
            ..sample()
        };
        assert_eq!(
            Superblock::decode(&newer.encode()[..]),
            Err(FormatError::IncompatFeatures(2))
        );
        let pipelined = Superblock {
            features_incompat: FEATURE_INCOMPAT_PIPELINED_COMMITS,
            ..sample()
        };
        assert_eq!(Superblock::decode(&pipelined.encode()[..]), Ok(pipelined));

        let ro = Superblock {
            features_ro_compat: 1 << 5,
            ..sample()
        };
        assert!(
            Superblock::decode(&ro.encode()[..])
                .unwrap()
                .requires_read_only()
        );

        let mut bad_layout = sample();
        bad_layout.layout.data_clusters *= 2;
        assert!(matches!(
            Superblock::decode(&bad_layout.encode()[..]),
            Err(FormatError::Invalid { .. })
        ));
    }

    #[test]
    fn rejects_other_major_versions() {
        let mut b = sample().encode();
        put_u16(&mut b[..], 8, 2);
        let crc = crc32c(&b[..CRC_OFFSET]);
        put_u32(&mut b[..], CRC_OFFSET, crc);
        assert!(matches!(
            Superblock::decode(&b[..]),
            Err(FormatError::UnsupportedVersion { major: 2, .. })
        ));
    }

    #[test]
    fn selection() {
        let a = sample();
        let b = Superblock {
            sb_generation: 2,
            ..sample()
        };
        let zero = [0u8; BLOCK_SIZE];
        assert_eq!(
            select_superblock(&a.encode()[..], &a.encode()[..])
                .unwrap()
                .1,
            SuperblockCopy::A
        );
        assert_eq!(
            select_superblock(&a.encode()[..], &b.encode()[..])
                .unwrap()
                .1,
            SuperblockCopy::B
        );
        assert_eq!(
            select_superblock(&zero, &a.encode()[..]).unwrap().1,
            SuperblockCopy::B
        );
        assert_eq!(select_superblock(&zero, &zero), Err(FormatError::BadMagic));
    }
}
