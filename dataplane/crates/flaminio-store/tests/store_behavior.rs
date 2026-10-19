//! Direct behavior of the replica store: the write paths, reads, snapshots, discard, fencing,
//! space reclamation and corruption detection.

mod common;

use common::*;
use flaminio_format::consts::MIB;
use flaminio_store::{
    Allocation, BlockDevice, MemDevice, ReadReq, Store, StoreConfig, StoreError, WriteReq,
};

const CLUSTER: u64 = 64 * 1024;

fn read(
    store: &flaminio_store::Store<flaminio_store::MemDevice>,
    k: u64,
    offset: u64,
    len: u64,
) -> Vec<u8> {
    let mut buf = vec![0u8; len as usize];
    store.read(key(k), offset, &mut buf, 1).unwrap();
    buf
}

#[test]
fn every_write_path_reads_back_before_and_after_reopen() {
    let mut s = new_store(16);
    s.create_chunk(key(0), CHUNK, 64 * MIB, 1).unwrap();
    assert_eq!(
        read(&s, 0, 0, CHUNK),
        vec![0; CHUNK as usize],
        "a new chunk reads as zeros"
    );

    // Full clusters (redirect-on-write), crossing a cluster boundary at both ends.
    let data = blocks(1, 8, 40);
    s.write(key(0), 8 * 4096, &data, 1, 1).unwrap();
    // Partial write into a mapped cluster (deferred, applied in place).
    s.write(key(0), 20 * 4096, &blocks(2, 20, 2), 2, 1).unwrap();
    // Partial write into an unmapped cluster (read-modify-write).
    s.write(key(0), 100 * 4096, &blocks(3, 100, 1), 3, 1)
        .unwrap();

    let mut expect = vec![0u8; CHUNK as usize];
    expect[8 * 4096..48 * 4096].copy_from_slice(&data);
    expect[20 * 4096..22 * 4096].copy_from_slice(&blocks(2, 20, 2));
    expect[100 * 4096..101 * 4096].copy_from_slice(&blocks(3, 100, 1));
    assert_eq!(read(&s, 0, 0, CHUNK), expect);
    assert_eq!(s.chunk_info(key(0)).unwrap().last_write_seq, 3);
    s.fsck(true).unwrap();

    let s = reopen(s);
    assert_eq!(read(&s, 0, 0, CHUNK), expect, "after recovery");
    s.fsck(true).unwrap();
}

#[test]
fn snapshot_preserves_old_layer_and_reads_through() {
    let mut s = new_store(16);
    s.create_chunk(key(0), CHUNK, CHUNK, 1).unwrap();
    s.write(key(0), 0, &blocks(1, 0, 32), 1, 1).unwrap();
    s.snapshot(key(0), 1, 1).unwrap();
    // Partial overwrite after a snapshot must copy the cluster, not modify the frozen one.
    s.write(key(0), 4096, &blocks(2, 1, 1), 2, 1).unwrap();
    let mut expect = blocks(1, 0, 32);
    expect[4096..8192].copy_from_slice(&blocks(2, 1, 1));
    assert_eq!(read(&s, 0, 0, 32 * 4096), expect);
    // Cluster 1 was never rewritten: read through to the snapshot layer.
    assert_eq!(read(&s, 0, CLUSTER, 16 * 4096), blocks(1, 16, 16));
    // Discard with a snapshot below writes zero entries rather than unmapping.
    s.discard(key(0), CLUSTER, CLUSTER, 1).unwrap();
    assert_eq!(read(&s, 0, CLUSTER, CLUSTER), vec![0; CLUSTER as usize]);
    expect[16 * 4096..].fill(0);
    assert_eq!(read(&s, 0, 0, 32 * 4096), expect);
    assert_eq!(s.chunk_info(key(0)).unwrap().layers, 2);
    assert!(
        s.snapshot(key(0), 1, 1).is_err(),
        "snapshot generations must increase"
    );
    s.fsck(true).unwrap();
    let s = reopen(s);
    assert_eq!(read(&s, 0, 0, 32 * 4096), expect);
    assert_eq!(read(&s, 0, CLUSTER, CLUSTER), vec![0; CLUSTER as usize]);
}

