//! Metadata pages and copy-on-write page pairs. See spec §4.

use crate::checksum::crc32c;
use crate::codec::{get_u16, get_u32, get_u64, put_u16, put_u32, put_u64};
use crate::consts::{BLOCK_SIZE, BLOCK_SIZE_U64, PAGE_MAGIC};
use crate::error::FormatError;

/// Length of the common page header.
pub const PAGE_HEADER_LEN: usize = 32;
/// Offset of the page CRC.
pub const PAGE_CRC_OFFSET: usize = BLOCK_SIZE - 4;
/// Bytes of type-specific payload per page.
pub const PAGE_PAYLOAD_LEN: usize = PAGE_CRC_OFFSET - PAGE_HEADER_LEN;
/// Page version written by this implementation.
pub const PAGE_VERSION: u16 = 1;

/// One physical metadata page.
pub type PageBuf = [u8; BLOCK_SIZE];

/// Metadata page types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum PageType {
    /// Checkpoint header (§5.1).
    Checkpoint = 1,
    /// Data-cluster allocation bitmap (§5.2).
    DataBitmap = 2,
    /// Page-pool allocation bitmap (§5.2).
    PoolBitmap = 3,
    /// Chunk header (§5.3).
    ChunkHeader = 4,
    /// Page table (§5.4).
    PageTable = 5,
    /// Map page (§5.5).
    Map = 6,
    /// Volume descriptor (reserved).
    VolumeDescriptor = 7,
}

/// The fields of a page header that matter after validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageHeader {
    /// Page type.
    pub page_type: PageType,
    /// Checkpoint sequence number that wrote the page.
    pub seq: u64,
    /// Page number (byte offset ÷ 4096) the page was written for.
    pub location: u64,
}

/// A zeroed page on the heap.
pub fn new_page() -> Box<PageBuf> {
    Box::new([0u8; BLOCK_SIZE])
}

/// The payload area of a page.
pub fn payload(page: &PageBuf) -> &[u8] {
    &page[PAGE_HEADER_LEN..PAGE_CRC_OFFSET]
}

/// The mutable payload area of a page.
pub fn payload_mut(page: &mut PageBuf) -> &mut [u8] {
    &mut page[PAGE_HEADER_LEN..PAGE_CRC_OFFSET]
}

/// Writes the header and CRC of a page whose payload has already been filled in.
/// `offset` is the page's byte offset on the device.
pub fn seal(page: &mut PageBuf, page_type: PageType, nonce: u64, seq: u64, offset: u64) {
    debug_assert!(
        offset.is_multiple_of(BLOCK_SIZE_U64),
        "pages are block aligned"
    );
    page[0..4].copy_from_slice(&PAGE_MAGIC);
    put_u16(page, 4, page_type as u16);
    put_u16(page, 6, PAGE_VERSION);
    put_u64(page, 8, nonce);
    put_u64(page, 16, seq);
    put_u64(page, 24, offset / BLOCK_SIZE_U64);
    let crc = crc32c(&page[..PAGE_CRC_OFFSET]);
    put_u32(page, PAGE_CRC_OFFSET, crc);
}

/// Validates a page read from byte `offset` and returns its header.
pub fn verify(
    page: &[u8],
    expected: PageType,
    nonce: u64,
    offset: u64,
) -> Result<PageHeader, FormatError> {
    if page.len() != BLOCK_SIZE {
        return Err(FormatError::Truncated {
            need: BLOCK_SIZE,
            have: page.len(),
        });
    }
    if page[0..4] != PAGE_MAGIC {
        return Err(FormatError::BadMagic);
    }
    if crc32c(&page[..PAGE_CRC_OFFSET]) != get_u32(page, PAGE_CRC_OFFSET) {
        return Err(FormatError::Checksum);
    }
    let found = get_u16(page, 4);
    if found != expected as u16 {
        return Err(FormatError::WrongPageType { expected, found });
    }
    let version = get_u16(page, 6);
    if version != PAGE_VERSION {
        return Err(FormatError::UnsupportedVersion {
            major: version,
            minor: 0,
        });
    }
    if get_u64(page, 8) != nonce {
        return Err(FormatError::NonceMismatch);
    }
    let location = get_u64(page, 24);
    if location != offset / BLOCK_SIZE_U64 {
        return Err(FormatError::Misdirected {
            expected: offset / BLOCK_SIZE_U64,
            found: location,
        });
    }
    Ok(PageHeader {
        page_type: expected,
        seq: get_u64(page, 16),
        location,
    })
}

/// Which physical slot of a page pair holds the current version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairChoice {
    /// The current slot (0 or 1) and its sequence number, or `None` if the logical page is absent.
    pub current: Option<(u8, u64)>,
}

