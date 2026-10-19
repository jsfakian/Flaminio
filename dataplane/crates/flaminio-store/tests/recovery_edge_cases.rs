//! Deterministic recovery scenarios that random crash testing reaches only rarely.

mod common;

use common::*;
use flaminio_format::consts::MIB;
use flaminio_store::{Allocation, MemDevice, Store, StoreConfig, StoreError, WriteReq};

const CLUSTER: u64 = 64 * 1024;

fn open(dev: &MemDevice, pipelined: bool) -> Store<&MemDevice> {
    Store::open(
        dev,
        StoreConfig {
            allocation: Allocation::FirstFit,
            pipelined_commits: pipelined,
            ..StoreConfig::default()
        },
    )
    .unwrap()
}

/// A batch whose journal block reached the disk but whose data did not is rejected by
/// recovery. It must not be found again: otherwise, when the next batch has to wrap to offset
/// 0, the next recovery finds the stale batch first, rejects it again, and loses the new,
/// acknowledged batch. Without pipelined commits its header is erased; with them, the next
/// batch's sequence number skips past it.
#[test]
fn rejected_batch_cannot_shadow_a_wrapped_successor() {
    for pipelined in [false, true] {
        rejected_batch_scenario(pipelined);
    }
}

fn rejected_batch_scenario(pipelined: bool) {
    let dev = MemDevice::new(DEVICE);
    Store::format(&dev, &options(4)).unwrap();
    let journal = 4 * MIB;
    let layout;
    {
        let mut s = open(&dev, pipelined);
        layout = s.superblock().layout;
        s.create_chunk(key(0), CHUNK, CHUNK, 1).unwrap();
        s.write(key(0), 0, &blocks(1, 0, 1024), 1, 1).unwrap();
        // Small deferred writes (one 8 KiB batch each) until the head is near the end.
        let mut seq = 2;
        while !(journal - 48 * 1024..journal - 16 * 1024).contains(&s.stats().journal_head) {
            s.write(
                key(0),
                (seq % 1024) * 4096,
                &blocks(seq as u32, seq % 1024, 1),
                seq,
                1,
            )
            .unwrap();
            seq += 1;
        }
        // A redirect-on-write of one whole cluster: data and batch written, then power loss
        // before the flush, keeping only the journal block.
        dev.fail_after(Some(2));
        let err = s
            .write(key(0), 512 * 4096, &blocks(999, 512, 16), seq, 1)
            .unwrap_err();
        assert!(
            matches!(err, StoreError::Io(_)),
            "expected the simulated power loss, got {err:?}"
        );
    }
    let journal_range = layout.journal_offset..layout.journal_offset + journal;
    let img = dev.crash_keeping(|o| journal_range.contains(&o));

    // Recovery rejects the batch.
    let before;
    {
        let mut s = open(&img, pipelined);
        let mut b = vec![0u8; CLUSTER as usize];
        s.read(key(0), 512 * 4096, &mut b, 1).unwrap();
        assert_eq!(
            b,
            blocks(1, 512, 16),
            "the unacknowledged write must not appear"
        );
        before = s.stats().journal_head;
        // The next batch reuses that sequence number and is too big for the space left, so
        // it goes to offset 0. Two 60 KiB deferred writes make a ~124 KiB batch.
        let (x, y) = (blocks(7, 33, 15), blocks(8, 65, 15));
        let reqs = [
            WriteReq {
                key: key(0),
                offset: 33 * 4096,
                data: (&x).into(),
                write_seq: 10_000,
                epoch: 1,
            },
            WriteReq {
                key: key(0),
                offset: 65 * 4096,
                data: (&y).into(),
                write_seq: 10_001,
                epoch: 1,
            },
        ];
        assert!(s.write_batch(&reqs).unwrap().iter().all(Result::is_ok));
        assert!(
            s.stats().journal_head < before,
            "the batch must have wrapped to offset 0"
        );
    }

    // Recover again: the wrapped batch is acknowledged and must be there.
    let s = open(&img, pipelined);
    let mut b = vec![0u8; 15 * 4096];
    s.read(key(0), 33 * 4096, &mut b, 1).unwrap();
    assert_eq!(b, blocks(7, 33, 15));
    s.read(key(0), 65 * 4096, &mut b, 1).unwrap();
    assert_eq!(b, blocks(8, 65, 15));
    s.fsck(true).unwrap();
}
