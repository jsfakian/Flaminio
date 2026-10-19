//! Write batches whose I/O completes asynchronously (`Store::start_write_batch`): nothing is
//! applied before the device reports the commit durable, other changes wait for it, only one is
//! in flight, and a commit that never completes is simply absent after recovery.

mod common;

use common::*;
use flaminio_format::consts::MIB;
use flaminio_store::{
    DeferredDevice, MemDevice, Started, Store, StoreConfig, StoreError, WriteReq,
};

fn pipeline_config() -> StoreConfig {
    StoreConfig {
        pipelined_commits: true,
        ..StoreConfig::default()
    }
}

fn store() -> Store<DeferredDevice> {
    store_with_journal(16)
}

fn store_with_journal(mib: u64) -> Store<DeferredDevice> {
    let dev = MemDevice::new(DEVICE);
    Store::format(&dev, &options(mib)).unwrap();
    let mut s = Store::open(DeferredDevice::new(dev), pipeline_config()).unwrap();
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
        data: data.into(),
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
    assert!(
        !s.can_stage(key(0), 512 * 4096, 15 * 4096),
        "a write without a whole cluster is not staged"
    );
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
    // checkpoints keep waiting, the 4 MiB journal fills (a batch takes at least 4 KiB), and
    // staging stops until a commit can checkpoint. Whole-cluster writes: only those are staged.
    let mut s = store_with_journal(4);
    let fill = blocks(9, 0, 1024);
    s.write(key(0), 0, &fill, 1, 1).unwrap();
    let mut expect = fill.clone();
    let write = |i: u64| (i % 64, blocks(2 + i as u32, (i % 64) * 16, 16));
    let (x, d) = write(0);
    assert!(matches!(
        s.start_write_batch(&[req(x * 16 * 4096, &d, 2)]).unwrap(),
        Started::InFlight
    ));
    expect[(x * 16 * 4096) as usize..][..d.len()].copy_from_slice(&d);
    let (mut staged, mut refused) = (0, 0);
    for i in 1..1500u64 {
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
    assert!(staged > 1000, "staged {staged}");
    assert!(
        refused > 0,
        "refused {refused}: the journal never ran short"
    );
    assert_eq!(read(&s, 0, 1024 * 4096), expect);
    s.fsck(true).unwrap();
    let s = Store::open(s.into_device().into_inner(), StoreConfig::default()).unwrap();
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

#[test]
fn shared_write_data_outlives_the_callers_buffer() {
    // W4.3: a write handed over as shared bytes, whole clusters and a small write, committed
    // after the caller let go of its own reference.
    let mut s = store();
    let whole = bytes::Bytes::from(blocks(1, 0, 32));
    let small = bytes::Bytes::from(blocks(2, 40, 2));
    let reqs = [
        WriteReq {
            key: key(0),
            offset: 0,
            data: whole.clone().into(),
            write_seq: 1,
            epoch: 1,
        },
        WriteReq {
            key: key(0),
            offset: 40 * 4096,
            data: small.clone().into(),
            write_seq: 2,
            epoch: 1,
        },
    ];
    assert!(matches!(
        s.start_write_batch(&reqs).unwrap(),
        Started::InFlight
    ));
    let (expect_whole, expect_small) = (whole.to_vec(), small.to_vec());
    drop((reqs, whole, small));
    s.device().complete();
    let results = s.poll_write_batch(false).unwrap().unwrap();
    assert!(results.iter().all(Result::is_ok));
    assert_eq!(read(&s, 0, 32 * 4096), expect_whole);
    assert_eq!(read(&s, 40 * 4096, 2 * 4096), expect_small);
    let s = reopen(s);
    let mut buf = vec![0u8; 32 * 4096];
    s.read(key(0), 0, &mut buf, 1).unwrap();
    assert_eq!(buf, expect_whole, "after recovery");
}

// Pipelined commits (spec §6.2): up to PIPELINE_DEPTH commits in flight at once.

#[test]
fn an_empty_staged_group_does_not_jump_the_queue() {
    // Every request in the staged group fails on its own (stale epoch): no journal records.
    // Its results must wait behind the commit already in flight; otherwise poll would hand
    // the failing group's Err to the earlier write's caller, and that write's Ok to the later
    // one.
    let mut s = store();
    let (o, d) = whole(0);
    assert!(matches!(
        s.start_write_batch(&[req(o, &d, 1)]).unwrap(),
        Started::InFlight
    ));
    let (o2, d2) = whole(1);
    let stale = WriteReq {
        key: key(0),
        offset: o2,
        data: (&d2).into(),
        write_seq: 2,
        epoch: 0,
    };
    assert_eq!(s.stage_writes(&[stale]).unwrap(), 1);
    assert!(s.launch_staged().unwrap());
    assert_eq!(
        s.device().commits_in_flight(),
        1,
        "the empty group has no device commit"
    );
    assert!(
        s.poll_write_batch(false).unwrap().is_none(),
        "the empty group must not settle ahead of the commit in flight"
    );
    s.device().complete();
    let first = s.poll_write_batch(false).unwrap().expect("the first commit");
    assert!(first.iter().all(Result::is_ok), "{first:?}");
    assert_eq!(&read(&s, o, d.len() as u64), &d);
    let second = s
        .poll_write_batch(false)
        .unwrap()
        .expect("the empty staged group");
    assert_eq!(second.len(), 1);
    assert!(matches!(second[0], Err(StoreError::StaleEpoch { .. })), "{second:?}");
    assert!(!s.commit_in_flight());
}

fn whole(i: u64) -> (u64, Vec<u8>) {
    // Whole clusters (16 blocks), each write to its own cluster: stageable.
    let x = i % 60;
    (x * 16 * 4096, blocks(1 + i as u32, x * 16, 16))
}

#[test]
fn pipelined_commits_land_in_order() {
    let mut s = store();
    let w: Vec<(u64, Vec<u8>)> = (0..6).map(whole).collect();
    assert!(matches!(
        s.start_write_batch(&[req(w[0].0, &w[0].1, 1)]).unwrap(),
        Started::InFlight
    ));
    let mut launched = 1;
    for (i, (o, d)) in w.iter().enumerate().skip(1) {
        assert_eq!(s.stage_writes(&[req(*o, d, 1 + i as u64)]).unwrap(), 1);
        if s.launch_staged().unwrap() {
            launched += 1;
        }
    }
    assert_eq!(launched, flaminio_format::consts::PIPELINE_DEPTH);
    assert_eq!(
        s.device().commits_in_flight(),
        launched,
        "the window is full"
    );
    // The last two writes were staged together behind the full window.
    let mut landed = 0;
    while s.commit_in_flight() {
        if s.device().in_flight() {
            s.device().complete();
        }
        if let Some(r) = s.poll_write_batch(false).unwrap() {
            assert!(r.iter().all(Result::is_ok));
            landed += r.len();
        }
    }
    assert_eq!(landed, 6);
    for (o, d) in &w {
        assert_eq!(&read(&s, *o, d.len() as u64), d);
    }
    s.fsck(true).unwrap();
    let s = reopen(s);
    let mut buf = vec![0u8; 16 * 4096];
    for (o, d) in &w {
        s.read(key(0), *o, &mut buf, 1).unwrap();
        assert_eq!(&buf, d, "after recovery");
    }
}

#[test]
fn a_power_cut_with_commits_in_flight_keeps_a_prefix_of_them() {
    // The writes of commits in flight reach the cache at once, and the crash keeps, loses or
    // tears each unflushed block independently: a later batch's journal may survive an
    // earlier batch's data. Recovery must show the landed commits and a prefix of the others,
    // each whole or absent.
    let (mut most_in_flight, mut kept, mut lost) = (0, 0, 0);
    for stop in [2u64, 5, 9, 23] {
        for seed in 0..16 {
            let dev = MemDevice::new(DEVICE);
            Store::format(&dev, &options(16)).unwrap();
            let mut s =
                Store::open(DeferredDevice::writing_through(dev), pipeline_config()).unwrap();
            s.create_chunk(key(0), CHUNK, 64 * MIB, 1).unwrap();
            let old = blocks(9, 0, 1024);
            s.write(key(0), 0, &old, 1, 1).unwrap();
            let mut landed = old.clone();
            let mut flying: std::collections::VecDeque<Vec<(u64, Vec<u8>)>> = Default::default();
            let (o, d) = whole(0);
            assert!(matches!(
                s.start_write_batch(&[req(o, &d, 2)]).unwrap(),
                Started::InFlight
            ));
            flying.push_back(vec![(o, d)]);
            let mut staged = Vec::new();
            for i in 1..stop {
                let (o, d) = whole(i);
                if s.stage_writes(&[req(o, &d, 2 + i)]).unwrap() == 1 {
                    staged.push((o, d));
                }
                if s.launch_staged().unwrap() {
                    flying.push_back(std::mem::take(&mut staged));
                }
                // Now and then, the oldest lands.
                if (i + seed) % 3 == 0 {
                    s.device().complete();
                    let r = s.poll_write_batch(false).unwrap().expect("landed");
                    assert!(r.iter().all(Result::is_ok));
                    for (o, d) in flying.pop_front().expect("in flight") {
                        landed[o as usize..][..d.len()].copy_from_slice(&d);
                    }
                    if s.commit_in_flight()
                        && !staged.is_empty()
                        && s.device().commits_in_flight() == flying.len() + 1
                    {
                        flying.push_back(std::mem::take(&mut staged));
                    }
                }
            }
            most_in_flight = most_in_flight.max(flying.len());
            let image = s.device().crash(seed);
            let s = Store::open(image, pipeline_config()).unwrap();
            let mut got = vec![0u8; 1024 * 4096];
            s.read(key(0), 0, &mut got, 1).unwrap();
            // Every landed write is there; the commits in flight form a prefix.
            let mut gap = false;
            for group in &flying {
                let present = group
                    .iter()
                    .all(|(o, d)| got[*o as usize..][..d.len()] == d[..]);
                let absent = group.iter().all(|(o, d)| {
                    got[*o as usize..][..d.len()] == landed[*o as usize..][..d.len()]
                });
                assert!(
                    present || absent,
                    "stop {stop} seed {seed}: a commit half there"
                );
                if present && !absent {
                    kept += 1;
                    assert!(
                        !gap,
                        "stop {stop} seed {seed}: a later commit without an earlier one"
                    );
                    for (o, d) in group {
                        landed[*o as usize..][..d.len()].copy_from_slice(d);
                    }
                } else if !present {
                    lost += 1;
                    gap = true;
                }
            }
            assert!(got == landed, "stop {stop} seed {seed}: landed writes lost");
            s.fsck(true).unwrap();
        }
    }
    // The cases that matter happened: several commits in flight, some kept and some lost.
    assert!(most_in_flight >= 3, "at most {most_in_flight} in flight");
    assert!(kept > 0 && lost > 0, "kept {kept}, lost {lost}");
}

#[test]
fn pipelining_through_full_journals_keeps_the_data() {
    // A full window kept up for long: checkpoints wait until nothing is in flight, staging
    // stops when the journal runs short, and everything lands.
    let mut s = store_with_journal(4);
    let mut expect = vec![0u8; 1024 * 4096];
    let (o, d) = whole(0);
    assert!(matches!(
        s.start_write_batch(&[req(o, &d, 1)]).unwrap(),
        Started::InFlight
    ));
    expect[o as usize..][..d.len()].copy_from_slice(&d);
    let mut launches = 0;
    for i in 1..3000u64 {
        let (o, d) = whole(i);
        // Writes to a cluster still in flight wait (not staged): commit them alone after.
        if s.stage_writes(&[req(o, &d, 1 + i)]).unwrap() == 0 {
            while s.commit_in_flight() {
                if s.device().in_flight() {
                    s.device().complete();
                }
                s.poll_write_batch(false).unwrap();
            }
            assert!(matches!(
                s.start_write_batch(&[req(o, &d, 1 + i)]).unwrap(),
                Started::InFlight
            ));
        } else if s.launch_staged().unwrap() {
            launches += 1;
        }
        expect[o as usize..][..d.len()].copy_from_slice(&d);
        if s.device().commits_in_flight() >= flaminio_format::consts::PIPELINE_DEPTH {
            s.device().complete();
            assert!(
                s.poll_write_batch(false)
                    .unwrap()
                    .unwrap()
                    .iter()
                    .all(Result::is_ok)
            );
        }
    }
    while s.commit_in_flight() {
        if s.device().in_flight() {
            s.device().complete();
        }
        s.poll_write_batch(false).unwrap();
    }
    assert!(launches > 1000, "launches {launches}");
    assert_eq!(read(&s, 0, 1024 * 4096), expect);
    s.fsck(true).unwrap();
    let s = Store::open(s.into_device().into_inner(), StoreConfig::default()).unwrap();
    let mut buf = vec![0u8; 1024 * 4096];
    s.read(key(0), 0, &mut buf, 1).unwrap();
    assert!(buf == expect, "after recovery");
    s.fsck(true).unwrap();
}
