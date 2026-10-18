use flaminio_format::layout::LayoutParams;
use flaminio_format::mkfs::FormatOptions;
use flaminio_net::token::Claims;
use flaminio_repl::types::Version;
use flaminio_store::{DeferredDevice, MemDevice, StoreConfig};

use super::*;

const MIB: u64 = 1 << 20;
const CHUNK: u64 = 4 * MIB;
const BLOCKS: u32 = (CHUNK / BLOCK as u64) as u32;

fn store_on(dev: MemDevice) -> Store<MemDevice> {
    Store::open(dev, StoreConfig::default()).unwrap()
}

fn service() -> ReplicaService<MemDevice> {
    ReplicaService::new(store_on(formatted()))
}

/// A service whose commits stay in flight until the test completes them.
fn deferred_service() -> ReplicaService<DeferredDevice> {
    let store = Store::open(DeferredDevice::new(formatted()), StoreConfig::default()).unwrap();
    ReplicaService::new(store)
}

fn formatted() -> MemDevice {
    let dev = MemDevice::new(256 * MIB);
    let opts = FormatOptions {
        disk_uuid: Uuid([1; 16]),
        cluster_uuid: Uuid::NIL,
        format_nonce: 9,
        created_unix_secs: 0,
        layout: LayoutParams {
            journal_size: Some(16 * MIB),
            ..Default::default()
        },
        label: String::new(),
    };
    Store::format(&dev, &opts).unwrap();
    dev
}

/// Reopens the service on the same device, as after a crash and restart.
fn restart(s: ReplicaService<MemDevice>) -> ReplicaService<MemDevice> {
    ReplicaService::new(store_on(s.into_store().into_device()))
}

fn accepted(epoch: u64, session: u64, chunks: &[u32]) -> Accepted {
    Accepted {
        peer_node: "fl-a".into(),
        req: 1,
        epoch,
        session,
        claims: Claims {
            volume: "pvc-1".into(),
            epoch,
            node: "fl-a".into(),
            issued_at: 0,
        },
        size: 4 * CHUNK,
        chunk_size: CHUNK,
        chunks: chunks.to_vec(),
    }
}

fn write(req: u64, chunk: u32, start: u32, blocks: &[u8], seq: u64) -> Frame {
    Frame::Write {
        req,
        chunk,
        ext: Extent::new(start, blocks.len() as u32),
        version: Version { epoch: 1, seq },
        prune: None,
        data: blocks.iter().flat_map(|&b| vec![b; BLOCK]).collect(),
    }
}

fn read(req: u64, chunk: u32, start: u32, len: u32) -> Frame {
    Frame::Read {
        req,
        chunk,
        ext: Extent::new(start, len),
    }
}

