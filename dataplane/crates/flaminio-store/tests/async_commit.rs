//! Write batches whose I/O completes asynchronously (`Store::start_write_batch`): nothing is
//! applied before the device reports the commit durable, other changes wait for it, only one is
//! in flight, and a commit that never completes is simply absent after recovery.

mod common;

use common::*;
use flaminio_format::consts::MIB;
use flaminio_store::{
    DeferredDevice, MemDevice, Started, Store, StoreConfig, StoreError, WriteReq,
};

fn store() -> Store<DeferredDevice> {
    let dev = MemDevice::new(DEVICE);
    Store::format(&dev, &options(16)).unwrap();
    let mut s = Store::open(DeferredDevice::new(dev), StoreConfig::default()).unwrap();
    s.create_chunk(key(0), CHUNK, 64 * MIB, 1).unwrap();
    s
}

fn read(s: &Store<DeferredDevice>, offset: u64, len: u64) -> Vec<u8> {
    let mut buf = vec![0u8; len as usize];
    s.read(key(0), offset, &mut buf, 1).unwrap();
    buf
}

fn req(offset: u64, data: &[u8], seq: u64) -> WriteReq<'_> {
    WriteReq {
        key: key(0),
        offset,
        data,
        write_seq: seq,
        epoch: 1,
    }
}

fn reopen(s: Store<DeferredDevice>) -> Store<MemDevice> {
    Store::open(s.into_device().into_inner(), StoreConfig::default()).unwrap()
}

#[test]
fn nothing_is_applied_before_the_commit_is_durable() {
    let mut s = store();
    let full = blocks(1, 0, 32); // two whole clusters: redirect-on-write
    s.write(key(0), 0, &full, 1, 1).unwrap();
    let small = blocks(2, 4, 1); // a mapped cluster: deferred, in place
    let fresh = blocks(3, 64, 1); // an unmapped cluster: read-modify-write
    let reqs = [req(4 * 4096, &small, 2), req(64 * 4096, &fresh, 3)];
    assert!(matches!(
        s.start_write_batch(&reqs).unwrap(),
        Started::InFlight
    ));
    assert!(s.commit_in_flight());
    assert!(s.poll_write_batch(false).unwrap().is_none());
    assert_eq!(read(&s, 0, 32 * 4096), full, "the old data, until durable");
    assert_eq!(read(&s, 64 * 4096, 4096), vec![0; 4096]);
    assert!(
        s.start_write_batch(&reqs).is_err(),
        "a second batch waits for the first"
    );

    s.device().complete();
    let results = s.poll_write_batch(false).unwrap().expect("durable");
    assert!(results.iter().all(Result::is_ok));
    assert!(!s.commit_in_flight());
    let mut expect = full.clone();
    expect[4 * 4096..5 * 4096].copy_from_slice(&small);
    assert_eq!(read(&s, 0, 32 * 4096), expect);
    assert_eq!(read(&s, 64 * 4096, 4096), fresh);
    s.fsck(true).unwrap();

    let s = reopen(s);
    let mut buf = vec![0u8; 32 * 4096];
    s.read(key(0), 0, &mut buf, 1).unwrap();
    assert_eq!(buf, expect, "after recovery");
    s.fsck(true).unwrap();
}

#[test]
fn other_changes_wait_for_the_commit_in_flight() {
    let mut s = store();
    let data = blocks(1, 0, 16);
    assert!(matches!(
        s.start_write_batch(&[req(0, &data, 1)]).unwrap(),
        Started::InFlight
    ));
    // A fence must not become durable before the batch ahead of it.
    s.fence(key(0).volume, 5).unwrap();
    assert!(!s.device().in_flight(), "the fence waited for the commit");
    let results = s
        .poll_write_batch(false)
        .unwrap()
        .expect("kept for the caller");
    assert!(results.iter().all(Result::is_ok));
    assert_eq!(read_at_epoch(&s, 5), data);

    let s = reopen(s);
    assert_eq!(s.chunk_info(key(0)).unwrap().fenced_epoch, 5);
    let mut buf = vec![0u8; 16 * 4096];
    s.read(key(0), 0, &mut buf, 5).unwrap();
    assert_eq!(buf, data);
}

