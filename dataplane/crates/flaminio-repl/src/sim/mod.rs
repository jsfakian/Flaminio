//! Deterministic simulator for the replication protocol (protocol spec §9).
//!
//! One run builds a cluster from a seed: nodes that each hold chunk replicas and may run a
//! target, a metadata service, a failover controller, and one client per node. Everything
//! happens on a single thread in event order, so a seed reproduces a run exactly.
//!
//! During the **chaos** phase the simulator injects:
//! - node crashes (volatile state lost);
//! - partitions of single links or whole nodes;
//! - metadata outages;
//! - message delay spikes;
//! - failovers at random moments;
//! - requested replica moves (drain). When a move drops its source, the source's copy of the
//!   chunk is deleted at once: nothing may still depend on it.
//!
//! During the **settle** phase it heals everything and waits for the current target to finish
//! resyncing. Then it reads every block and checks the run:
//! - linearizability per block, with the epoch rule for unanswered writes;
//! - identical replicas;
//! - a full ISR everywhere.

pub mod check;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use crate::extent::Extent;
use crate::meta::{MetaReply, MetaRequest, Metadata, VolumeMeta};
use crate::msg::{FromReplica, ToReplica};
use crate::replica::Replica;
use crate::target::{Op, OpResult, Output, Target};
use crate::types::{Block, ChunkIdx, Epoch, NodeId, Time, Timing, Value, VolumeId};

use check::{CKind, COp, check_register};

/// Payloads in the simulator: one value per block of the extent.
pub type Data = Vec<Value>;

/// SplitMix64: small, fast and good enough to drive a simulation.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next_u64() % (hi - lo + 1)
    }

    /// True with probability `p` percent.
    pub fn pct(&mut self, p: u64) -> bool {
        self.next_u64() % 100 < p
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.next_u64() % items.len() as u64) as usize]
    }
}

const MS: u64 = 1_000;
const SEC: u64 = 1_000_000;
/// The metadata service's network address.
pub const META: NodeId = NodeId::MAX;
const VOL: VolumeId = 1;
/// A run with more events than this is stuck in a loop.
const MAX_EVENTS: u64 = 3_000_000;

/// Shape and fault mix of one run.
#[derive(Debug, Clone)]
pub struct Config {
    pub nodes: u32,
    pub rf: usize,
    pub min_isr: usize,
    pub chunks: u32,
    pub blocks_per_chunk: Block,
    /// Largest client operation, in blocks.
    pub max_op: Block,
    /// Largest repair or copy, in blocks.
    pub max_io: Block,
    /// Repairs and copies in flight per target.
    pub max_bulk: usize,
    pub timing: Timing,
    /// Real time spent injecting faults.
    pub chaos: u64,
    /// Most real time allowed for settling before the run counts as a liveness failure.
    pub settle_limit: u64,
    /// Client operations issued during chaos, at most.
    pub max_ops: usize,
    /// Mean real time between faults.
    pub fault_every: u64,
    /// Percent chance that a controller round fails the volume over, even with its target
    /// alive.
    pub failover_pct: u64,
    /// Percent of messages that get a delay spike.
    pub spike_pct: u64,
    /// Range of real time between a client's operations.
    pub client_every: (u64, u64),
    /// Range of real time the controller takes to fail over a target it cannot reach.
    pub detect: (u64, u64),
    /// Percent of client operations that are reads.
    pub read_pct: u64,
    /// Percent chance that a controller round requests a replica move.
    pub move_pct: u64,
    /// Client reads take the fast path when the target admits them (spec §5, fast reads).
    pub fast_reads: bool,
}

impl Config {
    /// A random configuration for `seed`.
    pub fn random(seed: u64) -> Self {
        let mut r = Rng::new(seed ^ 0xC0FF_EE00);
        let nodes = r.range(3, 4) as u32;
        let rf = r.range(2, 3) as usize;
        let min_isr = r.range(1, rf as u64) as usize;
        Self {
            nodes,
            rf,
            min_isr,
            chunks: r.range(1, 2) as u32,
            blocks_per_chunk: r.range(2, 6) as Block,
            max_op: r.range(1, 3) as Block,
            max_io: r.range(1, 3) as Block,
            max_bulk: r.range(1, 4) as usize,
            timing: Timing {
                lease: r.range(2, 6) * SEC,
                drift_ppm: r.pick(&[0, 10_000, 100_000]),
                heartbeat: r.range(300, 1000) * MS,
                request_timeout: r.range(1000, 3000) * MS,
                reconnect: r.range(100, 800) * MS,
                meta_timeout: r.range(500, 2000) * MS,
                refresh: r.range(500, 3000) * MS,
            },
            chaos: r.range(20, 60) * SEC,
            settle_limit: 600 * SEC,
            max_ops: 1000,
            fault_every: r.range(1, 6) * SEC,
            failover_pct: r.range(5, 40),
            spike_pct: r.pick(&[0, 1, 3]),
            client_every: r.pick(&[(200, 5 * MS), (MS, 30 * MS), (5 * MS, 150 * MS)]),
            detect: r.pick(&[(100 * MS, 2 * SEC), (SEC, 10 * SEC)]),
            read_pct: r.pick(&[50, 80, 95]),
            move_pct: r.pick(&[0, 10, 30]),
            fast_reads: r.pct(50),
        }
    }
}

/// Statistics of a run that passed.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub ops: usize,
    pub completed: usize,
    pub failovers: u64,
    pub crashes: u64,
    pub partitions: u64,
    pub final_epoch: Epoch,
    pub events: u64,
}

