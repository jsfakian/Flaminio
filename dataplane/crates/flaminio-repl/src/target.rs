//! The volume target: the per-volume I/O coordinator (protocol spec §4–§7).
//!
//! A target is created from the metadata service's reply to `Attach`, and lives until it is
//! deposed or its node goes away. It holds no durable state. The caller feeds it client
//! operations, replica replies, metadata replies and clock ticks, and collects [`Output`]s.
//!
//! Operations cover an extent of one chunk; the caller splits I/O at chunk boundaries. `P` is
//! the payload type (see [`crate::msg`]): the target only stores and forwards payloads.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Debug;

use crate::extent::{Extent, ExtentSet};
use crate::meta::{IsrChange, MetaReply, MetaRequest, VolumeMeta};
use crate::msg::{FromReplica, MAX_DIGEST_BLOCKS, MAX_MAPPED_BLOCKS, Request, Response, ToReplica};
use crate::types::{Block, ChunkIdx, Epoch, NodeId, Time, Timing, Version, VolumeId};

/// A client operation on one extent of one chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op<P> {
    Read {
        chunk: ChunkIdx,
        ext: Extent,
    },
    Write {
        chunk: ChunkIdx,
        ext: Extent,
        data: P,
    },
}

impl<P> Op<P> {
    fn key(&self) -> (ChunkIdx, Extent) {
        match *self {
            Op::Read { chunk, ext } | Op::Write { chunk, ext, .. } => (chunk, ext),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpResult<P> {
    Written,
    Read(P),
    /// The target was deposed. A write may or may not have taken effect.
    Failed,
    /// A read that no ISR member could serve: the one that failed it was the only member left
    /// (docs/issues/5).
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output<P> {
    Send { to: NodeId, msg: ToReplica<P> },
    Meta { req: u64, body: MetaRequest },
    Done { op: u64, result: OpResult<P> },
}

/// Deliberate protocol violations, for checking that the simulator catches them.
#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Bugs {
    /// Complete writes once a failed member's removal is proposed, not committed.
    pub ack_before_shrink: bool,
    /// Return reads without checking the lease.
    pub no_lease: bool,
    /// After the first fence, remove unfenced ISR members without waiting `L × (1 + ρ)`.
    pub no_takeover_wait: bool,
    /// Read from ISR members whose removal is in flight.
    pub read_from_removing: bool,
    /// Serve without reconciling uncertain extents.
    pub no_reconcile: bool,
    /// A joining replica counts for leases and writes only once its expansion is committed.
    pub late_join: bool,
    /// Reconciliation writes prune uncertain sets.
    pub prune_reconcile: bool,
    /// The dirty set of a removed member omits the writes in flight at its removal.
    pub no_dirty_inflight: bool,
    /// Settle uncertain sets as soon as reconciliation starts, not once it is done.
    pub early_settle: bool,
    /// A moving replica joins the ISR as it is added, without a resync.
    pub move_without_resync: bool,
    /// Drop a move's source once the destination is a replica, not once it is in the ISR
    /// (counting the destination in the ISR).
    pub drop_before_join: bool,
    /// Admit and complete fast reads without checking the lease.
    pub fast_read_no_lease: bool,
    /// Complete fast reads without checking that the local replica stayed in the ISR.
    pub fast_read_ignores_membership: bool,
    /// (Checked by the caller, which owns the replica.) Complete fast reads without checking
    /// that the local replica is still fenced at the read's epoch.
    pub fast_read_no_fence_check: bool,
    /// Trust digests from a single ISR member and skip repair (must be caught by the checker).
    pub trust_single_digest: bool,
    /// Not a bug: `pump` tries the queue in its order always, as before docs/issues/11, so that
    /// runs can be compared with it (S2). The order of the digests queued by one progress pass
    /// (S1) stays the new one.
    pub today_pump: bool,
    /// A chunk's writes in flight keep the acknowledgements of a replica its list drops, so a
    /// node added back counts as holding them (S11).
    pub keep_dropped_acks: bool,
}

/// What the target counted, for the simulator's reports (docs/issues/11).
#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    /// Pumps that took the waited path: a queued client operation waited on a chunk.
    pub waited_pumps: u64,
    /// Of those, the ones with a resync item queued.
    pub waited_pumps_with_resync: u64,
    /// Client operations submitted while their chunk was not `Ready`.
    pub waiting_submits: u64,
}

/// A read admitted by [`Target::try_fast_read`]: carried out by the caller on the local
/// replica, then handed back to [`Target::finish_fast_read`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FastRead {
    pub chunk: ChunkIdx,
    pub ext: Extent,
    /// The local replica's session: the read goes to the replica's rules on it (fencing).
    pub session: u64,
    /// The chunk's local-membership generation at admission (see `Chunk::local_gen`).
    member_gen: u64,
}

/// Something worth logging that the target did on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A session failed, and why.
    SessionFailed { node: NodeId, reason: &'static str },
}

/// How much of the target's state an event may have changed.
enum Scope {
    All,
    Chunk(usize),
    Queue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Waiting for the ISR to be fenced (spec §6 steps 1–2).
    Fencing,
    /// Copying the authority's data of each uncertain extent to the ISR (step 3).
    Reconciling {
        pending: ExtentSet,
    },
    Ready,
}

#[derive(Debug)]
struct Resync {
    member: NodeId,
    round: u64,
    /// The member's session that carried `BeginResync`.
    session: u64,
    /// `Mapped` replies still awaited from the source before `BeginResync` (follow-ups §3).
    awaiting_mapped: usize,
    /// What the source's `Mapped` replies so far report as holding data.
    mapped: ExtentSet,
    started: bool,
    /// Blocks not yet copied and acknowledged.
    todo: ExtentSet,
    /// Writes that landed on the ISR while Mapped was in flight: always copy these, even if
    /// the stale Mapped reply omitted them.
    force_copy: ExtentSet,
    /// Unmapped on the source: discard on the joiner so stale data does not survive.
    discard: ExtentSet,
    /// Blocks of live writes sent to the member since `BeginResync`.
    live: ExtentSet,
    /// Live writes sent to the member and not yet acknowledged by it, completed or not.
    unacked: BTreeSet<u64>,
}

#[derive(Debug)]
struct Chunk {
    replicas: Vec<NodeId>,
    isr: BTreeSet<NodeId>,
    isr_ver: u64,
    phase: Phase,
    /// Replicas this target has fenced.
    fenced: BTreeSet<NodeId>,
    /// Union of the ISR members' uncertain sets.
    uncertain: ExtentSet,
    /// When the first ISR member was fenced.
    first_fence: Option<Time>,
    /// Blocks with an operation in flight.
    locks: ExtentSet,
    /// ISR members whose removal is (to be) proposed.
    removing: BTreeSet<NodeId>,
    /// A resynced replica whose expansion is (to be) proposed. It counts for leases and
    /// writes from now on.
    joining: Option<NodeId>,
    resync: Option<Resync>,
    /// For members this target removed during I/O: blocks they may lack. Members without an
    /// entry need a full copy.
    dirty: BTreeMap<NodeId, ExtentSet>,
    /// A requested move of the replica on `.0` to `.1` (spec §7, moves).
    moving: Option<(NodeId, NodeId)>,
    /// The move's next step, to be proposed: add the destination, or drop the source.
    add_replica: Option<NodeId>,
    drop_replica: Option<NodeId>,
    /// Counts the local node's departures from the ISR. A fast read counts only if the local
    /// replica stayed in the ISR throughout: a replica dropped and added again in between holds
    /// different data (possibly a chunk recreated empty), though the lease may hold.
    local_gen: u64,
}

impl Chunk {
    fn required(&self) -> BTreeSet<NodeId> {
        let mut s = self.isr.clone();
        s.extend(self.joining);
        s
    }

    fn change_pending(&self) -> bool {
        !self.removing.is_empty()
            || self.joining.is_some()
            || self.add_replica.is_some()
            || self.drop_replica.is_some()
    }
}

#[derive(Debug)]
enum Work<P> {
    Client {
        op: u64,
        kind: Op<P>,
    },
    /// Compare digests across the ISR before repairing (follow-ups §2).
    Digest {
        chunk: ChunkIdx,
        ext: Extent,
    },
    Repair {
        chunk: ChunkIdx,
        ext: Extent,
    },
    Copy {
        chunk: ChunkIdx,
        round: u64,
        ext: Extent,
    },
    /// Discard unmapped holes on a joining replica (follow-ups §3).
    Discard {
        chunk: ChunkIdx,
        round: u64,
        ext: Extent,
    },
    /// Compare source vs joiner digests before copying (follow-ups §3).
    ResyncDigest {
        chunk: ChunkIdx,
        round: u64,
        ext: Extent,
    },
}

/// What kind of bulk work a queued item is, and whose (docs/issues/11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Client,
    /// A digest or a repair of the chunk.
    Reconcile(ChunkIdx),
    /// A copy, a discard or a resync digest of the chunk's resync round.
    Resync(ChunkIdx, u64),
}

impl<P> Work<P> {
    /// The one place that sorts queued items by kind.
    fn class(&self) -> Class {
        match *self {
            Work::Client { .. } => Class::Client,
            Work::Digest { chunk, .. } | Work::Repair { chunk, .. } => Class::Reconcile(chunk),
            Work::Copy { chunk, round, .. }
            | Work::Discard { chunk, round, .. }
            | Work::ResyncDigest { chunk, round, .. } => Class::Resync(chunk, round),
        }
    }
}

#[derive(Debug)]
struct DigestOp {
    chunk: ChunkIdx,
    ext: Extent,
    need: BTreeSet<NodeId>,
    got: BTreeMap<NodeId, Vec<Option<u32>>>,
    /// When set, shrink that resync round's `todo` to digest mismatches instead of repairing.
    resync: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
enum ReadFor {
    Client { op: u64 },
    Repair,
    Copy { round: u64 },
}

#[derive(Debug)]
struct ReadOp {
    chunk: ChunkIdx,
    ext: Extent,
    purpose: ReadFor,
    locked: bool,
}

#[derive(Debug)]
struct WriteOp<P> {
    chunk: ChunkIdx,
    ext: Extent,
    data: P,
    client: Option<u64>,
    sent: BTreeSet<NodeId>,
    acked: BTreeSet<NodeId>,
}

#[derive(Debug, Clone, Copy)]
enum ReqKind {
    Hello,
    Heartbeat,
    Write {
        seq: u64,
        chunk: ChunkIdx,
    },
    Read {
        id: u64,
    },
    BeginResync {
        chunk: ChunkIdx,
        round: u64,
    },
    Settle,
    Copy {
        chunk: ChunkIdx,
        round: u64,
        ext: Extent,
    },
    Digest {
        id: u64,
    },
    Mapped {
        chunk: ChunkIdx,
        round: u64,
    },
    Discard {
        chunk: ChunkIdx,
        round: u64,
        ext: Extent,
    },
}

#[derive(Debug)]
struct Session {
    id: u64,
    open: bool,
    inflight: BTreeMap<u64, (Time, ReqKind)>,
}

#[derive(Debug, Default)]
struct Peer {
    session: Option<Session>,
    retry_at: Time,
    /// The replica has confirmed our epoch until this time.
    confirmed_until: Time,
}

/// A volume target.
#[derive(Debug)]
pub struct Target<P> {
    node: NodeId,
    vol: VolumeId,
    epoch: Epoch,
    min_isr: usize,
    blocks_per_chunk: Block,
    /// Largest extent of one repair or copy.
    max_io: Block,
    /// Largest extent of one `Mapped` probe.
    max_mapped: Block,
    /// Largest extent of one `Digest` request.
    max_digest: Block,
    timing: Timing,
    chunks: Vec<Chunk>,
    peers: BTreeMap<NodeId, Peer>,
    next_session: u64,
    next_req: u64,
    next_seq: u64,
    next_read: u64,
    next_round: u64,
    next_digest: u64,
    /// Repairs and resync copies in flight, and the most allowed. Bulk work shares the
    /// sessions with client I/O and heartbeats, so it is bounded: unbounded, it would queue
    /// enough data ahead of them to make their requests time out.
    bulk: usize,
    max_bulk: usize,
    writes: BTreeMap<u64, WriteOp<P>>,
    reads: BTreeMap<u64, ReadOp>,
    digests: BTreeMap<u64, DigestOp>,
    queue: VecDeque<Work<P>>,
    /// Client reads that failed on a member without a reply and wait to be sent again, by
    /// operation: the member each must not go to next, and how many times in a row it failed
    /// there (docs/issues/5).
    avoid: BTreeMap<u64, (NodeId, u32)>,
    meta_inflight: Option<(u64, Time)>,
    next_heartbeat: Time,
    /// When to read the volume from the metadata service next, and the read in flight. It has
    /// its own slot: an ISR change must never wait behind it.
    next_refresh: Time,
    refresh_inflight: Option<(u64, Time)>,
    deposed: bool,
    out: Vec<Output<P>>,
    events: Vec<Event>,
    /// Chunks fenced in the current progress pass, and their digests, for
    /// [`Target::queue_newly_fenced`].
    newly_fenced: Vec<(usize, Vec<Extent>)>,
    /// Chunks not `Ready`. Phases only move forward, so it only falls.
    unready: usize,
    /// Whether the last slot given went to a waited chunk's item (S3).
    last_waited: bool,
    /// Fast reads admitted and not finished, for the accounting check.
    #[cfg(test)]
    fast_reads: Vec<(ChunkIdx, Extent)>,
    #[cfg(test)]
    pub stats: Stats,
    #[cfg(test)]
    pub bugs: Bugs,
}

/// Repairs and resync copies in flight per target, unless set otherwise.
pub const DEFAULT_MAX_BULK: usize = 4;

impl<P: Clone + Debug> Target<P> {
    /// Starts a target from the metadata service's `Attach` reply, and fences every replica.
    /// Repairs and copies move at most `max_io` blocks at a time.
    pub fn new(
        node: NodeId,
        vol: VolumeId,
        view: &VolumeMeta,
        timing: Timing,
        max_io: Block,
        now: Time,
    ) -> Self {
        let chunks: Vec<Chunk> = view
            .chunks
            .iter()
            .map(|m| Chunk {
                replicas: m.replicas.clone(),
                isr: m.isr.clone(),
                isr_ver: m.isr_ver,
                phase: Phase::Fencing,
                fenced: BTreeSet::new(),
                uncertain: ExtentSet::new(),
                first_fence: None,
                locks: ExtentSet::new(),
                removing: BTreeSet::new(),
                joining: None,
                resync: None,
                dirty: BTreeMap::new(),
                moving: m.moving,
                add_replica: None,
                drop_replica: None,
                local_gen: 0,
            })
            .collect();
        let peers = chunks
            .iter()
            .flat_map(|c| c.replicas.iter().map(|&n| (n, Peer::default())))
            .collect();
        let chunks_len = chunks.len();
        let mut t = Self {
            node,
            vol,
            epoch: view.epoch,
            min_isr: view.min_isr,
            blocks_per_chunk: view.blocks_per_chunk,
            max_io: max_io.max(1),
            max_mapped: MAX_MAPPED_BLOCKS,
            max_digest: MAX_DIGEST_BLOCKS,
            timing,
            chunks,
            peers,
            next_session: 1,
            next_req: 1,
            next_seq: 0,
            next_read: 0,
            next_round: 0,
            next_digest: 0,
            bulk: 0,
            max_bulk: DEFAULT_MAX_BULK,
            writes: BTreeMap::new(),
            reads: BTreeMap::new(),
            digests: BTreeMap::new(),
            queue: VecDeque::new(),
            avoid: BTreeMap::new(),
            meta_inflight: None,
            next_heartbeat: now,
            next_refresh: now + timing.refresh,
            refresh_inflight: None,
            deposed: false,
            out: Vec::new(),
            events: Vec::new(),
            newly_fenced: Vec::new(),
            unready: chunks_len,
            last_waited: false,
            #[cfg(test)]
            fast_reads: Vec::new(),
            #[cfg(test)]
            stats: Stats::default(),
            #[cfg(test)]
            bugs: Bugs::default(),
        };
        t.tick_inner(now);
        #[cfg(test)]
        t.check_accounting();
        t
    }

    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    /// The client operation that request `req` to node `to` reads for, if it is a client's
    /// read. The data plane serves such reads from the local replica straight into the
    /// client's buffer.
    pub fn client_read_of(&self, to: NodeId, req: u64) -> Option<u64> {
        let s = self.peers.get(&to)?.session.as_ref()?;
        let ReqKind::Read { id } = s.inflight.get(&req)?.1 else {
            return None;
        };
        match self.reads.get(&id)?.purpose {
            ReadFor::Client { op } => Some(op),
            _ => None,
        }
    }

    /// Client reads waiting to be sent again after a failed read, for the tests: each entry goes
    /// once its read is sent elsewhere, or ends.
    #[cfg(test)]
    pub fn avoid_len(&self) -> usize {
        self.avoid.len()
    }

    /// Sets how many repairs and resync copies may be in flight at once (at least one).
    pub fn set_max_bulk(&mut self, n: usize) {
        self.max_bulk = n.max(1);
    }

    /// Sets the largest extent one `Mapped` probe covers, from one block to
    /// [`MAX_MAPPED_BLOCKS`] (the default).
    pub fn set_max_mapped(&mut self, n: Block) {
        self.max_mapped = n.clamp(1, MAX_MAPPED_BLOCKS);
    }

    /// Sets the largest extent one `Digest` request covers, from one block to
    /// [`MAX_DIGEST_BLOCKS`] (the default).
    pub fn set_max_digest(&mut self, n: Block) {
        self.max_digest = n.clamp(1, MAX_DIGEST_BLOCKS);
    }

    fn release_bulk(&mut self) {
        debug_assert!(self.bulk > 0, "bulk accounting");
        self.bulk = self.bulk.saturating_sub(1);
    }

    pub fn is_deposed(&self) -> bool {
        self.deposed
    }

    /// True when every chunk is serving and fully replicated, and nothing is in flight.
    pub fn is_settled(&self) -> bool {
        !self.deposed
            && self.queue.is_empty()
            && self.writes.is_empty()
            && self.reads.is_empty()
            && self.digests.is_empty()
            && self.meta_inflight.is_none()
            && self.chunks.iter().all(|c| {
                c.phase == Phase::Ready
                    && c.isr.len() == c.replicas.len()
                    && !c.change_pending()
                    && c.moving.is_none()
            })
    }

    /// A one-line summary of the target's state, for failure reports.
    pub fn describe(&self) -> String {
        let chunks: Vec<String> = self
            .chunks
            .iter()
            .map(|c| {
                format!(
                    "{:?} replicas {:?} isr {:?} fenced {:?} removing {:?} joining {:?} moving {:?} resync {:?} locks {:?}",
                    c.phase,
                    c.replicas,
                    c.isr,
                    c.fenced,
                    c.removing,
                    c.joining,
                    c.moving,
                    c.resync.as_ref().map(|r| (r.member, r.started, &r.todo)),
                    c.locks
                )
            })
            .collect();
        let peers: Vec<String> = self
            .peers
            .iter()
            .map(|(n, p)| match &p.session {
                Some(s) => format!(
                    "{n}:s{}{}+{}",
                    s.id,
                    if s.open { "" } else { "(opening)" },
                    s.inflight.len()
                ),
                None => format!("{n}:none"),
            })
            .collect();
        format!(
            "queue {} writes {:?} reads {} meta {:?} peers [{}] chunks [{}]",
            self.queue.len(),
            self.writes
                .iter()
                .map(|(s, w)| (*s, w.chunk, w.ext, &w.acked))
                .collect::<Vec<_>>(),
            self.reads.len(),
            self.meta_inflight,
            peers.join(" "),
            chunks.join(" | ")
        )
    }