#[test]
fn fencing_rejects_old_epochs() {
    let mut s = new_store(16);
    s.create_chunk(key(0), CHUNK, CHUNK, 3).unwrap();
    s.create_chunk(key(1), CHUNK, CHUNK, 3).unwrap();
    assert!(matches!(
        s.write(key(0), 0, &block(1, 0), 1, 2),
        Err(StoreError::StaleEpoch { fenced: 3, got: 2 })
    ));
    s.fence(key(0).volume, 5).unwrap();
    assert!(matches!(
        s.write(key(1), 0, &block(1, 0), 1, 4),
        Err(StoreError::StaleEpoch { fenced: 5, got: 4 })
    ));
    let mut buf = vec![0; 4096];
    assert!(
        matches!(
            s.read(key(1), 0, &mut buf, 4),
            Err(StoreError::StaleEpoch { .. })
        ),
        "reads are fenced too"
    );
    assert!(
        matches!(
            s.fence(key(0).volume, 4),
            Err(StoreError::StaleEpoch { .. })
        ),
        "fencing cannot go backwards"
    );
    s.write(key(1), 0, &block(1, 0), 1, 5).unwrap();
    let s = reopen(s);
    assert_eq!(
        s.chunk_info(key(0)).unwrap().fenced_epoch,
        5,
        "fence survives recovery"
    );
}

#[test]
fn deleted_space_is_reused_only_after_a_checkpoint() {
    let mut s = new_store(16);
    s.create_chunk(key(0), CHUNK, CHUNK, 1).unwrap();
    let used_empty = s.stats().data_used;
    s.write(key(0), 0, &blocks(1, 0, 1024), 1, 1).unwrap();
    assert_eq!(s.stats().data_used, used_empty + 64);
    s.delete_chunk(key(0), 1).unwrap();
    assert_eq!(
        s.stats().data_used,
        used_empty + 64,
        "freed clusters stay reserved until a checkpoint"
    );
    s.checkpoint().unwrap();
    assert_eq!(s.stats().data_used, used_empty);
    assert_eq!(s.stats().pool_used, 0);
    s.fsck(false).unwrap();
}

#[test]
fn small_journal_wraps_and_checkpoints_automatically() {
    let mut s = new_store(4);
    s.create_chunk(key(0), CHUNK, CHUNK, 1).unwrap();
    s.write(key(0), 0, &blocks(0, 0, 1024), 0, 1).unwrap();
    // 2000 deferred 8 KiB writes: about 16 MiB of journal through a 4 MiB journal.
    for i in 1..=2000u32 {
        let b = u64::from(i * 7) % 1023;
        s.write(key(0), b * 4096, &blocks(i, b, 2), u64::from(i), 1)
            .unwrap();
    }
    assert!(
        s.stats().checkpoint_seq > 3,
        "journal pressure must force checkpoints"
    );
    let before = read(&s, 0, 0, CHUNK);
    let s = reopen(s);
    assert_eq!(read(&s, 0, 0, CHUNK), before);
    s.fsck(true).unwrap();
}

#[test]
fn damaged_data_is_detected_on_read() {
    let mut s = new_store(16);
    s.create_chunk(key(0), CHUNK, CHUNK, 1).unwrap();
    s.write(key(0), 0, &blocks(1, 0, 16), 1, 1).unwrap();
    // Find the cluster on the device and flip a byte in its third block.
    let layout = s.superblock().layout;
    let mut found = None;
    for c in 0..layout.data_clusters {
        let mut b = vec![0u8; 4096];
        s.device()
            .read_at(layout.data_cluster_offset(c), &mut b)
            .unwrap();
        if b == block(1, 0) {
            found = Some(layout.data_cluster_offset(c));
            break;
        }
    }
    let off = found.expect("data on device") + 2 * 4096;
    let mut b = vec![0u8; 4096];
    s.device().read_at(off, &mut b).unwrap();
    b[100] ^= 1;
    s.device().write_at(off, &b).unwrap();
    let mut buf = vec![0u8; 16 * 4096];
    match s.read(key(0), 0, &mut buf, 1) {
        Err(StoreError::Checksum { offset, .. }) => assert_eq!(offset, 2 * 4096),
        other => panic!("expected a checksum error, got {other:?}"),
    }
    assert!(s.fsck(true).is_err());
}

#[test]
fn rejects_malformed_requests() {
    let mut s = new_store(16);
    s.create_chunk(key(0), CHUNK, CHUNK, 1).unwrap();
    assert!(matches!(
        s.create_chunk(key(0), CHUNK, CHUNK, 1),
        Err(StoreError::ChunkExists(_))
    ));
    assert!(s.create_chunk(key(1), 1000, CHUNK, 1).is_err());
    assert!(
        s.write(key(0), 100, &block(1, 0), 1, 1).is_err(),
        "unaligned"
    );
    assert!(
        s.write(key(0), CHUNK, &block(1, 0), 1, 1).is_err(),
        "past the end"
    );
    assert!(matches!(
        s.write(key(9), 0, &block(1, 0), 1, 1),
        Err(StoreError::NoSuchChunk(_))
    ));
    // None of these may poison the store.
    s.write(key(0), 0, &block(1, 0), 1, 1).unwrap();
}

