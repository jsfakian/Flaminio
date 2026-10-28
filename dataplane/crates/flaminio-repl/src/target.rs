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
use crate::msg::{FromReplica, MAX_MAPPED_BLOCKS, Request, Response, ToReplica};
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
    meta_inflight: Option<(u64, Time)>,
    next_heartbeat: Time,
    /// When to read the volume from the metadata service next, and the read in flight. It has
    /// its own slot: an ISR change must never wait behind it.
    next_refresh: Time,
    refresh_inflight: Option<(u64, Time)>,
    deposed: bool,
    out: Vec<Output<P>>,
    events: Vec<Event>,
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
        let mut t = Self {
            node,
            vol,
            epoch: view.epoch,
            min_isr: view.min_isr,
            blocks_per_chunk: view.blocks_per_chunk,
            max_io: max_io.max(1),
            max_mapped: MAX_MAPPED_BLOCKS,
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
            meta_inflight: None,
            next_heartbeat: now,
            next_refresh: now + timing.refresh,
            refresh_inflight: None,
            deposed: false,
            out: Vec::new(),
            events: Vec::new(),
            #[cfg(test)]
            bugs: Bugs::default(),
        };
        t.tick(now);
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

    /// Sets how many repairs and resync copies may be in flight at once (at least one).
    pub fn set_max_bulk(&mut self, n: usize) {
        self.max_bulk = n.max(1);
    }

    /// Sets the largest extent one `Mapped` probe covers, from one block to
    /// [`MAX_MAPPED_BLOCKS`] (the default).
    pub fn set_max_mapped(&mut self, n: Block) {
        self.max_mapped = n.clamp(1, MAX_MAPPED_BLOCKS);
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
        if self.deposed {
            self.out.push(Output::Done {
                op,
                result: OpResult::Failed,
            });
            return;
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
                self.on_digest(now, from, id, sums);
                scope = Scope::All;
            }
            (ReqKind::Mapped { chunk, round }, Response::Mapped(exts)) => {
                self.on_mapped(now, chunk, round, exts);
            }
            (ReqKind::BeginResync { chunk, round }, Response::Ok) => {
                let max_io = self.max_io;
                if let Some(r) = self.resync_mut(chunk, round) {
                    // Writes that landed after Mapped was applied, before this Ack.
                    let force = std::mem::take(&mut r.force_copy);
                    r.todo.extend(&force);
                    for e in force.iter() {
                        r.discard.remove(&e);
                    }
                    r.started = true;
                    let discards = r.discard.pieces(max_io);
                    let digests = r.todo.pieces(max_io);
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
                // A reply that does not answer its request: treat the session as broken.
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
                    self.send_meta(now);
                    self.pump(now);
                }
            }
            Scope::Queue => self.pump(now),
        }
    }

    /// Handles the reply to metadata request `req`.
    pub fn on_meta(&mut self, now: Time, req: u64, reply: &MetaReply) {
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
        self.reads.clear();
        self.writes.clear();
        self.digests.clear();
        self.peers.clear();
        self.meta_inflight = None;
        self.refresh_inflight = None;
        self.bulk = 0;
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
    fn on_digest(&mut self, now: Time, from: NodeId, id: u64, sums: Vec<Option<u32>>) {
        let Some(d) = self.digests.get_mut(&id) else {
            return;
        };
        if !d.need.contains(&from) || sums.len() != d.ext.len as usize {
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
        #[cfg(test)]
        let removing_ok = self.bugs.read_from_removing;
        #[cfg(not(test))]
        let removing_ok = false;
        let usable = |n: &NodeId| {
            (removing_ok || !c.removing.contains(n))
                && Some(*n) != exclude
                && self.session_of(*n).is_some()
        };
        if c.isr.contains(&self.node) && usable(&self.node) {
            return Some(self.node);
        }
        c.isr.iter().copied().find(|n| usable(n))
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
                        let Some(from) = self.source(i, None) else {
                            return false;
                        };
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

    /// Starts whatever queued work can start. Client operations on overlapping extents start
    /// in submission order; internal work (repairs, copies) is ordered by locks alone.
    fn pump(&mut self, now: Time) {
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
                        // Digests first, then repairs only where ISR members disagree.
                        for ext in pending.pieces(self.max_io).into_iter().rev() {
                            self.queue.push_front(Work::Digest {
                                chunk: i as ChunkIdx,
                                ext,
                            });
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
        for r in &removed {
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