    /// Takes the events recorded so far, for logging.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Takes the outputs produced so far.
    pub fn take_output(&mut self) -> Vec<Output<P>> {
        std::mem::take(&mut self.out)
    }

    /// Admits a client read to be served by the local replica without going through the queue
    /// (spec §5, fast reads). It is admitted only when the queue would start it at once, by the
    /// same rules: the chunk is serving, the local replica is a usable ISR member (not being
    /// removed, its session open), the lease holds, no operation in flight holds an
    /// overlapping lock, and no client operation waits on an overlapping extent ahead of it.
    /// The read takes its blocks' lock until [`Target::finish_fast_read`].
    pub fn try_fast_read(&mut self, now: Time, chunk: ChunkIdx, ext: Extent) -> Option<FastRead> {
        let fr = self.try_fast_read_inner(now, chunk, ext);
        #[cfg(test)]
        {
            if fr.is_some() {
                self.fast_reads.push((chunk, ext));
            }
            self.check_accounting();
        }
        fr
    }

    fn try_fast_read_inner(&mut self, now: Time, chunk: ChunkIdx, ext: Extent) -> Option<FastRead> {
        let i = chunk as usize;
        let c = self.chunks.get(i)?;
        if self.deposed
            || ext.len == 0
            || ext.end() > self.blocks_per_chunk
            || c.phase != Phase::Ready
            || c.locks.overlaps(&ext)
            || self.source(i, None) != Some(self.node)
        {
            return None;
        }
        #[cfg(test)]
        let lease = self.bugs.fast_read_no_lease || self.lease_ok(i, now);
        #[cfg(not(test))]
        let lease = self.lease_ok(i, now);
        if !lease
            || self.queue.iter().any(|w| match w {
                Work::Client { kind, .. } => {
                    let (qc, qx) = kind.key();
                    qc == chunk && qx.overlaps(&ext)
                }
                _ => false,
            })
        {
            return None;
        }
        let session = self.session_of(self.node)?;
        let c = &mut self.chunks[i];
        c.locks.insert(ext);
        Some(FastRead {
            chunk,
            ext,
            session,
            member_gen: c.local_gen,
        })
    }

    /// Ends a fast read. Its data counts only if the local replica served it (`ok`), the lease
    /// still holds now, after the read, and the local replica stayed a usable ISR member on the
    /// same session throughout: then the read is linearized while all of that held. Otherwise
    /// the caller submits the read again, through the queue.
    pub fn finish_fast_read(&mut self, now: Time, fr: FastRead, ok: bool) -> bool {
        #[cfg(test)]
        if !self.deposed
            && let Some(k) = self
                .fast_reads
                .iter()
                .position(|&x| x == (fr.chunk, fr.ext))
        {
            self.fast_reads.remove(k);
        }
        let served = self.finish_fast_read_inner(now, fr, ok);
        #[cfg(test)]
        self.check_accounting();
        served
    }

    fn finish_fast_read_inner(&mut self, now: Time, fr: FastRead, ok: bool) -> bool {
        if self.deposed {
            return false;
        }
        let i = fr.chunk as usize;
        self.chunks[i].locks.remove(&fr.ext);
        #[cfg(test)]
        let lease = self.bugs.fast_read_no_lease || self.lease_ok(i, now);
        #[cfg(not(test))]
        let lease = self.lease_ok(i, now);
        let member = self.chunks[i].local_gen == fr.member_gen
            && self.source(i, None) == Some(self.node)
            && self.session_of(self.node) == Some(fr.session);
        #[cfg(test)]
        let member = member || self.bugs.fast_read_ignores_membership;
        // The lock's release may let queued work start.
        self.pump(now);
        ok && lease && member
    }

    /// Submits a client operation. Its result comes as an [`Output::Done`].
    pub fn submit(&mut self, now: Time, op: u64, kind: Op<P>) {
        self.submit_inner(now, op, kind);
        #[cfg(test)]
        self.check_accounting();
    }

    fn submit_inner(&mut self, now: Time, op: u64, kind: Op<P>) {
        if self.deposed {
            self.out.push(Output::Done {
                op,
                result: OpResult::Failed,
            });
            return;
        }
        #[cfg(test)]
        if self
            .chunks
            .get(kind.key().0 as usize)
            .is_some_and(|c| c.phase != Phase::Ready)
        {
            self.stats.waiting_submits += 1;
        }
        self.queue.push_back(Work::Client { op, kind });
        // A new operation changes no chunk's state: only queued work can start.
        self.pump(now);
    }

    /// The earliest local time after `now` at which [`Target::tick`] has something to do.
    pub fn next_wakeup(&self, now: Time) -> Option<Time> {
        if self.deposed {
            return None;
        }
        let wait = self.timing.takeover_wait();
        let requests = self.peers.values().filter_map(|p| match &p.session {
            Some(s) => s
                .inflight
                .values()
                .map(|(t, _)| t + self.timing.request_timeout)
                .min(),
            None => Some(p.retry_at),
        });
        let fences = self.chunks.iter().filter_map(|c| match c.phase {
            Phase::Fencing => c.first_fence.map(|t| t + wait),
            _ => None,
        });
        let meta = self
            .meta_inflight
            .iter()
            .chain(&self.refresh_inflight)
            .map(|(_, t)| t + self.timing.meta_timeout)
            .min();
        requests
            .chain(fences)
            .chain(meta)
            .chain([self.next_heartbeat, self.next_refresh])
            .filter(|&t| t > now)
            .min()
    }

    /// Handles timeouts, reconnects, heartbeats and waits that are due at `now`.
    pub fn tick(&mut self, now: Time) {
        self.tick_inner(now);
        #[cfg(test)]
        self.check_accounting();
    }

    fn tick_inner(&mut self, now: Time) {
        if self.deposed {
            return;
        }
        let timeout = self.timing.request_timeout;
        let expired: Vec<NodeId> = self
            .peers
            .iter()
            .filter(|(_, p)| {
                p.session
                    .as_ref()
                    .is_some_and(|s| s.inflight.values().any(|(t, _)| t + timeout <= now))
            })
            .map(|(&n, _)| n)
            .collect();
        for n in expired {
            self.events.push(Event::SessionFailed {
                node: n,
                reason: "a request timed out",
            });
            self.fail_peer(now, n);
        }
        let reconnect: Vec<NodeId> = self
            .peers
            .iter()
            .filter(|(_, p)| p.session.is_none() && p.retry_at <= now)
            .map(|(&n, _)| n)
            .collect();
        for n in reconnect {
            self.open_session(now, n);
        }
        if self.next_heartbeat <= now {
            self.next_heartbeat = now + self.timing.heartbeat;
            let open: Vec<NodeId> = self
                .peers
                .iter()
                .filter(|(_, p)| p.session.as_ref().is_some_and(|s| s.open))
                .map(|(&n, _)| n)
                .collect();
            for n in open {
                self.send(now, n, Request::Heartbeat, ReqKind::Heartbeat);
            }
        }
        if self
            .meta_inflight
            .is_some_and(|(_, t)| t + self.timing.meta_timeout <= now)
        {
            self.meta_inflight = None;
        }
        if self
            .refresh_inflight
            .is_some_and(|(_, t)| t + self.timing.meta_timeout <= now)
        {
            self.refresh_inflight = None;
        }
        self.progress(now);
        self.refresh(now);
    }

    /// Reads the volume: the reply goes through [`Target::on_meta`] like any other view. A
    /// view older than what this target knows changes nothing (versions only grow).
    fn refresh(&mut self, now: Time) {
        if self.deposed || self.refresh_inflight.is_some() || now < self.next_refresh {
            return;
        }
        self.next_refresh = now + self.timing.refresh;
        let req = self.next_req;
        self.next_req += 1;
        self.refresh_inflight = Some((req, now));
        self.out.push(Output::Meta {
            req,
            body: MetaRequest::Get { vol: self.vol },
        });
    }

    /// Handles a reply from replica node `from`.
    pub fn on_reply(&mut self, now: Time, from: NodeId, m: &FromReplica<P>) {
        self.on_reply_inner(now, from, m);
        #[cfg(test)]
        self.check_accounting();
    }

    fn on_reply_inner(&mut self, now: Time, from: NodeId, m: &FromReplica<P>) {
        if self.deposed || m.vol != self.vol || m.epoch != self.epoch {
            return;
        }
        let Some(peer) = self.peers.get_mut(&from) else {
            return;
        };
        let Some(session) = peer.session.as_mut().filter(|s| s.id == m.session) else {
            return;
        };
        let Some((sent_at, kind)) = session.inflight.remove(&m.req) else {
            return;
        };
        if let Response::Stale { .. } = m.body {
            self.depose();
            return;
        }
        if matches!(kind, ReqKind::Hello) {
            session.open = true;
        }
        peer.confirmed_until = peer.confirmed_until.max(sent_at + self.timing.lease);
        // How much to advance afterwards: the hot replies (reads, writes) touch at most one
        // chunk's state; everything else gets a full pass.
        let mut scope = Scope::All;
        match (kind, &m.body) {
            (ReqKind::Hello, Response::Fenced { uncertain }) => {
                self.on_fenced(now, from, uncertain);
                self.retransmit(now, from);
            }
            (ReqKind::Heartbeat | ReqKind::Settle, Response::Ok) => {}
            (ReqKind::Write { seq, chunk }, Response::Ok) => {
                if let Some(r) = self.chunks[chunk as usize]
                    .resync
                    .as_mut()
                    .filter(|r| r.member == from)
                {
                    r.unacked.remove(&seq);
                }
                if let Some(w) = self.writes.get_mut(&seq) {
                    w.acked.insert(from);
                    self.check_write(seq);
                }
                // A completed write may finish the chunk's reconciliation or unblock a join.
                scope = Scope::Chunk(chunk as usize);
            }
            (ReqKind::Read { id }, Response::Data(data)) => {
                let data = data.clone();
                self.on_read(now, id, data);
                scope = Scope::Queue;
            }
            (ReqKind::Digest { id }, Response::Digests(sums)) => {
                let sums = sums.clone();
                self.on_digest(now, from, (m.req, sent_at), id, sums);
                scope = Scope::All;
            }
            (ReqKind::Mapped { chunk, round }, Response::Mapped(exts)) => {
                self.on_mapped(now, chunk, round, exts);
            }
            (ReqKind::BeginResync { chunk, round }, Response::Ok) => {
                let (max_io, max_digest) = (self.max_io, self.max_digest);
                if let Some(r) = self.resync_mut(chunk, round) {
                    // Writes that landed after Mapped was applied, before this Ack.
                    let force = std::mem::take(&mut r.force_copy);
                    r.todo.extend(&force);
                    for e in force.iter() {
                        r.discard.remove(&e);
                    }
                    r.started = true;
                    let discards = r.discard.pieces(max_io);
                    let digests = r.todo.pieces(max_digest);
                    for ext in discards {
                        self.queue.push_back(Work::Discard { chunk, round, ext });
                    }
                    // Digests first: copy only blocks that still disagree with the source.
                    for ext in digests {
                        self.queue
                            .push_back(Work::ResyncDigest { chunk, round, ext });
                    }
                }
            }
            (ReqKind::Copy { chunk, round, ext }, Response::Ok) => {
                self.release_bulk();
                if let Some(r) = self.resync_mut(chunk, round) {
                    r.todo.remove(&ext);
                }
            }
            (ReqKind::Discard { chunk, round, ext }, Response::Ok) => {
                self.release_bulk();
                if let Some(r) = self.resync_mut(chunk, round) {
                    r.discard.remove(&ext);
                }
            }
            _ => {
                // A reply that does not answer its request: treat the session as broken. The
                // request goes back in flight first, so that the session's failure handles it
                // with the others.
                self.put_back(from, m.req, sent_at, kind);
                self.events.push(Event::SessionFailed {
                    node: from,
                    reason: "a reply did not answer its request",
                });
                self.fail_peer(now, from);
            }
        }
        match scope {
            Scope::All => self.progress(now),
            Scope::Chunk(i) => {
                if !self.deposed {
                    self.advance_chunk(now, i);
                    self.queue_newly_fenced();
                    self.send_meta(now);
                    self.pump(now);
                }
            }
            Scope::Queue => self.pump(now),
        }
    }

    /// The client read `req`, sent to `from` at `epoch` on `session`, failed there without a
    /// reply: a local read whose device read or checksums failed (docs/issues/5). It is sent again
    /// to another ISR member, and `from` stays a member, its session open. A report that does not
    /// match a client read in flight is ignored, as a stray reply is: then it returns false.
    pub fn on_read_failed(
        &mut self,
        now: Time,
        from: NodeId,
        epoch: Epoch,
        session: u64,
        req: u64,
    ) -> bool {
        let failed = self.on_read_failed_inner(now, from, epoch, session, req);
        #[cfg(test)]
        self.check_accounting();
        failed
    }

    fn on_read_failed_inner(
        &mut self,
        now: Time,
        from: NodeId,
        epoch: Epoch,
        session: u64,
        req: u64,
    ) -> bool {
        if self.deposed || epoch != self.epoch {
            return false;
        }
        let Some(op) = self.client_read_of(from, req) else {
            return false;
        };
        let Some(s) = self
            .peers
            .get_mut(&from)
            .and_then(|p| p.session.as_mut())
            .filter(|s| s.id == session)
        else {
            return false;
        };
        let Some((_, ReqKind::Read { id })) = s.inflight.remove(&req) else {
            unreachable!("client_read_of found a read");
        };
        let r = self.reads.remove(&id).expect("client_read_of found it");
        // Client reads take their blocks' lock, and no bulk slot.
        if r.locked {
            self.chunks[r.chunk as usize].locks.remove(&r.ext);
        }
        let failures = match self.avoid.get(&op) {
            Some(&(n, f)) if n == from => f + 1,
            _ => 1,
        };
        self.avoid.insert(op, (from, failures));
        // Ahead of what was queued after it, as when a session fails.
        self.queue.push_front(Work::Client {
            op,
            kind: Op::Read {
                chunk: r.chunk,
                ext: r.ext,
            },
        });
        self.pump(now);
        true
    }

    /// Handles the reply to metadata request `req`.
    pub fn on_meta(&mut self, now: Time, req: u64, reply: &MetaReply) {
        self.on_meta_inner(now, req, reply);
        #[cfg(test)]
        self.check_accounting();
    }

    fn on_meta_inner(&mut self, now: Time, req: u64, reply: &MetaReply) {
        if self.deposed {
            return;
        }
        if self.refresh_inflight.is_some_and(|(r, _)| r == req) {
            self.refresh_inflight = None;
        } else if self.meta_inflight.is_some_and(|(r, _)| r == req) {
            self.meta_inflight = None;
        } else {
            return;
        }
        match reply {
            MetaReply::Applied { view, .. } | MetaReply::Rejected { view, .. } => {
                if view.epoch > self.epoch {
                    self.depose();
                    return;
                }
                if view.epoch == self.epoch {
                    self.apply_view(now, view);
                }
            }
            MetaReply::NotFound { .. } => {
                self.depose();
                return;
            }
        }
        self.progress(now);
    }

    fn resync_mut(&mut self, chunk: ChunkIdx, round: u64) -> Option<&mut Resync> {
        self.chunks[chunk as usize]
            .resync
            .as_mut()
            .filter(|r| r.round == round)
    }

    fn depose(&mut self) {
        self.deposed = true;
        let mut failed: Vec<u64> = Vec::new();
        failed.extend(self.queue.iter().filter_map(|w| match w {
            Work::Client { op, .. } => Some(*op),
            _ => None,
        }));
        failed.extend(self.reads.values().filter_map(|r| match r.purpose {
            ReadFor::Client { op } => Some(op),
            _ => None,
        }));
        failed.extend(self.writes.values().filter_map(|w| w.client));
        for op in failed {
            self.out.push(Output::Done {
                op,
                result: OpResult::Failed,
            });
        }
        self.queue.clear();
        self.avoid.clear();
        self.reads.clear();
        self.writes.clear();
        self.digests.clear();
        self.peers.clear();
        self.meta_inflight = None;
        self.refresh_inflight = None;
        self.bulk = 0;
        // A deposed target starts nothing: no lock is held for it, and fast reads are refused.
        self.newly_fenced.clear();
        self.last_waited = false;
        for c in &mut self.chunks {
            c.locks.clear();
        }
        #[cfg(test)]
        self.fast_reads.clear();
    }

    /// Puts a request whose reply did not answer it back in flight on `to`'s session, before
    /// that session fails (`fail_peer`).
    fn put_back(&mut self, to: NodeId, req: u64, sent_at: Time, kind: ReqKind) {
        if let Some(s) = self.peers.get_mut(&to).and_then(|p| p.session.as_mut()) {
            s.inflight.insert(req, (sent_at, kind));
        }
    }

    fn open_session(&mut self, now: Time, to: NodeId) {
        let id = self.next_session;
        self.next_session += 1;
        let peer = self.peers.get_mut(&to).expect("known peer");
        peer.session = Some(Session {
            id,
            open: false,
            inflight: BTreeMap::new(),
        });
        self.send(now, to, Request::Hello, ReqKind::Hello);
    }

    /// Sends a request on the peer's session. Only `Hello` may go on a session that is not
    /// open yet. Returns false if there is no usable session.
    fn send(&mut self, now: Time, to: NodeId, body: Request<P>, kind: ReqKind) -> bool {
        let Some(s) = self.peers.get_mut(&to).and_then(|p| p.session.as_mut()) else {
            return false;
        };
        if !s.open && !matches!(kind, ReqKind::Hello) {
            return false;
        }
        let req = self.next_req;
        self.next_req += 1;
        s.inflight.insert(req, (now, kind));
        self.out.push(Output::Send {
            to,
            msg: ToReplica {
                vol: self.vol,
                epoch: self.epoch,
                session: s.id,
                req,
                body,
            },
        });
        true
    }

    fn session_of(&self, n: NodeId) -> Option<u64> {
        self.peers
            .get(&n)
            .and_then(|p| p.session.as_ref())
            .filter(|s| s.open)
            .map(|s| s.id)
    }

    /// A session failed: drop it, retry reads elsewhere, and handle the member's failure.
    fn fail_peer(&mut self, now: Time, n: NodeId) {
        let Some(peer) = self.peers.get_mut(&n) else {
            return;
        };
        let Some(s) = peer.session.take() else {
            return;
        };
        peer.retry_at = now + self.timing.reconnect;
        for (_, kind) in s.inflight.into_values().rev() {
            if let ReqKind::Copy { .. } | ReqKind::Discard { .. } = kind {
                self.release_bulk();
            }
            if let ReqKind::Mapped { chunk, round } = kind {
                // Probe failed: abort this resync round; drive_resync will try again.
                let abort = self.chunks[chunk as usize]
                    .resync
                    .as_ref()
                    .is_some_and(|r| r.round == round && r.awaiting_mapped > 0);
                if abort {
                    self.abort_resync(chunk as usize);
                }
            }
            if let ReqKind::Digest { id } = kind
                && let Some(d) = self.digests.remove(&id)
            {
                self.release_bulk();
                let work = match d.resync {
                    Some(round) => Work::ResyncDigest {
                        chunk: d.chunk,
                        round,
                        ext: d.ext,
                    },
                    None => Work::Digest {
                        chunk: d.chunk,
                        ext: d.ext,
                    },
                };
                self.queue.push_front(work);
            }
            if let ReqKind::Read { id } = kind
                && let Some(r) = self.reads.remove(&id)
            {
                if !matches!(r.purpose, ReadFor::Client { .. }) {
                    self.release_bulk();
                }
                if r.locked {
                    self.chunks[r.chunk as usize].locks.remove(&r.ext);
                }
                let (chunk, ext) = (r.chunk, r.ext);
                let work = match r.purpose {
                    ReadFor::Client { op } => Work::Client {
                        op,
                        kind: Op::Read { chunk, ext },
                    },
                    ReadFor::Repair => Work::Repair { chunk, ext },
                    ReadFor::Copy { round } => Work::Copy { chunk, round, ext },
                };
                self.queue.push_front(work);
            }
        }
        for i in 0..self.chunks.len() {
            if self.chunks[i]
                .resync
                .as_ref()
                .is_some_and(|r| r.member == n)
            {
                self.abort_resync(i);
            }
            if self.chunks[i].isr.contains(&n) {
                self.member_failed(i, n);
            }
        }
    }

