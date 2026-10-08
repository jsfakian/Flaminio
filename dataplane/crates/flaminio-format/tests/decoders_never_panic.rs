//! Property tests: every decoder survives arbitrary input, including input with valid CRCs,
//! and every record survives an encode/decode round trip.

use std::borrow::Cow;

use flaminio_format::Uuid;
use flaminio_format::checksum::crc32c;
use flaminio_format::consts::{BLOCK_SIZE, GIB, SUPERBLOCK_MAGIC};
use flaminio_format::journal::{BatchBuilder, BatchHeader};
use flaminio_format::layout::{Layout, LayoutParams};
use flaminio_format::meta::{
    BitmapKind, BitmapPage, CheckpointHeader, ChunkHeader, MapPage, PageTable,
};
use flaminio_format::page::{PageType, new_page, payload_mut, seal};
use flaminio_format::record::{Record, RecordIter};
use flaminio_format::superblock::Superblock;
use proptest::prelude::*;

const NONCE: u64 = 1234;

fn layout() -> Layout {
    Layout::compute(4 * GIB, LayoutParams::default()).unwrap()
}

/// A page of `page_type` with an arbitrary payload but a valid header and CRC.
fn sealed(page_type: PageType, payload: &[u8], offset: u64) -> Vec<u8> {
    let mut p = new_page();
    payload_mut(&mut p).copy_from_slice(payload);
    seal(&mut p, page_type, NONCE, 1, offset);
    p.to_vec()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn superblock_decode_never_panics(mut bytes in prop::collection::vec(any::<u8>(), BLOCK_SIZE)) {
        // Make magic and CRC valid so the field validation itself is exercised.
        bytes[0..8].copy_from_slice(&SUPERBLOCK_MAGIC);
        bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
        let crc = crc32c(&bytes[..BLOCK_SIZE - 4]);
        bytes[BLOCK_SIZE - 4..].copy_from_slice(&crc.to_le_bytes());
        let _ = Superblock::decode(&bytes);
    }

    #[test]
    fn metadata_page_decoders_never_panic(payload in prop::collection::vec(any::<u8>(), 4060)) {
        let l = layout();
        let off = l.pool_pair_offset(0);
        let _ = CheckpointHeader::decode(&sealed(PageType::Checkpoint, &payload, off), NONCE, off, &l);
        let _ = BitmapPage::decode(&sealed(PageType::DataBitmap, &payload, off), BitmapKind::Data, NONCE, off, 0);
        let _ = ChunkHeader::decode(&sealed(PageType::ChunkHeader, &payload, off), NONCE, off, &l);
        let _ = PageTable::decode(&sealed(PageType::PageTable, &payload, off), NONCE, off, &l, 0, 0);
        let _ = MapPage::decode(&sealed(PageType::Map, &payload, off), NONCE, off, &l, 0, 0);
    }

    #[test]
    fn record_decoders_never_panic(
        record_type in 0u16..12,
        flags in any::<u16>(),
        body in prop::collection::vec(any::<u8>(), 0..300),
        count in 0u32..8,
    ) {
        let _ = Record::decode(record_type, flags, &body);
        for r in RecordIter::new(&body, count) {
            let _ = r;
        }
        let _ = BatchHeader::decode(&body, NONCE);
    }

    #[test]
    fn records_roundtrip(
        slot in any::<u32>(),
        first in 0u32..1_000_000,
        count in 1u32..8,
        cpc_shift in 2u32..7,
        phys in 0u64..u64::MAX / 2,
        seq in any::<u64>(),
        blocks in 1usize..4,
        fill in any::<u8>(),
        uuid in any::<[u8; 16]>(),
    ) {
        let cpc = 1u32 << cpc_shift;
        let data = vec![fill; blocks * BLOCK_SIZE];
        let csums: Vec<u32> = (0..count * cpc).collect();
        let records = vec![
            Record::MapUpdate { slot, first_cluster: first, count, csums_per_cluster: cpc, phys_first: phys, write_seq: seq, csums: Cow::Owned(csums) },
            Record::DeferredWrite { slot, offset: first * 4096 % (1 << 30), write_seq: seq, csums: Cow::Owned(vec![7; blocks]), data: &data },
            Record::Fence { volume_uuid: Uuid(uuid), epoch: seq },
            Record::Discard { slot, first_cluster: first, count },
        ];
        let mut b = BatchBuilder::new(1 << 20);
        for r in &records {
            b.push(r).unwrap();
        }
        let n = b.record_count();
        let bytes = b.finish(NONCE, 1, 0);
        let h = BatchHeader::decode(&bytes, NONCE).unwrap();
        let payload = &bytes[64..64 + h.payload_len as usize];
        let back: Vec<_> = RecordIter::new(payload, n).collect::<Result<_, _>>().unwrap();
        prop_assert_eq!(back, records);
    }
}
