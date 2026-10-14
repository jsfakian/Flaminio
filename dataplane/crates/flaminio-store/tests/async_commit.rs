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
