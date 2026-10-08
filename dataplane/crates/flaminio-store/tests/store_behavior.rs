//! Direct behavior of the replica store: the write paths, reads, snapshots, discard, fencing,
//! space reclamation and corruption detection.

mod common;

use common::*;
use flaminio_format::consts::MIB;
use flaminio_store::{BlockDevice, StoreError};

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