#[test]
fn fills_the_disk_then_reports_out_of_space() {
    let mut s = new_store(16);
    let mut i = 0;
    let err = loop {
        if let Err(e) = s.create_chunk(key(i), CHUNK, CHUNK, 1) {
            break e;
        }
        if let Err(e) = s.write(key(i), 0, &vec![7u8; CHUNK as usize], 1, 1) {
            break e;
        }
        i += 1;
    };
    assert!(matches!(err, StoreError::OutOfSpace(_)), "{err:?}");
    s.fsck(false).unwrap();
    // Space comes back after delete + checkpoint.
    s.delete_chunk(key(0), 1).unwrap();
    s.checkpoint().unwrap();
    s.create_chunk(key(1000), CHUNK, CHUNK, 1).unwrap();
    s.write(key(1000), 0, &vec![8u8; CHUNK as usize], 1, 1)
        .unwrap();
}

#[test]
fn group_commit_orders_overlaps_and_isolates_failures() {
    let mut s = new_store(16);
    s.create_chunk(key(0), CHUNK, CHUNK, 1).unwrap();
    s.create_chunk(key(1), CHUNK, CHUNK, 5).unwrap();
    s.write(key(0), 0, &blocks(1, 0, 1024), 1, 1).unwrap();
    let (a, b, c, d) = (
        blocks(2, 3, 1),
        blocks(3, 3, 2),
        blocks(4, 500, 4),
        blocks(5, 0, 1),
    );
    let reqs = [
        WriteReq {
            key: key(0),
            offset: 3 * 4096,
            data: (&a).into(),
            write_seq: 2,
            epoch: 1,
        },
        // Overlaps the first: must be applied after it, in a later group.
        WriteReq {
            key: key(0),
            offset: 3 * 4096,
            data: (&b).into(),
            write_seq: 3,
            epoch: 1,
        },
        WriteReq {
            key: key(0),
            offset: 500 * 4096,
            data: (&c).into(),
            write_seq: 4,
            epoch: 1,
        },
        // Stale epoch and a bad offset: fail alone.
        WriteReq {
            key: key(1),
            offset: 0,
            data: (&d).into(),
            write_seq: 5,
            epoch: 4,
        },
        WriteReq {
            key: key(0),
            offset: 7,
            data: (&d).into(),
            write_seq: 6,
            epoch: 1,
        },
    ];
    let results = s.write_batch(&reqs).unwrap();
    assert!(results[0].is_ok() && results[1].is_ok() && results[2].is_ok());
    assert!(matches!(results[3], Err(StoreError::StaleEpoch { .. })));
    assert!(matches!(results[4], Err(StoreError::Invalid(_))));

    let mut expect = blocks(1, 0, 1024);
    expect[3 * 4096..5 * 4096].copy_from_slice(&b);
    expect[500 * 4096..504 * 4096].copy_from_slice(&c);
    assert_eq!(
        read(&s, 0, 0, CHUNK),
        expect,
        "the later overlapping write wins"
    );

    let (mut x, mut y, mut z) = (vec![0u8; 8192], vec![0u8; 16384], vec![0u8; 4096]);
    let mut rr = [
        ReadReq {
            key: key(0),
            offset: 3 * 4096,
            buf: &mut x,
            epoch: 1,
        },
        ReadReq {
            key: key(0),
            offset: 500 * 4096,
            buf: &mut y,
            epoch: 1,
        },
        ReadReq {
            key: key(1),
            offset: 0,
            buf: &mut z,
            epoch: 1,
        },
    ];
    let results = s.read_batch(&mut rr).unwrap();
    assert!(results[0].is_ok() && results[1].is_ok());
    assert!(matches!(results[2], Err(StoreError::StaleEpoch { .. })));
    assert_eq!((x, y), (b, c));
    let s = reopen(s);
    assert_eq!(read(&s, 0, 0, CHUNK), expect);
    s.fsck(true).unwrap();
}