    fn abort_resync(&mut self, i: usize) {
        let c = &mut self.chunks[i];
        if let Some(r) = c.resync.take()
            && let Some(d) = c.dirty.get_mut(&r.member)
        {
            d.extend(&r.live);
            d.extend(&r.todo);
        }
    }

    /// An ISR member failed. Remove it if the ISR stays at or above the minimum; otherwise
    /// writes wait for it to come back (spec §4 step 4).
    fn member_failed(&mut self, i: usize, n: NodeId) {
        let min = self.min_isr;
        let c = &mut self.chunks[i];
        if c.phase == Phase::Fencing || c.removing.contains(&n) {
            return;
        }
        if c.isr.len() - c.removing.len() > min {
            c.removing.insert(n);
        }
    }

    fn on_fenced(&mut self, now: Time, from: NodeId, uncertain: &[(ChunkIdx, Vec<Extent>)]) {
        for (i, c) in self.chunks.iter_mut().enumerate() {
            if !c.replicas.contains(&from) {
                continue;
            }
            c.fenced.insert(from);
            if c.phase == Phase::Fencing && c.isr.contains(&from) {
                c.first_fence.get_or_insert(now);
                if let Some((_, exts)) = uncertain.iter().find(|(ci, _)| *ci as usize == i) {
                    for x in exts {
                        if x.len > 0 && x.end() <= self.blocks_per_chunk {
                            c.uncertain.insert(*x);
                        }
                    }
                }
            }
        }
    }

    /// A session to `n` (re)opened: resend every in-flight write that `n` must hold and has
    /// not acknowledged (spec §4 step 4).
    fn retransmit(&mut self, now: Time, n: NodeId) {
        let seqs: Vec<u64> = self
            .writes
            .iter()
            .filter(|(_, w)| {
                !w.acked.contains(&n) && self.chunks[w.chunk as usize].required().contains(&n)
            })
            .map(|(&s, _)| s)
            .collect();
        for seq in seqs {
            self.send_write(now, seq, n);
        }
    }

    fn send_write(&mut self, now: Time, seq: u64, to: NodeId) {
        let w = &self.writes[&seq];
        let repair = w.client.is_none();
        #[cfg(test)]
        let repair = repair && !self.bugs.prune_reconcile;
        let acked_below = self.writes.keys().next().copied().unwrap_or(self.next_seq);
        let body = Request::Write {
            chunk: w.chunk,
            ext: w.ext,
            data: w.data.clone(),
            version: Version {
                epoch: self.epoch,
                seq,
            },
            prune: (!repair).then_some(Version {
                epoch: self.epoch,
                seq: acked_below,
            }),
        };
        let chunk = w.chunk;
        if self.send(now, to, body, ReqKind::Write { seq, chunk }) {
            self.writes.get_mut(&seq).expect("write").sent.insert(to);
            if let Some(r) = self.chunks[chunk as usize]
                .resync
                .as_mut()
                .filter(|r| r.member == to)
            {
                r.unacked.insert(seq);
            }
        }
    }

    fn start_write(
        &mut self,
        now: Time,
        chunk: ChunkIdx,
        ext: Extent,
        data: P,
        client: Option<u64>,
    ) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.writes.insert(
            seq,
            WriteOp {
                chunk,
                ext,
                data,
                client,
                sent: BTreeSet::new(),
                acked: BTreeSet::new(),
            },
        );
        let c = &mut self.chunks[chunk as usize];
        let mut targets = c.required();
        if let Some(r) = c.resync.as_mut() {
            if r.started {
                targets.insert(r.member);
                r.live.insert(ext);
            } else {
                // Before BeginResync Ack: Mapped (or the gap until Ack) may omit this write.
                r.force_copy.insert(ext);
            }
        }
        for (r, d) in c.dirty.iter_mut() {
            if !targets.contains(r) {
                d.insert(ext);
            }
        }
        for n in targets {
            self.send_write(now, seq, n);
        }
        self.check_write(seq);
    }

    fn check_write(&mut self, seq: u64) {
        let w = &self.writes[&seq];
        if !self.write_quorum(w.chunk as usize).is_subset(&w.acked) {
            return;
        }
        let w = self.writes.remove(&seq).expect("write");
        let c = &mut self.chunks[w.chunk as usize];
        c.locks.remove(&w.ext);
        match w.client {
            Some(op) => self.out.push(Output::Done {
                op,
                result: OpResult::Written,
            }),
            None => {
                if let Phase::Reconciling { pending } = &mut c.phase {
                    pending.remove(&w.ext);
                }
                self.release_bulk();
            }
        }
    }

    /// Source mapped extents for a resync: once every probe has answered, copy only those and
    /// discard the rest of the plan.
    fn on_mapped(&mut self, now: Time, chunk: ChunkIdx, round: u64, exts: &[Extent]) {
        let Some(r) = self.resync_mut(chunk, round) else {
            return;
        };
        if r.awaiting_mapped == 0 {
            return;
        }
        for e in exts {
            r.mapped.insert(*e);
        }
        r.awaiting_mapped -= 1;
        if r.awaiting_mapped > 0 {
            return;
        }
        let mapped = std::mem::take(&mut r.mapped);
        let planned = r.todo.clone();
        let force = r.force_copy.clone();
        let mut todo = planned.intersection(&mapped);
        todo.extend(&force);
        let mut holes = ExtentSet::new();
        for e in planned.iter() {
            for h in mapped.missing_from(&e) {
                holes.insert(h);
            }
        }
        for e in force.iter() {
            holes.remove(&e);
        }
        r.todo = todo;
        r.discard = holes;
        // Force-copies so far are in todo; clear so post-Mapped writes accumulate until Ack.
        r.force_copy.clear();
        let member = r.member;
        let body = Request::BeginResync { chunk };
        if !self.send(now, member, body, ReqKind::BeginResync { chunk, round }) {
            self.abort_resync(chunk as usize);
        }
    }

    /// Digests from one ISR member. When every member has answered, repair only the blocks
    /// where digests disagree (or skip repair entirely when they all match).
    /// `req` is the reply's request and when it was sent.
    fn on_digest(
        &mut self,
        now: Time,
        from: NodeId,
        req: (u64, Time),
        id: u64,
        sums: Vec<Option<u32>>,
    ) {
        let Some(d) = self.digests.get_mut(&id) else {
            return;
        };
        if !d.need.contains(&from) || sums.len() != d.ext.len as usize {
            // As in `on_reply`: back in flight, so that the session's failure sends it again.
            self.put_back(from, req.0, req.1, ReqKind::Digest { id });
            self.events.push(Event::SessionFailed {
                node: from,
                reason: "a digest reply did not answer its request",
            });
            self.fail_peer(now, from);
            return;
        }
        d.got.insert(from, sums);
        #[cfg(test)]
        let trust_one = self.bugs.trust_single_digest;
        #[cfg(not(test))]
        let trust_one = false;
        let ready = trust_one || d.got.len() == d.need.len();
        if !ready {
            return;
        }
        let d = self.digests.remove(&id).expect("digest");
        self.release_bulk();
        let mismatch = if trust_one && d.resync.is_none() {
            // Bug: treat the first reply as agreement and never repair.
            ExtentSet::new()
        } else {
            digest_mismatch(&d.ext, &d.got)
        };
        if let Some(round) = d.resync {
            let max_io = self.max_io;
            let pieces = mismatch.pieces(max_io);
            if let Some(r) = self.resync_mut(d.chunk, round) {
                r.todo.remove(&d.ext);
                for &ext in &pieces {
                    r.todo.insert(ext);
                }
            }
            for ext in pieces.into_iter().rev() {
                self.queue.push_front(Work::Copy {
                    chunk: d.chunk,
                    round,
                    ext,
                });
            }
            let _ = now;
            return;
        }
        let i = d.chunk as usize;
        if let Phase::Reconciling { pending } = &mut self.chunks[i].phase {
            // Agreed blocks need no copy; remove the whole piece, then re-queue mismatches.
            pending.remove(&d.ext);
            for ext in mismatch.pieces(self.max_io).into_iter().rev() {
                pending.insert(ext);
                self.queue.push_front(Work::Repair {
                    chunk: d.chunk,
                    ext,
                });
            }
        }
        let _ = now;
    }

    fn on_read(&mut self, now: Time, id: u64, data: P) {
        let Some(r) = self.reads.remove(&id) else {
            return;
        };
        let (chunk, ext) = (r.chunk, r.ext);
        match r.purpose {
            ReadFor::Client { op } => {
                self.chunks[chunk as usize].locks.remove(&ext);
                if self.lease_ok(chunk as usize, now) {
                    // Served, possibly by the member that failed it before (docs/issues/5).
                    self.avoid.remove(&op);
                    self.out.push(Output::Done {
                        op,
                        result: OpResult::Read(data),
                    });
                } else {
                    self.queue.push_front(Work::Client {
                        op,
                        kind: Op::Read { chunk, ext },
                    });
                }
            }
            // Keep the lock: the repair write takes it over.
            ReadFor::Repair => self.start_write(now, chunk, ext, data, None),
            ReadFor::Copy { round } => {
                if r.locked {
                    self.chunks[chunk as usize].locks.remove(&ext);
                }
                let sent = match self.resync_mut(chunk, round) {
                    Some(rs) => {
                        let (member, session) = (rs.member, rs.session);
                        self.session_of(member) == Some(session) && {
                            let body = Request::Copy { chunk, ext, data };
                            self.send(now, member, body, ReqKind::Copy { chunk, round, ext })
                        }
                    }
                    None => false,
                };
                if !sent {
                    self.release_bulk();
                }
            }
        }
    }

    /// Members whose acknowledgement completes a write: the ISR and a joining replica.
    fn write_quorum(&self, i: usize) -> BTreeSet<NodeId> {
        let c = &self.chunks[i];
        #[cfg(test)]
        {
            let mut r = if self.bugs.late_join {
                c.isr.clone()
            } else {
                c.required()
            };
            if self.bugs.ack_before_shrink {
                r.retain(|n| !c.removing.contains(n));
            }
            r
        }
        #[cfg(not(test))]
        c.required()
    }

    /// Every member that must confirm the lease has done so recently (spec §5).
    fn lease_ok(&self, i: usize, now: Time) -> bool {
        let c = &self.chunks[i];
        #[cfg(test)]
        {
            if self.bugs.no_lease {
                return true;
            }
            if self.bugs.late_join {
                return c
                    .isr
                    .iter()
                    .all(|n| self.peers.get(n).is_some_and(|p| p.confirmed_until > now));
            }
        }
        c.required()
            .iter()
            .all(|n| self.peers.get(n).is_some_and(|p| p.confirmed_until > now))
    }

    /// An ISR member to read from: this node if possible, else the lowest id.
    fn source(&self, i: usize, exclude: Option<NodeId>) -> Option<NodeId> {
        let c = &self.chunks[i];
        let usable = |n: &NodeId| {
            self.may_read_from(i, *n) && Some(*n) != exclude && self.session_of(*n).is_some()
        };
        if c.isr.contains(&self.node) && usable(&self.node) {
            return Some(self.node);
        }
        c.isr.iter().copied().find(|n| usable(n))
    }

    /// Whether chunk `i` has an ISR member other than `failed` that reads may go to, now or
    /// once its session opens: one whose removal is not in flight.
    fn has_other_source(&self, i: usize, failed: NodeId) -> bool {
        self.chunks[i]
            .isr
            .iter()
            .any(|&n| n != failed && self.may_read_from(i, n))
    }

    /// Whether ISR member `n` of chunk `i` may serve reads: not while its removal is in flight
    /// (§5, read sources).
    fn may_read_from(&self, i: usize, n: NodeId) -> bool {
        #[cfg(test)]
        if self.bugs.read_from_removing {
            return true;
        }
        !self.chunks[i].removing.contains(&n)
    }

    fn start_read(
        &mut self,
        now: Time,
        i: usize,
        ext: Extent,
        from: NodeId,
        purpose: ReadFor,
        lock: bool,
    ) {
        let id = self.next_read;
        self.next_read += 1;
        if lock {
            self.chunks[i].locks.insert(ext);
        }
        self.reads.insert(
            id,
            ReadOp {
                chunk: i as ChunkIdx,
                ext,
                purpose,
                locked: lock,
            },
        );
        let body = Request::Read {
            chunk: i as ChunkIdx,
            ext,
        };
        let sent = self.send(now, from, body, ReqKind::Read { id });
        debug_assert!(sent);
    }

    /// Tries to start one queued item. Returns false if it must wait.
    fn try_start(&mut self, now: Time, w: &Work<P>) -> bool {
        match w {
            Work::Client { op, kind } => {
                let op = *op;
                let (chunk, ext) = kind.key();
                let i = chunk as usize;
                let c = &self.chunks[i];
                if c.phase != Phase::Ready || c.locks.overlaps(&ext) {
                    return false;
                }
                match kind {
                    Op::Write { data, .. } => {
                        self.chunks[i].locks.insert(ext);
                        self.start_write(now, chunk, ext, data.clone(), Some(op));
                        true
                    }
                    Op::Read { .. } => {
                        if !self.lease_ok(i, now) {
                            return false;
                        }
                        // A read that failed on a member goes to another one. With no other
                        // member left to serve it, it goes back to that member once more, in
                        // case the failure passed, and ends as unreadable if it fails there
                        // again (docs/issues/5).
                        let avoid = self.avoid.get(&op).copied();
                        let other =
                            avoid.is_some_and(|(failed, _)| self.has_other_source(i, failed));
                        let exclude = match avoid {
                            Some((failed, n)) if n >= 2 || other => Some(failed),
                            _ => None,
                        };
                        let Some(from) = self.source(i, exclude) else {
                            if exclude.is_some() && !other {
                                self.avoid.remove(&op);
                                self.out.push(Output::Done {
                                    op,
                                    result: OpResult::Unreadable,
                                });
                                return true;
                            }
                            return false;
                        };
                        // Sent elsewhere, the read avoids the member that failed it no longer:
                        // if this one fails too, that member may serve it again.
                        if avoid.is_some_and(|(failed, _)| failed != from) {
                            self.avoid.remove(&op);
                        }
                        self.start_read(now, i, ext, from, ReadFor::Client { op }, true);
                        true
                    }
                }
            }
            &Work::Digest { chunk, ext } => {
                let i = chunk as usize;
                let c = &self.chunks[i];
                if !matches!(c.phase, Phase::Reconciling { .. }) || self.bulk >= self.max_bulk {
                    return false;
                }
                let need: BTreeSet<NodeId> = c.isr.iter().copied().collect();
                if need.is_empty() || need.iter().any(|&n| self.session_of(n).is_none()) {
                    return false;
                }
                let id = self.next_digest;
                self.next_digest += 1;
                self.bulk += 1;
                self.digests.insert(
                    id,
                    DigestOp {
                        chunk,
                        ext,
                        need: need.clone(),
                        got: BTreeMap::new(),
                        resync: None,
                    },
                );
                let body = Request::Digest { chunk, ext };
                for n in need {
                    if !self.send(now, n, body.clone(), ReqKind::Digest { id }) {
                        // Session vanished mid-send: fail the digest and retry later.
                        self.digests.remove(&id);
                        self.release_bulk();
                        return false;
                    }
                }
                true
            }
            &Work::Repair { chunk, ext } => {
                let i = chunk as usize;
                let c = &self.chunks[i];
                if !matches!(c.phase, Phase::Reconciling { .. })
                    || c.locks.overlaps(&ext)
                    || self.bulk >= self.max_bulk
                {
                    return false;
                }
                let Some(from) = self.source(i, None) else {
                    return false;
                };
                self.bulk += 1;
                self.start_read(now, i, ext, from, ReadFor::Repair, true);
                true
            }
            &Work::Copy { chunk, round, ext } => {
                // Copies take no lock: the replica's resync filter keeps them from
                // overwriting live writes, so a resync never stalls client I/O.
                let i = chunk as usize;
                let Some(r) = self.chunks[i].resync.as_ref().filter(|r| r.round == round) else {
                    return true; // resync aborted or finished: drop
                };
                let member = r.member;
                if self.bulk >= self.max_bulk {
                    return false;
                }
                let Some(from) = self.source(i, Some(member)) else {
                    return false;
                };
                self.bulk += 1;
                self.start_read(now, i, ext, from, ReadFor::Copy { round }, false);
                true
            }
            &Work::Discard { chunk, round, ext } => {
                let i = chunk as usize;
                let Some(r) = self.chunks[i].resync.as_ref().filter(|r| r.round == round) else {
                    return true;
                };
                let member = r.member;
                if self.bulk >= self.max_bulk || self.session_of(member).is_none() {
                    return false;
                }
                self.bulk += 1;
                let body = Request::Discard { chunk, ext };
                if !self.send(now, member, body, ReqKind::Discard { chunk, round, ext }) {
                    self.release_bulk();
                    return false;
                }
                true
            }
            &Work::ResyncDigest { chunk, round, ext } => {
                let i = chunk as usize;
                let Some(r) = self.chunks[i].resync.as_ref().filter(|r| r.round == round) else {
                    return true;
                };
                let member = r.member;
                if self.bulk >= self.max_bulk || self.session_of(member).is_none() {
                    return false;
                }
                let Some(from) = self.source(i, Some(member)) else {
                    return false;
                };
                if self.session_of(from).is_none() {
                    return false;
                }
                let id = self.next_digest;
                self.next_digest += 1;
                self.bulk += 1;
                let need: BTreeSet<NodeId> = [from, member].into_iter().collect();
                self.digests.insert(
                    id,
                    DigestOp {
                        chunk,
                        ext,
                        need: need.clone(),
                        got: BTreeMap::new(),
                        resync: Some(round),
                    },
                );
                let body = Request::Digest { chunk, ext };
                for n in need {
                    if !self.send(now, n, body.clone(), ReqKind::Digest { id }) {
                        self.digests.remove(&id);
                        self.release_bulk();
                        return false;
                    }
                }
                true
            }
        }
    }

    /// Queues the digests of the chunks fenced in this progress pass ahead of what was queued
    /// before, from the lowest chunk's first piece up (S1).
    fn queue_newly_fenced(&mut self) {
        for (i, pieces) in std::mem::take(&mut self.newly_fenced).into_iter().rev() {
            for ext in pieces.into_iter().rev() {
                self.queue.push_front(Work::Digest {
                    chunk: i as ChunkIdx,
                    ext,
                });
            }
        }
    }