fn read_at_epoch(s: &Store<DeferredDevice>, epoch: u64) -> Vec<u8> {
    let mut buf = vec![0u8; 16 * 4096];
    s.read(key(0), 0, &mut buf, epoch).unwrap();
    buf
}

#[test]
fn only_the_last_group_stays_in_flight() {
    let mut s = store();
    // The same cluster twice: two groups, in order.
    let (a, b) = (blocks(1, 0, 16), blocks(2, 0, 16));
    let reqs = [req(0, &a, 1), req(0, &b, 2)];
    assert!(matches!(
        s.start_write_batch(&reqs).unwrap(),
        Started::InFlight
    ));
    assert_eq!(
        read(&s, 0, 16 * 4096),
        a,
        "the first group is durable already"
    );
    s.device().complete();
    let results = s.poll_write_batch(false).unwrap().unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(Result::is_ok));
    assert_eq!(read(&s, 0, 16 * 4096), b);
}

#[test]
fn a_commit_that_never_completes_is_absent_after_recovery() {
    let mut s = store();
    let old = blocks(1, 0, 16);
    s.write(key(0), 0, &old, 1, 1).unwrap();
    let new = blocks(2, 0, 16);
    assert!(matches!(
        s.start_write_batch(&[req(0, &new, 2)]).unwrap(),
        Started::InFlight
    ));
    // The process dies with the commit in flight: none of its writes reached the disk.
    let s = reopen(s);
    let mut buf = vec![0u8; 16 * 4096];
    s.read(key(0), 0, &mut buf, 1).unwrap();
    assert_eq!(buf, old);
    assert_eq!(s.chunk_info(key(0)).unwrap().last_write_seq, 1);
    s.fsck(true).unwrap();
}

#[test]
fn a_failed_commit_poisons_the_store() {
    let mut s = store();
    let data = blocks(1, 0, 16);
    assert!(matches!(
        s.start_write_batch(&[req(0, &data, 1)]).unwrap(),
        Started::InFlight
    ));
    s.device().fail_commit();
    assert!(matches!(s.poll_write_batch(false), Err(StoreError::Io(_))));
    assert!(matches!(
        s.write(key(0), 0, &data, 2, 1),
        Err(StoreError::Poisoned)
    ));
}

// Staging (W4.2): the next group written behind the commit in flight.

#[test]
fn staged_writes_commit_right_after_the_batch_in_flight() {
    let mut s = store();
    let (a, b, c) = (blocks(1, 0, 16), blocks(2, 256, 16), blocks(3, 0, 16));
    assert!(matches!(
        s.start_write_batch(&[req(0, &a, 1)]).unwrap(),
        Started::InFlight
    ));
    // A write elsewhere is staged; one to a cluster of the batch in flight is not.
    assert_eq!(s.stage_writes(&[req(256 * 4096, &b, 2)]).unwrap(), 1);
    assert!(!s.can_stage(key(0), 0, 16 * 4096));
    assert_eq!(s.stage_writes(&[req(0, &c, 3)]).unwrap(), 0);
    assert!(s.start_write_batch(&[req(0, &c, 3)]).is_err());

    s.device().complete();
    let first = s.poll_write_batch(false).unwrap().expect("the first batch");
    assert_eq!(first.len(), 1);
    assert!(first[0].is_ok());
    // The staged batch is in flight now: its journal batch only, its data went early.
    assert!(s.device().in_flight());
    assert_eq!(s.device().pending_writes(), 1);
    assert!(s.poll_write_batch(false).unwrap().is_none());
    assert_eq!(
        read(&s, 256 * 4096, 16 * 4096),
        vec![0; 16 * 4096],
        "not before durable"
    );
    s.device().complete();
    let second = s
        .poll_write_batch(false)
        .unwrap()
        .expect("the staged batch");
    assert_eq!(second.len(), 1);
    assert!(second[0].is_ok());
    assert!(!s.commit_in_flight());
    assert_eq!(read(&s, 0, 16 * 4096), a);
    assert_eq!(read(&s, 256 * 4096, 16 * 4096), b);
    s.fsck(true).unwrap();
    let s = reopen(s);
    let mut buf = vec![0u8; 16 * 4096];
    s.read(key(0), 256 * 4096, &mut buf, 1).unwrap();
    assert_eq!(buf, b, "after recovery");
    s.fsck(true).unwrap();
}

