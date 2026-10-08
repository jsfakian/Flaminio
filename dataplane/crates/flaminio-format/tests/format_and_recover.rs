//! Formats sparse in-memory devices and reads them back the way recovery will (spec §7).

mod common;

use common::SparseDevice;
use flaminio_format::consts::{BITS_PER_BITMAP_PAGE, GIB, MIB};
use flaminio_format::journal::{BatchBuilder, JournalPosition, JournalScanner};
use flaminio_format::layout::LayoutParams;
use flaminio_format::meta::{
    BitmapKind, BitmapPage, CheckpointHeader, ChunkHeader, ChunkState, LayerRef,
};
use flaminio_format::mkfs::{FormatOptions, plan};
use flaminio_format::page::{PageType, PairChoice, select_pair};
use flaminio_format::record::Record;
use flaminio_format::superblock::{SuperblockCopy, select_superblock};
use flaminio_format::{FormatError, Uuid};

const TIB: u64 = 1 << 40;

fn options(nonce: u64) -> FormatOptions {
    FormatOptions {
        disk_uuid: Uuid([0xD1; 16]),
        cluster_uuid: Uuid([0xC1; 16]),
        format_nonce: nonce,
        created_unix_secs: 1_790_000_000,
        layout: LayoutParams::default(),
        label: "test".into(),
    }
}

fn superblock_of(
    dev: &SparseDevice,
) -> Result<(flaminio_format::superblock::Superblock, SuperblockCopy), FormatError> {
    let a = dev.read_block(0);
    let b = dev.read_block((dev.size & !4095) - 4096);
    select_superblock(&a, &b)
}

fn pair_choice(dev: &SparseDevice, offset: u64, page_type: PageType, nonce: u64) -> PairChoice {
    select_pair(
        &dev.read_block(offset),
        &dev.read_block(offset + 4096),
        page_type,
        nonce,
        offset,
        u64::MAX,
    )
}

#[test]
fn format_4tib_device_and_recover_initial_state() {
    let mut dev = SparseDevice::new(4 * TIB);
    let p = plan(dev.size, &options(77)).unwrap();
    dev.apply(&p, 3);

    let (sb, copy) = superblock_of(&dev).unwrap();
    assert_eq!(copy, SuperblockCopy::A);
    assert_eq!(sb, p.superblock);
    let l = sb.layout;
    assert!(l.overhead_ratio() < 0.01);

    let ck = pair_choice(&dev, l.checkpoint_offset, PageType::Checkpoint, 77);
    assert_eq!(ck.current, Some((0, 1)));
    let (seq, header) = CheckpointHeader::decode(
        &dev.read_block(l.checkpoint_offset),
        77,
        l.checkpoint_offset,
        &l,
    )
    .unwrap();
    assert_eq!(seq, 1);
    assert_eq!(
        (header.journal_start_offset, header.journal_start_seq),
        (0, 1)
    );
    assert_eq!(header.data_free_clusters, l.data_clusters);

    for i in 0..l.data_bitmap_pairs {
        let off = l.data_bitmap_pair_offset(i);
        let (_, bm) = BitmapPage::decode(
            &dev.read_block(off),
            BitmapKind::Data,
            77,
            off,
            i * BITS_PER_BITMAP_PAGE,
        )
        .unwrap();
        assert_eq!(bm.count_ones(), 0);
    }

    // Nothing in the chunk slots or journal yet.
    assert_eq!(
        pair_choice(&dev, l.chunk_slot_offset(0), PageType::ChunkHeader, 77).current,
        None
    );
    let mut scan = JournalScanner::new(&dev, &l, 77, JournalPosition { offset: 0, seq: 1 });
    assert!(scan.next().is_none());
    assert_eq!(scan.position(), JournalPosition { offset: 0, seq: 1 });
}

#[test]
fn superblock_b_takes_over_when_a_is_destroyed() {
    let mut dev = SparseDevice::new(100 * GIB);
    dev.apply(&plan(dev.size, &options(5)).unwrap(), 3);
    dev.zero(0);
    let (sb, copy) = superblock_of(&dev).unwrap();
    assert_eq!(copy, SuperblockCopy::B);
    assert_eq!(sb.format_nonce, 5);
}

#[test]
fn crash_during_format_leaves_no_superblock() {
    let mut dev = SparseDevice::new(100 * GIB);
    dev.apply(&plan(dev.size, &options(5)).unwrap(), 3);
    // Reformat, but crash after the metadata phase and before the superblocks are written.
    dev.apply(&plan(dev.size, &options(6)).unwrap(), 2);
    assert!(
        superblock_of(&dev).is_err(),
        "the old superblock must not survive a partial reformat"
    );
}

#[test]
fn reformat_hides_everything_from_the_previous_format() {
    let mut dev = SparseDevice::new(100 * GIB);
    dev.apply(&plan(dev.size, &options(5)).unwrap(), 3);
    let l = superblock_of(&dev).unwrap().0.layout;

    // Old format: one chunk header and one journal batch.
    let chunk = ChunkHeader {
        state: ChunkState::Active,
        volume_uuid: Uuid([3; 16]),
        chunk_index: 0,
        chunk_size: 256 * MIB,
        volume_size: GIB,
        fenced_epoch: 1,
        last_write_seq: 0,
        layers: vec![LayerRef {
            snapshot_gen: 0,
            page_table: None,
        }],
    };
    let slot0 = l.chunk_slot_offset(0);
    dev.write(slot0, &chunk.encode(5, 2, slot0)[..]);
    let mut b = BatchBuilder::new(4096);
    b.push(&Record::ChunkDelete { slot: 0 }).unwrap();
    dev.write(l.journal_offset, &b.finish(5, 1, 0));
    assert_eq!(
        pair_choice(&dev, slot0, PageType::ChunkHeader, 5).current,
        Some((0, 2))
    );

    // New format with a new nonce: none of it is visible, although none of it was erased.
    dev.apply(&plan(dev.size, &options(6)).unwrap(), 3);
    assert_eq!(
        pair_choice(&dev, slot0, PageType::ChunkHeader, 6).current,
        None
    );
    let scan = JournalScanner::new(&dev, &l, 6, JournalPosition { offset: 0, seq: 1 });
    assert_eq!(scan.count(), 0);
}

#[test]
fn plan_rejects_unsafe_options() {
    let size = 100 * GIB;
    assert!(
        plan(
            size,
            &FormatOptions {
                format_nonce: 0,
                ..options(1)
            }
        )
        .is_err()
    );
    assert!(
        plan(
            size,
            &FormatOptions {
                disk_uuid: Uuid::NIL,
                ..options(1)
            }
        )
        .is_err()
    );
    assert!(
        plan(
            size,
            &FormatOptions {
                label: "x".repeat(65),
                ..options(1)
            }
        )
        .is_err()
    );
    assert!(plan(MIB, &options(1)).is_err());
}