/// Deliberate bugs to inject into every replica and target of a run.
#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Bugs {
    pub target: crate::target::Bugs,
    pub replica: crate::replica::Bugs,
}

/// A scripted step, for scenario tests: exact failures at exact times.
#[derive(Debug, Clone, Copy)]
pub enum Action {
    /// The failover controller attaches the volume to this node.
    Attach(NodeId),
    /// The link between two endpoints fails ([`META`] is the metadata service).
    Cut(NodeId, NodeId),
    Heal(NodeId, NodeId),
    /// Messages on the link take exactly this long.
    Slow(NodeId, NodeId, u64),
    /// The client on `node` reads or writes one block, if the node runs a live target.
    Read(NodeId, ChunkIdx, Block),
    Write(NodeId, ChunkIdx, Block),
}

#[derive(Debug, Clone)]
enum Body {
    ToReplica(ToReplica<Data>),
    FromReplica(FromReplica<Data>),
    MetaReq { req: u64, body: MetaRequest },
    MetaRep { req: u64, reply: MetaReply },
    Start { view: VolumeMeta },
}

#[derive(Debug, Clone)]
struct Packet {
    from: NodeId,
    to: NodeId,
    /// Incarnation of the receiving node at send time: a restarted node never sees packets
    /// meant for its previous life.
    to_inc: u64,
    conn: Conn,
    body: Body,
}

/// A connection. Session traffic is an ordered stream that breaks for good on its first
/// lost message, like TCP. Everything else is a datagram.
type Conn = Option<(Epoch, u64)>;

#[derive(Debug, Clone)]
enum Ev {
    Deliver(Packet),
    Wake {
        node: NodeId,
    },
    ClientStep {
        node: NodeId,
        inc: u64,
    },
    Fault,
    Restart {
        node: NodeId,
    },
    Heal {
        a: NodeId,
        b: NodeId,
    },
    Unslow {
        a: NodeId,
        b: NodeId,
    },
    Failover {
        epoch: Epoch,
    },
    MetaUp,
    Controller,
    EndChaos,
    Script(Action),
    /// A fast read's device read on the local replica, then its completion.
    FastDevice {
        node: NodeId,
        inc: u64,
        op: u64,
    },
    FastDone {
        node: NodeId,
        inc: u64,
        op: u64,
    },
}

/// A fast read in flight on a node: the admission, the target's epoch, and the data once read.
#[derive(Debug)]
struct Fast {
    fr: crate::target::FastRead,
    epoch: Epoch,
    data: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Copy)]
struct Clock {
    rate_ppm: u64,
    offset: u64,
}

impl Clock {
    fn local(&self, real: u64) -> Time {
        self.offset + (real as u128 * self.rate_ppm as u128 / 1_000_000) as u64
    }

    /// The first real time at which the local clock reads at least `local`.
    fn real_at(&self, local: Time) -> u64 {
        let d = local.saturating_sub(self.offset) as u128;
        (d * 1_000_000).div_ceil(self.rate_ppm as u128) as u64
    }
}

#[derive(Debug)]
struct Node {
    up: bool,
    inc: u64,
    clock: Clock,
    replica: Replica,
    target: Option<Target<Data>>,
    /// Client operations in flight: op id → extent.
    client: BTreeMap<u64, (ChunkIdx, Extent)>,
    /// Fast reads in flight: op id → read.
    fast: BTreeMap<u64, Fast>,
    wake_at: Option<u64>,
}

#[derive(Debug, Clone)]
struct HistOp {
    epoch: Epoch,
    chunk: ChunkIdx,
    ext: Extent,
    write: Option<Value>,
    invoke: u64,
    result: Option<(u64, OpResult<Data>)>,
}

/// One simulated run.
pub struct Sim {
    cfg: Config,
    rng: Rng,
    now: u64,
    seq: u64,
    events: BinaryHeap<Reverse<(u64, u64, usize)>>,
    payloads: Vec<Option<Ev>>,
    nodes: Vec<Node>,
    meta: Metadata,
    meta_up: bool,
    down_links: BTreeSet<(NodeId, NodeId)>,
    /// Links with high latency for a while (gray failure): range of extra delay.
    slow_links: BTreeMap<(NodeId, NodeId), (u64, u64)>,
    last_delivery: BTreeMap<(NodeId, NodeId, Conn), u64>,
    broken: BTreeSet<(NodeId, NodeId, Conn)>,
    history: Vec<HistOp>,
    chaos: bool,
    report: Report,
    /// `FLAMINIO_SIM_TRACE=chunk,block` prints what happens to that block.
    trace: Option<(ChunkIdx, Block)>,
    /// Scripted run: no random faults, operations or delays.
    script: Option<Vec<(u64, Action)>>,
    #[cfg(test)]
    bugs: Bugs,
}

impl Sim {
    pub fn new(seed: u64, cfg: Config) -> Self {
        let mut rng = Rng::new(seed);
        let ids: Vec<NodeId> = (0..cfg.nodes).collect();
        let mut placement = Vec::new();
        for _ in 0..cfg.chunks {
            let mut pool = ids.clone();
            let mut replicas = Vec::new();
            for _ in 0..cfg.rf {
                let i = (rng.next_u64() % pool.len() as u64) as usize;
                replicas.push(pool.remove(i));
            }
            placement.push(replicas);
        }
        Self::build(rng, cfg, placement, None)
    }

    /// A scripted run with the given placement. The script covers the chaos phase; settling
    /// and checking are as usual.
    pub fn scripted(cfg: Config, placement: Vec<Vec<NodeId>>, script: Vec<(u64, Action)>) -> Self {
        Self::build(Rng::new(1), cfg, placement, Some(script))
    }