/// The first byte of each block of a data reply.
fn blocks(r: &Reply) -> Vec<u8> {
    match r {
        Reply::Send(Frame::Data { data, .. }) => data.chunks(BLOCK).map(|b| b[0]).collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn hello_creates_the_placed_chunks_fenced() {
    let mut s = service();
    let (sess, reply) = s.hello(&accepted(3, 1, &[0, 2])).unwrap();
    assert!(matches!(reply, Frame::Fenced { req: 1, .. }), "{reply:?}");
    let mut chunks: Vec<(u64, u64)> = s
        .store()
        .chunks()
        .iter()
        .map(|c| (c.key.index, c.fenced_epoch))
        .collect();
    chunks.sort();
    assert_eq!(chunks, vec![(0, 3), (2, 3)]);
    // A chunk not placed here is not served.
    let r = s.handle(&[(sess, write(2, 1, 0, &[9], 0))]);
    assert_eq!(r, vec![Reply::Nothing]);
}

#[test]
fn reads_return_writes_and_unwritten_blocks_are_zero() {
    let mut s = service();
    let (sess, _) = s.hello(&accepted(1, 1, &[0])).unwrap();
    let r = s.handle(&[
        (sess, write(2, 0, 4, &[1, 2, 3], 0)),
        (sess, read(3, 0, 3, 5)),
    ]);
    assert_eq!(r[0], Reply::Send(Frame::Ok { req: 2 }));
    assert_eq!(blocks(&r[1]), vec![0, 1, 2, 3, 0]);
}

#[test]
fn writes_in_one_batch_share_a_flush() {
    let mut s = service();
    let (sess, _) = s.hello(&accepted(1, 1, &[0])).unwrap();
    // One write per 64 KiB cluster: a group ends at a write touching a cluster an earlier
    // write of the group touched.
    let items: Vec<_> = (0..32)
        .map(|i| (sess, write(10 + i, 0, (i * 16) as u32, &[i as u8], i)))
        .collect();
    let before = s.store().device().flushes();
    let r = s.handle(&items);
    assert!(r.iter().all(|r| matches!(r, Reply::Send(Frame::Ok { .. }))));
    assert_eq!(
        s.store().device().flushes() - before,
        1,
        "32 writes, one flush"
    );
}

#[test]
fn after_a_restart_the_fence_holds_and_whole_chunks_are_uncertain() {
    let mut s = service();
    let (sess, _) = s.hello(&accepted(5, 1, &[0, 2])).unwrap();
    s.handle(&[(sess, write(2, 0, 0, &[7], 0))]);
    let mut s = restart(s);
    // An older epoch is refused: the fence was durable.
    let (_, reply) = s.hello(&accepted(4, 1, &[0, 2])).unwrap();
    assert_eq!(reply, Frame::Stale { req: 1, fenced: 5 });
    // The current one learns that every block may hold an unfinished write.
    let (sess, reply) = s.hello(&accepted(5, 2, &[0, 2])).unwrap();
    assert_eq!(
        reply,
        Frame::Fenced {
            req: 1,
            uncertain: vec![
                (0, vec![Extent::new(0, BLOCKS)]),
                (2, vec![Extent::new(0, BLOCKS)])
            ]
        }
    );
    assert_eq!(blocks(&s.handle(&[(sess, read(3, 0, 0, 1))])[0]), vec![7]);
}

#[test]
fn a_fence_is_durable_even_before_any_write() {
    let mut s = service();
    s.hello(&accepted(9, 1, &[1])).unwrap();
    let mut s = restart(s);
    let (_, reply) = s.hello(&accepted(8, 1, &[1])).unwrap();
    assert_eq!(reply, Frame::Stale { req: 1, fenced: 9 });
}

#[test]
fn old_sessions_are_ignored() {
    let mut s = service();
    let (old, _) = s.hello(&accepted(1, 1, &[0])).unwrap();
    let (new, _) = s.hello(&accepted(1, 2, &[0])).unwrap();
    let r = s.handle(&[(old, write(2, 0, 0, &[1], 0)), (new, read(3, 0, 0, 1))]);
    assert_eq!(r[0], Reply::Nothing);
    assert_eq!(blocks(&r[1]), vec![0]);
}

#[test]
fn copies_skip_blocks_with_live_writes() {
    let mut s = service();
    let (sess, _) = s.hello(&accepted(1, 1, &[0])).unwrap();
    let copy = Frame::Copy {
        req: 4,
        chunk: 0,
        ext: Extent::new(0, 4),
        data: [5u8, 6, 7, 8]
            .iter()
            .flat_map(|&b| vec![b; BLOCK])
            .collect(),
    };
    let r = s.handle(&[
        (sess, Frame::BeginResync { req: 2, chunk: 0 }),
        (sess, write(3, 0, 1, &[1, 2], 0)),
        (sess, copy),
        (sess, read(5, 0, 0, 4)),
    ]);
    assert_eq!(r[2], Reply::Send(Frame::Ok { req: 4 }));
    assert_eq!(blocks(&r[3]), vec![5, 1, 2, 8]);
}

#[test]
fn a_volume_keeps_its_geometry() {
    let mut s = service();
    s.hello(&accepted(1, 1, &[0])).unwrap();
    let mut other = accepted(2, 1, &[0]);
    other.chunk_size = 2 * CHUNK;
    other.size = 4 * CHUNK;
    assert!(s.hello(&other).unwrap_err().contains("exists here"));
    let mut bad = accepted(2, 1, &[7]);
    bad.size = 4 * CHUNK;
    assert!(s.hello(&bad).unwrap_err().contains("beyond the volume"));
}

#[test]
fn a_hello_inside_a_session_is_refused() {
    let mut s = service();
    let (sess, hello) = s.hello(&accepted(1, 1, &[0])).unwrap();
    let r = s.handle(&[(sess, hello)]);
    assert!(matches!(r[0], Reply::Close(_)), "{r:?}");
}

#[test]
fn a_commit_in_flight_holds_the_requests_after_its_writes() {
    let mut s = deferred_service();
    let (sess, _) = s.hello(&accepted(1, 1, &[0])).unwrap();
    let items = [
        (sess, Frame::Heartbeat { req: 2 }),
        (sess, write(3, 0, 4, &[7, 8], 0)),
        (sess, Frame::Heartbeat { req: 4 }),
        (sess, read(5, 0, 4, 2)),
    ];
    let p = s.start(&items);
    assert!(p.in_flight());
    assert_eq!(
        p.replies.len(),
        1,
        "the heartbeat before the write is answered at once"
    );
    assert_eq!(p.handled, 3, "the read waits for the commit");
    assert!(s.poll(false).is_none());

    s.store().device().complete();
    let held = s.poll(false).expect("durable");
    assert_eq!(held.len(), 2);
    assert_eq!(held[0], Reply::Send(Frame::Ok { req: 3 }));
    assert!(!s.commit_in_flight());
    let p = s.start(&items[3..]);
    assert!(!p.in_flight());
    assert_eq!(blocks(&p.replies[0]), vec![7, 8]);
}

#[test]
fn a_hello_waits_for_the_commit_in_flight() {
    let mut s = deferred_service();
    let (sess, _) = s.hello(&accepted(1, 1, &[0])).unwrap();
    let p = s.start(&[(sess, write(2, 0, 0, &[5], 0))]);
    assert!(p.in_flight());
    // A new target's fence must not be durable before the writes ahead of it.
    s.hello(&accepted(2, 2, &[0])).unwrap();
    assert!(!s.store().device().in_flight());
    assert_eq!(
        s.poll(false).expect("kept for the caller"),
        vec![Reply::Send(Frame::Ok { req: 2 })]
    );
}

#[test]
fn handle_waits_for_its_commits() {
    let mut s = deferred_service();
    let (sess, _) = s.hello(&accepted(1, 1, &[0])).unwrap();
    let r = s.handle(&[
        (sess, write(2, 0, 4, &[1, 2, 3], 0)),
        (sess, read(3, 0, 3, 5)),
        (sess, write(4, 0, 0, &[9], 0)),
    ]);
    assert_eq!(r.len(), 3);
    assert_eq!(r[0], Reply::Send(Frame::Ok { req: 2 }));
    assert_eq!(blocks(&r[1]), vec![0, 1, 2, 3, 0]);
    assert_eq!(r[2], Reply::Send(Frame::Ok { req: 4 }));
    assert!(!s.commit_in_flight());
}

#[test]
fn a_deleted_volume_can_be_created_again_from_epoch_one() {
    let mut s = service();
    let (old, _) = s.hello(&accepted(2, 1, &[0, 1])).unwrap();
    let r = s.handle(&[(old, write(2, 0, 0, &[3], 1))]);
    assert_eq!(r, vec![Reply::Send(Frame::Ok { req: 2 })]);
    assert_eq!(s.delete_volume("pvc-1"), Ok(2));
    assert!(s.store().chunks().is_empty());
    assert_eq!(s.delete_volume("pvc-1"), Ok(0), "nothing left");

    // A new volume of the same name, from epoch 1: not refused by the old fence.
    let (new, reply) = s.hello(&accepted(1, 2, &[0])).unwrap();
    assert!(matches!(reply, Frame::Fenced { .. }), "{reply:?}");
    let r = s.handle(&[
        (new, write(3, 0, 0, &[4], 1)),
        (new, read(4, 0, 0, 2)),
        (old, write(5, 0, 0, &[5], 2)),
    ]);
    assert_eq!(r[0], Reply::Send(Frame::Ok { req: 3 }));
    assert_eq!(blocks(&r[1]), vec![4, 0], "the old data is gone");
    assert!(
        matches!(&r[2], Reply::Close(why) if why.contains("unknown volume")),
        "{:?}",
        r[2]
    );

    let s = restart(s);
    assert_eq!(s.store().chunks().len(), 1, "the deletion is durable");
}

#[test]
fn staged_writes_are_answered_after_their_own_commit() {
    let mut s = deferred_service();
    let (sess, _) = s.hello(&accepted(1, 1, &[0])).unwrap();
    let p = s.start(&[(sess, write(2, 0, 0, &[1], 1))]);
    assert!(p.in_flight());
    // Behind it: a whole-cluster write elsewhere is staged; one into its cluster is not, nor
    // what follows. A small write is not staged either.
    assert_eq!(s.stage(&[(sess, write(9, 0, 128, &[7], 9))]), 0);
    let rest = [
        (sess, write(3, 0, 64, &[2; 16], 2)),
        (sess, write(4, 0, 1, &[3], 3)),
        (sess, read(5, 0, 64, 1)),
    ];
    assert_eq!(s.stage(&rest), 1);
    assert!(s.poll(false).is_none());
    s.store().device().complete();
    assert_eq!(s.poll(false), Some(vec![Reply::Send(Frame::Ok { req: 2 })]));
    assert!(
        s.commit_in_flight(),
        "the staged write's commit is in flight"
    );
    assert!(s.poll(false).is_none());
    s.store().device().complete();
    assert_eq!(s.poll(false), Some(vec![Reply::Send(Frame::Ok { req: 3 })]));
    assert!(!s.commit_in_flight());
    let r = s.handle(&rest[1..]);
    assert_eq!(r[0], Reply::Send(Frame::Ok { req: 4 }));
    assert_eq!(blocks(&r[1]), vec![2], "the staged write reads back");
}