#[test]
fn group_commit_shares_flushes() {
    let mut s = new_store(64);
    s.create_chunk(key(0), CHUNK, CHUNK, 1).unwrap();
    s.write(key(0), 0, &blocks(1, 0, 1024), 1, 1).unwrap();
    // 32 small writes into mapped clusters: journal-only (deferred) writes.
    let data: Vec<Vec<u8>> = (0..32u64).map(|i| blocks(2, i * 32, 1)).collect();
    let reqs: Vec<WriteReq<'_>> = (0..32u64)
        .map(|i| WriteReq {
            key: key(0),
            offset: i * 32 * 4096,
            data: (&data[i as usize]).into(),
            write_seq: 2 + i,
            epoch: 1,
        })
        .collect();

    let before = s.device().flushes();
    for r in &reqs {
        s.write(r.key, r.offset, &r.data, r.write_seq, 1).unwrap();
    }
    let one_by_one = s.device().flushes() - before;

    let before = s.device().flushes();
    assert!(s.write_batch(&reqs).unwrap().iter().all(Result::is_ok));
    let grouped = s.device().flushes() - before;
    assert_eq!(one_by_one, 32, "one journal flush per write");
    assert_eq!(grouped, 1, "one journal flush for the whole group");
}

/// Performs a plan's device reads synchronously, as the frontend does asynchronously.
fn perform(dev: &impl BlockDevice, plan: &flaminio_store::ReadPlan, buf: &mut [u8]) {
    for r in plan.reads() {
        dev.read_at(
            r.device_offset,
            &mut buf[r.buf_offset..r.buf_offset + r.len],
        )
        .unwrap();
    }
}

#[test]
fn planned_reads_zero_fill_merge_and_verify() {
    let mut s = new_store(16);
    s.create_chunk(key(0), CHUNK, CHUNK, 1).unwrap();
    s.write(key(0), 0, &blocks(1, 0, 32), 1, 1).unwrap(); // two contiguous clusters
    let mut buf = vec![0xFFu8; 48 * 4096];
    let plan = s.plan_read(key(0), 0, &mut buf, 1).unwrap();
    assert_eq!(
        plan.reads().len(),
        1,
        "two adjacent clusters become one device read"
    );
    assert!(
        buf[32 * 4096..].iter().all(|&b| b == 0),
        "the unmapped third cluster is zero-filled"
    );
    perform(s.device(), &plan, &mut buf);
    plan.verify(&buf).unwrap();
    assert_eq!(&buf[..32 * 4096], &blocks(1, 0, 32)[..]);
    buf[4096] ^= 1;
    assert!(matches!(
        plan.verify(&buf),
        Err(StoreError::Checksum { offset: 4096, .. })
    ));
    s.finish_read(plan);
    assert!(matches!(
        s.plan_read(key(0), 0, &mut buf, 0),
        Err(StoreError::StaleEpoch { .. })
    ));
}

#[test]
fn clusters_under_an_in_flight_read_are_not_reused() {
    let dev = MemDevice::new(DEVICE);
    Store::format(&dev, &options(16)).unwrap();
    let config = StoreConfig {
        allocation: Allocation::FirstFit,
        ..StoreConfig::default()
    };
    let mut s = Store::open(&dev, config).unwrap();
    s.create_chunk(key(0), CHUNK, CHUNK, 1).unwrap();
    s.write(key(0), 0, &blocks(1, 0, 16), 1, 1).unwrap();

    // Plan a read of cluster 0, then overwrite it (redirect-on-write frees the old cluster)
    // and checkpoint, which would normally make the old cluster reusable at once.
    let mut buf = vec![0u8; 16 * 4096];
    let plan = s.plan_read(key(0), 0, &mut buf, 1).unwrap();
    s.write(key(0), 0, &blocks(2, 0, 16), 2, 1).unwrap();
    s.checkpoint().unwrap();
    assert_eq!(s.stats().data_quarantined, 1);
    // First-fit would hand out the lowest free cluster: the one the read is still targeting.
    s.create_chunk(key(1), CHUNK, CHUNK, 1).unwrap();
    s.write(key(1), 0, &blocks(3, 0, 16), 3, 1).unwrap();

    perform(&dev, &plan, &mut buf);
    plan.verify(&buf)
        .expect("the in-flight read must still see the data it planned for");
    assert_eq!(buf, blocks(1, 0, 16));
    s.finish_read(plan);
    s.write(key(1), 64 * 1024, &blocks(4, 16, 16), 4, 1)
        .unwrap();
    assert_eq!(
        s.stats().data_quarantined,
        0,
        "released on the next mutation once no reads are in flight"
    );
    s.fsck(true).unwrap();
}