    fn build(
        mut rng: Rng,
        cfg: Config,
        placement: Vec<Vec<NodeId>>,
        script: Option<Vec<(u64, Action)>>,
    ) -> Self {
        let mut meta = Metadata::default();
        meta.create_volume(VOL, cfg.min_isr, cfg.blocks_per_chunk, placement.clone());
        let drift = cfg.timing.drift_ppm;
        let nodes = (0..cfg.nodes)
            .map(|n| {
                let mut replica = Replica::with_chunk_size(cfg.blocks_per_chunk);
                for (c, r) in placement.iter().enumerate() {
                    if r.contains(&n) {
                        replica.create_chunk(VOL, c as ChunkIdx, cfg.blocks_per_chunk);
                    }
                }
                Node {
                    up: true,
                    inc: 0,
                    clock: Clock {
                        rate_ppm: 1_000_000 + rng.range(0, drift),
                        offset: rng.range(0, 1000 * SEC),
                    },
                    replica,
                    target: None,
                    client: BTreeMap::new(),
                    fast: BTreeMap::new(),
                    wake_at: None,
                }
            })
            .collect();
        Self {
            cfg,
            rng,
            now: 0,
            seq: 0,
            events: BinaryHeap::new(),
            payloads: Vec::new(),
            nodes,
            meta,
            meta_up: true,
            down_links: BTreeSet::new(),
            slow_links: BTreeMap::new(),
            last_delivery: BTreeMap::new(),
            broken: BTreeSet::new(),
            history: Vec::new(),
            chaos: true,
            report: Report::default(),
            trace: std::env::var("FLAMINIO_SIM_TRACE").ok().and_then(|v| {
                let (c, b) = v.split_once(',')?;
                Some((c.parse().ok()?, b.parse().ok()?))
            }),
            script,
            #[cfg(test)]
            bugs: Bugs::default(),
        }
    }

    #[cfg(test)]
    pub fn cfg_mut(&mut self) -> &mut Config {
        &mut self.cfg
    }

    #[cfg(test)]
    pub fn with_bugs(mut self, bugs: Bugs) -> Self {
        self.bugs = bugs;
        for n in &mut self.nodes {
            n.replica.core.bugs = bugs.replica;
        }
        self
    }

    fn at(&mut self, t: u64, ev: Ev) {
        self.payloads.push(Some(ev));
        self.events
            .push(Reverse((t, self.seq, self.payloads.len() - 1)));
        self.seq += 1;
    }

    /// Runs chaos, settles, reads every block and checks the run.
    pub fn run(mut self) -> Result<Report, String> {
        self.at(self.cfg.chaos, Ev::EndChaos);
        if let Some(script) = self.script.clone() {
            for (t, a) in script {
                self.at(t, Ev::Script(a));
            }
            self.at(self.cfg.chaos, Ev::Controller);
        } else {
            self.at(0, Ev::Controller);
            self.at(self.cfg.fault_every, Ev::Fault);
            for n in 0..self.cfg.nodes {
                self.at(0, Ev::ClientStep { node: n, inc: 0 });
            }
        }
        let deadline = self.cfg.chaos + self.cfg.settle_limit;
        // Chaos, then settle until the current target is settled.
        loop {
            if !self.chaos && self.settled().is_some() {
                break;
            }
            if self.now > deadline {
                return Err(self.fail("liveness: did not settle"));
            }
            if !self.step() {
                return Err(self.fail("liveness: no events left"));
            }
            if self.seq > MAX_EVENTS {
                return Err(self.fail("event storm"));
            }
        }
        // Read every block through the settled target.
        let node = self.settled().expect("settled");
        let mut pending = BTreeSet::new();
        for c in 0..self.cfg.chunks {
            for b in 0..self.cfg.blocks_per_chunk {
                pending.insert(self.submit(node, c, Extent::new(b, 1), None));
            }
        }
        let read_deadline = self.now + 60 * SEC;
        while pending
            .iter()
            .any(|&op| self.history[op as usize].result.is_none())
        {
            if self.now > read_deadline || !self.step() {
                return Err(self.fail("liveness: final reads did not complete"));
            }
        }
        // Messages delayed during chaos may still fail a session: settle again.
        while self.settled().is_none() {
            if self.now > read_deadline + self.cfg.settle_limit || !self.step() {
                return Err(self.fail("liveness: did not settle after the final reads"));
            }
        }
        self.check().map_err(|e| self.fail(&e))?;
        self.report.events = self.seq;
        self.report.ops = self.history.len();
        self.report.completed = self.history.iter().filter(|h| h.result.is_some()).count();
        self.report.final_epoch = self.meta.volume(VOL).expect("volume").epoch;
        Ok(self.report)
    }