#[test]
fn a_staged_group_that_never_commits_is_absent_after_recovery() {
    for promoted in [false, true] {
        let mut s = store();
        let (a, b) = (blocks(1, 0, 16), blocks(2, 256, 16));
        assert!(matches!(
            s.start_write_batch(&[req(0, &a, 1)]).unwrap(),
            Started::InFlight
        ));
        assert_eq!(s.stage_writes(&[req(256 * 4096, &b, 2)]).unwrap(), 1);
        if promoted {
            // The first batch lands; the staged one is in flight, and never lands.
            s.device().complete();
            assert!(s.poll_write_batch(false).unwrap().is_some());
        }
        let s = reopen(s);
        let mut buf = vec![0u8; 16 * 4096];
        s.read(key(0), 0, &mut buf, 1).unwrap();
        assert_eq!(buf == a, promoted, "promoted={promoted}");
        s.read(key(0), 256 * 4096, &mut buf, 1).unwrap();
        assert_eq!(buf, vec![0; 16 * 4096], "promoted={promoted}");
        // Its data went to clusters nothing references: none leaked.
        s.fsck(true).unwrap();
    }
}

#[test]
fn a_fence_waits_for_the_staged_group_too() {
    let mut s = store();
    let (a, b) = (blocks(1, 0, 16), blocks(2, 256, 16));
    assert!(matches!(
        s.start_write_batch(&[req(0, &a, 1)]).unwrap(),
        Started::InFlight
    ));
    assert_eq!(s.stage_writes(&[req(256 * 4096, &b, 2)]).unwrap(), 1);
    s.fence(key(0).volume, 5).unwrap();
    assert!(!s.device().in_flight());
    let first = s.poll_write_batch(false).unwrap().expect("the first batch");
    let second = s
        .poll_write_batch(false)
        .unwrap()
        .expect("the staged batch");
    assert!(first.iter().chain(&second).all(Result::is_ok));
    assert!(!s.commit_in_flight());
    let s = reopen(s);
    let mut buf = vec![0u8; 16 * 4096];
    s.read(key(0), 256 * 4096, &mut buf, 5).unwrap();
    assert_eq!(buf, b);
    assert_eq!(s.chunk_info(key(0)).unwrap().fenced_epoch, 5);
}