    /// Starts whatever queued work can start. Client operations on overlapping extents start
    /// in submission order. While a queued client operation waits on a chunk that does not
    /// serve, the free slots go first to that chunk's reconciliation, with at most one resync
    /// item in flight beside it (docs/issues/11, S3); otherwise the queue is tried in its
    /// order.
    fn pump(&mut self, now: Time) {
        debug_assert!(self.newly_fenced.is_empty(), "digests left unqueued");
        if self.deposed {
            return;
        }
        #[cfg(test)]
        let in_order = self.unready == 0 || self.bugs.today_pump;
        #[cfg(not(test))]
        let in_order = self.unready == 0;
        if in_order {
            self.last_waited = false;
            self.pump_in_order(now);
            return;
        }
        // Pass 1: drop stale resync items, find the waited chunks in wait order, and note each
        // bulk item's place.
        let mut items: Vec<Option<Work<P>>> = std::mem::take(&mut self.queue)
            .into_iter()
            .map(Some)
            .collect();
        let mut waited_chunks: Vec<ChunkIdx> = Vec::new();
        let mut reconcile_all: Vec<(usize, ChunkIdx)> = Vec::new();
        let mut resync: Vec<usize> = Vec::new();
        for (k, item) in items.iter_mut().enumerate() {
            let Some(w) = item.as_ref() else {
                continue;
            };
            match w.class() {
                Class::Client => {
                    let Work::Client { kind, .. } = w else {
                        continue;
                    };
                    let chunk = kind.key().0;
                    let serves = self
                        .chunks
                        .get(chunk as usize)
                        .is_none_or(|c| c.phase == Phase::Ready);
                    if !serves && !waited_chunks.contains(&chunk) {
                        waited_chunks.push(chunk);
                    }
                }
                Class::Reconcile(chunk) => reconcile_all.push((k, chunk)),
                Class::Resync(chunk, round) => {
                    let live = self.chunks[chunk as usize]
                        .resync
                        .as_ref()
                        .is_some_and(|r| r.round == round);
                    if live {
                        resync.push(k);
                    } else {
                        // As `try_start` does: the round was aborted or finished.
                        *item = None;
                    }
                }
            }
        }
        if waited_chunks.is_empty() {
            self.last_waited = false;
            self.queue = items.into_iter().flatten().collect();
            self.pump_in_order(now);
            return;
        }
        #[cfg(test)]
        {
            self.stats.waited_pumps += 1;
            if !resync.is_empty() {
                self.stats.waited_pumps_with_resync += 1;
            }
        }
        let is_waited = |c: ChunkIdx| waited_chunks.contains(&c);
        let mut waited: Vec<usize> = Vec::new();
        for &c in &waited_chunks {
            waited.extend(
                reconcile_all
                    .iter()
                    .filter(|&&(_, rc)| rc == c)
                    .map(|&(k, _)| k),
            );
        }
        let reconcile: Vec<usize> = reconcile_all
            .iter()
            .filter(|&&(_, c)| !is_waited(c))
            .map(|&(k, _)| k)
            .collect();
        // What is in flight, from the maps.
        let mut resync_in_flight = self.digests.values().filter(|d| d.resync.is_some()).count()
            + self
                .reads
                .values()
                .filter(|r| matches!(r.purpose, ReadFor::Copy { .. }))
                .count()
            + self
                .peers
                .values()
                .filter_map(|p| p.session.as_ref())
                .flat_map(|s| s.inflight.values())
                .filter(|(_, k)| matches!(k, ReqKind::Copy { .. } | ReqKind::Discard { .. }))
                .count();
        let mut waited_in_flight = self
            .digests
            .values()
            .filter(|d| d.resync.is_none() && is_waited(d.chunk))
            .count()
            + self
                .reads
                .values()
                .filter(|r| matches!(r.purpose, ReadFor::Repair) && is_waited(r.chunk))
                .count()
            + self
                .writes
                .values()
                .filter(|w| w.client.is_none() && is_waited(w.chunk))
                .count();
        #[cfg(test)]
        let before = self.client_view();
        // The selection (S3): one start per round, while a slot is free.
        let (mut wc, mut rc, mut cc) = (0, 0, 0);
        while self.bulk < self.max_bulk {
            if resync_in_flight == 0
                && self.last_waited
                && self.start_first(now, &mut items, &resync, &mut rc)
            {
                resync_in_flight += 1;
                self.last_waited = false;
                continue;
            }
            if self.start_first(now, &mut items, &waited, &mut wc) {
                waited_in_flight += 1;
                self.last_waited = true;
                continue;
            }
            let started = if waited_in_flight > 0 {
                if resync_in_flight == 0 && self.start_first(now, &mut items, &resync, &mut rc) {
                    resync_in_flight += 1;
                    true
                } else {
                    self.start_first(now, &mut items, &reconcile, &mut cc)
                }
            } else {
                match self.start_merged(now, &mut items, (&resync, &mut rc), (&reconcile, &mut cc))
                {
                    Some(true) => {
                        resync_in_flight += 1;
                        true
                    }
                    Some(false) => true,
                    None => false,
                }
            };
            if !started {
                break;
            }
            self.last_waited = false;
        }
        #[cfg(test)]
        assert!(
            before == self.client_view(),
            "bulk starts changed what client operations depend on"
        );
        // Pass 2: every client operation, in queue order, as `pump_in_order` tries them.
        debug_assert!(self.queue.is_empty(), "the selection queued work");
        let mut blocked: BTreeMap<ChunkIdx, ExtentSet> = BTreeMap::new();
        let mut keep = VecDeque::new();
        for w in items.into_iter().flatten() {
            if self.deposed {
                return;
            }
            let Work::Client { kind, .. } = &w else {
                keep.push_back(w);
                continue;
            };
            let (c, x) = kind.key();
            let behind = blocked.get(&c).is_some_and(|b| b.overlaps(&x));
            if behind || !self.try_start(now, &w) {
                blocked.entry(c).or_default().insert(x);
                keep.push_back(w);
            }
        }
        self.queue = keep;
    }

    /// Starts the first item of `list` from `cursor` on that starts, and moves the cursor past
    /// every item tried: an item that does not start now cannot start later in the same pump.
    fn start_first(
        &mut self,
        now: Time,
        items: &mut [Option<Work<P>>],
        list: &[usize],
        cursor: &mut usize,
    ) -> bool {
        while let Some(&k) = list.get(*cursor) {
            *cursor += 1;
            if self.try_item(now, items, k) {
                return true;
            }
        }
        false
    }

    /// Starts the first item that starts of two lists merged by queue index. Returns whether
    /// it was one of the first list's, or `None` if nothing started.
    fn start_merged(
        &mut self,
        now: Time,
        items: &mut [Option<Work<P>>],
        (a, ac): (&[usize], &mut usize),
        (b, bc): (&[usize], &mut usize),
    ) -> Option<bool> {
        loop {
            let first = match (a.get(*ac), b.get(*bc)) {
                (None, None) => return None,
                (Some(x), Some(y)) => x < y,
                (x, _) => x.is_some(),
            };
            let k = if first {
                *ac += 1;
                a[*ac - 1]
            } else {
                *bc += 1;
                b[*bc - 1]
            };
            if self.try_item(now, items, k) {
                return Some(first);
            }
        }
    }

    /// Tries the queued item at index `k`; once started, it leaves the queue.
    fn try_item(&mut self, now: Time, items: &mut [Option<Work<P>>], k: usize) -> bool {
        let Some(w) = items[k].take() else {
            return false;
        };
        if self.try_start(now, &w) {
            return true;
        }
        items[k] = Some(w);
        false
    }

    /// What client operations depend on, which bulk starts must leave as it is: the locks of
    /// every serving chunk, and the open sessions.
    #[cfg(test)]
    fn client_view(&self) -> (Vec<Option<ExtentSet>>, Vec<(NodeId, u64)>) {
        let locks = self
            .chunks
            .iter()
            .map(|c| (c.phase == Phase::Ready).then(|| c.locks.clone()))
            .collect();
        let open = self
            .peers
            .iter()
            .filter_map(|(&n, p)| p.session.as_ref().filter(|s| s.open).map(|s| (n, s.id)))
            .collect();
        (locks, open)
    }

    /// Tries the queue in its order: the pump before docs/issues/11, unchanged.
    fn pump_in_order(&mut self, now: Time) {
        let mut blocked: BTreeMap<ChunkIdx, ExtentSet> = BTreeMap::new();
        let mut keep = VecDeque::new();
        while let Some(w) = self.queue.pop_front() {
            if self.deposed {
                return;
            }
            let client = match &w {
                Work::Client { kind, .. } => Some(kind.key()),
                _ => None,
            };
            let behind =
                client.is_some_and(|(c, x)| blocked.get(&c).is_some_and(|b| b.overlaps(&x)));
            if behind || !self.try_start(now, &w) {
                if let Some((c, x)) = client {
                    blocked.entry(c).or_default().insert(x);
                }
                keep.push_back(w);
            }
        }
        self.queue = keep;
    }

    /// Advances fencing, reconciliation, resync, ISR changes and queued work.
    fn progress(&mut self, now: Time) {
        if self.deposed {
            return;
        }
        for i in 0..self.chunks.len() {
            self.advance_chunk(now, i);
        }
        self.queue_newly_fenced();
        self.send_meta(now);
        self.pump(now);
    }

    /// Advances one chunk: failed members, fencing, reconciliation, resync.
    fn advance_chunk(&mut self, now: Time, i: usize) {
        let wait = self.timing.takeover_wait();
        {
            // ISR members without a session have failed.
            if self.chunks[i].phase != Phase::Fencing
                && self.chunks[i]
                    .isr
                    .iter()
                    .any(|&n| self.session_of(n).is_none())
            {
                let lost: Vec<NodeId> = self.chunks[i]
                    .isr
                    .iter()
                    .copied()
                    .filter(|&n| self.session_of(n).is_none())
                    .collect();
                for n in lost {
                    self.member_failed(i, n);
                }
            }
            let min = self.min_isr;
            let c = &mut self.chunks[i];
            match &c.phase {
                Phase::Fencing => {
                    if c.isr.is_subset(&c.fenced) {
                        let pending = c.uncertain.clone();
                        #[cfg(test)]
                        let pending = if self.bugs.no_reconcile {
                            ExtentSet::new()
                        } else {
                            pending
                        };
                        // Digests first, then repairs only where ISR members disagree. Queued
                        // once the progress pass is over (`queue_newly_fenced`).
                        let pieces = pending.pieces(self.max_digest);
                        if !pieces.is_empty() {
                            self.newly_fenced.push((i, pieces));
                        }
                        c.phase = Phase::Reconciling { pending };
                        #[cfg(test)]
                        if self.bugs.early_settle {
                            self.settle(now, i);
                        }
                    } else if let Some(t) = c.first_fence {
                        #[cfg(test)]
                        let t = if self.bugs.no_takeover_wait { 0 } else { t };
                        let fenced = c.isr.intersection(&c.fenced).count();
                        if t + wait <= now && fenced >= min {
                            let unfenced: Vec<NodeId> =
                                c.isr.difference(&c.fenced).copied().collect();
                            c.removing.extend(unfenced);
                        }
                    }
                }
                Phase::Reconciling { .. } | Phase::Ready => {}
            }
            let c = &mut self.chunks[i];
            if matches!(&c.phase, Phase::Reconciling { pending } if pending.is_empty()) {
                c.phase = Phase::Ready;
                self.unready -= 1;
                // The ISR agrees on everything older than this epoch now.
                #[cfg(test)]
                let settle = !self.bugs.early_settle;
                #[cfg(not(test))]
                let settle = true;
                if settle {
                    self.settle(now, i);
                }
            }
            let c = &self.chunks[i];
            if c.phase == Phase::Ready {
                self.drive_resync(now, i);
                self.drive_move(i);
            }
        }
    }

    /// The next step of a requested move: add the destination as a replica outside the ISR
    /// (it is then resynced and joins like any replica out of the ISR); once it is in the ISR,
    /// drop the source. [`Target::send_meta`] checks each step against the current state.
    fn drive_move(&mut self, i: usize) {
        #[cfg(test)]
        let early = self.bugs.drop_before_join;
        #[cfg(not(test))]
        let early = false;
        let c = &mut self.chunks[i];
        let Some((from, to)) = c.moving else {
            return;
        };
        if c.add_replica.is_some() || c.drop_replica.is_some() || !c.replicas.contains(&from) {
            return;
        }
        if !c.replicas.contains(&to) {
            c.add_replica = Some(to);
        } else if (c.isr.contains(&to) || early)
            && c.joining.is_none()
            && c.resync.as_ref().is_none_or(|r| r.member != from)
        {
            c.drop_replica = Some(from);
        }
    }

    /// Starts a resync of an out-of-ISR replica, or proposes the expansion once it is done.
    fn drive_resync(&mut self, now: Time, i: usize) {
        let c = &self.chunks[i];
        if c.joining.is_some() {
            return;
        }
        if let Some(r) = &c.resync {
            let member = r.member;
            // Every write in flight on the chunk must be on the joiner already.
            let unacked = self
                .writes
                .values()
                .any(|w| w.chunk as usize == i && !w.acked.contains(&member));
            if r.started
                && r.todo.is_empty()
                && r.discard.is_empty()
                && r.unacked.is_empty()
                && !unacked
            {
                let c = &mut self.chunks[i];
                c.resync = None;
                c.joining = Some(member);
            }
            return;
        }
        // A move's source out of the ISR is not worth a resync once the destination is a
        // replica: it is about to go.
        let leaving = c
            .moving
            .filter(|(_, to)| c.replicas.contains(to))
            .map(|(from, _)| from);
        let Some(member) =
            c.replicas.iter().copied().find(|n| {
                !c.isr.contains(n) && Some(*n) != leaving && self.session_of(*n).is_some()
            })
        else {
            return;
        };
        let Some(from) = self.source(i, Some(member)) else {
            return;
        };
        let session = self.session_of(member).expect("open");
        let round = self.next_round;
        self.next_round += 1;
        let todo = match c.dirty.get(&member) {
            Some(d) => d.clone(),
            None => ExtentSet::of(Extent::new(0, self.blocks_per_chunk)),
        };
        // Ask the source what holds data; copy only that, discard the rest (follow-ups §3).
        // A chunk larger than one probe is asked in pieces.
        let probes = ExtentSet::of(Extent::new(0, self.blocks_per_chunk)).pieces(self.max_mapped);
        self.chunks[i].resync = Some(Resync {
            member,
            round,
            session,
            awaiting_mapped: probes.len(),
            mapped: ExtentSet::new(),
            started: false,
            todo,
            force_copy: ExtentSet::new(),
            discard: ExtentSet::new(),
            live: ExtentSet::new(),
            unacked: BTreeSet::new(),
        });
        let chunk = i as ChunkIdx;
        for ext in probes {
            if !self.send(
                now,
                from,
                Request::Mapped { chunk, ext },
                ReqKind::Mapped { chunk, round },
            ) {
                self.abort_resync(i);
                return;
            }
        }
        // Writes already in flight are not sent to the member. Every copy of their blocks
        // reads its source after them, over the same ordered session, so it carries them;
        // and the member joins only once they have completed.
    }

    /// Proposes the pending ISR changes, one request at a time.
    fn send_meta(&mut self, now: Time) {
        if self.meta_inflight.is_some() {
            return;
        }
        // A move's step goes only while it still applies: the destination not yet a replica;
        // or the destination in the ISR, staying, and the ISR minimum kept without the source.
        let min = self.min_isr;
        #[cfg(test)]
        let early = self.bugs.drop_before_join;
        #[cfg(not(test))]
        let early = false;
        for c in &mut self.chunks {
            let Some((from, to)) = c.moving else {
                c.add_replica = None;
                c.drop_replica = None;
                continue;
            };
            if c.add_replica.is_some() && c.replicas.contains(&to) {
                c.add_replica = None;
            }
            if c.drop_replica.is_some() {
                let after = c
                    .isr
                    .iter()
                    .filter(|n| !c.removing.contains(n) && **n != from)
                    .count()
                    + usize::from(early && !c.isr.contains(&to));
                let joined = (c.isr.contains(&to) || early) && !c.removing.contains(&to);
                if !c.replicas.contains(&from) || !joined || after < min {
                    c.drop_replica = None;
                }
            }
        }
        if !self.chunks.iter().any(Chunk::change_pending) {
            return;
        }
        #[cfg(test)]
        let skip_resync = self.bugs.move_without_resync;
        #[cfg(not(test))]
        let skip_resync = false;
        let changes: Vec<IsrChange> = self
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| c.change_pending())
            .map(|(i, c)| {
                let mut new_isr: BTreeSet<NodeId> =
                    c.isr.difference(&c.removing).copied().collect();
                new_isr.extend(c.joining);
                let new_replicas = if let Some(to) = c.add_replica {
                    if skip_resync {
                        new_isr.insert(to);
                    }
                    let mut r = c.replicas.clone();
                    r.push(to);
                    Some(r)
                } else if let Some(from) = c.drop_replica {
                    new_isr.remove(&from);
                    if early && let Some((_, to)) = c.moving {
                        new_isr.insert(to);
                    }
                    Some(c.replicas.iter().copied().filter(|&n| n != from).collect())
                } else {
                    None
                };
                IsrChange {
                    chunk: i as ChunkIdx,
                    expected_ver: c.isr_ver,
                    new_isr,
                    new_replicas,
                }
            })
            .collect();
        if changes.is_empty() {
            return;
        }
        let req = self.next_req;
        self.next_req += 1;
        self.meta_inflight = Some((req, now));
        self.out.push(Output::Meta {
            req,
            body: MetaRequest::ChangeIsr {
                vol: self.vol,
                epoch: self.epoch,
                changes,
            },
        });
    }

    /// Adopts the metadata service's view of our own epoch: our changes that were applied.
    /// The oldest write still in flight on chunk `i`: everything below it has completed.
    fn settled_below(&self, i: usize) -> Version {
        let seq = self
            .writes
            .iter()
            .find(|(_, w)| w.chunk as usize == i)
            .map_or(self.next_seq, |(&s, _)| s);
        Version {
            epoch: self.epoch,
            seq,
        }
    }

    /// Tells the chunk's ISR members that the entries below its oldest write in flight are
    /// settled.
    fn settle(&mut self, now: Time, i: usize) {
        let below = self.settled_below(i);
        let members: Vec<NodeId> = self.chunks[i].isr.iter().copied().collect();
        for n in members {
            self.send(
                now,
                n,
                Request::Settle {
                    chunk: i as ChunkIdx,
                    below,
                },
                ReqKind::Settle,
            );
        }
    }

    fn apply_view(&mut self, now: Time, view: &VolumeMeta) {
        for (i, m) in view.chunks.iter().enumerate() {
            let c = &mut self.chunks[i];
            // Requested moves change no version: any view at least as new as ours tells.
            if m.isr_ver >= c.isr_ver {
                c.moving = m.moving;
            }
            if m.isr_ver <= c.isr_ver {
                continue;
            }
            let removed: Vec<NodeId> = c.isr.difference(&m.isr).copied().collect();
            let during_io = c.phase != Phase::Fencing;
            // Digest-first reconciliation may shrink the ISR before every member has been
            // compared: an empty inflight set must not mean "already in sync".
            let reconciling = matches!(c.phase, Phase::Reconciling { .. });
            let blocks = self.blocks_per_chunk;
            for r in removed {
                let mut d = ExtentSet::new();
                for w in self.writes.values() {
                    if w.chunk as usize == i && !w.acked.contains(&r) {
                        d.insert(w.ext);
                    }
                }
                if reconciling {
                    d = ExtentSet::of(Extent::new(0, blocks));
                }
                #[cfg(test)]
                if self.bugs.no_dirty_inflight {
                    d.clear();
                }
                let c = &mut self.chunks[i];
                c.removing.remove(&r);
                if during_io {
                    c.dirty.insert(r, std::mem::take(&mut d));
                }
            }
            let c = &mut self.chunks[i];
            let mut joined = None;
            if let Some(j) = c.joining
                && m.isr.contains(&j)
            {
                c.joining = None;
                c.dirty.remove(&j);
                joined = Some(j);
            }
            if c.isr.contains(&self.node) && !m.isr.contains(&self.node) {
                c.local_gen += 1;
            }
            c.isr = m.isr.clone();
            c.isr_ver = m.isr_ver;
            let isr = c.isr.clone();
            c.removing.retain(|n| isr.contains(n));
            // A move's drop may have shrunk the ISR under removals decided against the larger
            // one: keep only as many as the minimum allows. Writes wait for the others, as
            // for any member that cannot be removed (spec §4); they are proposed again if
            // still failed once that is possible.
            while c.isr.len() - c.removing.len() < self.min_isr.min(c.isr.len()) {
                let last = *c
                    .removing
                    .iter()
                    .next_back()
                    .expect("removing is not empty");
                c.removing.remove(&last);
            }
            if m.replicas != c.replicas {
                self.adopt_replicas(now, i, &m.replicas);
            }
            if let Some(j) = joined {
                // The joiner holds what the ISR holds: its older entries are settled too.
                let below = self.settled_below(i);
                self.send(
                    now,
                    j,
                    Request::Settle {
                        chunk: i as ChunkIdx,
                        below,
                    },
                    ReqKind::Settle,
                );
            }
        }
        // Nodes that hold none of this volume's chunks any more: their sessions end.
        let gone: Vec<NodeId> = self
            .peers
            .keys()
            .copied()
            .filter(|n| !self.chunks.iter().any(|c| c.replicas.contains(n)))
            .collect();
        for n in gone {
            self.fail_peer(now, n);
            self.peers.remove(&n);
        }
        let seqs: Vec<u64> = self.writes.keys().copied().collect();
        for seq in seqs {
            self.check_write(seq);
        }
    }

    /// A move's step committed: chunk `i`'s replica set is now `replicas`.
    fn adopt_replicas(&mut self, now: Time, i: usize, replicas: &[NodeId]) {
        let c = &mut self.chunks[i];
        let added: Vec<NodeId> = replicas
            .iter()
            .copied()
            .filter(|n| !c.replicas.contains(n))
            .collect();
        let removed: Vec<NodeId> = c
            .replicas
            .iter()
            .copied()
            .filter(|n| !replicas.contains(n))
            .collect();
        #[cfg(test)]
        let forget = !self.bugs.keep_dropped_acks;
        #[cfg(not(test))]
        let forget = true;
        for r in &removed {
            // A write counts as on a replica only while it exists: a node added back is a new
            // replica, which holds the write once it acknowledges it or its resync copies it
            // (S11).
            if forget {
                for w in self.writes.values_mut().filter(|w| w.chunk as usize == i) {
                    w.acked.remove(r);
                    w.sent.remove(r);
                }
            }
            c.dirty.remove(r);
            c.removing.remove(r);
            if c.resync.as_ref().is_some_and(|x| x.member == *r) {
                c.resync = None;
            }
        }
        c.replicas = replicas.to_vec();
        c.add_replica = None;
        c.drop_replica = None;
        for n in added {
            if let std::collections::btree_map::Entry::Vacant(e) = self.peers.entry(n) {
                e.insert(Peer::default());
                self.open_session(now, n);
            }
        }
    }
}

