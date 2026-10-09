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
use crate::msg::{FromReplica, Request, Response, ToReplica};
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
    started: bool,
    /// Blocks not yet copied and acknowledged.
    todo: ExtentSet,
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
}

impl Chunk {
    fn required(&self) -> BTreeSet<NodeId> {
        let mut s = self.isr.clone();
        s.extend(self.joining);
        s
    }
}

#[derive(Debug)]
enum Work<P> {
    Client {
        op: u64,
        kind: Op<P>,
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
    Copy {
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
    timing: Timing,
    chunks: Vec<Chunk>,
    peers: BTreeMap<NodeId, Peer>,
    next_session: u64,
    next_req: u64,
    next_seq: u64,
    next_read: u64,
    next_round: u64,
    /// Repairs and resync copies in flight, and the most allowed. Bulk work shares the
    /// sessions with client I/O and heartbeats, so it is bounded: unbounded, it would queue
    /// enough data ahead of them to make their requests time out.
    bulk: usize,
    max_bulk: usize,
    writes: BTreeMap<u64, WriteOp<P>>,
    reads: BTreeMap<u64, ReadOp>,
    queue: VecDeque<Work<P>>,
    meta_inflight: Option<(u64, Time)>,
    next_heartbeat: Time,
    deposed: bool,
    out: Vec<Output<P>>,
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
            timing,
            chunks,
            peers,
            next_session: 1,
            next_req: 1,
            next_seq: 0,
            next_read: 0,
            next_round: 0,
            bulk: 0,
            max_bulk: DEFAULT_MAX_BULK,
            writes: BTreeMap::new(),
            reads: BTreeMap::new(),
            queue: VecDeque::new(),
            meta_inflight: None,
            next_heartbeat: now,
            deposed: false,
            out: Vec::new(),
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
            && self.meta_inflight.is_none()
            && self.chunks.iter().all(|c| {
                c.phase == Phase::Ready
                    && c.isr.len() == c.replicas.len()
                    && c.removing.is_empty()
                    && c.joining.is_none()
            })
    }

    /// A one-line summary of the target's state, for failure reports.
    pub fn describe(&self) -> String {
        let chunks: Vec<String> = self
            .chunks
            .iter()
            .map(|c| {
                format!(
                    "{:?} isr {:?} fenced {:?} removing {:?} joining {:?} resync {:?} locks {:?}",
                    c.phase,
                    c.isr,
                    c.fenced,
                    c.removing,
                    c.joining,
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

    /// Takes the outputs produced so far.
    pub fn take_output(&mut self) -> Vec<Output<P>> {
        std::mem::take(&mut self.out)
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
        self.progress(now);
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
            .map(|(_, t)| t + self.timing.meta_timeout);
        requests
            .chain(fences)
            .chain(meta)
            .chain([self.next_heartbeat])
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
        self.progress(now);
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
        match (kind, &m.body) {
            (ReqKind::Hello, Response::Fenced { uncertain }) => {
                self.on_fenced(now, from, uncertain);
                self.retransmit(now, from);
            }
            (ReqKind::Heartbeat, Response::Ok) => {}
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
            }
            (ReqKind::Read { id }, Response::Data(data)) => {
                let data = data.clone();
                self.on_read(now, id, data)
            }
            (ReqKind::BeginResync { chunk, round }, Response::Ok) => {
                let max_io = self.max_io;
                if let Some(r) = self.resync_mut(chunk, round) {
                    r.started = true;
                    let pieces = r.todo.pieces(max_io);
                    for ext in pieces {
                        self.queue.push_back(Work::Copy { chunk, round, ext });
                    }
                }
            }
            (ReqKind::Copy { chunk, round, ext }, Response::Ok) => {
                self.release_bulk();
                if let Some(r) = self.resync_mut(chunk, round) {
                    r.todo.remove(&ext);
                }
            }
            _ => {
                // A reply that does not answer its request: treat the session as broken.
                self.fail_peer(now, from);
            }
        }
        self.progress(now);
    }

    /// Handles the reply to metadata request `req`.
    pub fn on_meta(&mut self, now: Time, req: u64, reply: &MetaReply) {
        if self.deposed || self.meta_inflight.is_none_or(|(r, _)| r != req) {
            return;
        }
        self.meta_inflight = None;
        match reply {
            MetaReply::Applied { view, .. } | MetaReply::Rejected { view, .. } => {
                if view.epoch > self.epoch {
                    self.depose();
                    return;
                }
                if view.epoch == self.epoch {
                    self.apply_view(view);
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
        self.peers.clear();
        self.meta_inflight = None;
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
            if let ReqKind::Copy { .. } = kind {
                self.release_bulk();
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
            targets.insert(r.member);
            r.live.insert(ext);
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
        let wait = self.timing.takeover_wait();
        for i in 0..self.chunks.len() {
            // ISR members without a session have failed.
            if self.chunks[i].phase != Phase::Fencing {
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
                        // Repairs go first: client operations wait for the chunk anyway.
                        for ext in pending.pieces(self.max_io).into_iter().rev() {
                            self.queue.push_front(Work::Repair {
                                chunk: i as ChunkIdx,
                                ext,
                            });
                        }
                        c.phase = Phase::Reconciling { pending };
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
            }
            if c.phase == Phase::Ready {
                self.drive_resync(now, i);
            }
        }
        self.send_meta(now);
        self.pump(now);
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
            if r.started && r.todo.is_empty() && r.unacked.is_empty() && !unacked {
                let c = &mut self.chunks[i];
                c.resync = None;
                c.joining = Some(member);
            }
            return;
        }
        let Some(member) = c
            .replicas
            .iter()
            .copied()
            .find(|n| !c.isr.contains(n) && self.session_of(*n).is_some())
        else {
            return;
        };
        let session = self.session_of(member).expect("open");
        let round = self.next_round;
        self.next_round += 1;
        let todo = match c.dirty.get(&member) {
            Some(d) => d.clone(),
            None => ExtentSet::of(Extent::new(0, self.blocks_per_chunk)),
        };
        self.chunks[i].resync = Some(Resync {
            member,
            round,
            session,
            started: false,
            todo,
            live: ExtentSet::new(),
            unacked: BTreeSet::new(),
        });
        let chunk = i as ChunkIdx;
        self.send(
            now,
            member,
            Request::BeginResync { chunk },
            ReqKind::BeginResync { chunk, round },
        );
        // Writes already in flight are not sent to the member. Every copy of their blocks
        // reads its source after them, over the same ordered session, so it carries them;
        // and the member joins only once they have completed.
    }

    /// Proposes the pending ISR changes, one request at a time.
    fn send_meta(&mut self, now: Time) {
        if self.meta_inflight.is_some() {
            return;
        }
        let changes: Vec<IsrChange> = self
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.removing.is_empty() || c.joining.is_some())
            .map(|(i, c)| {
                let mut new_isr: BTreeSet<NodeId> =
                    c.isr.difference(&c.removing).copied().collect();
                new_isr.extend(c.joining);
                IsrChange {
                    chunk: i as ChunkIdx,
                    expected_ver: c.isr_ver,
                    new_isr,
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
    fn apply_view(&mut self, view: &VolumeMeta) {
        for (i, m) in view.chunks.iter().enumerate() {
            let c = &self.chunks[i];
            if m.isr_ver <= c.isr_ver {
                continue;
            }
            let removed: Vec<NodeId> = c.isr.difference(&m.isr).copied().collect();
            let during_io = c.phase != Phase::Fencing;
            for r in removed {
                let mut d = ExtentSet::new();
                for w in self.writes.values() {
                    if w.chunk as usize == i && !w.acked.contains(&r) {
                        d.insert(w.ext);
                    }
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
            if let Some(j) = c.joining
                && m.isr.contains(&j)
            {
                c.joining = None;
                c.dirty.remove(&j);
            }
            c.isr = m.isr.clone();
            c.isr_ver = m.isr_ver;
            let isr = c.isr.clone();
            c.removing.retain(|n| isr.contains(n));
        }
        let seqs: Vec<u64> = self.writes.keys().copied().collect();
        for seq in seqs {
            self.check_write(seq);
        }
    }
}