    fn fail(&self, what: &str) -> String {
        let v = self.meta.volume(VOL).expect("volume");
        let targets: Vec<String> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                n.target.as_ref().map(|t| {
                    format!(
                        "node {i}: epoch {} deposed {} settled {}: {}",
                        t.epoch(),
                        t.is_deposed(),
                        t.is_settled(),
                        t.describe()
                    )
                })
            })
            .collect();
        format!(
            "{what} at t={}s; meta epoch {} target {:?} isr {:?}; client {:?}; targets [{}]",
            self.now / SEC,
            v.epoch,
            v.target,
            v.chunks.iter().map(|c| &c.isr).collect::<Vec<_>>(),
            self.nodes
                .iter()
                .map(|n| n.client.len())
                .collect::<Vec<_>>(),
            targets.join("; ")
        )
    }

    /// The node whose target holds the current epoch and has settled, if any.
    fn settled(&self) -> Option<NodeId> {
        let v = self.meta.volume(VOL)?;
        let n = v.target?;
        let node = &self.nodes[n as usize];
        let t = node.target.as_ref()?;
        (node.up && t.epoch() == v.epoch && t.is_settled() && node.client.is_empty()).then_some(n)
    }

    fn step(&mut self) -> bool {
        let Some(Reverse((t, _, idx))) = self.events.pop() else {
            return false;
        };
        self.now = t;
        let ev = self.payloads[idx].take().expect("event");
        match ev {
            Ev::Deliver(p) => self.deliver(p),
            Ev::Wake { node } => {
                let n = &mut self.nodes[node as usize];
                if n.wake_at == Some(t) {
                    n.wake_at = None;
                    let local = n.clock.local(t);
                    if let Some(tg) = n.target.as_mut() {
                        tg.tick(local);
                    }
                    self.after(node);
                }
            }
            Ev::ClientStep { node, inc } => self.client_step(node, inc),
            Ev::Fault => self.fault(),
            Ev::Restart { node } => self.restart(node),
            Ev::Heal { a, b } => {
                self.down_links.remove(&(a, b));
            }
            Ev::Unslow { a, b } => {
                self.slow_links.remove(&(a, b));
            }
            Ev::Failover { epoch } => self.failover(epoch),
            Ev::MetaUp => self.meta_up = true,
            Ev::Controller => self.controller(),
            Ev::FastDevice { node, inc, op } => self.fast_device(node, inc, op),
            Ev::FastDone { node, inc, op } => self.fast_done(node, inc, op),
            Ev::EndChaos => self.end_chaos(),
            Ev::Script(a) => self.script_step(a),
        }
        true
    }

    fn link_down(&self, a: NodeId, b: NodeId) -> bool {
        a != b && self.down_links.contains(&(a.min(b), a.max(b)))
    }

    fn up(&self, n: NodeId) -> bool {
        if n == META {
            self.meta_up
        } else {
            self.nodes[n as usize].up
        }
    }

    fn inc(&self, n: NodeId) -> u64 {
        if n == META {
            0
        } else {
            self.nodes[n as usize].inc
        }
    }

    fn send(&mut self, from: NodeId, to: NodeId, conn: Conn, body: Body) {
        let key = (from, to, conn);
        let random = self.chaos && self.script.is_none();
        let lost = !self.up(to)
            || self.link_down(from, to)
            || self.broken.contains(&key)
            || (conn.is_none() && random && self.rng.pct(1));
        if lost {
            self.break_conn(from, to, conn);
            return;
        }
        let latency = if from == to {
            self.rng.range(5, 50)
        } else if random && self.rng.pct(self.cfg.spike_pct) {
            self.rng.range(500 * MS, 8 * SEC)
        } else if let Some(&(lo, hi)) = self.slow_links.get(&(from.min(to), from.max(to))) {
            self.rng.range(lo, hi)
        } else if self.script.is_some() {
            500
        } else {
            self.rng.range(100, 2 * MS)
        };
        let mut t = self.now + latency;
        if conn.is_some() {
            let last = self.last_delivery.entry(key).or_insert(0);
            t = t.max(*last);
            *last = t;
        }
        let to_inc = self.inc(to);
        self.at(
            t,
            Ev::Deliver(Packet {
                from,
                to,
                to_inc,
                conn,
                body,
            }),
        );
    }

    fn break_conn(&mut self, a: NodeId, b: NodeId, conn: Conn) {
        if conn.is_some() {
            self.broken.insert((a, b, conn));
            self.broken.insert((b, a, conn));
        }
    }

    fn deliver(&mut self, p: Packet) {
        let key = (p.from, p.to, p.conn);
        if !self.up(p.to)
            || p.to_inc != self.inc(p.to)
            || self.link_down(p.from, p.to)
            || self.broken.contains(&key)
        {
            self.break_conn(p.from, p.to, p.conn);
            return;
        }
        match p.body {
            Body::ToReplica(m) => {
                if let Some((tc, tb)) = self.trace {
                    use crate::msg::Request as R;
                    let watched = match m.body {
                        R::Write { chunk, ext, .. }
                        | R::Copy { chunk, ext, .. }
                        | R::Read { chunk, ext } => chunk == tc && ext.contains(tb),
                        R::BeginResync { chunk } | R::Settle { chunk, .. } => chunk == tc,
                        R::Hello => true,
                        R::Heartbeat => false,
                    };
                    if watched {
                        let before = self.nodes[p.to as usize].replica.block(VOL, tc, tb);
                        let r = self.nodes[p.to as usize].replica.clone().handle(&m);
                        let mut probe = self.nodes[p.to as usize].replica.clone();
                        probe.handle(&m);
                        eprintln!(
                            "{:>10} {}->{} e{} s{} {:?} | block {:?}->{:?} reply {:?}",
                            self.now,
                            p.from,
                            p.to,
                            m.epoch,
                            m.session,
                            m.body,
                            before,
                            probe.block(VOL, tc, tb),
                            r.map(|r| r.body)
                        );
                    }
                }
                let reply = self.nodes[p.to as usize].replica.handle(&m);
                if let Some(r) = reply {
                    self.send(p.to, p.from, p.conn, Body::FromReplica(r));
                }
            }
            Body::FromReplica(m) => {
                let n = &mut self.nodes[p.to as usize];
                let local = n.clock.local(self.now);
                if let Some(t) = n.target.as_mut() {
                    t.on_reply(local, p.from, &m);
                }
                self.after(p.to);
            }
            Body::MetaReq { req, body } => {
                let before: Vec<Vec<NodeId>> = self
                    .meta
                    .volume(VOL)
                    .expect("volume")
                    .chunks
                    .iter()
                    .map(|c| c.replicas.clone())
                    .collect();
                let reply = self.meta.handle(&body);
                // A move dropped its source: that copy is gone for good.
                let after = &self.meta.volume(VOL).expect("volume").chunks;
                for (c, old) in before.iter().enumerate() {
                    for &n in old.iter().filter(|n| !after[c].replicas.contains(n)) {
                        if self.trace.is_some() {
                            eprintln!(
                                "{:>10} chunk {c} moved off node {n}: deleted there",
                                self.now
                            );
                        }
                        self.nodes[n as usize]
                            .replica
                            .remove_chunk(VOL, c as ChunkIdx);
                    }
                }
                if self.trace.is_some() {
                    eprintln!(
                        "{:>10} meta from {}: {body:?} -> {reply:?}",
                        self.now, p.from
                    );
                }
                self.send(META, p.from, None, Body::MetaRep { req, reply });
            }
            Body::MetaRep { req, reply } => {
                let n = &mut self.nodes[p.to as usize];
                let local = n.clock.local(self.now);
                if let Some(t) = n.target.as_mut() {
                    t.on_meta(local, req, &reply);
                }
                self.after(p.to);
            }
            Body::Start { view } => self.start_target(p.to, view),
        }
    }

    fn start_target(&mut self, node: NodeId, view: VolumeMeta) {
        let timing = self.cfg.timing;
        let n = &mut self.nodes[node as usize];
        if n.target.as_ref().is_some_and(|t| t.epoch() >= view.epoch) {
            return;
        }
        // The previous target on this node goes away: its operations stay unanswered.
        n.client.clear();
        n.fast.clear();
        let local = n.clock.local(self.now);
        #[allow(unused_mut)]
        let mut t = Target::new(node, VOL, &view, timing, self.cfg.max_io, local);
        t.set_max_bulk(self.cfg.max_bulk);
        #[cfg(test)]
        {
            t.bugs = self.bugs.target;
            // Outputs of `new` were produced before the bugs were set; they are only Hellos.
        }
        n.target = Some(t);
        self.after(node);
    }

    /// Routes a node's target outputs and schedules its next wakeup.
    fn after(&mut self, node: NodeId) {
        let n = &mut self.nodes[node as usize];
        let Some(t) = n.target.as_mut() else {
            return;
        };
        let out = t.take_output();
        let epoch = t.epoch();
        let local = n.clock.local(self.now);
        let wake = t
            .next_wakeup(local)
            .map(|l| n.clock.real_at(l).max(self.now + 1));
        for o in out {
            match o {
                Output::Send { to, msg } => {
                    if let Some((tc, tb)) = self.trace
                        && let crate::msg::Request::Write {
                            chunk,
                            ext,
                            ref data,
                            ..
                        } = msg.body
                        && chunk == tc
                        && ext.contains(tb)
                    {
                        eprintln!(
                            "{:>10} send {node}->{to} s{} write {ext:?} {data:?}",
                            self.now, msg.session
                        );
                    }
                    let conn = Some((msg.epoch, msg.session));
                    self.send(node, to, conn, Body::ToReplica(msg));
                }
                Output::Meta { req, body } => {
                    self.send(node, META, None, Body::MetaReq { req, body })
                }
                Output::Done { op, result } => {
                    if let Some((tc, tb)) = self.trace {
                        let h = &self.history[op as usize];
                        if h.chunk == tc && h.ext.contains(tb) {
                            eprintln!(
                                "{:>10} done op {op} on node {node}: write {:?} -> {result:?}",
                                self.now, h.write
                            );
                        }
                    }
                    self.nodes[node as usize].client.remove(&op);
                    let h = &mut self.history[op as usize];
                    debug_assert_eq!(h.epoch, epoch);
                    match result {
                        OpResult::Failed => {}
                        r => h.result = Some((self.now, r)),
                    }
                }
            }
        }
        if let Some(w) = wake {
            let n = &mut self.nodes[node as usize];
            if n.wake_at.is_none_or(|cur| w < cur) {
                n.wake_at = Some(w);
                self.at(w, Ev::Wake { node });
            }
        }
    }

    /// Submits a client operation. A write puts the same value in every block of its extent.
    fn submit(&mut self, node: NodeId, chunk: ChunkIdx, ext: Extent, write: Option<Value>) -> u64 {
        let op = self.history.len() as u64;
        let n = &mut self.nodes[node as usize];
        let t = n.target.as_mut().expect("target");
        self.history.push(HistOp {
            epoch: t.epoch(),
            chunk,
            ext,
            write,
            invoke: self.now,
            result: None,
        });
        n.client.insert(op, (chunk, ext));
        let local = n.clock.local(self.now);
        if write.is_none() && self.cfg.fast_reads && self.fast_read(node, op, chunk, ext) {
            return op;
        }
        let n = &mut self.nodes[node as usize];
        let t = n.target.as_mut().expect("target");
        let kind = match write {
            Some(value) => Op::Write {
                chunk,
                ext,
                data: vec![value; ext.len as usize],
            },
            None => Op::Read { chunk, ext },
        };
        t.submit(local, op, kind);
        self.after(node);
        op
    }

    /// Starts a fast read if the target admits it: the local replica's rules (fencing, the
    /// session) apply now, its device read happens later, and the target completes it after
    /// that, as in the data plane. Returns false if it must go through the queue.
    fn fast_read(&mut self, node: NodeId, op: u64, chunk: ChunkIdx, ext: Extent) -> bool {
        let n = &mut self.nodes[node as usize];
        let local = n.clock.local(self.now);
        let t = n.target.as_mut().expect("target");
        let Some(fr) = t.try_fast_read(local, chunk, ext) else {
            return false;
        };
        let epoch = t.epoch();
        let msg = ToReplica {
            vol: VOL,
            epoch,
            session: fr.session,
            req: u64::MAX,
            body: crate::msg::Request::<Data>::Read { chunk, ext },
        };
        let admitted = matches!(
            n.replica.core.handle(&msg),
            crate::replica::Action::Read { .. }
        );
        if !admitted {
            // Refused by the replica (fenced at a newer epoch, or another session): back to
            // the queue, where its reply deposes the target or times out.
            let ok = t.finish_fast_read(local, fr, false);
            debug_assert!(!ok);
            self.after(node);
            return false;
        }
        n.fast.insert(
            op,
            Fast {
                fr,
                epoch,
                data: None,
            },
        );
        // A device read: usually quick, sometimes slow enough to outlast a lease.
        let delay = if self.rng.pct(95) {
            self.rng.range(10, 3 * MS)
        } else {
            self.rng.range(MS, 2 * self.cfg.timing.lease)
        };
        let inc = n.inc;
        self.at(self.now + delay, Ev::FastDevice { node, inc, op });
        true
    }

    fn fast_device(&mut self, node: NodeId, inc: u64, op: u64) {
        let n = &mut self.nodes[node as usize];
        if n.inc != inc {
            return;
        }
        let Some(f) = n.fast.get_mut(&op) else {
            return;
        };
        let (chunk, ext) = (f.fr.chunk, f.fr.ext);
        // The replica may have lost the chunk since (a move dropped it): the read fails.
        f.data = ext
            .blocks()
            .map(|b| n.replica.block(VOL, chunk, b))
            .collect::<Option<Vec<Value>>>();
        let delay = self.rng.range(1, 300);
        self.at(self.now + delay, Ev::FastDone { node, inc, op });
    }

    fn fast_done(&mut self, node: NodeId, inc: u64, op: u64) {
        let n = &mut self.nodes[node as usize];
        if n.inc != inc {
            return;
        }
        let Some(f) = n.fast.remove(&op) else {
            return;
        };
        let local = n.clock.local(self.now);
        // The local replica still fenced at the read's epoch: no newer target can have written
        // to it while the device read was under way (spec §5, fast reads).
        let fenced = n.replica.core.fenced(VOL) == f.epoch;
        #[cfg(test)]
        let fenced = fenced || self.bugs.target.fast_read_no_fence_check;
        let Some(t) = n.target.as_mut().filter(|t| t.epoch() == f.epoch) else {
            return;
        };
        let (chunk, ext) = (f.fr.chunk, f.fr.ext);
        if t.finish_fast_read(local, f.fr, f.data.is_some() && fenced)
            && let Some(data) = f.data
        {
            n.client.remove(&op);
            self.history[op as usize].result = Some((self.now, OpResult::Read(data)));
        } else {
            t.submit(local, op, Op::Read { chunk, ext });
        }
        self.after(node);
    }

    fn client_step(&mut self, node: NodeId, inc: u64) {
        let n = &self.nodes[node as usize];
        if !self.chaos || n.inc != inc || !n.up {
            return;
        }
        let live = n.target.as_ref().is_some_and(|t| !t.is_deposed());
        let busy: Vec<(ChunkIdx, Extent)> = n.client.values().copied().collect();
        let (lo, hi) = self.cfg.client_every;
        let next = self.now + self.rng.range(lo, hi);
        self.at(next, Ev::ClientStep { node, inc });
        if !live || self.history.len() >= self.cfg.max_ops {
            return;
        }
        let chunk = self.rng.range(0, self.cfg.chunks as u64 - 1) as ChunkIdx;
        let bpc = self.cfg.blocks_per_chunk;
        let start = self.rng.range(0, bpc as u64 - 1) as Block;
        let len = self.rng.range(1, self.cfg.max_op.min(bpc - start) as u64) as Block;
        let ext = Extent::new(start, len);
        // One operation in flight per block, and at most MAX_OPS per block in the history.
        let crowded = ext.blocks().any(|b| {
            self.history
                .iter()
                .filter(|h| h.chunk == chunk && h.ext.contains(b))
                .count()
                >= check::MAX_OPS - 2
        });
        if crowded || busy.iter().any(|(c, x)| *c == chunk && x.overlaps(&ext)) {
            return;
        }
        // Values are unique: the operation id, plus one so that 0 stays "never written".
        let write = (!self.rng.pct(self.cfg.read_pct)).then_some(self.history.len() as Value + 1);
        self.submit(node, chunk, ext, write);
    }

    fn script_step(&mut self, a: Action) {
        let key = |a: NodeId, b: NodeId| (a.min(b), a.max(b));
        match a {
            Action::Attach(node) => {
                let epoch = self.meta.volume(VOL).expect("volume").epoch;
                let reply = self.meta.handle(&MetaRequest::Attach { vol: VOL, node });
                if let MetaReply::Applied { view, .. } = reply {
                    debug_assert_eq!(view.epoch, epoch + 1);
                    self.report.failovers += 1;
                    self.send(META, node, None, Body::Start { view });
                }
            }
            Action::Cut(a, b) => {
                self.down_links.insert(key(a, b));
            }
            Action::Heal(a, b) => {
                self.down_links.remove(&key(a, b));
            }
            Action::Slow(a, b, latency) => {
                self.slow_links.insert(key(a, b), (latency, latency));
            }
            Action::Read(node, chunk, block) | Action::Write(node, chunk, block) => {
                let n = &self.nodes[node as usize];
                let ext = Extent::new(block, 1);
                let busy = n
                    .client
                    .values()
                    .any(|&(c, x)| c == chunk && x.overlaps(&ext));
                if n.up && !busy && n.target.as_ref().is_some_and(|t| !t.is_deposed()) {
                    let write =
                        matches!(a, Action::Write(..)).then_some(self.history.len() as Value + 1);
                    self.submit(node, chunk, ext, write);
                }
            }
        }
    }

    fn crash(&mut self, node: NodeId) {
        if self.trace.is_some() {
            eprintln!("{:>10} crash node {node}", self.now);
        }
        let n = &mut self.nodes[node as usize];
        n.up = false;
        n.inc += 1;
        n.target = None;
        n.client.clear();
        n.fast.clear();
        n.wake_at = None;
        n.replica.restart();
        self.report.crashes += 1;
    }

    fn restart(&mut self, node: NodeId) {
        let drift = self.cfg.timing.drift_ppm;
        let rate = 1_000_000 + self.rng.range(0, drift);
        let offset = self.rng.range(0, 1000 * SEC);
        let n = &mut self.nodes[node as usize];
        if n.up {
            return;
        }
        n.up = true;
        n.clock = Clock {
            rate_ppm: rate,
            offset,
        };
        let inc = n.inc;
        self.at(self.now + 1, Ev::ClientStep { node, inc });
    }

    fn fault(&mut self) {
        if !self.chaos {
            return;
        }
        let next = self.now
            + self
                .rng
                .range(self.cfg.fault_every / 4, self.cfg.fault_every * 2);
        self.at(next, Ev::Fault);
        let ids: Vec<NodeId> = (0..self.cfg.nodes).collect();
        match self.rng.range(0, 13) {
            12..=13 => {
                // A random cut: every link, metadata included, fails with some probability.
                let mut ends = ids.clone();
                ends.push(META);
                let heal = self.now + self.rng.range(200 * MS, 15 * SEC);
                for (i, &a) in ends.iter().enumerate() {
                    for &b in &ends[i + 1..] {
                        if self.rng.pct(35) {
                            let key = (a.min(b), a.max(b));
                            self.down_links.insert(key);
                            self.at(heal, Ev::Heal { a: key.0, b: key.1 });
                        }
                    }
                }
                self.report.partitions += 1;
            }
            10..=11 => {
                // A slow link, possibly to the metadata service.
                let a = self.rng.pick(&ids);
                let mut others: Vec<NodeId> = ids.iter().copied().filter(|&x| x != a).collect();
                others.push(META);
                let b = self.rng.pick(&others);
                let key = (a.min(b), a.max(b));
                let lo = self.rng.range(MS, 100 * MS);
                let hi = lo + self.rng.range(0, 700 * MS);
                self.slow_links.insert(key, (lo, hi));
                let until = self.now + self.rng.range(SEC, 20 * SEC);
                self.at(until, Ev::Unslow { a: key.0, b: key.1 });
            }
            0..=2 => {
                let n = self.rng.pick(&ids);
                if self.nodes[n as usize].up {
                    self.crash(n);
                    let back = self.now + self.rng.range(200 * MS, 20 * SEC);
                    self.at(back, Ev::Restart { node: n });
                }
            }
            3..=5 => {
                // One link, possibly to the metadata service.
                let a = self.rng.pick(&ids);
                let mut others: Vec<NodeId> = ids.iter().copied().filter(|&x| x != a).collect();
                others.push(META);
                let b = self.rng.pick(&others);
                self.partition(a, b);
            }
            6..=7 => {
                // A whole node, metadata included. Half the time the target's node: the
                // split-brain case, where the old target keeps serving its client.
                let target = self.meta.volume(VOL).expect("volume").target;
                let a = match target {
                    Some(t) if self.rng.pct(50) => t,
                    _ => self.rng.pick(&ids),
                };
                let mut others: Vec<NodeId> = ids.iter().copied().filter(|&x| x != a).collect();
                others.push(META);
                let heal = self.now + self.rng.range(200 * MS, 15 * SEC);
                for b in others {
                    let key = (a.min(b), a.max(b));
                    self.down_links.insert(key);
                    self.at(heal, Ev::Heal { a: key.0, b: key.1 });
                }
                self.report.partitions += 1;
            }
            _ => {
                if self.meta_up {
                    self.meta_up = false;
                    let up = self.now + self.rng.range(500 * MS, 10 * SEC);
                    self.at(up, Ev::MetaUp);
                }
            }
        }
    }

    fn partition(&mut self, a: NodeId, b: NodeId) {
        let key = (a.min(b), a.max(b));
        self.down_links.insert(key);
        let heal = self.now + self.rng.range(200 * MS, 15 * SEC);
        self.at(heal, Ev::Heal { a: key.0, b: key.1 });
        self.report.partitions += 1;
    }

    /// The failover controller: attaches the volume when its target is gone or unreachable
    /// from the metadata service (after a detection delay), and during chaos also at random
    /// moments, with the old target alive.
    fn controller(&mut self) {
        let next = self.now + self.rng.range(300 * MS, 5 * SEC);
        self.at(next, Ev::Controller);
        if !self.meta_up {
            return;
        }
        let epoch = self.meta.volume(VOL).expect("volume").epoch;
        if self.chaos && self.rng.pct(self.cfg.move_pct) {
            self.request_move();
        }
        if !self.target_alive() {
            let (lo, hi) = self.cfg.detect;
            let at = self.now
                + if self.chaos {
                    self.rng.range(lo, hi)
                } else {
                    0
                };
            self.at(at, Ev::Failover { epoch });
        } else if self.chaos && self.rng.pct(self.cfg.failover_pct) {
            self.at(self.now, Ev::Failover { epoch });
        }
    }

    /// Requests a move of a random chunk's replica to a node without one (drain).
    fn request_move(&mut self) {
        let chunk = self.rng.range(0, self.cfg.chunks as u64 - 1) as ChunkIdx;
        let replicas = self.meta.volume(VOL).expect("volume").chunks[chunk as usize]
            .replicas
            .clone();
        let spare: Vec<NodeId> = (0..self.cfg.nodes)
            .filter(|n| !replicas.contains(n))
            .collect();
        if spare.is_empty() {
            return;
        }
        let from = self.rng.pick(&replicas);
        let to = self.rng.pick(&spare);
        let reply = self.meta.handle(&MetaRequest::MoveReplica {
            vol: VOL,
            chunk,
            from,
            to,
        });
        if self.trace.is_some() {
            eprintln!(
                "{:>10} move chunk {chunk} from {from} to {to}: {}",
                self.now,
                matches!(reply, MetaReply::Applied { .. })
            );
        }
    }

    /// The current target is running and reachable from the metadata service.
    fn target_alive(&self) -> bool {
        let v = self.meta.volume(VOL).expect("volume");
        v.target.is_some_and(|n| {
            let node = &self.nodes[n as usize];
            node.up
                && !self.link_down(META, n)
                && node
                    .target
                    .as_ref()
                    .is_some_and(|t| t.epoch() == v.epoch && !t.is_deposed())
        })
    }

    /// Attaches the volume to a random reachable node, unless the epoch moved on meanwhile.
    /// After chaos, only if the current target is gone.
    fn failover(&mut self, epoch: Epoch) {
        if !self.meta_up
            || self.meta.volume(VOL).expect("volume").epoch != epoch
            || (!self.chaos && self.target_alive())
        {
            return;
        }
        let up: Vec<NodeId> = (0..self.cfg.nodes)
            .filter(|&n| self.nodes[n as usize].up && !self.link_down(META, n))
            .collect();
        if up.is_empty() {
            return;
        }
        let node = self.rng.pick(&up);
        let reply = self.meta.handle(&MetaRequest::Attach { vol: VOL, node });
        if let MetaReply::Applied { view, .. } = reply {
            if self.trace.is_some() {
                eprintln!(
                    "{:>10} attach epoch {} on node {node}",
                    self.now, view.epoch
                );
            }
            self.report.failovers += 1;
            self.send(META, node, None, Body::Start { view });
        }
    }

    fn end_chaos(&mut self) {
        self.chaos = false;
        self.down_links.clear();
        self.slow_links.clear();
        self.meta_up = true;
        for n in 0..self.cfg.nodes {
            if !self.nodes[n as usize].up {
                self.restart(n);
            }
        }
    }

    /// Checks linearizability, convergence and full ISRs.
    fn check(&self) -> Result<(), String> {
        let v = self.meta.volume(VOL).expect("volume");
        for (c, m) in v.chunks.iter().enumerate() {
            if m.isr.len() != m.replicas.len() {
                return Err(format!("liveness: chunk {c} ISR {:?} not full", m.isr));
            }
        }
        for c in 0..self.cfg.chunks {
            for b in 0..self.cfg.blocks_per_chunk {
                self.check_block(c, b)?;
                let values: BTreeSet<Option<Value>> = v.chunks[c as usize]
                    .replicas
                    .iter()
                    .map(|&n| self.nodes[n as usize].replica.block(VOL, c, b))
                    .collect();
                if values.len() != 1 {
                    return Err(format!(
                        "replicas diverge on chunk {c} block {b}: {values:?}"
                    ));
                }
            }
        }
        Ok(())
    }

    fn check_block(&self, c: ChunkIdx, b: Block) -> Result<(), String> {
        let ops: Vec<&HistOp> = self
            .history
            .iter()
            .filter(|h| h.chunk == c && h.ext.contains(b))
            .collect();
        let mut cops = Vec::new();
        for h in &ops {
            let kind = match (h.write, &h.result) {
                (None, Some((_, OpResult::Read(v)))) => CKind::Read(v[(b - h.ext.start) as usize]),
                (None, _) => continue,
                (Some(v), _) => CKind::Write(v),
            };
            let (ret, optional) = match h.result {
                Some((t, _)) => (t, false),
                None => {
                    // An unanswered write may take effect only before the first operation of
                    // a later epoch on this block completes.
                    let deadline = ops
                        .iter()
                        .filter(|o| o.epoch > h.epoch)
                        .filter_map(|o| o.result.as_ref().map(|(t, _)| *t))
                        .min()
                        .unwrap_or(u64::MAX);
                    if deadline < h.invoke {
                        continue;
                    }
                    (deadline, true)
                }
            };
            cops.push(COp {
                invoke: h.invoke,
                ret,
                kind,
                optional,
            });
        }
        check_register(&cops).map_err(|e| {
            let lines: Vec<String> = ops
                .iter()
                .map(|h| {
                    format!(
                        "  e{} {} invoke {} -> {:?}",
                        h.epoch,
                        match h.write {
                            Some(v) => format!("write {v}"),
                            None => "read".to_string(),
                        },
                        h.invoke,
                        h.result
                    )
                })
                .collect();
            format!("chunk {c} block {b}: {e}\n{}", lines.join("\n"))
        })
    }
}

/// Runs one seed with a random configuration.
pub fn run_seed(seed: u64) -> Result<Report, String> {
    Sim::new(seed, Config::random(seed)).run()
}

#[cfg(test)]
mod tests;