#[cfg(test)]
impl<P: Clone + Debug> Target<P> {
    /// Deposes the target, then checks it, as the `pub` methods do.
    pub fn depose_checked(&mut self) {
        self.depose();
        self.check_accounting();
    }

    /// Checks that the slots, the requests in flight and the locks agree with the work in
    /// flight (docs/issues/11). Run after every `pub` call that changes state.
    pub fn check_accounting(&self) {
        let requests = || {
            self.peers.iter().flat_map(|(&n, p)| {
                p.session
                    .iter()
                    .flat_map(move |s| s.inflight.values().map(move |&(_, k)| (n, k)))
            })
        };
        let copies = requests()
            .filter(|(_, k)| matches!(k, ReqKind::Copy { .. } | ReqKind::Discard { .. }))
            .count();
        let bulk = self.digests.len()
            + self
                .reads
                .values()
                .filter(|r| !matches!(r.purpose, ReadFor::Client { .. }))
                .count()
            + self.writes.values().filter(|w| w.client.is_none()).count()
            + copies;
        assert_eq!(self.bulk, bulk, "bulk slots: {}", self.describe());
        for (&id, d) in &self.digests {
            for n in d.need.iter().filter(|n| !d.got.contains_key(n)) {
                assert!(
                    requests()
                        .any(|(m, k)| m == *n && matches!(k, ReqKind::Digest { id: x } if x == id)),
                    "digest {id} awaits node {n}, with no request in flight: {}",
                    self.describe()
                );
            }
        }
        for &id in self.reads.keys() {
            assert!(
                requests().any(|(_, k)| matches!(k, ReqKind::Read { id: x } if x == id)),
                "read {id} with no request in flight: {}",
                self.describe()
            );
        }
        for (i, c) in self.chunks.iter().enumerate() {
            let mut locks = ExtentSet::new();
            for r in self.reads.values() {
                if r.locked && r.chunk as usize == i {
                    locks.insert(r.ext);
                }
            }
            for w in self.writes.values() {
                if w.chunk as usize == i {
                    locks.insert(w.ext);
                }
            }
            for &(fc, ext) in &self.fast_reads {
                if fc as usize == i {
                    locks.insert(ext);
                }
            }
            assert_eq!(c.locks, locks, "chunk {i}'s locks: {}", self.describe());
        }
        if self.deposed {
            assert!(
                self.queue.is_empty()
                    && self.reads.is_empty()
                    && self.writes.is_empty()
                    && self.digests.is_empty()
                    && self.avoid.is_empty()
                    && self.peers.is_empty()
                    && self.newly_fenced.is_empty()
                    && self.fast_reads.is_empty()
                    && self.bulk == 0,
                "a deposed target holds work: {}",
                self.describe()
            );
        }
    }
}

/// Blocks of `ext` where ISR digest replies disagree (or a reply is the wrong length).
fn digest_mismatch(ext: &Extent, got: &BTreeMap<NodeId, Vec<Option<u32>>>) -> ExtentSet {
    let mut bad = ExtentSet::new();
    let Some(first) = got.values().next() else {
        return bad;
    };
    for i in 0..ext.len as usize {
        let want = first.get(i).copied().unwrap_or(None);
        if got
            .values()
            .any(|v| v.get(i).copied().unwrap_or(None) != want)
        {
            bad.insert(Extent::new(ext.start + i as u32, 1));
        }
    }
    bad
}

#[cfg(test)]
mod tests {
    //! The target against in-memory replicas and metadata, its messages delivered at once
    //! unless a test holds them back: a failed local read (docs/issues/5), and the order of
    //! bulk work (docs/issues/11).
    use super::*;
    use crate::meta::{MetaRequest, Metadata};
    use crate::msg::Request;
    use crate::replica::Replica;
    use crate::sim::Rng;
    use crate::types::Value;

    const VOL: VolumeId = 1;
    const MS: Time = 1_000;
    const TARGET: NodeId = 1;
    /// Blocks per chunk.
    const BLOCKS: Block = 4;

    type Pred = fn(&ToReplica<Vec<Value>>) -> bool;
    /// A request to a node, and its reply, held back from the target.
    type Held = (NodeId, ToReplica<Vec<Value>>, FromReplica<Vec<Value>>);