#[test]
fn staging_through_full_journals_keeps_the_data() {
    // A continuous pipeline, as the engine runs it: a group is staged behind every commit, so
    // checkpoints keep waiting, the 16 MiB journal fills with journaled small writes (15 blocks
    // into mapped clusters), and staging stops until a commit can checkpoint.
    let mut s = store();
    let fill = blocks(9, 0, 1024);
    s.write(key(0), 0, &fill, 1, 1).unwrap();
    let mut expect = fill.clone();
    let write = |i: u64| (i % 64, blocks(2 + i as u32, (i % 64) * 16, 15));
    let (x, d) = write(0);
    assert!(matches!(
        s.start_write_batch(&[req(x * 16 * 4096, &d, 2)]).unwrap(),
        Started::InFlight
    ));
    expect[(x * 16 * 4096) as usize..][..d.len()].copy_from_slice(&d);
    let (mut staged, mut refused) = (0, 0);
    for i in 1..800u64 {
        let (x, d) = write(i);
        let took = s.stage_writes(&[req(x * 16 * 4096, &d, 2 + i)]).unwrap();
        // The commit in flight lands; the staged group, if any, takes its place.
        s.device().complete();
        let results = s.poll_write_batch(false).unwrap().expect("landed");
        assert!(results.iter().all(Result::is_ok));
        if took == 1 {
            staged += 1;
        } else {
            refused += 1;
            assert!(!s.commit_in_flight());
            assert!(matches!(
                s.start_write_batch(&[req(x * 16 * 4096, &d, 2 + i)])
                    .unwrap(),
                Started::InFlight
            ));
        }
        expect[(x * 16 * 4096) as usize..][..d.len()].copy_from_slice(&d);
    }
    while s.commit_in_flight() {
        if s.device().in_flight() {
            s.device().complete();
        }
        s.poll_write_batch(false).unwrap();
    }
    assert!(staged > 100, "staged {staged}");
    assert!(
        refused > 0,
        "refused {refused}: the journal never ran short"
    );
    assert_eq!(read(&s, 0, 1024 * 4096), expect);
    s.fsck(true).unwrap();
    let s = reopen(s);
    let mut buf = vec![0u8; 1024 * 4096];
    s.read(key(0), 0, &mut buf, 1).unwrap();
    assert!(buf == expect, "after recovery");
    s.fsck(true).unwrap();
}

#[test]
fn a_crash_in_the_pipeline_keeps_exactly_the_landed_writes() {
    // Power cut at various points of a staging pipeline, with every unflushed block (early data
    // included) independently kept, lost or torn: recovery shows the writes whose commits
    // landed, and no more.
    for stop in [3u64, 10, 37, 120] {
        for seed in 0..8 {
            let mut s = store();
            let fill = blocks(9, 0, 1024);
            s.write(key(0), 0, &fill, 1, 1).unwrap();
            let mut landed = fill.clone();
            // Alternate journaled small writes and whole-cluster (redirect-on-write) writes.
            let write = |i: u64| {
                let x = i % 64;
                let n = if i.is_multiple_of(2) { 15 } else { 16 };
                (x, blocks(2 + i as u32, x * 16, n))
            };
            let (x, d) = write(0);
            assert!(matches!(
                s.start_write_batch(&[req(x * 16 * 4096, &d, 2)]).unwrap(),
                Started::InFlight
            ));
            let mut flying = vec![(x, d)];
            for i in 1..stop {
                let (x, d) = write(i);
                let took = s.stage_writes(&[req(x * 16 * 4096, &d, 2 + i)]).unwrap();
                s.device().complete();
                assert!(s.poll_write_batch(false).unwrap().is_some());
                for (x, d) in flying.drain(..) {
                    landed[(x * 16 * 4096) as usize..][..d.len()].copy_from_slice(&d);
                }
                if took == 1 {
                    flying.push((x, d));
                } else {
                    assert!(matches!(
                        s.start_write_batch(&[req(x * 16 * 4096, &d, 2 + i)])
                            .unwrap(),
                        Started::InFlight
                    ));
                    flying.push((x, d));
                }
            }
            // One more staged behind the commit in flight, then the power goes.
            let (x, d) = write(stop);
            s.stage_writes(&[req(x * 16 * 4096, &d, 2 + stop)]).unwrap();
            let image = s.into_device().into_inner().crash(seed);
            let s = Store::open(image, StoreConfig::default()).unwrap();
            let mut buf = vec![0u8; 1024 * 4096];
            s.read(key(0), 0, &mut buf, 1).unwrap();
            assert!(buf == landed, "stop {stop} seed {seed}");
            s.fsck(true).unwrap();
        }
    }
}