impl PairChoice {
    /// The slot the next update must be written to: never the current one.
    pub fn write_slot(&self) -> u8 {
        match self.current {
            Some((slot, _)) => 1 - slot,
            None => 0,
        }
    }

    /// Byte offset of `slot` within the pair starting at `pair_offset`.
    pub fn slot_offset(pair_offset: u64, slot: u8) -> u64 {
        pair_offset + u64::from(slot) * BLOCK_SIZE_U64
    }
}

/// Chooses the current slot of the pair at `pair_offset` (spec §4): the valid slot with the
/// highest `seq` that is not newer than `max_seq`. Slots that fail any check are ignored.
pub fn select_pair(
    slot0: &[u8],
    slot1: &[u8],
    page_type: PageType,
    nonce: u64,
    pair_offset: u64,
    max_seq: u64,
) -> PairChoice {
    let candidate = |slot: u8, buf: &[u8]| {
        verify(
            buf,
            page_type,
            nonce,
            PairChoice::slot_offset(pair_offset, slot),
        )
        .ok()
        .filter(|h| h.seq <= max_seq)
        .map(|h| (slot, h.seq))
    };
    let current = match (candidate(0, slot0), candidate(1, slot1)) {
        (Some(a), Some(b)) => Some(if b.1 > a.1 { b } else { a }),
        (a, b) => a.or(b),
    };
    PairChoice { current }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONCE: u64 = 0xF1A3_1210;
    const PAIR: u64 = 40 * BLOCK_SIZE_U64;

    fn page(seq: u64, slot: u8, fill: u8) -> Box<PageBuf> {
        let mut p = new_page();
        payload_mut(&mut p).fill(fill);
        seal(
            &mut p,
            PageType::Map,
            NONCE,
            seq,
            PairChoice::slot_offset(PAIR, slot),
        );
        p
    }

    #[test]
    fn seal_then_verify() {
        let p = page(7, 0, 0xAB);
        let h = verify(&p[..], PageType::Map, NONCE, PAIR).unwrap();
        assert_eq!(h.seq, 7);
        assert!(payload(&p).iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn verify_rejects_each_kind_of_damage() {
        let p = page(7, 0, 1);
        let mut torn = p.clone();
        torn[100] ^= 0xFF;
        assert_eq!(
            verify(&torn[..], PageType::Map, NONCE, PAIR),
            Err(FormatError::Checksum)
        );
        assert_eq!(
            verify(&p[..], PageType::Map, NONCE + 1, PAIR),
            Err(FormatError::NonceMismatch)
        );
        assert!(matches!(
            verify(&p[..], PageType::Map, NONCE, PAIR + BLOCK_SIZE_U64),
            Err(FormatError::Misdirected { .. })
        ));
        assert!(matches!(
            verify(&p[..], PageType::PageTable, NONCE, PAIR),
            Err(FormatError::WrongPageType { .. })
        ));
        assert_eq!(
            verify(&[0u8; BLOCK_SIZE], PageType::Map, NONCE, PAIR),
            Err(FormatError::BadMagic)
        );
    }

    #[test]
    fn pair_prefers_newest_valid_slot() {
        let a = page(3, 0, 0);
        let b = page(4, 1, 0);
        let c = select_pair(&a[..], &b[..], PageType::Map, NONCE, PAIR, u64::MAX);
        assert_eq!(c.current, Some((1, 4)));
        assert_eq!(c.write_slot(), 0);
    }

    #[test]
    fn pair_survives_torn_write_of_newer_slot() {
        let a = page(3, 0, 0);
        let mut b = page(4, 1, 0);
        b[2000] ^= 1;
        let c = select_pair(&a[..], &b[..], PageType::Map, NONCE, PAIR, u64::MAX);
        assert_eq!(c.current, Some((0, 3)));
        assert_eq!(
            c.write_slot(),
            1,
            "rewrite over the torn slot, never the good one"
        );
    }

    #[test]
    fn pair_ignores_slots_from_an_unfinished_checkpoint() {
        let a = page(3, 0, 0);
        let b = page(5, 1, 0);
        let c = select_pair(&a[..], &b[..], PageType::Map, NONCE, PAIR, 4);
        assert_eq!(c.current, Some((0, 3)));
    }

    #[test]
    fn pair_rejects_slots_swapped_on_disk() {
        // A page written for slot 1 but found in slot 0 is misdirected.
        let wrong = page(9, 1, 0);
        let empty = [0u8; BLOCK_SIZE];
        let c = select_pair(&wrong[..], &empty, PageType::Map, NONCE, PAIR, u64::MAX);
        assert_eq!(c.current, None);
        assert_eq!(c.write_slot(), 0);
    }
}