    /// What the target started, read off the requests it sent: the chunk, and the first block
    /// of the extent.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Start {
        /// A reconciliation digest (sent to each ISR member, counted once).
        Dg(ChunkIdx, Block),
        /// A repair, at its read.
        Rp(ChunkIdx, Block),
        /// A resync digest (counted once).
        Rs(ChunkIdx, Block),
        /// A copy, at its read.
        Cp(ChunkIdx, Block),
        /// A discard.
        Ds(ChunkIdx, Block),
        /// A client read, at each send.
        Rd(ChunkIdx, Block),
        /// A client write (sent to each member, counted once).
        Wr(ChunkIdx, Block),
    }
    use Start::*;

    struct Bench {
        now: Time,
        meta: Metadata,
        replicas: BTreeMap<NodeId, Replica>,
        t: Target<Vec<Value>>,
        /// Nodes whose requests are dropped.
        down: BTreeSet<NodeId>,
        /// Requests to each node whose replies are held back.
        hold: BTreeMap<NodeId, Pred>,
        held: Vec<Held>,
        /// Metadata requests are held back, unanswered.
        hold_meta: bool,
        /// Metadata requests that match are held back, to be answered later (`release_meta`).
        hold_meta_if: Option<fn(&MetaRequest) -> bool>,
        held_meta: Vec<(u64, MetaRequest)>,
        /// Where each client read was sent, in order.
        reads_to: Vec<NodeId>,
        done: BTreeMap<u64, OpResult<Vec<Value>>>,
        next_op: u64,
        /// Client operations submitted to the current target.
        submitted: Vec<u64>,
        /// What the current target started, in order.
        log: Vec<Start>,
        /// The client operation of each client read sent, and of each client write at its first
        /// send, in order.
        client_log: Vec<u64>,
        seen_digests: BTreeSet<u64>,
        seen_writes: BTreeSet<u64>,
        /// The settings of the targets `restart` makes.
        max_bulk: usize,
        max_digest: Block,
        max_io: Block,
        today: bool,
        /// The test deposes the target: the teardown does not drain it.
        deposed_ok: bool,
    }

    fn is_any(_: &ToReplica<Vec<Value>>) -> bool {
        true
    }

    fn is_hello(m: &ToReplica<Vec<Value>>) -> bool {
        matches!(m.body, Request::Hello)
    }

    fn is_read(m: &ToReplica<Vec<Value>>) -> bool {
        matches!(m.body, Request::Read { .. })
    }

    fn is_write(m: &ToReplica<Vec<Value>>) -> bool {
        matches!(m.body, Request::Write { .. })
    }

    fn is_digest(m: &ToReplica<Vec<Value>>) -> bool {
        matches!(m.body, Request::Digest { .. })
    }

    fn is_digest_or_write(m: &ToReplica<Vec<Value>>) -> bool {
        is_digest(m) || is_write(m)
    }

    fn is_copy(m: &ToReplica<Vec<Value>>) -> bool {
        matches!(m.body, Request::Copy { .. })
    }

    fn is_discard(m: &ToReplica<Vec<Value>>) -> bool {
        matches!(m.body, Request::Discard { .. })
    }

    fn is_mapped(m: &ToReplica<Vec<Value>>) -> bool {
        matches!(m.body, Request::Mapped { .. })
    }

    /// A resync's requests to its joiner.
    fn is_resync(m: &ToReplica<Vec<Value>>) -> bool {
        matches!(
            m.body,
            Request::BeginResync { .. }
                | Request::Digest { .. }
                | Request::Copy { .. }
                | Request::Discard { .. }
        )
    }

    /// A digest or a read of a chunk other than 0.
    fn is_reconcile_off_chunk_0(m: &ToReplica<Vec<Value>>) -> bool {
        matches!(m.body, Request::Digest { chunk, .. } | Request::Read { chunk, .. } if chunk != 0)
    }

    /// A reply of another type than `body`, which answers no request `body` answers.
    fn wrong(body: &Response<Vec<Value>>) -> Response<Vec<Value>> {
        match body {
            Response::Ok => Response::Digests(Vec::new()),
            _ => Response::Ok,
        }
    }

    impl Bench {
        /// A one-chunk volume of 4 blocks on `replicas`, attached on node 1, serving.
        fn new(replicas: &[NodeId], min_isr: usize) -> Self {
            Self::with_chunks(&[replicas], min_isr)
        }

        /// A volume of 4-block chunks, chunk `i` on `placement[i]`, attached on node 1,
        /// serving.
        fn with_chunks(placement: &[&[NodeId]], min_isr: usize) -> Self {
            let mut meta = Metadata::default();
            let lists = placement.iter().map(|p| p.to_vec()).collect();
            meta.create_volume(VOL, 0, min_isr, BLOCKS, lists);
            let MetaReply::Applied { view, .. } = meta.handle(&MetaRequest::Attach {
                vol: VOL,
                node: TARGET,
            }) else {
                panic!("attach refused");
            };
            let mut replicas: BTreeMap<NodeId, Replica> = BTreeMap::new();
            for (i, nodes) in placement.iter().enumerate() {
                for &n in *nodes {
                    replicas
                        .entry(n)
                        .or_insert_with(|| Replica::with_chunk_size(BLOCKS))
                        .create_chunk(VOL, i as ChunkIdx, BLOCKS);
                }
            }
            let t = Target::new(TARGET, VOL, &view, Timing::default(), 4, 0);
            let mut b = Self {
                now: 0,
                meta,
                replicas,
                t,
                down: BTreeSet::new(),
                hold: BTreeMap::new(),
                held: Vec::new(),
                hold_meta: false,
                hold_meta_if: None,
                held_meta: Vec::new(),
                reads_to: Vec::new(),
                done: BTreeMap::new(),
                next_op: 1,
                submitted: Vec::new(),
                log: Vec::new(),
                client_log: Vec::new(),
                seen_digests: BTreeSet::new(),
                seen_writes: BTreeSet::new(),
                max_bulk: DEFAULT_MAX_BULK,
                max_digest: MAX_DIGEST_BLOCKS,
                max_io: 4,
                today: false,
                deposed_ok: false,
            };
            b.run();
            b.until("serving", |b| b.t.is_settled());
            b
        }

        /// Restarts `node`'s replica, which then reports every block of its chunks as
        /// uncertain, and attaches a new target, at the next epoch, with the bench's settings.
        fn restart(&mut self, node: NodeId) {
            self.replicas.get_mut(&node).expect("replica").restart();
            let MetaReply::Applied { view, .. } = self.meta.handle(&MetaRequest::Attach {
                vol: VOL,
                node: TARGET,
            }) else {
                panic!("attach refused");
            };
            self.t = Target::new(TARGET, VOL, &view, Timing::default(), self.max_io, self.now);
            self.t.set_max_bulk(self.max_bulk);
            self.t.set_max_digest(self.max_digest);
            self.t.bugs.today_pump = self.today;
            self.held.clear();
            self.held_meta.clear();
            self.submitted.clear();
            self.log.clear();
            self.client_log.clear();
            self.seen_digests.clear();
            self.seen_writes.clear();
            self.run();
        }

        /// Takes `node` out of the chunk's ISR in the metadata, for the next target
        /// (`restart`): it is then resynced.
        fn out_of_isr(&mut self, chunk: ChunkIdx, node: NodeId) {
            let v = self.meta.volume(VOL).expect("volume").clone();
            let m = &v.chunks[chunk as usize];
            let mut new_isr = m.isr.clone();
            new_isr.remove(&node);
            let reply = self.meta.handle(&MetaRequest::ChangeIsr {
                vol: VOL,
                epoch: v.epoch,
                changes: vec![IsrChange {
                    chunk,
                    expected_ver: m.isr_ver,
                    new_isr,
                    new_replicas: None,
                }],
            });
            assert!(
                matches!(reply, MetaReply::Applied { .. }),
                "ISR change refused"
            );
        }

        /// Sets a block on one replica only, so that digests disagree.
        fn diverge(&mut self, node: NodeId, chunk: ChunkIdx, block: Block, v: Value) {
            self.replicas
                .get_mut(&node)
                .expect("replica")
                .set_block(VOL, chunk, block, v);
        }

        /// Holds back the replies that match `p` from every node.
        fn hold_all(&mut self, p: Pred) {
            for &n in self.replicas.keys() {
                self.hold.insert(n, p);
            }
        }

        /// Delivers everything the target sends, and what that causes, until it is quiet.
        fn run(&mut self) {
            loop {
                let out = self.t.take_output();
                if out.is_empty() {
                    return;
                }
                for o in out {
                    match o {
                        Output::Send { to, msg } => self.send(to, msg),
                        Output::Meta { req, body }
                            if self.hold_meta_if.is_some_and(|p| p(&body)) =>
                        {
                            self.held_meta.push((req, body));
                        }
                        Output::Meta { req, body } if !self.hold_meta => {
                            let reply = self.meta.handle(&body);
                            self.t.on_meta(self.now, req, &reply);
                        }
                        Output::Meta { .. } => {}
                        Output::Done { op, result } => {
                            assert!(self.done.insert(op, result).is_none(), "op {op} done twice");
                        }
                    }
                }
            }
        }

        fn send(&mut self, to: NodeId, msg: ToReplica<Vec<Value>>) {
            if is_read(&msg) && self.t.client_read_of(to, msg.req).is_some() {
                self.reads_to.push(to);
            }
            self.record(to, &msg);
            if self.down.contains(&to) {
                return;
            }
            let Some(reply) = self.replicas.get_mut(&to).expect("replica").handle(&msg) else {
                return;
            };
            if self.hold.get(&to).is_some_and(|p| p(&msg)) {
                self.held.push((to, msg, reply));
            } else {
                self.t.on_reply(self.now, to, &reply);
            }
        }

        /// Notes what the request starts, if anything, from the target's state as it sends it.
        fn record(&mut self, to: NodeId, msg: &ToReplica<Vec<Value>>) {
            let Some(kind) = self
                .t
                .peers
                .get(&to)
                .and_then(|p| p.session.as_ref())
                .filter(|s| s.id == msg.session)
                .and_then(|s| s.inflight.get(&msg.req))
                .map(|&(_, k)| k)
            else {
                return;
            };
            let start = match (&msg.body, kind) {
                (&Request::Digest { chunk, ext }, ReqKind::Digest { id }) => {
                    if !self.seen_digests.insert(id) {
                        return;
                    }
                    match self.t.digests.get(&id) {
                        Some(d) if d.resync.is_some() => Rs(chunk, ext.start),
                        Some(_) => Dg(chunk, ext.start),
                        None => return,
                    }
                }
                (&Request::Read { chunk, ext }, ReqKind::Read { id }) => {
                    match self.t.reads.get(&id).map(|r| r.purpose) {
                        Some(ReadFor::Client { op }) => {
                            self.client_log.push(op);
                            Rd(chunk, ext.start)
                        }
                        Some(ReadFor::Repair) => Rp(chunk, ext.start),
                        Some(ReadFor::Copy { .. }) => Cp(chunk, ext.start),
                        None => return,
                    }
                }
                (&Request::Write { chunk, ext, .. }, ReqKind::Write { seq, .. }) => {
                    let Some(op) = self.t.writes.get(&seq).and_then(|w| w.client) else {
                        return;
                    };
                    if !self.seen_writes.insert(seq) {
                        return;
                    }
                    self.client_log.push(op);
                    Wr(chunk, ext.start)
                }
                (&Request::Discard { chunk, ext }, _) => Ds(chunk, ext.start),
                _ => return,
            };
            self.log.push(start);
        }

        /// The starts so far must be `want`: the assertion the order tests rest on.
        fn expect_starts(&self, want: &[Start]) {
            assert!(
                self.log == want,
                "out of order: started {:?}, want {want:?}",
                self.log
            );
        }

        fn advance(&mut self, dt: Time) {
            self.now += dt;
            self.t.tick(self.now);
            self.run();
        }

        /// Advances in steps of 10 ms until `f` holds, for at most 30 s.
        fn until(&mut self, what: &str, f: impl Fn(&Self) -> bool) {
            for _ in 0..3000 {
                if f(self) {
                    return;
                }
                self.advance(10 * MS);
            }
            panic!("not {what} after 30 s: {}", self.t.describe());
        }

        fn submit(&mut self, op: Op<Vec<Value>>) -> u64 {
            let id = self.next_op;
            self.next_op += 1;
            self.submitted.push(id);
            self.t.submit(self.now, id, op);
            self.run();
            id
        }

        fn write(&mut self, block: Block, v: Value) {
            let op = self.write_at(0, block, v);
            assert_eq!(self.done.get(&op), Some(&OpResult::Written));
        }

        fn read(&mut self, block: Block) -> u64 {
            self.read_at(0, block)
        }

        fn read_at(&mut self, chunk: ChunkIdx, block: Block) -> u64 {
            self.submit(Op::Read {
                chunk,
                ext: Extent::new(block, 1),
            })
        }

        fn write_at(&mut self, chunk: ChunkIdx, block: Block, v: Value) -> u64 {
            self.submit(Op::Write {
                chunk,
                ext: Extent::new(block, 1),
                data: vec![v],
            })
        }

        /// Delivers the held reply `i`.
        fn deliver(&mut self, i: usize) {
            let (to, _, reply) = self.held.remove(i);
            self.t.on_reply(self.now, to, &reply);
            self.run();
        }

        /// The first reply held from `node`, whose request matches `f`.
        fn held_from(&self, node: NodeId, f: Pred) -> usize {
            self.held
                .iter()
                .position(|(n, m, _)| *n == node && f(m))
                .unwrap_or_else(|| panic!("no reply held from node {node}"))
        }

        /// Delivers the first reply held from `node`.
        fn deliver_from(&mut self, node: NodeId) {
            let i = self.held_from(node, is_any);
            self.deliver(i);
        }

        /// Delivers the held reply `i` with a body of another type: a reply that does not
        /// answer its request.
        fn deliver_wrong(&mut self, i: usize) {
            let body = wrong(&self.held[i].2.body);
            self.deliver_as(i, body);
        }

        /// Delivers the held reply `i` with another body.
        fn deliver_as(&mut self, i: usize, body: Response<Vec<Value>>) {
            let (to, _, mut reply) = self.held.remove(i);
            reply.body = body;
            self.t.on_reply(self.now, to, &reply);
            self.run();
        }

        /// Reports the held reply `i` as a failed read.
        fn fail_held(&mut self, i: usize) {
            let (to, msg, _) = self.held.remove(i);
            self.t
                .on_read_failed(self.now, to, msg.epoch, msg.session, msg.req);
            self.run();
        }

        fn isr(&self) -> BTreeSet<NodeId> {
            self.t.chunks[0].isr.clone()
        }

        /// Answers the metadata requests held back, in order.
        fn release_meta(&mut self) {
            for (req, body) in std::mem::take(&mut self.held_meta) {
                let reply = self.meta.handle(&body);
                self.t.on_meta(self.now, req, &reply);
                self.run();
            }
        }

        fn request_move(&mut self, from: NodeId, to: NodeId) {
            let reply = self.meta.handle(&MetaRequest::MoveReplica {
                vol: VOL,
                chunk: 0,
                from,
                to,
            });
            assert!(matches!(reply, MetaReply::Applied { .. }), "move refused");
        }

        /// Resync items in flight: resync digests, copies and discards.
        fn resync_in_flight(&self) -> usize {
            let t = &self.t;
            let copies = t
                .peers
                .values()
                .filter_map(|p| p.session.as_ref())
                .flat_map(|s| s.inflight.values())
                .filter(|(_, k)| matches!(k, ReqKind::Copy { .. } | ReqKind::Discard { .. }))
                .count();
            t.digests.values().filter(|d| d.resync.is_some()).count()
                + t.reads
                    .values()
                    .filter(|r| matches!(r.purpose, ReadFor::Copy { .. }))
                    .count()
                + copies
        }

        fn serves(&self, chunk: ChunkIdx) -> bool {
            self.t.chunks[chunk as usize].phase == Phase::Ready
        }

        /// Lets everything through, then ticks until the target is settled: every chunk
        /// serves, every replica is in its ISR, and every operation is done.
        fn drain(&mut self) {
            self.down.clear();
            self.hold.clear();
            self.hold_meta = false;
            self.hold_meta_if = None;
            while !self.held.is_empty() {
                self.deliver(0);
            }
            self.release_meta();
            for _ in 0..6000 {
                if self.t.is_settled() {
                    break;
                }
                self.advance(10 * MS);
            }
            assert!(
                self.t.is_settled(),
                "not settled after 60 s of draining: {}",
                self.t.describe()
            );
            for op in &self.submitted {
                assert!(self.done.contains_key(op), "op {op} never done");
            }
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            if !std::thread::panicking() && !self.deposed_ok {
                self.drain();
            }
        }
    }

    /// A metadata request that drops node 2's replica.
    fn drops_node_2(m: &MetaRequest) -> bool {
        matches!(m, MetaRequest::ChangeIsr { changes, .. }
            if changes.iter().any(|c| c.new_replicas.as_ref().is_some_and(|r| !r.contains(&2))))
    }

    #[test]
    fn a_failed_local_read_is_answered_by_another_member() {
        let mut b = Bench::new(&[1, 2], 1);
        b.write(0, 42);
        b.hold.insert(TARGET, is_read);
        let op = b.read(0);
        assert_eq!((b.held.len(), b.done.get(&op)), (1, None));
        b.fail_held(0);
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![42])));
        assert_eq!(b.reads_to, [1, 2]);
        // The local replica stays a member, on the same session, and nothing is left to avoid.
        assert_eq!(b.isr(), BTreeSet::from([1, 2]));
        assert!(b.t.session_of(TARGET).is_some());
        assert_eq!(b.t.avoid_len(), 0);
        // Only that read avoided it: the next one goes to the local replica again.
        b.hold.clear();
        let next = b.read(0);
        assert_eq!(b.done.get(&next), Some(&OpResult::Read(vec![42])));
        assert_eq!(b.reads_to, [1, 2, 1]);
    }

    #[test]
    fn a_read_sent_elsewhere_may_come_back_to_the_local_replica() {
        let mut b = Bench::new(&[1, 2], 1);
        b.write(0, 42);
        b.hold.insert(TARGET, is_read);
        let op = b.read(0);
        // Node 2 takes the read again but never answers; its session fails and it is removed.
        b.down.insert(2);
        b.fail_held(0);
        assert_eq!(b.reads_to, [1, 2]);
        assert_eq!(
            b.t.avoid_len(),
            0,
            "sent elsewhere, the read avoids nothing"
        );
        b.hold.clear();
        b.until("read done", |b| b.done.contains_key(&op));
        // Not unreadable: the local replica, still a member, serves it.
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![42])));
        assert_eq!(b.reads_to, [1, 2, 1]);
    }

    #[test]
    fn a_failed_read_goes_again_before_later_overlapping_reads() {
        let mut b = Bench::new(&[1, 2], 1);
        b.write(0, 42);
        b.hold.insert(TARGET, is_read);
        let first = b.read(0);
        // Waits for the first read's lock.
        let second = b.read(0);
        assert_eq!(b.reads_to, [1]);
        b.hold.clear();
        b.fail_held(0);
        assert_eq!(b.reads_to, [1, 2, 1]);
        assert_eq!(b.done.get(&first), Some(&OpResult::Read(vec![42])));
        assert_eq!(b.done.get(&second), Some(&OpResult::Read(vec![42])));
    }

    #[test]
    fn a_read_that_failed_twice_on_the_only_member_is_unreadable() {
        let mut b = Bench::new(&[1], 1);
        b.write(0, 42);
        b.hold.insert(TARGET, is_read);
        let op = b.read(0);
        b.fail_held(0);
        // Tried once more on the only member.
        assert_eq!(
            (b.reads_to.as_slice(), b.held.len()),
            ([1, 1].as_slice(), 1)
        );
        assert_eq!(b.done.get(&op), None);
        b.fail_held(0);
        assert_eq!(b.done.get(&op), Some(&OpResult::Unreadable));
        assert_eq!(b.reads_to, [1, 1]);
        assert_eq!(b.isr(), BTreeSet::from([1]));
        assert_eq!(b.t.avoid_len(), 0);
    }

    #[test]
    fn a_read_that_failed_once_on_the_only_member_is_served_there() {
        let mut b = Bench::new(&[1], 1);
        b.write(0, 42);
        b.hold.insert(TARGET, is_read);
        let op = b.read(0);
        b.hold.clear();
        b.fail_held(0);
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![42])));
        assert_eq!(b.reads_to, [1, 1]);
        assert_eq!(b.t.avoid_len(), 0);
    }

    #[test]
    fn a_member_being_removed_does_not_count() {
        let mut b = Bench::new(&[1, 2], 1);
        b.write(0, 42);
        // Node 2 stops answering; its removal is proposed but never committed.
        b.hold_meta = true;
        b.down.insert(2);
        b.until("removing node 2", |b| b.t.chunks[0].removing.contains(&2));
        b.hold.insert(TARGET, is_read);
        let op = b.read(0);
        assert_eq!(b.held.len(), 1, "the read went to the local replica");
        b.fail_held(0);
        assert_eq!(
            b.held.len(),
            1,
            "tried once more on the local replica, not on node 2"
        );
        b.fail_held(0);
        assert_eq!(b.done.get(&op), Some(&OpResult::Unreadable));
        assert_eq!(b.reads_to, [1, 1]);
        assert_eq!(b.t.avoid_len(), 0);
    }

    #[test]
    fn a_waiting_read_goes_back_once_its_other_member_is_being_removed() {
        let mut b = Bench::new(&[1, 2], 1);
        b.write(0, 42);
        // Node 2 stops answering, and its removal stays proposed.
        b.hold_meta = true;
        b.down.insert(2);
        b.until("node 2's session down", |b| b.t.session_of(2).is_none());
        b.hold.insert(TARGET, is_read);
        let op = b.read(0);
        b.hold.clear();
        b.fail_held(0);
        // Waits for node 2, until it is being removed: then back to the local replica.
        b.until("read done", |b| b.done.contains_key(&op));
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![42])));
        assert_eq!(b.reads_to, [1, 1]);
    }

    #[test]
    fn a_failed_read_waits_for_a_member_whose_session_is_down() {
        // With min_isr 2, node 2 stays in the ISR while its session is down.
        let mut b = Bench::new(&[1, 2], 2);
        b.write(0, 42);
        b.down.insert(2);
        b.until("node 2's session down", |b| b.t.session_of(2).is_none());
        b.hold.insert(TARGET, is_read);
        let op = b.read(0);
        assert_eq!(b.held.len(), 1, "the read went to the local replica");
        b.fail_held(0);
        assert_eq!(b.done.get(&op), None);
        assert_eq!(b.t.avoid_len(), 1);
        b.down.clear();
        b.until("read done", |b| b.done.contains_key(&op));
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![42])));
        assert_eq!(b.reads_to, [1, 2]);
        assert_eq!(b.isr(), BTreeSet::from([1, 2]));
        assert_eq!(b.t.avoid_len(), 0);
    }

    #[test]
    fn stray_failure_reports_are_ignored() {
        let mut b = Bench::new(&[1, 2], 1);
        b.write(0, 42);
        b.hold.insert(TARGET, is_read);
        let op = b.read(0);
        let (_, msg, reply) = b.held.pop().expect("held read");
        let (e, s, r) = (msg.epoch, msg.session, msg.req);
        for (epoch, session, req) in [(e + 1, s, r), (e, s + 1, r), (e, s, r + 1000)] {
            b.t.on_read_failed(b.now, TARGET, epoch, session, req);
            b.run();
            assert_eq!(
                b.t.client_read_of(TARGET, r),
                Some(op),
                "{epoch} {session} {req}"
            );
        }
        assert_eq!(b.reads_to, [1]);
        b.t.on_reply(b.now, TARGET, &reply);
        b.run();
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![42])));

        // A report for a request that is not a client read leaves it in flight.
        b.hold.insert(TARGET, is_write);
        let w = b.submit(Op::Write {
            chunk: 0,
            ext: Extent::new(1, 1),
            data: vec![7],
        });
        let (_, msg, reply) = b.held.pop().expect("held write");
        b.t.on_read_failed(b.now, TARGET, msg.epoch, msg.session, msg.req);
        b.run();
        assert_eq!(b.done.get(&w), None);
        b.t.on_reply(b.now, TARGET, &reply);
        b.run();
        assert_eq!(b.done.get(&w), Some(&OpResult::Written));
    }

    /// S11: node 2's replica acknowledges a write that waits on node 1, a move drops it, and
    /// node 2 comes back as a new replica. Its resync copies the block, but node 2 joins only
    /// once the write has completed: its old acknowledgement does not count.
    #[test]
    fn a_dropped_replicas_ack_does_not_count_after_it_rejoins() {
        let mut b = Bench::new(&[1, 2], 1);
        b.replicas.insert(3, Replica::with_chunk_size(4));
        // Node 3 joins; the drop of node 2 that follows is held back.
        b.hold_meta_if = Some(drops_node_2);
        b.request_move(2, 3);
        b.until("node 3 in the ISR", |b| b.isr().contains(&3));
        assert_eq!(b.held_meta.len(), 1, "the drop of node 2 is proposed");
        // A write that nodes 2 and 3 acknowledge, and that waits on node 1.
        b.hold.insert(TARGET, is_write);
        let op = b.submit(Op::Write {
            chunk: 0,
            ext: Extent::new(0, 1),
            data: vec![42],
        });
        assert_eq!((b.held.len(), b.done.get(&op)), (1, None));
        // The drop commits, and node 2's copy is deleted.
        b.hold_meta_if = None;
        b.release_meta();
        assert_eq!(b.t.chunks[0].replicas, [1, 3]);
        b.replicas
            .get_mut(&2)
            .expect("replica")
            .remove_chunk(VOL, 0);
        // Node 2 comes back as a new replica, and its resync copies everything.
        b.request_move(3, 2);
        b.until("node 2 resynced or joined", |b| {
            let c = &b.t.chunks[0];
            c.joining == Some(2)
                || c.isr.contains(&2)
                || c.resync.as_ref().is_some_and(|r| {
                    r.member == 2 && r.started && r.todo.is_empty() && r.discard.is_empty()
                })
        });
        assert_eq!(
            b.replicas[&2].block(VOL, 0, 0),
            Some(42),
            "the copy has the block"
        );
        assert_eq!(b.done.get(&op), None, "the write still waits on node 1");
        let c = &b.t.chunks[0];
        assert!(
            c.joining != Some(2) && !c.isr.contains(&2),
            "node 2 joined on its old replica's acknowledgement: {}",
            b.t.describe()
        );
        // Node 1's acknowledgement completes the write; then node 2 joins.
        let (_, _, reply) = b.held.remove(0);
        b.hold.clear();
        b.t.on_reply(b.now, TARGET, &reply);
        b.run();
        assert_eq!(b.done.get(&op), Some(&OpResult::Written));
        b.until("node 2 in the ISR", |b| b.isr().contains(&2));
        assert_eq!(b.replicas[&2].block(VOL, 0, 0), Some(42));
    }

    #[test]
    fn deposition_ends_reads_waiting_to_go_again() {
        let mut b = Bench::new(&[1, 2], 2);
        b.deposed_ok = true;
        b.write(0, 42);
        b.down.insert(2);
        b.until("node 2's session down", |b| b.t.session_of(2).is_none());
        b.hold.insert(TARGET, is_read);
        let op = b.read(0);
        let (_, msg, _) = b.held[0].clone();
        b.fail_held(0);
        assert_eq!(b.t.avoid_len(), 1);
        b.t.depose_checked();
        b.run();
        assert_eq!(b.done.get(&op), Some(&OpResult::Failed));
        assert_eq!(b.t.avoid_len(), 0);
        // A report after deposition changes nothing.
        b.t.on_read_failed(b.now, TARGET, msg.epoch, msg.session, msg.req);
        b.run();
        assert_eq!(b.t.avoid_len(), 0);
    }

    // docs/issues/11. Unless said, `max_bulk` is 1: each start is then the queue's choice
    // for one free slot. The tests of S3, S6 and S8 that `ORDER_TESTS` lists are built so that
    // they fail with today's pump: the waited chunk's work sits behind other work in queue
    // order.

    /// An order test, run with today's pump or not.
    type OrderTest = fn(bool);

    /// The tests that fail with today's pump (`Bugs::today_pump`), each run with it by
    /// `the_order_tests_fail_with_todays_pump`.
    const ORDER_TESTS: &[(&str, OrderTest)] = &[
        ("read_on_a_middle_chunk", read_on_a_middle_chunk),
        ("write_moves_its_chunk", write_moves_its_chunk),
        ("wait_order", wait_order),
        ("two_pieces", two_pieces),
        ("repairs_not_waited", repairs_not_waited),
        ("alternate", alternate),
        ("one_resync_item", one_resync_item),
        ("last_start_resets", last_start_resets),
        ("next_free_slot", next_free_slot),
        ("fenced_first_and_waited", fenced_first_and_waited),
        ("fenced_late", fenced_late),
        ("ahead_of_repairs", ahead_of_repairs),
        ("sent_again", sent_again),
    ];

    #[test]
    fn the_order_tests_fail_with_todays_pump() {
        for &(name, test) in ORDER_TESTS {
            let err = std::panic::catch_unwind(|| test(true))
                .expect_err(&format!("{name} passes with today's pump"));
            let msg = err
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| err.downcast_ref::<&str>().copied())
                .unwrap_or_default();
            assert!(
                msg.starts_with("out of order"),
                "{name} fails with today's pump for another reason: {msg}"
            );
        }
    }

    /// S1: chunks fenced in one progress pass queue their digests from chunk 0 up.
    #[test]
    fn digests_of_chunks_fenced_together_go_from_chunk_0() {
        let mut b = Bench::with_chunks(&[&[1, 2], &[1, 2], &[1, 2]], 1);
        b.max_bulk = 1;
        b.max_digest = 2;
        b.restart(1);
        b.expect_starts(&[Dg(0, 0), Dg(0, 2), Dg(1, 0), Dg(1, 2), Dg(2, 0), Dg(2, 2)]);
    }

    /// S2: with no chunk waited, the queue goes in its order: a copy at the front, a chunk
    /// fenced later ahead of those fenced before it, and chunks fenced together from chunk 0.
    #[test]
    fn with_nothing_waited_requests_go_in_queue_order() {
        let mut b = Bench::with_chunks(&[&[1, 3, 4], &[1, 2], &[1, 2], &[1, 2, 5]], 1);
        // Node 4 falls behind on chunk 0: block 0 is on its ISR only.
        b.out_of_isr(0, 4);
        b.diverge(1, 0, 0, 7);
        b.diverge(3, 0, 0, 7);
        b.max_bulk = 1;
        b.hold.insert(2, is_hello);
        b.hold.insert(5, is_hello);
        b.hold.insert(4, is_digest);
        b.restart(2);
        // Chunk 0 serves and resyncs node 4: a discard of what its source does not hold, then a
        // digest of the rest, held.
        b.expect_starts(&[Ds(0, 1), Rs(0, 0)]);
        // Chunks 1 and 2 are fenced together; chunk 3 later.
        b.deliver_from(2);
        b.deliver_from(5);
        // Block 0 differs on node 4: its copy goes to the front.
        b.deliver_from(4);
        b.expect_starts(&[Ds(0, 1), Rs(0, 0), Cp(0, 0), Dg(3, 0), Dg(1, 0), Dg(2, 0)]);
        assert_eq!(b.t.stats.waited_pumps, 0);
    }

    /// Five chunks on nodes 1 and 2, node 1 restarted: every chunk reconciles, and fences once
    /// node 2's `Hello` is delivered.
    fn five_chunks_fencing(today: bool, max_bulk: usize) -> Bench {
        let mut b = Bench::with_chunks(&[&[1, 2][..]; 5], 1);
        b.today = today;
        b.max_bulk = max_bulk;
        b.hold.insert(2, is_hello);
        b.restart(1);
        b
    }

    /// S3.2: a read queued before the fence completes, on chunk 2 of 5.
    fn read_on_a_middle_chunk(today: bool) {
        let mut b = five_chunks_fencing(today, 1);
        let op = b.read_at(2, 0);
        b.deliver_from(2);
        b.expect_starts(&[Dg(2, 0), Dg(0, 0), Rd(2, 0), Dg(1, 0), Dg(3, 0), Dg(4, 0)]);
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![0])));
    }

    #[test]
    fn a_read_on_a_middle_chunk_reconciles_it_first() {
        read_on_a_middle_chunk(false);
    }

    fn write_moves_its_chunk(today: bool) {
        let mut b = five_chunks_fencing(today, 1);
        let op = b.write_at(3, 1, 42);
        b.deliver_from(2);
        b.expect_starts(&[Dg(3, 0), Dg(0, 0), Wr(3, 1), Dg(1, 0), Dg(2, 0), Dg(4, 0)]);
        assert_eq!(b.done.get(&op), Some(&OpResult::Written));
    }

    #[test]
    fn a_write_moves_its_chunk_ahead() {
        write_moves_its_chunk(false);
    }

    /// Reads on chunk 3, then on chunk 1: chunk 3 first; with two slots, both at once.
    fn wait_order(today: bool) {
        let mut b = five_chunks_fencing(today, 1);
        let (r3, r1) = (b.read_at(3, 0), b.read_at(1, 0));
        b.deliver_from(2);
        b.expect_starts(&[
            Dg(3, 0),
            Dg(1, 0),
            Rd(3, 0),
            Dg(0, 0),
            Rd(1, 0),
            Dg(2, 0),
            Dg(4, 0),
        ]);
        assert!(b.done.contains_key(&r3) && b.done.contains_key(&r1));

        let mut b = five_chunks_fencing(today, 2);
        b.read_at(3, 0);
        b.read_at(1, 0);
        b.hold.insert(2, is_digest);
        b.deliver_from(2);
        b.expect_starts(&[Dg(3, 0), Dg(1, 0)]);
        assert_eq!(b.t.digests.len(), 2);
    }

    #[test]
    fn waited_chunks_go_in_wait_order() {
        wait_order(false);
    }

    /// A request across chunks 3 and 4, in two pieces submitted one after the other.
    fn two_pieces(today: bool) {
        let mut b = five_chunks_fencing(today, 1);
        b.read_at(3, 3);
        b.read_at(4, 0);
        b.deliver_from(2);
        b.expect_starts(&[
            Dg(3, 0),
            Dg(4, 0),
            Rd(3, 3),
            Dg(0, 0),
            Rd(4, 0),
            Dg(1, 0),
            Dg(2, 0),
        ]);
    }

    #[test]
    fn two_pieces_go_in_submission_order() {
        two_pieces(false);
    }

    /// Chunk 0's digests differ; chunk 1 is waited: its digest goes before chunk 0's
    /// repairs, which were queued ahead of it.
    fn repairs_not_waited(today: bool) {
        let mut b = Bench::with_chunks(&[&[1, 2], &[1, 2]], 1);
        b.today = today;
        b.max_bulk = 1;
        b.diverge(2, 0, 0, 9);
        b.diverge(2, 0, 2, 9);
        b.hold.insert(2, is_digest);
        b.restart(1);
        let op = b.read_at(1, 0);
        b.deliver_from(2);
        b.expect_starts(&[Dg(0, 0), Dg(1, 0)]);
        b.deliver_from(2);
        b.expect_starts(&[Dg(0, 0), Dg(1, 0), Rp(0, 0), Rd(1, 0), Rp(0, 2)]);
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![0])));
        assert_eq!(b.replicas[&2].block(VOL, 0, 2), Some(0), "repaired");
    }

    #[test]
    fn repairs_of_a_chunk_not_waited_wait() {
        repairs_not_waited(false);
    }

    /// Chunk 0 on nodes 1, 3 and 4 serves and resyncs node 4, which lacks all of it (four
    /// copies, with `max_io` 1); chunk 1 on nodes 1 and 2 reconciles, node 2 having restarted,
    /// and its four blocks differ on node 2. Node 2's `Hello` and node 4's digests are held.
    fn resync_beside_a_waited_chunk(today: bool, max_bulk: usize) -> Bench {
        let mut b = Bench::with_chunks(&[&[1, 3, 4], &[1, 2]], 1);
        b.today = today;
        b.max_bulk = max_bulk;
        b.max_io = 1;
        b.out_of_isr(0, 4);
        for blk in 0..BLOCKS {
            b.diverge(1, 0, blk, 7);
            b.diverge(3, 0, blk, 7);
            b.diverge(2, 1, blk, 9);
        }
        b.hold.insert(2, is_hello);
        b.hold.insert(4, is_digest);
        b.restart(2);
        b.expect_starts(&[Rs(0, 0)]);
        b
    }

    /// S3.1, S3.3: one slot, a waited chunk with four repairs, a resync with four copies.
    fn alternate(today: bool) {
        let mut b = resync_beside_a_waited_chunk(today, 1);
        let op = b.read_at(1, 0);
        b.deliver_from(2);
        b.deliver_from(4);
        b.expect_starts(&[
            Rs(0, 0),
            Dg(1, 0),
            Cp(0, 0),
            Rp(1, 0),
            Cp(0, 1),
            Rp(1, 1),
            Cp(0, 2),
            Rp(1, 2),
            Cp(0, 3),
            Rp(1, 3),
            Rd(1, 0),
        ]);
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![0])));
        assert!(b.serves(1));
    }

    #[test]
    fn waited_work_and_a_resync_alternate() {
        alternate(false);
    }

    /// S3.1, S3.3: four slots, all free in one pump, with four copies queued ahead of the
    /// waited chunk's digest. The resync digest, in flight when the chunk became waited, runs
    /// to its end.
    fn one_resync_item(today: bool) {
        let mut b = resync_beside_a_waited_chunk(today, 1);
        let op = b.read_at(1, 0);
        b.hold.insert(2, is_digest);
        b.deliver_from(2);
        b.t.set_max_bulk(4);
        b.hold.insert(4, is_copy);
        b.deliver_from(4);
        b.expect_starts(&[Rs(0, 0), Dg(1, 0), Cp(0, 0)]);
        assert_eq!((b.resync_in_flight(), b.t.bulk), (1, 2));
        // The digest's reply queues four repairs: three start, beside the one copy.
        b.deliver_from(2);
        b.expect_starts(&[
            Rs(0, 0),
            Dg(1, 0),
            Cp(0, 0),
            Rp(1, 0),
            Rp(1, 1),
            Rp(1, 2),
            Rp(1, 3),
            Cp(0, 1),
            Cp(0, 2),
            Cp(0, 3),
            Rd(1, 0),
        ]);
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![0])));
    }

    #[test]
    fn one_resync_item_in_flight_while_a_chunk_is_waited() {
        one_resync_item(false);
    }

    /// S3.4: chunk 1's ISR has node 4, which is down, and the ISR minimum is 2: chunk 1
    /// stays fencing, its read waits, and caps nothing. (Today's pump has no cap: this test
    /// passes with it.)
    #[test]
    fn a_chunk_stuck_fencing_caps_no_resync() {
        let mut b = Bench::with_chunks(&[&[1, 2, 3], &[1, 4]], 2);
        b.down.insert(4);
        b.out_of_isr(0, 3);
        for blk in 0..BLOCKS {
            b.diverge(1, 0, blk, 7);
            b.diverge(2, 0, blk, 7);
        }
        b.max_bulk = 4;
        b.max_digest = 1;
        b.hold.insert(2, is_digest);
        b.hold.insert(3, is_digest);
        b.restart(1);
        b.expect_starts(&[Dg(0, 0), Dg(0, 1), Dg(0, 2), Dg(0, 3)]);
        let op = b.read_at(1, 0);
        b.hold.remove(&2);
        for _ in 0..4 {
            b.deliver_from(2);
        }
        assert!(b.serves(0) && !b.serves(1));
        b.expect_starts(&[
            Dg(0, 0),
            Dg(0, 1),
            Dg(0, 2),
            Dg(0, 3),
            Rs(0, 0),
            Rs(0, 1),
            Rs(0, 2),
            Rs(0, 3),
        ]);
        assert_eq!((b.resync_in_flight(), b.t.bulk), (4, 4));
        assert_eq!(b.done.get(&op), None);
        assert!(b.t.stats.waited_pumps_with_resync > 0);
    }

    /// S3, S7: reads on five reconciling chunks, a resync of chunk 0, and node 2, in the ISR
    /// of the five at the ISR minimum, failing every 1.3 s. Replies from nodes 2 and 4 come
    /// 200 ms late. (Liveness: passes with today's pump.)
    #[test]
    fn resyncs_progress_while_a_member_flaps() {
        let mut b = Bench::with_chunks(
            &[&[1, 3, 4], &[1, 2], &[1, 2], &[1, 2], &[1, 2], &[1, 2]],
            2,
        );
        b.out_of_isr(0, 4);
        for blk in 0..BLOCKS {
            b.diverge(1, 0, blk, 7);
            b.diverge(3, 0, blk, 7);
        }
        for c in 1..=5 {
            b.diverge(2, c, 1, 9);
        }
        b.max_io = 1;
        b.max_bulk = 1;
        b.hold.insert(2, is_any);
        b.hold.insert(4, is_any);
        b.restart(2);
        let ops: Vec<u64> = (1..=5).map(|c| b.read_at(c, 1)).collect();
        let over = |b: &Bench| {
            (0..6).all(|c| b.serves(c))
                && b.t.chunks[0].isr.contains(&4)
                && ops.iter().all(|op| b.done.contains_key(op))
        };
        let mut failures = 0;
        for step in 1..=3000 {
            if over(&b) {
                break;
            }
            b.advance(10 * MS);
            if step % 20 == 0 {
                for _ in 0..b.held.len() {
                    b.deliver(0);
                }
            }
            if step % 130 == 0
                && let Some(i) = b.held.iter().position(|h| h.0 == 2)
            {
                b.deliver_wrong(i);
                failures += 1;
            }
        }
        assert!(over(&b), "not done after 30 s: {}", b.t.describe());
        assert!(failures > 0, "node 2 never failed");
        assert!(b.t.stats.waited_pumps_with_resync > 0);
        for op in &ops {
            assert_eq!(b.done.get(op), Some(&OpResult::Read(vec![0])));
        }
    }

    /// The last start resets once no chunk is waited: chunk 2, waited after chunk 1 served,
    /// takes the first free slot, ahead of the copies queued before its digest.
    fn last_start_resets(today: bool) {
        let mut b = Bench::with_chunks(&[&[1, 3, 4], &[1, 2], &[1, 2]], 1);
        b.today = today;
        b.max_bulk = 1;
        b.max_io = 1;
        b.out_of_isr(0, 4);
        for blk in 0..BLOCKS {
            b.diverge(1, 0, blk, 7);
            b.diverge(3, 0, blk, 7);
        }
        b.hold.insert(2, is_hello);
        b.hold.insert(4, is_digest);
        b.restart(2);
        b.read_at(1, 0);
        b.hold.insert(2, is_digest);
        b.deliver_from(2);
        b.hold.insert(4, is_copy);
        // The resync digest's reply queues the copies; chunk 1's digest starts (a waited start).
        b.deliver_from(4);
        b.expect_starts(&[Rs(0, 0), Dg(1, 0)]);
        // Chunk 1 serves: nothing is waited, a copy starts.
        b.deliver_from(2);
        b.expect_starts(&[Rs(0, 0), Dg(1, 0), Cp(0, 0), Rd(1, 0)]);
        b.read_at(2, 0);
        b.deliver_from(4);
        b.expect_starts(&[Rs(0, 0), Dg(1, 0), Cp(0, 0), Rd(1, 0), Dg(2, 0)]);
        b.deliver_from(2);
        b.expect_starts(&[
            Rs(0, 0),
            Dg(1, 0),
            Cp(0, 0),
            Rd(1, 0),
            Dg(2, 0),
            Cp(0, 1),
            Rd(2, 0),
        ]);
    }

    #[test]
    fn the_last_start_resets_when_nothing_is_waited() {
        last_start_resets(false);
    }

    /// S4: a joiner whose session fails on each round: the round's items leave the queue in
    /// the pump that follows, so the queue does not grow. Chunk 1 stays fencing, waited, so
    /// that the pumps take the waited path.
    #[test]
    fn aborted_rounds_leave_the_queue() {
        let mut b = Bench::with_chunks(&[&[1, 3, 4], &[1, 2]], 2);
        b.out_of_isr(0, 4);
        b.diverge(1, 0, 0, 7);
        b.diverge(3, 0, 0, 7);
        b.max_bulk = 1;
        b.hold.insert(2, is_hello);
        b.hold.insert(4, is_discard);
        b.restart(2);
        b.read_at(1, 0);
        let resync_items = |b: &Bench| {
            b.t.queue
                .iter()
                .filter(|w| matches!(w.class(), Class::Resync(..)))
                .count()
        };
        for _ in 0..4 {
            // The discard in flight, its round's digest queued.
            let round = b.t.chunks[0].resync.as_ref().expect("a resync").round;
            assert_eq!(resync_items(&b), 1);
            assert!(b.t.queue.iter().all(|w| match w.class() {
                Class::Resync(_, r) => r == round,
                _ => true,
            }));
            let i = b.held_from(4, is_discard);
            b.deliver_wrong(i);
            assert!(b.t.chunks[0].resync.is_none(), "the round is aborted");
            assert_eq!((resync_items(&b), b.t.bulk), (0, 0));
            b.until("node 4's next round", |b| b.t.chunks[0].resync.is_some());
            b.until("its discard in flight", |b| b.t.bulk == 1);
        }
        assert!(b.t.stats.waited_pumps > 0);
    }

    /// S4: every slot is taken, by chunk 1's digest; a read and a write queued on chunk 0,
    /// which serves, start at once.
    #[test]
    fn every_client_operation_is_tried_with_every_slot_taken() {
        let mut b = Bench::with_chunks(&[&[1, 3], &[1, 2], &[1, 2]], 1);
        b.max_bulk = 1;
        b.hold.insert(2, is_digest);
        b.restart(2);
        b.read_at(1, 0);
        let r = b.read_at(0, 0);
        let w = b.write_at(0, 1, 5);
        b.expect_starts(&[Dg(1, 0), Rd(0, 0), Wr(0, 1)]);
        assert_eq!(b.done.get(&r), Some(&OpResult::Read(vec![0])));
        assert_eq!(b.done.get(&w), Some(&OpResult::Written));
        assert!(b.t.stats.waited_pumps > 0);
    }

    /// S5: node 1 holds 5 in block 0, node 2 0: a read queued on the chunk goes once the
    /// repair is acknowledged, and returns 5.
    #[test]
    fn a_waited_read_returns_the_repaired_value() {
        let mut b = Bench::new(&[1, 2], 1);
        b.diverge(1, 0, 0, 5);
        b.hold.insert(2, is_hello);
        b.restart(2);
        let op = b.read_at(0, 0);
        b.hold.insert(2, is_write);
        b.deliver_from(2);
        b.expect_starts(&[Dg(0, 0), Rp(0, 0)]);
        assert_eq!(b.done.get(&op), None);
        b.deliver_from(2);
        b.expect_starts(&[Dg(0, 0), Rp(0, 0), Rd(0, 0)]);
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![5])));
        assert_eq!(b.replicas[&2].block(VOL, 0, 0), Some(5));
    }

    /// S5: a write, then a read of its block, both queued while the chunk is waited; then the
    /// same with node 2's session failing while the write waits on it.
    #[test]
    fn a_write_then_a_read_keep_their_order_while_waited() {
        for fail in [false, true] {
            let mut b = Bench::new(&[1, 2], 2);
            b.hold.insert(2, is_hello);
            b.restart(2);
            let w = b.write_at(0, 0, 42);
            let r = b.read_at(0, 0);
            if fail {
                b.hold.insert(2, is_write);
            }
            b.deliver_from(2);
            if fail {
                assert_eq!(b.done.get(&w), None);
                let i = b.held_from(2, is_write);
                b.deliver_wrong(i);
                b.hold.clear();
                b.until("read done", |b| b.done.contains_key(&r));
            }
            assert_eq!(b.client_log, [w, r], "fail {fail}");
            assert_eq!(b.done.get(&w), Some(&OpResult::Written));
            assert_eq!(b.done.get(&r), Some(&OpResult::Read(vec![42])));
        }
    }

    /// S5: a fast read is refused on a chunk that does not serve; then on blocks of a queued
    /// write; then on blocks of a read sent again and queued, after a failed local read and
    /// after a lapsed lease.
    #[test]
    fn fast_reads_wait_for_the_chunk_and_queued_operations() {
        let x = |b: Block| Extent::new(b, 1);
        let mut b = Bench::new(&[1, 2], 2);
        b.hold.insert(2, is_hello);
        b.restart(2);
        b.read_at(0, 3);
        assert_eq!(b.t.try_fast_read(b.now, 0, x(0)), None, "fencing");
        b.hold.insert(2, is_digest);
        b.deliver_from(2);
        assert_eq!(b.t.try_fast_read(b.now, 0, x(0)), None, "reconciling");
        b.deliver_from(2);
        assert!(b.serves(0));
        let fr = b.t.try_fast_read(b.now, 0, x(0)).expect("serving");
        assert!(b.t.finish_fast_read(b.now, fr, true));
        b.run();

        // A write on blocks 0 and 1 in flight; one on blocks 1 and 2 queued behind it.
        b.hold.insert(2, is_write);
        let w1 = b.submit(Op::Write {
            chunk: 0,
            ext: Extent::new(0, 2),
            data: vec![10, 11],
        });
        let w2 = b.submit(Op::Write {
            chunk: 0,
            ext: Extent::new(1, 2),
            data: vec![21, 22],
        });
        assert_eq!(b.t.try_fast_read(b.now, 0, x(2)), None, "a write queued");
        let fr = b.t.try_fast_read(b.now, 0, x(3)).expect("block 3");
        assert!(b.t.finish_fast_read(b.now, fr, true));
        b.run();
        b.deliver_from(2);
        b.deliver_from(2);
        assert_eq!(b.done.get(&w1), Some(&OpResult::Written));
        assert_eq!(b.done.get(&w2), Some(&OpResult::Written));
        b.hold.clear();

        // Node 2 stops answering; with the ISR minimum 2 it stays a member. Its lease lasts a
        // little beyond its session.
        b.down.insert(2);
        b.until("node 2's session down", |b| b.t.session_of(2).is_none());
        assert!(b.t.lease_ok(0, b.now));
        b.hold.insert(TARGET, is_read);
        let r3 = b.read_at(0, 3);
        b.fail_held(0);
        assert_eq!(b.t.avoid_len(), 1, "waits for node 2");
        assert_eq!(b.t.try_fast_read(b.now, 0, x(3)), None, "a read sent again");
        let fr = b.t.try_fast_read(b.now, 0, x(0)).expect("block 0");
        assert!(b.t.finish_fast_read(b.now, fr, true));
        b.run();
        // A read whose reply comes after the lease lapsed is queued again.
        let r2 = b.read_at(0, 2);
        b.until("the lease lapsed", |b| !b.t.lease_ok(0, b.now));
        let i = b.held_from(TARGET, is_read);
        b.deliver(i);
        assert_eq!(b.done.get(&r2), None);
        assert!(
            b.t.queue
                .iter()
                .any(|w| matches!(w, Work::Client { op, .. } if *op == r2))
        );
        assert_eq!(b.t.try_fast_read(b.now, 0, x(2)), None, "a read sent again");
        b.hold.clear();
        b.down.clear();
        b.until("reads done", |b| {
            b.done.contains_key(&r2) && b.done.contains_key(&r3)
        });
        assert_eq!(b.done.get(&r2), Some(&OpResult::Read(vec![22])));
        assert_eq!(b.done.get(&r3), Some(&OpResult::Read(vec![0])));
    }

    /// S9: a deposed target fails the operations waiting on its chunks.
    #[test]
    fn deposition_fails_waiting_operations() {
        let mut b = Bench::with_chunks(&[&[1, 2], &[1, 2], &[1, 2]], 1);
        b.deposed_ok = true;
        b.hold.insert(2, is_hello);
        b.restart(1);
        let ops = [b.read_at(1, 0), b.write_at(2, 0, 7), b.read_at(0, 1)];
        // Another target takes over; this one learns of it from its next read of the volume.
        b.meta.handle(&MetaRequest::Attach { vol: VOL, node: 9 });
        b.until("deposed", |b| b.t.is_deposed());
        for op in ops {
            assert_eq!(b.done.get(&op), Some(&OpResult::Failed));
        }
    }

    /// S6: four digests in flight; a read on chunk 5 takes the first slot that frees.
    fn next_free_slot(today: bool) {
        let mut b = Bench::with_chunks(&[&[1, 2][..]; 6], 1);
        b.today = today;
        b.max_bulk = 4;
        b.hold.insert(2, is_digest);
        b.restart(1);
        b.expect_starts(&[Dg(0, 0), Dg(1, 0), Dg(2, 0), Dg(3, 0)]);
        b.read_at(5, 0);
        b.deliver_from(2);
        b.expect_starts(&[Dg(0, 0), Dg(1, 0), Dg(2, 0), Dg(3, 0), Dg(5, 0)]);
    }

    #[test]
    fn a_waited_chunk_takes_the_next_free_slot() {
        next_free_slot(false);
    }

    /// S6: chunk A (0) on nodes 1 and 2, B (1) on 1 and 3, digests of one block. A is fenced
    /// first and waited; B's digests, fenced later, are queued ahead of A's last three.
    fn fenced_first_and_waited(today: bool) {
        let mut b = Bench::with_chunks(&[&[1, 2], &[1, 3]], 1);
        b.today = today;
        b.max_bulk = 1;
        b.max_digest = 1;
        b.hold.insert(3, is_hello);
        b.hold.insert(2, is_digest);
        b.restart(1);
        b.expect_starts(&[Dg(0, 0)]);
        let op = b.read_at(0, 0);
        b.deliver_from(3);
        b.hold.remove(&2);
        b.deliver_from(2);
        b.expect_starts(&[
            Dg(0, 0),
            Dg(0, 1),
            Dg(0, 2),
            Dg(0, 3),
            Dg(1, 0),
            Rd(0, 0),
            Dg(1, 1),
            Dg(1, 2),
            Dg(1, 3),
        ]);
        assert!(b.done.contains_key(&op));
    }

    #[test]
    fn a_chunk_fenced_first_and_waited_goes_before_one_fenced_later() {
        fenced_first_and_waited(false);
    }

    /// S6: the same placement, A's digests differing. B, fenced late and waited, takes the
    /// slot A's digest frees, ahead of A's repairs.
    fn fenced_late(today: bool) {
        let mut b = Bench::with_chunks(&[&[1, 2], &[1, 3]], 1);
        b.today = today;
        b.max_bulk = 1;
        b.diverge(2, 0, 0, 9);
        b.diverge(2, 0, 2, 9);
        b.hold.insert(3, is_hello);
        b.hold.insert(2, is_digest);
        b.restart(1);
        let op = b.read_at(1, 0);
        b.deliver_from(3);
        b.deliver_from(2);
        b.expect_starts(&[Dg(0, 0), Dg(1, 0), Rp(0, 0), Rd(1, 0), Rp(0, 2)]);
        assert!(b.done.contains_key(&op));
    }

    #[test]
    fn a_chunk_fenced_late_takes_the_next_free_slot() {
        fenced_late(false);
    }

    /// S6: two slots; chunks 0 (X) and 1 (W) differ in blocks 0 and 2, and W is waited. X's
    /// repairs, queued at the front after W's, go only to slots W cannot use.
    fn ahead_of_repairs(today: bool) {
        let mut b = Bench::with_chunks(&[&[1, 2], &[1, 2]], 1);
        b.today = today;
        b.max_bulk = 2;
        for c in 0..2 {
            b.diverge(2, c, 0, 9);
            b.diverge(2, c, 2, 9);
        }
        b.hold.insert(2, is_digest);
        b.restart(1);
        let op = b.read_at(1, 0);
        b.hold.insert(2, is_digest_or_write);
        fn w_digest(m: &ToReplica<Vec<Value>>) -> bool {
            matches!(m.body, Request::Digest { chunk: 1, .. })
        }
        fn w_write_0(m: &ToReplica<Vec<Value>>) -> bool {
            matches!(m.body, Request::Write { chunk: 1, ext, .. } if ext.start == 0)
        }
        fn w_write_2(m: &ToReplica<Vec<Value>>) -> bool {
            matches!(m.body, Request::Write { chunk: 1, ext, .. } if ext.start == 2)
        }
        let i = b.held_from(2, w_digest);
        b.deliver(i);
        b.expect_starts(&[Dg(0, 0), Dg(1, 0), Rp(1, 0)]);
        // X's digest: its repairs go to the front, the slot it frees to W.
        let i = b.held_from(2, is_digest);
        b.deliver(i);
        b.expect_starts(&[Dg(0, 0), Dg(1, 0), Rp(1, 0), Rp(1, 2)]);
        // W has nothing left to start: the slot goes to X meanwhile.
        let i = b.held_from(2, w_write_0);
        b.deliver(i);
        b.expect_starts(&[Dg(0, 0), Dg(1, 0), Rp(1, 0), Rp(1, 2), Rp(0, 0)]);
        let i = b.held_from(2, w_write_2);
        b.deliver(i);
        b.expect_starts(&[
            Dg(0, 0),
            Dg(1, 0),
            Rp(1, 0),
            Rp(1, 2),
            Rp(0, 0),
            Rp(0, 2),
            Rd(1, 0),
        ]);
        assert_eq!(b.done.get(&op), Some(&OpResult::Read(vec![0])));
    }

    #[test]
    fn a_waited_chunk_goes_ahead_of_repairs_queued_before_it() {
        ahead_of_repairs(false);
    }

    /// S6, no idle slot: chunk 1 on nodes 1 and 3 is waited, and its digest cannot start
    /// (node 3's session is down, the ISR at its minimum); chunks 0 and 2 reconcile meanwhile.
    /// (Today's pump idles no slot either: this test passes with it.)
    #[test]
    fn a_waited_digest_that_cannot_start_holds_nothing() {
        let mut b = Bench::with_chunks(&[&[1, 2], &[1, 3], &[1, 2]], 2);
        b.max_bulk = 1;
        b.hold.insert(2, is_hello);
        b.hold.insert(3, is_digest);
        b.restart(1);
        b.expect_starts(&[Dg(1, 0)]);
        b.hold.insert(3, is_hello);
        let i = b.held_from(3, is_digest);
        b.deliver_wrong(i);
        assert!(b.t.session_of(3).is_none());
        let op = b.read_at(1, 0);
        b.deliver_from(2);
        b.expect_starts(&[Dg(1, 0), Dg(0, 0), Dg(2, 0)]);
        assert!(b.serves(0) && b.serves(2) && !b.serves(1));
        assert_eq!(b.done.get(&op), None);
    }

    /// S8: chunk 0's digest fails in flight with node 2's session (the ISR at its minimum, so
    /// no removal) and goes back to the front; chunk 1's, waited, still goes first.
    fn sent_again(today: bool) {
        let mut b = Bench::with_chunks(&[&[1, 2], &[1, 2]], 2);
        b.today = today;
        b.max_bulk = 1;
        b.hold.insert(2, is_digest);
        b.restart(1);
        let op = b.read_at(1, 0);
        let i = b.held_from(2, is_digest);
        b.deliver_wrong(i);
        assert!(matches!(
            b.t.queue.front(),
            Some(Work::Digest { chunk: 0, .. })
        ));
        b.hold.clear();
        b.until("node 2's session open", |b| b.t.session_of(2).is_some());
        b.expect_starts(&[Dg(0, 0), Dg(1, 0), Dg(0, 0), Rd(1, 0)]);
        assert!(b.done.contains_key(&op));
    }

    #[test]
    fn a_digest_sent_again_does_not_pass_a_waited_chunk() {
        sent_again(false);
    }

    /// A reply that does not match: a digest reply of the wrong length, or of the wrong type,
    /// fails the session and sends the digest again, its slot freed.
    #[test]
    fn a_digest_reply_of_the_wrong_length_frees_its_slot() {
        for body in [Response::Digests(vec![Some(0)]), Response::Ok] {
            let mut b = Bench::new(&[1, 2], 2);
            b.max_bulk = 1;
            b.hold.insert(2, is_digest);
            b.restart(1);
            assert_eq!((b.t.bulk, b.t.digests.len()), (1, 1));
            let i = b.held_from(2, is_digest);
            b.deliver_as(i, body.clone());
            assert_eq!((b.t.bulk, b.t.digests.len()), (0, 0), "{body:?}");
            assert!(matches!(
                b.t.queue.front(),
                Some(Work::Digest { chunk: 0, .. })
            ));
            b.hold.clear();
            b.until("serving", |b| b.serves(0));
            b.expect_starts(&[Dg(0, 0), Dg(0, 0)]);
        }
    }

    /// A reply of the wrong type: a client read goes again, before the write on its block
    /// queued behind it. (The wrong-type digest is in the test above.)
    #[test]
    fn a_reply_of_the_wrong_type_requeues_its_request() {
        let mut b = Bench::new(&[1, 2], 2);
        b.write(0, 42);
        b.hold.insert(TARGET, is_read);
        let r = b.read(0);
        let w = b.write_at(0, 0, 43);
        assert_eq!(b.done.get(&w), None, "behind the read");
        let i = b.held_from(TARGET, is_read);
        b.deliver_wrong(i);
        b.hold.clear();
        b.until("write done", |b| b.done.contains_key(&w));
        assert_eq!(b.done.get(&r), Some(&OpResult::Read(vec![42])));
        assert_eq!(b.done.get(&w), Some(&OpResult::Written));
        assert_eq!(b.reads_to, [1, 2]);
        let first_write = b.client_log.iter().position(|&op| op == w);
        assert_eq!(first_write, Some(3), "{:?}", b.client_log);
    }

    /// Chunk 0 on nodes 1, 3 and 4, node 4 out of its ISR and lacking block 0: a discard of
    /// blocks 1 to 3, a digest of block 0, then its copy.
    fn resyncing(hold: Pred) -> Bench {
        let mut b = Bench::with_chunks(&[&[1, 3, 4]], 1);
        b.out_of_isr(0, 4);
        b.diverge(1, 0, 0, 7);
        b.diverge(3, 0, 0, 7);
        b.max_bulk = 1;
        b.hold.insert(4, hold);
        b.restart(3);
        b
    }

    /// A reply of the wrong type to a copy, or to a discard: the slot is freed once, nothing
    /// goes back to the queue, and the round is aborted.
    #[test]
    fn a_copy_reply_of_the_wrong_type_frees_its_slot_once() {
        let cases: [(Pred, &[Start]); 2] = [
            (is_copy, &[Dg(0, 0), Ds(0, 1), Rs(0, 0), Cp(0, 0)]),
            (is_discard, &[Dg(0, 0), Ds(0, 1)]),
        ];
        for (hold, starts) in cases {
            let mut b = resyncing(hold);
            b.expect_starts(starts);
            let round = b.t.chunks[0].resync.as_ref().expect("a resync").round;
            assert_eq!(b.t.bulk, 1);
            let i = b.held_from(4, hold);
            b.deliver_wrong(i);
            assert_eq!(b.t.bulk, 0);
            assert!(b.t.chunks[0].resync.is_none(), "the round is aborted");
            assert!(
                b.t.queue
                    .iter()
                    .all(|w| !matches!(w.class(), Class::Resync(_, r) if r == round)),
                "nothing of the round is queued"
            );
            b.hold.clear();
            b.until("node 4 in the ISR", |b| b.isr().contains(&4));
            assert_eq!(b.replicas[&4].block(VOL, 0, 0), Some(7));
        }
    }

    /// A reply of the wrong type to a `Mapped` probe aborts its round: it does not wait for
    /// the probe for ever.
    #[test]
    fn a_mapped_reply_of_the_wrong_type_aborts_the_round() {
        let mut b = Bench::with_chunks(&[&[1, 3, 4]], 2);
        b.out_of_isr(0, 4);
        b.diverge(1, 0, 0, 7);
        b.diverge(3, 0, 0, 7);
        b.hold.insert(TARGET, is_mapped);
        b.restart(3);
        let round = {
            let r = b.t.chunks[0].resync.as_ref().expect("a resync");
            assert_eq!(r.awaiting_mapped, 1);
            r.round
        };
        let i = b.held_from(TARGET, is_mapped);
        b.deliver_wrong(i);
        // The next round asks node 3, node 1's session being down.
        assert!(
            b.t.chunks[0]
                .resync
                .as_ref()
                .is_none_or(|r| r.round != round),
            "the round is aborted"
        );
        b.hold.clear();
        b.until("node 4 in the ISR", |b| b.isr().contains(&4));
        assert_eq!(b.replicas[&4].block(VOL, 0, 0), Some(7));
    }

    /// A random extent of a chunk.
    fn any_extent(rng: &mut Rng) -> Extent {
        let start = rng.range(0, u64::from(BLOCKS) - 1) as Block;
        let len = rng.range(1, u64::from(BLOCKS - start)) as Block;
        Extent::new(start, len)
    }

    /// S5: on random queues the pump starts the same client operations, in the same order, as
    /// today's, and admits the same fast reads. Chunks 1 up reconcile throughout (their
    /// digests and reads are held); in half the cases chunk 0 serves and resyncs node 4.
    #[test]
    fn client_starts_match_todays_pump() {
        for case in 0..1000u64 {
            let mut rng = Rng::new(case);
            let resync = case % 2 == 0;
            let k = rng.range(2, 4) as usize;
            let max_digest = rng.range(1, u64::from(BLOCKS)) as Block;
            let max_io = rng.range(1, u64::from(BLOCKS)) as Block;
            let mut data: Vec<(Block, Value)> = Vec::new();
            for blk in 0..BLOCKS {
                if rng.pct(60) {
                    data.push((blk, rng.range(1, 99)));
                }
            }
            let pieces = BLOCKS.div_ceil(max_digest) as usize * k;
            let build = |today: bool| {
                let first: &[NodeId] = if resync { &[1, 3, 4] } else { &[1, 3] };
                let mut placement = vec![first];
                placement.extend(std::iter::repeat_n(&[1, 2][..], k));
                let mut b = Bench::with_chunks(&placement, 1);
                if resync {
                    b.out_of_isr(0, 4);
                    for &(blk, v) in &data {
                        b.diverge(1, 0, blk, v);
                        b.diverge(3, 0, blk, v);
                    }
                    b.hold.insert(4, is_resync);
                }
                b.today = today;
                b.max_digest = max_digest;
                b.max_io = max_io;
                // Every digest in flight, and two slots more for the resync.
                b.max_bulk = if resync { pieces + 2 } else { pieces };
                b.hold.insert(1, is_reconcile_off_chunk_0);
                b.hold.insert(2, is_reconcile_off_chunk_0);
                b.restart(2);
                b
            };
            let mut benches = [build(false), build(true)];
            let mut answers: [Vec<String>; 2] = Default::default();
            let mut fast: [Vec<FastRead>; 2] = Default::default();
            for _ in 0..rng.range(10, 60) {
                let chunk = rng.range(0, k as u64) as ChunkIdx;
                let ext = any_extent(&mut rng);
                let roll = rng.range(0, 99);
                let pick = rng.next_u64();
                for (j, b) in benches.iter_mut().enumerate() {
                    match roll {
                        0..=29 => {
                            b.submit(Op::Read { chunk, ext });
                        }
                        30..=54 => {
                            let data = vec![pick % 100; ext.len as usize];
                            b.submit(Op::Write { chunk, ext, data });
                        }
                        55..=69 => {
                            let fr = b.t.try_fast_read(b.now, chunk, ext);
                            answers[j].push(format!("{fr:?}"));
                            fast[j].extend(fr);
                        }
                        70..=79 if !fast[j].is_empty() => {
                            let fr = fast[j].remove(pick as usize % fast[j].len());
                            let ok = b.t.finish_fast_read(b.now, fr, true);
                            answers[j].push(format!("finish {ok}"));
                            b.run();
                        }
                        _ => {
                            if let Some(i) = b.held.iter().position(|h| h.0 == 4) {
                                b.deliver(i);
                            }
                        }
                    }
                }
            }
            for (j, b) in benches.iter_mut().enumerate() {
                for fr in std::mem::take(&mut fast[j]) {
                    let ok = b.t.finish_fast_read(b.now, fr, true);
                    answers[j].push(format!("finish {ok}"));
                    b.run();
                }
                assert!((1..=k).all(|c| !b.serves(c as ChunkIdx)), "case {case}");
            }
            let [a, t] = &benches;
            assert_eq!(a.client_log, t.client_log, "case {case}: client starts");
            assert_eq!(answers[0], answers[1], "case {case}: fast reads");
            assert_eq!(a.done, t.done, "case {case}: results");
        }
    }

    /// S5: of two client operations on overlapping blocks, the one submitted first starts
    /// first, with replies delivered in random order (in order per node) and at random times,
    /// sessions failing on replies of the wrong type, and chunks coming to serve along the
    /// way.
    #[test]
    fn overlapping_client_operations_start_in_queue_order() {
        for case in 0..1000u64 {
            let mut rng = Rng::new(case);
            let nodes: &[NodeId] = if rng.pct(50) { &[1, 2, 3] } else { &[1, 2] };
            let chunks = rng.range(1, 3) as usize;
            let min_isr = rng.range(1, 2) as usize;
            let mut b = Bench::with_chunks(&vec![nodes; chunks], min_isr);
            for c in 0..chunks as ChunkIdx {
                for blk in 0..BLOCKS {
                    if rng.pct(30) {
                        let v = rng.range(1, 99);
                        b.diverge(rng.pick(nodes), c, blk, v);
                    }
                }
            }
            b.max_bulk = rng.range(1, 4) as usize;
            b.max_digest = rng.range(1, u64::from(BLOCKS)) as Block;
            b.max_io = rng.range(1, u64::from(BLOCKS)) as Block;
            b.hold_all(is_any);
            b.restart(rng.pick(nodes));
            let mut ops: Vec<(u64, ChunkIdx, Extent)> = Vec::new();
            for _ in 0..rng.range(20, 100) {
                let roll = rng.range(0, 99);
                let from = rng.pick(nodes);
                match roll {
                    0..=29 => {
                        let chunk = rng.range(0, chunks as u64 - 1) as ChunkIdx;
                        let ext = any_extent(&mut rng);
                        let op = if rng.pct(50) {
                            b.submit(Op::Read { chunk, ext })
                        } else {
                            let v = rng.range(1, 99);
                            b.submit(Op::Write {
                                chunk,
                                ext,
                                data: vec![v; ext.len as usize],
                            })
                        };
                        ops.push((op, chunk, ext));
                    }
                    30..=79 => {
                        if let Some(i) = b.held.iter().position(|h| h.0 == from) {
                            b.deliver(i);
                        }
                    }
                    80..=94 => b.advance(rng.range(1, 400) * MS),
                    _ => {
                        if let Some(i) = b.held.iter().position(|h| h.0 == from) {
                            b.deliver_wrong(i);
                        }
                    }
                }
            }
            b.drain();
            let first = |op: u64| {
                b.client_log
                    .iter()
                    .position(|&o| o == op)
                    .unwrap_or_else(|| panic!("case {case}: op {op} never started"))
            };
            for (i, &(a, ca, xa)) in ops.iter().enumerate() {
                for &(z, cz, xz) in &ops[i + 1..] {
                    if ca == cz && xa.overlaps(&xz) {
                        assert!(
                            first(a) < first(z),
                            "case {case}: op {z} started before op {a}: {:?}",
                            b.client_log
                        );
                    }
                }
            }
        }
    }
}
