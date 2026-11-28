//! The engine: one thread that owns the replica service and the volume targets.
//!
//! **Inputs** ([`Input`]):
//! - network events (sessions from other nodes' targets, replies to ours);
//! - metadata answers;
//! - attach requests;
//! - client I/O.
//!
//! **Outputs** ([`Output`]): I/O completions and target state changes.
//!
//! Each turn the engine also ticks its targets, and sends the requests that queued for the
//! local replica as **one batch**, so concurrent writes share a group commit.
//!
//! **Targets** run the verified state machine (`flaminio_repl::target`), and the engine only
//! routes their messages:
//! - to the local replica service, in process;
//! - to other nodes over the network thread, one TLS connection per session;
//! - to the metadata service.
//!
//! **Untrusted data is checked before it reaches a target.** A data reply must be exactly the
//! length the engine asked for. Otherwise it is dropped, and the target's timeout treats the
//! replica as failed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use crate::clock::{ClockDomain, ClockGeneration, ClockObserver, ClockSample, SuccessPermit};
use bytes::Bytes;
use flaminio_metaclient::{Answer, Call, MetaClient, pb};
use flaminio_net::BLOCK;
use flaminio_net::frame::Frame;
use flaminio_net::handoff::{Purpose, VerifiedHandoff};
use flaminio_net::session::Accepted;
use flaminio_net::token::Claims;
use flaminio_repl::extent::Extent;
use flaminio_repl::geometry::{self, Geometry};
use flaminio_repl::meta::{ChunkMeta, MAX_CHANGES_PER_REQUEST, MetaReply, MetaRequest, VolumeMeta};
use flaminio_repl::msg::{FromReplica, Request, Response, ToReplica};
use flaminio_repl::target::{FastRead, Op, OpResult, Output as TOut, Target};
use flaminio_repl::types::{Block, NodeId, Timing};
use flaminio_store::BlockDevice;
use smol::channel;

use crate::net::{ConnId, Key, Net, NetCommand, NetEvent};
use crate::replica::{ReplicaService, Reply, SessionRef};

/// Data in flight.
#[derive(Clone, PartialEq, Eq)]
pub enum Payload {
    /// A shared, immutable buffer: sliced and passed on (to frames, to the store) without a
    /// copy.
    Bytes(Bytes),
    /// A client read's data, already placed in the client's buffer by the frontend.
    Placed(usize),
}

impl Payload {
    pub fn len(&self) -> usize {
        match self {
            Payload::Bytes(b) => b.len(),
            Payload::Placed(n) => *n,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The bytes, shared (no copy).
    fn shared(&self) -> Bytes {
        match self {
            Payload::Bytes(b) => b.clone(),
            Payload::Placed(_) => unreachable!("placed data is never forwarded"),
        }
    }
}

impl std::fmt::Debug for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{} bytes>", self.len())
    }
}

/// Where metadata calls go. The engine's answers come back as [`Input::Meta`].
pub trait MetaPort: Send {
    fn call(&self, id: u64, call: Call);
}

impl MetaPort for MetaClient {
    fn call(&self, id: u64, call: Call) {
        MetaClient::call(self, id, call);
    }
}

/// How long, by default, a commit may wait for writes to gather (see `Engine::set_commit_gather`):
/// longer than a client takes to send a write again after its acknowledgement, short against a
/// commit (a flush takes milliseconds). Measured in the lab's VMs at 4K QD32: 100 µs ran out
/// for 80% of the commits (16 writes each), 300 µs gathered 23.5, like commits that block the
/// queue thread, and longer windows gained nothing.
pub const DEFAULT_GATHER: Duration = Duration::from_micros(300);

/// Default floor on write-data bytes gathered into one commit (see [`Engine::set_commit_target_bytes`]).
/// With it, staged groups are held (docs/issues/10): 1M QD8 on fl-b, from ~686 MiB/s to ~849 (86%
/// of raw across the disk), every commit carrying the 16 requests in flight. Zero: count/time
/// gather only, as before.
pub const DEFAULT_COMMIT_TARGET_BYTES: u64 = 8 << 20;

/// The least window of a large-write wait (docs/issues/10, S1): a client sends its 1 MiB writes
/// again 0.4–1.1 ms after a landing (fl-b).
const LARGE_WINDOW: Duration = Duration::from_millis(2);

/// How long past its window a large-write wait goes on while large writes are being copied in
/// (docs/issues/10, S4).
const ARRIVALS_LIMIT: Duration = Duration::from_millis(8);

/// Requests a local commit holds back (docs/issues/10): who sent them, in order; how many are
/// writes, their payload bytes, and how many of the writes are large ([`flaminio_store::large`]).
#[derive(Default)]
struct LocalGroup {
    from: Vec<LocalFrom>,
    writes: usize,
    bytes: u64,
    large: usize,
    /// Started at once for a request queued behind it (or to settle): no wait after its
    /// landing, so that the request starts then.
    forced: bool,
}

/// Which wait for writes to gather a commit is in (docs/issues/10, S1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wait {
    /// After large writes, holding: 2 ms at least, and longer while large writes are being
    /// copied in.
    Large,
    /// Otherwise: the gather window.
    Small,
}

/// A wait for writes to gather: until `target` writes are queued or staged, or `until`.
#[derive(Clone, Copy, Debug)]
struct Gather {
    target: usize,
    until: Instant,
    wait: Wait,
    /// It went on past its window for writes being copied in (counted once).
    held: bool,
    /// ... and reached the limit (counted once).
    limited: bool,
}

/// Local commits since the last report.
#[derive(Default)]
struct CommitStats {
    commits: u64,
    writes: u64,
    /// Started when their wait for writes ran out.
    expired: u64,
    /// Staged groups among them.
    staged: u64,
    /// Waits that went on past their window for writes being copied in.
    arrival_waits: u64,
    /// ... up to the limit.
    limit_hits: u64,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// This node's name.
    pub node: String,
    /// This node's replication address (`ip:port`), registered with the metadata service.
    pub address: String,
    pub timing: Timing,
    /// Largest repair or resync copy, in blocks.
    pub max_io: Block,
    /// Interval of node-liveness heartbeats; None: none.
    pub heartbeat: Option<Duration>,
}

/// Client I/O on a volume. Offsets and lengths are 4 KiB-aligned.
#[derive(Debug, Clone)]
pub enum Io {
    Read {
        offset: u64,
        len: u64,
    },
    /// A read whose data the frontend places itself: pieces from the local replica come as
    /// [`Output::LocalRead`] to carry out, pieces from other replicas as [`Output::Piece`],
    /// and `Done` carries no data.
    ReadDirect {
        offset: u64,
        len: u64,
    },
    Write {
        offset: u64,
        /// Shared: sliced per chunk and passed to the replicas without a copy. Best 4 KiB
        /// aligned, so the local store writes it as is.
        data: Bytes,
    },
}

/// A client read admitted on the fast path ([`Engine::try_fast_read`]): every piece is on the
/// local replica, and the frontend reads them from the store itself.
#[derive(Debug)]
pub struct FastLocal {
    generation: ClockGeneration,
    pieces: Vec<FastPiece>,
}

#[derive(Debug)]
struct FastPiece {
    fr: FastRead,
    /// The local replica's session it was admitted on.
    sref: SessionRef,
    /// Byte offset and length of the piece within the client's read.
    at: usize,
    len: usize,
    key: flaminio_store::ChunkKey,
    offset: u64,
    epoch: u64,
}

impl FastLocal {
    /// The pieces, as (byte offset in the client's read, length).
    pub fn pieces(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.pieces.iter().map(|p| (p.at, p.len))
    }
}

/// A piece of a direct read to serve from the local replica: plan it with
/// [`Engine::plan_local_read`] into the client's buffer at `at`, read it, verify it, and report
/// with [`Engine::local_read_done`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRead {
    pub op: u64,
    /// Byte offset of the piece within the operation.
    pub at: usize,
    pub len: usize,
    token: u64,
    generation: ClockGeneration,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum IoError {
    #[error("clock quarantined; operator restart required")]
    ClockQuarantined,
    #[error("the volume is not attached here")]
    NotAttached,
    #[error("misaligned or out of range")]
    Invalid,
    #[error("the target was deposed: the volume moved to another node")]
    Deposed,
    #[error("no replica could serve the read")]
    Unreadable,
}

pub enum Input {
    Net(NetEvent),
    Meta {
        id: u64,
        answer: Answer,
    },
    /// Become the target of `volume`, with the capability token from its attach.
    Attach {
        volume: String,
        proof: VerifiedHandoff,
    },
    Io {
        volume: String,
        op: u64,
        io: Io,
    },
    /// Test and diagnosis: read the local replica's copy of a range, around the target.
    InspectLocal {
        volume: String,
        offset: u64,
        len: u64,
        reply: mpsc::Sender<Option<Vec<u8>>>,
    },
    /// Test only: drop every request from other nodes' targets until this instant.
    #[doc(hidden)]
    Deafen {
        until: Instant,
    },
    /// Stop being the target of `volume`, as if the target had crashed: its pending I/O
    /// fails, and the next target reconciles what was in flight.
    Detach {
        volume: String,
    },
    /// Delete the local replicas of a volume deleted from the metadata service (see
    /// `ReplicaService::delete_volume`). Refused while this node is its target.
    DeleteReplicas {
        volume: String,
        reply: mpsc::Sender<Result<usize, String>>,
    },
    /// Nothing: wakes the engine's driver.
    Nop,
    Stop,
}

#[derive(Debug)]
pub enum Output {
    Done {
        op: u64,
        result: Result<Option<Vec<u8>>, IoError>,
        permit: Option<SuccessPermit>,
    },
    Attached {
        volume: String,
        epoch: u64,
        size: u64,
    },
    AttachFailed {
        volume: String,
        why: String,
    },
    /// A newer target took the volume over. Its pending I/O failed.
    Deposed {
        volume: String,
    },
    /// The volume was detached. Its pending I/O failed.
    Detached {
        volume: String,
    },
    /// A piece of a direct read to read from the local store (see [`LocalRead`]).
    LocalRead(LocalRead),
    /// A piece of a direct read that came from another replica: copy it to the operation's
    /// buffer at `at`.
    Piece {
        generation: ClockGeneration,
        op: u64,
        at: usize,
        data: Bytes,
    },
}

// Equality compares observable results; a permit is an uncloneable delivery capability,
// not part of the client's result. Callers inspecting admission use the permit explicitly.
impl PartialEq for Output {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Done {
                    op: a, result: ar, ..
                },
                Self::Done {
                    op: b, result: br, ..
                },
            ) => a == b && ar == br,
            (
                Self::Attached {
                    volume: a,
                    epoch: ae,
                    size: az,
                },
                Self::Attached {
                    volume: b,
                    epoch: be,
                    size: bz,
                },
            ) => a == b && ae == be && az == bz,
            (
                Self::AttachFailed { volume: a, why: ar },
                Self::AttachFailed { volume: b, why: br },
            ) => a == b && ar == br,
            (Self::Deposed { volume: a }, Self::Deposed { volume: b })
            | (Self::Detached { volume: a }, Self::Detached { volume: b }) => a == b,
            (Self::LocalRead(a), Self::LocalRead(b)) => a == b,
            (
                Self::Piece {
                    op: a,
                    at: aa,
                    data: ad,
                    ..
                },
                Self::Piece {
                    op: b,
                    at: ba,
                    data: bd,
                    ..
                },
            ) => a == b && aa == ba && ad == bd,
            _ => false,
        }
    }
}
impl Eq for Output {}

/// An admitted local read, waiting for the frontend.
struct PendingLocal {
    generation: ClockGeneration,
    volume: String,
    /// The local replica's session the read was admitted on.
    sref: SessionRef,
    session: u64,
    req: u64,
    key: flaminio_store::ChunkKey,
    offset: u64,
    epoch: u64,
}

/// A client operation, split into one target operation per chunk it touches.
struct ClientOp {
    chunks: Vec<u32>,
    pending: usize,
    failed: Option<IoError>,
    read: Option<Vec<u8>>,
    direct: bool,
}

enum Sess {
    Local {
        session: u64,
        sref: Option<SessionRef>,
    },
    Remote {
        session: u64,
        key: Key,
    },
}

struct TargetRt {
    sm: Target<Payload>,
    view: VolumeMeta,
    /// The attach attempt that owns this target, including its metadata requests.
    generation: u64,
    token: Vec<u8>,
    /// The volume's first epoch, from the token: the local replica's session carries it, as a
    /// remote one's token does (docs/issues/30, S6).
    first_epoch: u64,
    size: u64,
    chunk_size: u64,
    replica_count: usize,
    /// NodeId → node name, and address.
    nodes: Vec<String>,
    addrs: Vec<Option<SocketAddr>>,
    local: NodeId,
    sessions: BTreeMap<NodeId, Sess>,
    ops: BTreeMap<u64, ClientOp>,
    /// Target operation → (client operation, byte offset in it, length).
    subs: BTreeMap<u64, (u64, usize, usize)>,
    /// Outstanding read requests to replicas: (node, req) → expected bytes.
    reads: BTreeMap<(NodeId, u64), usize>,
    deposed_reported: bool,
}

impl TargetRt {
    /// The id of node `name` in this target's views, adding it if new. A new node's address
    /// comes from the next node list ([`TargetRt::missing_addrs`]).
    fn node_id(&mut self, name: &str) -> NodeId {
        match self.nodes.iter().position(|n| n == name) {
            Some(i) => i as NodeId,
            None => {
                self.nodes.push(name.to_string());
                self.addrs.push(None);
                (self.nodes.len() - 1) as NodeId
            }
        }
    }

    fn chunk_meta(&mut self, c: &pb::ChunkMeta) -> ChunkMeta {
        ChunkMeta {
            replicas: c.replicas.iter().map(|r| self.node_id(r)).collect(),
            isr: c.isr.iter().map(|r| self.node_id(r)).collect(),
            isr_ver: c.isr_ver,
            moving: (!c.moving_from.is_empty())
                .then(|| (self.node_id(&c.moving_from), self.node_id(&c.moving_to))),
        }
    }

    /// Validate a complete metadata payload before assigning any new peer IDs.
    fn valid_chunks(&self, chunks: &[pb::ChunkMeta]) -> bool {
        if !valid_chunk_membership(chunks, self.replica_count, self.view.min_isr) {
            return false;
        }
        let mut added = BTreeSet::new();
        for c in chunks {
            for name in c.replicas.iter().chain([&c.moving_from, &c.moving_to]) {
                if !name.is_empty() && !self.nodes.contains(name) {
                    added.insert(name);
                }
            }
        }
        // Peer IDs are retained across moves, so bound the accumulated map before narrowing.
        self.nodes
            .len()
            .checked_add(added.len())
            .and_then(|n| n.checked_sub(1))
            .is_some_and(|last| NodeId::try_from(last).is_ok())
    }

    fn valid_refresh(&self, volume: &str, v: &pb::Volume) -> bool {
        validate_volume(volume, v).is_ok()
            && v.size_bytes == self.size
            && v.chunk_bytes == self.chunk_size
            && v.replicas as usize == self.replica_count
            && v.min_isr as usize == self.view.min_isr
            && v.chunks.len() == self.view.chunks.len()
            && valid_node_name(&v.target)
            && (v.epoch != self.view.epoch || v.target == self.nodes[self.local as usize])
            && self.valid_chunks(&v.chunks)
    }

    /// Retain per-chunk monotonicity when refresh and ISR replies cross in flight.
    fn merge_chunk(&mut self, i: usize, new: &pb::ChunkMeta) {
        let version = self.view.chunks[i].isr_ver;
        if new.isr_ver > version {
            let meta = self.chunk_meta(new);
            self.view.chunks[i] = meta;
        } else if new.isr_ver == version {
            // A requested move has no ISR version of its own.
            let moving = (!new.moving_from.is_empty())
                .then(|| (self.node_id(&new.moving_from), self.node_id(&new.moving_to)));
            self.view.chunks[i].moving = moving;
        }
    }

    fn missing_addrs(&self) -> bool {
        self.addrs
            .iter()
            .enumerate()
            .any(|(i, a)| a.is_none() && i as NodeId != self.local)
    }
}

fn valid_node_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

fn valid_chunk_membership(chunks: &[pb::ChunkMeta], replica_count: usize, min_isr: usize) -> bool {
    chunks.len() <= geometry::MAX_CHUNKS as usize
        && chunks.iter().all(|c| {
            let replicas = &c.replicas;
            let moving = !c.moving_from.is_empty() || !c.moving_to.is_empty();
            let unique = |names: &[String]| {
                names
                    .iter()
                    .enumerate()
                    .all(|(i, name)| valid_node_name(name) && !names[..i].contains(name))
            };
            replicas.len() >= replica_count
                && replicas.len() <= replica_count + usize::from(moving)
                && unique(replicas)
                && c.isr.len() >= min_isr
                && c.isr.len() <= replicas.len()
                && unique(&c.isr)
                && c.isr.iter().all(|n| replicas.contains(n))
                && (!moving
                    || (valid_node_name(&c.moving_from)
                        && valid_node_name(&c.moving_to)
                        && c.moving_from != c.moving_to
                        && replicas.contains(&c.moving_from)
                        && (replicas.contains(&c.moving_to)
                            == (replicas.len() == replica_count + 1))))
        })
}

/// Metadata chunk rows carry no index: their exact position is the volume's chunk index.
fn validate_volume(volume: &str, v: &pb::Volume) -> Result<Geometry, String> {
    let shape = geometry::validate(v.size_bytes, v.chunk_bytes, v.replicas, v.min_isr)
        .map_err(|e| format!("unsupported volume geometry: {e}"))?;
    if v.id != volume || !valid_node_name(&v.id) || !valid_node_name(&v.target) {
        return Err("invalid volume or target identity in metadata".into());
    }
    if v.chunks.len() != shape.chunks as usize {
        return Err("volume chunk-record count does not match its geometry".into());
    }
    if !valid_chunk_membership(&v.chunks, v.replicas as usize, v.min_isr as usize) {
        return Err("invalid replica, ISR or move membership in metadata".into());
    }
    // Exact positional rows form unique in-range chunk IDs. Every node's placed/moving
    // union is a subset of those rows and therefore also fits the scalar Hello limit.
    Ok(shape)
}

/// Saved request order is the reply order; ChangeIsr's ChunkMeta rows have no wire index.
fn valid_isr_indices(chunks: &[u32], count: usize) -> bool {
    !chunks.is_empty()
        && chunks.len() <= MAX_CHANGES_PER_REQUEST
        && chunks.iter().all(|&c| (c as usize) < count)
        && chunks.iter().copied().collect::<BTreeSet<_>>().len() == chunks.len()
}

struct Attaching {
    /// Which attach of the volume this is: answers to an earlier one, dropped since, carry
    /// another (docs/issues/6).
    attempt: u64,
    proof: VerifiedHandoff,
    volume: Option<pb::Volume>,
    nodes: Option<Vec<pb::Node>>,
}

/// How long after a failed registration it is sent again.
const REGISTER_RETRY: Duration = Duration::from_secs(3);

/// Where the node's registration with the metadata service stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Registration {
    InFlight,
    Done,
    /// It failed, or the service lost it: sent again at the next heartbeat.
    Needed,
}

enum MetaWait {
    /// The node's registration.
    Register,
    /// A node heartbeat, sent under the registration numbered `registered`.
    Heartbeat { registered: u64 },
    Attach {
        volume: String,
        part: u8,
        attempt: u64,
    },
    Isr {
        volume: String,
        generation: u64,
        sm_req: u64,
        chunks: Vec<u32>,
    },
    /// The target's read of its volume.
    Refresh {
        volume: String,
        generation: u64,
        sm_req: u64,
    },
    /// Addresses of nodes the target learned of after it started (a move's destination).
    Nodes { volume: String },
}

/// Where a reply from the local replica goes.
enum LocalFrom {
    Target { volume: String, session: u64 },
    Conn(ConnId),
}

pub struct Engine<D: BlockDevice> {
    cfg: EngineConfig,
    service: ReplicaService<D>,
    net: Net,
    meta: Box<dyn MetaPort>,
    clock: Arc<ClockDomain>,
    observers: HashMap<std::thread::ThreadId, ClockObserver>,
    nonce: [u8; 32],
    ran_epochs: BTreeMap<String, u64>,
    quarantined: bool,
    conns: BTreeMap<ConnId, (SessionRef, channel::Sender<Reply>)>,
    targets: BTreeMap<String, TargetRt>,
    attaching: BTreeMap<String, Attaching>,
    next_attempt: u64,
    keys: BTreeMap<Key, (String, NodeId, u64)>,
    next_key: Key,
    next_meta: u64,
    meta_waits: BTreeMap<u64, MetaWait>,
    next_sub: u64,
    local: Vec<(LocalFrom, SessionRef, Frame)>,
    /// For each of the local replica's commits in flight, oldest first (several with pipelined
    /// commits): who sent the requests it holds back, in order, how many are writes, and their
    /// payload bytes.
    committing: std::collections::VecDeque<LocalGroup>,
    /// The same for the group staged behind them. With pipelined commits it launches after a
    /// gather wait (see [`Self::maybe_launch_staged`]); otherwise its commit starts when the
    /// oldest in flight lands.
    staged: Option<LocalGroup>,
    /// After a commit completes (or a staged group is waiting to launch), the next waits until
    /// this many writes are queued or staged, or until the instant (see `finish_commit`).
    gather: Option<Gather>,
    /// How long the next commit may wait for writes to gather.
    gather_window: Duration,
    /// A wait for writes to gather began: the driver arms a timer for its end.
    gather_timer: Option<Instant>,
    /// Floor on write-data bytes before the next commit starts (0: count/time only). Raised into
    /// the count target as `ceil(bytes / avg_write)`, so 1 MiB traffic packs toward 8 MiB while
    /// 4K still ends the wait on the time window.
    commit_target_bytes: u64,
    /// Large writes the frontend is copying in (zero copy) and has not handed over yet
    /// (docs/issues/10, S4).
    arrivals: Arc<AtomicUsize>,
    /// The local store's data cluster size, for telling large writes.
    cluster_size: u64,
    /// Inside `settle_local`: every wait for writes is over.
    settling: bool,
    /// Landings, launches and requests started: how `settle_local` sees that it progresses.
    progress: u64,
    /// Local commits since the last report.
    commit_stats: CommitStats,
    /// Whether the local replica's commits may stay in flight between turns. Off, each commit is
    /// waited for: for a thread that cannot learn when one completes.
    async_commits: bool,
    out: Vec<Output>,
    local_reads: BTreeMap<u64, PendingLocal>,
    next_token: u64,
    draining: bool,
    pending_drain: std::collections::BTreeSet<String>,
    next_heartbeat: Instant,
    /// Whether the metadata service has this node's registration.
    registration: Registration,
    /// Successful registrations so far: a heartbeat's NotFound counts only if no registration
    /// succeeded after the heartbeat was sent.
    registrations: u64,
    /// When a failed registration is sent again, heartbeats or not.
    next_register: Instant,
    deaf_until: Option<Instant>,
}

impl<D: BlockDevice> Engine<D> {
    pub fn new(
        cfg: EngineConfig,
        service: ReplicaService<D>,
        net: Net,
        meta: Box<dyn MetaPort>,
        clock: Arc<ClockDomain>,
        nonce: [u8; 32],
    ) -> Self {
        let now = Instant::now();
        if !Arc::ptr_eq(&clock, net.clock_domain()) {
            clock.quarantine(crate::clock::ClockFault::Profile);
            net.clock_domain()
                .quarantine(crate::clock::ClockFault::Profile);
        }
        let mut e = Self {
            cfg,
            service,
            net,
            meta,
            clock,
            observers: HashMap::new(),
            nonce,
            ran_epochs: BTreeMap::new(),
            quarantined: false,
            conns: BTreeMap::new(),
            targets: BTreeMap::new(),
            attaching: BTreeMap::new(),
            next_attempt: 1,
            keys: BTreeMap::new(),
            next_key: 1,
            next_meta: 1,
            meta_waits: BTreeMap::new(),
            next_sub: 1,
            local: Vec::new(),
            committing: std::collections::VecDeque::new(),
            staged: None,
            gather: None,
            gather_window: DEFAULT_GATHER,
            gather_timer: None,
            // Through the setter, below, so that the replica service and the store hold too.
            commit_target_bytes: 0,
            arrivals: Arc::default(),
            cluster_size: 0,
            settling: false,
            progress: 0,
            commit_stats: CommitStats::default(),
            async_commits: false,
            out: Vec::new(),
            local_reads: BTreeMap::new(),
            next_token: 1,
            draining: false,
            pending_drain: std::collections::BTreeSet::new(),
            next_heartbeat: now,
            deaf_until: None,
            registration: Registration::Needed,
            registrations: 0,
            next_register: now,
        };
        e.cluster_size = e.service.cluster_size();
        e.set_commit_target_bytes(DEFAULT_COMMIT_TARGET_BYTES);
        e.register();
        e
    }

    /// Registers the node's address with the metadata service. A registration that fails is
    /// sent again `REGISTER_RETRY` later (docs/issues/7).
    fn register(&mut self) {
        let reg = pb::RegisterNodeRequest {
            node: Some(pb::Node {
                id: self.cfg.node.clone(),
                address: self.cfg.address.clone(),
                ..Default::default()
            }),
        };
        self.registration = Registration::InFlight;
        self.meta_call(Call::RegisterNode(reg), MetaWait::Register);
    }

    pub fn service(&self) -> &ReplicaService<D> {
        &self.service
    }

    pub fn into_service(mut self) -> ReplicaService<D> {
        self.settle_local(Instant::now());
        self.service
    }

    pub fn clock_domain(&self) -> &Arc<ClockDomain> {
        &self.clock
    }

    pub fn attach_challenge(&mut self) -> Result<[u8; 32], String> {
        let sample = self
            .sample()
            .ok_or("clock quarantined; operator restart required")?;
        self.clock.admit(sample).map_err(|e| e.to_string())?;
        Ok(self.nonce)
    }

    /// Validate a frontend's already-started lifetime immediately before publishing its
    /// device. This cannot start a target or revive a quarantined/deposed lifetime.
    pub fn attachment_permit(
        &mut self,
        volume: &str,
        proof: &VerifiedHandoff,
    ) -> Option<SuccessPermit> {
        let sample = self.sample()?;
        let rt = self.targets.get(volume)?;
        if rt.sm.is_deposed()
            || !proof.is_current(&self.nonce, Purpose::Normal)
            || !proof.matches_target(volume, &self.cfg.node, rt.view.epoch)
            || rt.token != proof.capability()
        {
            return None;
        }
        self.clock.success_permit(sample).ok()
    }

    fn sample(&mut self) -> Option<ClockSample> {
        let observer = self
            .observers
            .entry(std::thread::current().id())
            .or_insert_with(|| self.clock.observer());
        match observer.sample() {
            Ok(sample) => Some(sample),
            Err(_) => {
                self.invalidate_clock();
                None
            }
        }
    }

    fn admit(&mut self) -> bool {
        let Some(sample) = self.sample() else {
            return false;
        };
        if self.clock.admit(sample).is_err() {
            self.invalidate_clock();
            return false;
        }
        true
    }

    fn invalidate_clock(&mut self) {
        if self.quarantined {
            return;
        }
        self.quarantined = true;
        eprintln!(
            "flaminio-node: clock quarantined: {}; operator restart required",
            self.clock.status().reason.map_or("profile", |r| r.as_str())
        );
        self.net.send(NetCommand::Quarantine);
        self.service.invalidate_clock();
        self.conns.clear();
        self.keys.clear();
        self.local.clear();
        self.local_reads.clear();
        self.meta_waits.clear();
        // Keep device work and its buffers alive. Poll/settle only work already submitted;
        // no queued network or client request is admitted after the gate closes.
        let permitted: BTreeSet<u64> = self
            .out
            .iter()
            .filter_map(|o| match o {
                Output::Done {
                    op,
                    permit: Some(_),
                    ..
                } => Some(*op),
                _ => None,
            })
            .collect();
        let unpublished: Vec<String> = self
            .out
            .iter()
            .filter_map(|o| match o {
                Output::Attached { volume, .. } => Some(volume.clone()),
                _ => None,
            })
            .collect();
        for volume in unpublished {
            self.out.push(Output::AttachFailed {
                volume,
                why: "clock quarantined; operator restart required".into(),
            });
        }
        self.out.retain(|o| {
            matches!(
                o,
                Output::AttachFailed { .. }
                    | Output::Done { result: Err(_), .. }
                    | Output::Done {
                        permit: Some(_),
                        ..
                    }
            ) || matches!(o, Output::Piece { op, .. } if permitted.contains(op))
        });
        for (volume, _) in std::mem::take(&mut self.attaching) {
            self.out.push(Output::AttachFailed {
                volume,
                why: "clock quarantined; operator restart required".into(),
            });
        }
        for (_, mut rt) in std::mem::take(&mut self.targets) {
            rt.sm.invalidate_clock();
            for op in rt.ops.keys() {
                self.out.push(Output::Done {
                    op: *op,
                    result: Err(IoError::ClockQuarantined),
                    permit: None,
                });
            }
        }
    }

    fn finish_quarantined_io(&mut self, _now: Instant) {
        loop {
            if self.staged.is_some() && self.service.holding() && self.service.launch() {
                self.take_staged(true);
            }
            if self.committing.is_empty() {
                break;
            }
            if self.service.poll(false).is_none() {
                break;
            }
            self.committing.pop_front();
            if !self.service.holding()
                && let Some(st) = self.staged.take()
            {
                self.committing.push_back(st);
            }
        }
        self.gather = None;
        self.gather_timer = None;
    }

    fn reject_quarantined(&mut self, input: Input) -> bool {
        match input {
            Input::Stop => {
                self.settle_local(Instant::now());
                return false;
            }
            Input::Detach { volume } => self.detach(&volume),
            Input::Io { op, .. } => self.out.push(Output::Done {
                op,
                result: Err(IoError::ClockQuarantined),
                permit: None,
            }),
            Input::Attach { volume, .. } => self.out.push(Output::AttachFailed {
                volume,
                why: "clock quarantined; operator restart required".into(),
            }),
            Input::DeleteReplicas { reply, .. } => {
                let _ = reply.send(Err("clock quarantined; operator restart required".into()));
            }
            Input::InspectLocal { reply, .. } => {
                let _ = reply.send(None);
            }
            _ => {}
        }
        true
    }

    fn meta_call(&mut self, call: Call, wait: MetaWait) {
        if !self.admit() {
            return;
        }
        let id = self.next_meta;
        self.next_meta += 1;
        self.meta_waits.insert(id, wait);
        self.meta.call(id, call);
    }

    /// Takes the outputs produced so far.
    pub fn take_output(&mut self) -> Vec<Output> {
        let _ = self.sample();
        std::mem::take(&mut self.out)
    }

    /// The next time [`Engine::turn`] has work, if no input arrives before.
    pub fn next_wakeup(&self) -> Instant {
        let mut t = self.next_heartbeat;
        if self.registration == Registration::Needed {
            t = t.min(self.next_register);
        }
        // Relative timers only wake the driver; every awakened transition resamples BOOTTIME.
        if !self.targets.is_empty() {
            t = t.min(Instant::now() + Duration::from_millis(10));
        }
        t
    }

    /// Handles one input. Returns false on [`Input::Stop`].
    pub fn handle(&mut self, input: Input) -> bool {
        let now = Instant::now();
        if !self.admit() {
            return self.reject_quarantined(input);
        }
        match input {
            Input::Net(e) => self.on_net(now, e),
            Input::Meta { id, answer } => self.on_meta(now, id, answer),
            Input::Attach { volume, proof } => {
                if !proof.is_current(&self.nonce, Purpose::Normal)
                    || !proof.matches_target(&volume, &self.cfg.node, proof.claims().epoch)
                {
                    self.out.push(Output::AttachFailed {
                        volume,
                        why: "a current-process agent/admin Attach handoff is required".into(),
                    });
                    return true;
                }
                if self.targets.contains_key(&volume) {
                    self.out.push(Output::AttachFailed {
                        volume,
                        why: "already attached".into(),
                    });
                    return true;
                }
                // Each attach gets an answer: a second one would silently replace the first.
                if self.attaching.contains_key(&volume) {
                    self.out.push(Output::AttachFailed {
                        volume,
                        why: "already attaching".into(),
                    });
                    return true;
                }
                let attempt = self.next_attempt;
                self.next_attempt += 1;
                self.attaching.insert(
                    volume.clone(),
                    Attaching {
                        attempt,
                        proof,
                        volume: None,
                        nodes: None,
                    },
                );
                self.meta_call(
                    Call::GetVolume(pb::GetVolumeRequest { id: volume.clone() }),
                    MetaWait::Attach {
                        volume: volume.clone(),
                        part: 0,
                        attempt,
                    },
                );
                self.meta_call(
                    Call::ListNodes(pb::ListNodesRequest {}),
                    MetaWait::Attach {
                        volume,
                        part: 1,
                        attempt,
                    },
                );
            }
            Input::Io { volume, op, io } => self.submit(now, &volume, op, io),
            Input::InspectLocal {
                volume,
                offset,
                len,
                reply,
            } => {
                self.settle_local(now);
                let _ = reply.send(self.inspect(&volume, offset, len));
            }
            Input::Deafen { until } => self.deaf_until = Some(until),
            Input::Detach { volume } => self.detach(&volume),
            Input::DeleteReplicas { volume, reply } => {
                let r =
                    if self.targets.contains_key(&volume) || self.attaching.contains_key(&volume) {
                        Err(format!("{volume} is attached here"))
                    } else {
                        self.settle_local(now);
                        if self.admit() {
                            self.service.delete_volume(&volume)
                        } else {
                            Err("clock quarantined; operator restart required".into())
                        }
                    };
                let _ = reply.send(r);
            }
            Input::Nop => {}
            Input::Stop => {
                self.settle_local(now);
                return false;
            }
        }
        true
    }

    /// Ticks targets and heartbeats, and sends what queued for the local replica.
    pub fn turn(&mut self) {
        let now = Instant::now();
        if self.sample().is_none() {
            self.finish_quarantined_io(now);
            return;
        }
        // A registration that failed, or that the service lost, goes again, with or without
        // heartbeats. Without this, a metadata service that was down or contended when the node
        // started would leave it unregistered, and never live, until a restart.
        if self.registration == Registration::Needed && now >= self.next_register {
            self.register();
        }
        if let Some(every) = self.cfg.heartbeat
            && now >= self.next_heartbeat
        {
            self.next_heartbeat = now + every;
            // Sent whatever the registration's state: a node whose record exists stays live
            // while a registration that keeps failing is retried.
            let hb = pb::NodeHeartbeatRequest {
                node: self.cfg.node.clone(),
            };
            let registered = self.registrations;
            self.meta_call(Call::NodeHeartbeat(hb), MetaWait::Heartbeat { registered });
        } else if self.cfg.heartbeat.is_none() {
            self.next_heartbeat = now + Duration::from_secs(3600);
        }
        let vols: Vec<String> = self.targets.keys().cloned().collect();
        for v in vols {
            let Some(sample) = self.sample() else {
                return;
            };
            let us = sample.elapsed_us();
            if let Some(rt) = self.targets.get_mut(&v) {
                rt.sm.tick(us);
            }
            self.drain(now, &v);
        }
        self.flush_local(now);
    }

    /// Sends what queued for the local replica and routes the targets' outputs, without
    /// ticking: for after an I/O event. Ticks (timeouts, heartbeats, leases) scan every
    /// request in flight, so they belong on a timer, not on every I/O.
    pub fn flush(&mut self) {
        let now = Instant::now();
        if self.sample().is_none() {
            self.finish_quarantined_io(now);
            return;
        }
        self.flush_local(now);
    }

    /// Runs the engine on this thread until [`Input::Stop`] or until every sender is gone.
    pub fn run(mut self, inputs: mpsc::Receiver<Input>, mut outputs: impl FnMut(Output)) -> Self {
        loop {
            self.turn();
            for mut o in self.take_output() {
                if let Output::Done { permit, .. } = &mut o
                    && let Some(p) = permit.take()
                {
                    p.consume();
                }
                outputs(o);
            }
            let wait = self.next_wakeup().saturating_duration_since(Instant::now());
            let first = match inputs.recv_timeout(wait.max(Duration::from_micros(100))) {
                Ok(i) => i,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return self,
            };
            if !self.handle(first) {
                return self;
            }
            // Everything that has arrived is one batch.
            for _ in 0..1024 {
                match inputs.try_recv() {
                    Ok(i) => {
                        if !self.handle(i) {
                            return self;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }

    // --- Replica side: sessions from other nodes' targets ---------------------------------

    fn on_net(&mut self, now: Instant, e: NetEvent) {
        let deaf = self.deaf_until.is_some_and(|t| now < t);
        match e {
            NetEvent::Opened {
                conn,
                accepted,
                replies,
            } => {
                if deaf {
                    return;
                }
                // A Hello fences the store, which settles it: a held group starts first.
                self.settle_local(now);
                if !self.admit() {
                    return;
                }
                match self.service.hello(&accepted) {
                    Ok((s, reply)) => {
                        let _ = replies.try_send(Reply::Send(reply));
                        self.conns.insert(conn, (s, replies));
                    }
                    Err(why) => {
                        let _ = replies.try_send(Reply::Close(why));
                    }
                }
            }
            NetEvent::Request { conn, frame } => {
                if deaf {
                    return;
                }
                if let Some((s, _)) = self.conns.get(&conn) {
                    self.local.push((LocalFrom::Conn(conn), *s, frame));
                }
            }
            NetEvent::Closed { conn } => {
                self.settle_local(now);
                self.conns.remove(&conn);
            }
            NetEvent::Reply { key, frame } => {
                let Some((vol, node, session)) = self.keys.get(&key).cloned() else {
                    return;
                };
                self.reply_to_target(now, &vol, node, session, frame);
                self.drain(now, &vol);
            }
            NetEvent::Lost { key, why } => {
                if let Some((vol, node, _)) = self.keys.remove(&key) {
                    eprintln!(
                        "flaminio-node: {vol}: session to {} lost: {why}",
                        self.node_name(&vol, node)
                    );
                }
            }
        }
    }

    fn node_name(&self, vol: &str, n: NodeId) -> String {
        self.targets
            .get(vol)
            .and_then(|rt| rt.nodes.get(n as usize).cloned())
            .unwrap_or_else(|| format!("node {n}"))
    }

    /// Lets the local replica's commits stay in flight between turns (`on`), for a thread that
    /// pumps the engine when the store's device signals a commit done. Turning it off settles
    /// the local replica: the commits in flight, a held group and the queued requests.
    pub fn set_async_commits(&mut self, on: bool) {
        let now = Instant::now();
        // Settled while still asynchronous, so that a held group is handled by the held rules
        // (`holding()` reads this flag).
        if !on {
            self.settle_local(now);
        }
        self.async_commits = on;
        if !on {
            self.gather = None;
            let s = std::mem::take(&mut self.commit_stats);
            if s.commits > 0 {
                eprintln!(
                    "flaminio-node: {} local commits ({} staged), {:.1} writes each; {} started when their wait for writes ran out, {} waited on writes being copied in, {} of them to the limit",
                    s.commits,
                    s.staged,
                    s.writes as f64 / s.commits as f64,
                    s.expired,
                    s.arrival_waits,
                    s.limit_hits
                );
            }
        }
    }

    /// How long a commit may wait for writes to gather (see `finish_commit`); zero: never.
    pub fn set_commit_gather(&mut self, window: Duration) {
        self.gather_window = window;
    }

    /// Floor on write-data bytes gathered into one commit (see [`DEFAULT_COMMIT_TARGET_BYTES`]).
    /// Non-zero also holds the staged group (docs/issues/10): after a landing it waits, and grows,
    /// before its commit starts, and large writes are staged with nothing in flight. Zero:
    /// gather by write count and time only, as before the byte floor. Turning it off first
    /// settles a held group, which nothing would start otherwise.
    pub fn set_commit_target_bytes(&mut self, bytes: u64) {
        if bytes == 0 && self.staged.is_some() {
            self.settle_local(Instant::now());
        }
        self.commit_target_bytes = bytes;
        self.service.set_hold_staged(bytes > 0);
    }

    /// True while the local replica's groups are held (docs/issues/10): commits asynchronous and a
    /// byte floor set. The frontend then hands large writes over as soon as each is copied in,
    /// and counts them in [`Self::set_arrivals`]'s counter meanwhile.
    pub fn holding(&self) -> bool {
        self.async_commits && self.commit_target_bytes > 0
    }

    /// The local store's data cluster size, for telling large writes
    /// ([`flaminio_store::large`]).
    pub fn cluster_size(&self) -> u64 {
        self.cluster_size
    }

    /// The counter of large writes the frontend is copying in (zero copy) and has not handed
    /// over yet: one per device, so that a new device never inherits a count (docs/issues/10, S4).
    pub fn set_arrivals(&mut self, arrivals: Arc<AtomicUsize>) {
        self.arrivals = arrivals;
    }

    /// The instant to pump the engine at, once, when a commit starts waiting for writes to
    /// gather: the driver pumps it then.
    pub fn take_gather_timer(&mut self) -> Option<Instant> {
        self.gather_timer.take()
    }

    /// Writes (and copies) queued for the local replica.
    fn queued_writes(&self) -> usize {
        self.local.iter().filter(|(_, _, f)| is_write(f)).count()
    }

    fn queued_write_bytes(&self) -> u64 {
        self.local
            .iter()
            .filter(|(_, _, f)| is_write(f))
            .map(|(_, _, f)| write_data_len(f))
            .sum()
    }

    fn pending_write_count(&self) -> usize {
        self.queued_writes() + self.staged.as_ref().map_or(0, |s| s.writes)
    }

    fn pending_write_bytes(&self) -> u64 {
        self.queued_write_bytes() + self.staged.as_ref().map_or(0, |s| s.bytes)
    }

    /// A large write: one the store can stage, of at least 64 KiB (docs/issues/10).
    fn is_large(&self, f: &Frame) -> bool {
        match f {
            Frame::Write { ext, data, .. } => flaminio_store::large(
                self.cluster_size,
                u64::from(ext.start) * BLOCK as u64,
                data.len() as u64,
            ),
            _ => false,
        }
    }

    /// Arms a wait for writes to gather: until `target` writes are queued or staged, or the
    /// window passes, 2 ms at least for a large-write wait (docs/issues/10, S1).
    fn arm(&mut self, now: Instant, target: usize, wait: Wait) {
        if self.gather_window.is_zero() {
            return;
        }
        let window = match wait {
            Wait::Large => self.gather_window.max(LARGE_WINDOW),
            Wait::Small => self.gather_window,
        };
        let until = now + window;
        self.gather = Some(Gather {
            target,
            until,
            wait,
            held: false,
            limited: false,
        });
        self.gather_timer = Some(until);
    }

    /// Arms the wait after the landing of `landed`'s commit. Holding, after large writes, it
    /// waits for every write outstanding at the landing: the commit's, the held group's and the
    /// queued ones, which a client keeping its queue full sends again at once. Otherwise, as
    /// before, for those the commit acknowledged plus those queued. Starting at once would split
    /// the client's writes into two halves that commit in turn (4K QD32: 13 writes per commit
    /// against 23; 1M QD8: 4 MiB per flush against 8).
    fn arm_after_landing(&mut self, now: Instant, landed: &LocalGroup) {
        if landed.forced {
            // It started for what is queued behind it, which starts now.
            self.gather = None;
            return;
        }
        let queued = self.queued_writes();
        let (staged, staged_large) = self.staged.as_ref().map_or((0, 0), |s| (s.writes, s.large));
        let large = staged_large > 0 || (landed.writes > 0 && landed.large == landed.writes);
        if self.commit_target_bytes > 0 && large {
            self.arm(now, landed.writes + staged + queued, Wait::Large);
        } else {
            self.arm(now, landed.writes + queued, Wait::Small);
        }
    }

    /// Whether the wait for writes to gather is over: the floor's bytes are there, or the count,
    /// or the window has passed. A large-write wait goes on past its window while large writes
    /// are being copied in, up to `ARRIVALS_LIMIT` more, and before its window its count is met
    /// only once none is: so the two halves of a 1 MiB write join one commit (docs/issues/10, S1
    /// and S4). Settling, every wait is over. Returns `(ready, expired)`.
    fn gather_ready(&mut self, now: Instant, count: usize) -> (bool, bool) {
        let Some(g) = self.gather else {
            return (true, false);
        };
        if self.settling {
            return (true, false);
        }
        if self.commit_target_bytes > 0 && self.pending_write_bytes() >= self.commit_target_bytes {
            return (true, false);
        }
        let arriving = g.wait == Wait::Large && self.arrivals.load(Ordering::Relaxed) > 0;
        if now < g.until {
            return (count >= g.target && !arriving, false);
        }
        if count >= g.target {
            return (true, false);
        }
        if arriving {
            let g = self.gather.as_mut().expect("gather");
            if !g.held {
                g.held = true;
                self.commit_stats.arrival_waits += 1;
            }
            if now < g.until + ARRIVALS_LIMIT {
                return (false, false);
            }
            if !g.limited {
                g.limited = true;
                self.commit_stats.limit_hits += 1;
            }
        }
        (true, true)
    }

    /// Sends the requests queued for the local replica, a batch at a time, until none are left
    /// or a commit stays in flight.
    fn flush_local(&mut self, now: Instant) {
        if self.sample().is_none() {
            return;
        }
        for _ in 0..16 {
            if !self.admit() {
                return;
            }
            if !self.finish_commit(now, !self.async_commits) {
                self.stage_local(now);
                self.maybe_launch_staged(now);
                return;
            }
            // Holding: the writes the store can stage go to the held group, even with nothing in
            // flight, their data to the disk at once; the group's commit starts by its wait
            // (docs/issues/10, S1 and S2). Nothing else starts while a group is staged: the store
            // holds it, and every other request would settle it behind the engine's back.
            if self.holding() {
                self.stage_local(now);
                if self.staged.is_some() {
                    if self.local.is_empty() {
                        self.maybe_launch_staged(now);
                    } else {
                        // Queued behind the group, a request it cannot take (a small write, a
                        // read, a write the store refuses): in order, it can grow no more.
                        self.launch_held();
                    }
                    if self.staged.is_some() || !self.committing.is_empty() {
                        return;
                    }
                }
            }
            if self.local.is_empty() {
                return;
            }
            if self.gather.is_some() {
                let (ready, expired) = self.gather_ready(now, self.queued_writes());
                if !ready {
                    return;
                }
                if expired {
                    self.commit_stats.expired += 1;
                }
                self.gather = None;
            }
            let batch = std::mem::take(&mut self.local);
            let mut from = Vec::with_capacity(batch.len());
            let mut items = Vec::with_capacity(batch.len());
            for (f, s, frame) in batch {
                from.push(f);
                items.push((s, frame));
            }
            // The store holds no group the engine has not launched (docs/issues/10, S5).
            debug_assert!(self.staged.is_none(), "start with a group staged");
            let p = self.service.start(&items);
            self.progress += p.handled as u64;
            // Requests not handled yet wait for the commit, ahead of anything queued since.
            let rest: Vec<_> = from
                .drain(p.handled..)
                .zip(items.drain(p.handled..))
                .map(|(f, (s, frame))| (f, s, frame))
                .collect();
            if !rest.is_empty() {
                let newer = std::mem::replace(&mut self.local, rest);
                self.local.extend(newer);
            }
            let held = from.split_off(p.replies.len());
            if !held.is_empty() {
                let held_items = &items[p.replies.len()..];
                let writes = held_items.iter().filter(|(_, f)| is_write(f)).count();
                let bytes: u64 = held_items
                    .iter()
                    .filter(|(_, f)| is_write(f))
                    .map(|(_, f)| write_data_len(f))
                    .sum();
                let large = held_items.iter().filter(|(_, f)| self.is_large(f)).count();
                self.commit_stats.commits += 1;
                self.commit_stats.writes += writes as u64;
                self.committing.push_back(LocalGroup {
                    from: held,
                    writes,
                    bytes,
                    large,
                    forced: false,
                });
            }
            self.deliver_local(now, from, p.replies);
        }
        if !self.async_commits {
            self.finish_commit(now, true);
        }
    }

    /// Ends the local replica's commit in flight if it is done (with `wait`, waits for it), and
    /// delivers the replies it held back. Returns true if no commit is in flight any more.
    ///
    /// With commits in flight, the next commit then waits, briefly, for writes to gather
    /// ([`Self::arm_after_landing`]). A client with one write in flight waits for nothing: its
    /// next write is the target.
    ///
    /// Commits land in order. Without a byte floor, a group staged behind them joins them when
    /// the oldest lands, its commit started then. With one, the group is held: it starts when
    /// it holds the floor's bytes, or by its wait.
    fn finish_commit(&mut self, now: Instant, wait: bool) -> bool {
        loop {
            if self.committing.is_empty() {
                return true;
            }
            let Some(replies) = self.service.poll(wait) else {
                return false;
            };
            let landed = self.committing.pop_front().expect("a commit in flight");
            self.progress += 1;
            let holding = self.commit_target_bytes > 0;
            debug_assert_eq!(
                holding,
                self.service.holding(),
                "the engine and the replica service disagree on holding staged groups"
            );
            // The staged group's bookkeeping comes before the replies: delivering them can make
            // a target say Hello to its local replica, which settles (docs/issues/10, S5), and the
            // engine's view of the group must match the service's by then.
            if !holding {
                // The service started the group's commit with this landing.
                if let Some(st) = self.staged.take() {
                    self.count_staged(st.writes);
                    self.committing.push_back(st);
                }
            } else if let Some(st) = self.staged.as_ref() {
                if st.bytes >= self.commit_target_bytes && self.service.launch() {
                    self.take_staged(false);
                } else if self.async_commits {
                    self.arm_after_landing(now, &landed);
                }
            }
            let LocalGroup {
                from,
                writes,
                large,
                forced,
                ..
            } = landed;
            self.deliver_local(now, from, replies);
            if !self.committing.is_empty() {
                continue;
            }
            if self.async_commits && self.staged.is_none() {
                let landed = LocalGroup {
                    writes,
                    large,
                    forced,
                    ..LocalGroup::default()
                };
                self.arm_after_landing(now, &landed);
            }
            return true;
        }
    }

    /// Stages the writes at the head of the local queue (`ReplicaService::stage`): behind the
    /// commit in flight, or, holding, with nothing in flight too. Their data goes to the disk
    /// now. The staged group's commit starts when the oldest in flight lands, or, holding or
    /// with pipelined commits, by [`Self::maybe_launch_staged`].
    fn stage_local(&mut self, now: Instant) {
        if !self.admit() {
            return;
        }
        if !self.async_commits {
            return;
        }
        let holding = self.commit_target_bytes > 0;
        if self.committing.is_empty() && !holding {
            return;
        }
        let n = self
            .local
            .iter()
            .take_while(|(_, _, f)| matches!(f, Frame::Write { .. }))
            .count();
        if n == 0 {
            self.service.clear_coalesce_expect();
            return;
        }
        let mut from = Vec::with_capacity(n);
        let mut items = Vec::with_capacity(n);
        for (f, s, frame) in self.local.drain(..n) {
            from.push(f);
            items.push((s, frame));
        }
        let taken = self.service.stage(&items);
        if taken == 0 {
            self.service.clear_coalesce_expect();
        }
        let rest: Vec<_> = from
            .drain(taken..)
            .zip(items.drain(taken..))
            .map(|(f, (s, frame))| (f, s, frame))
            .collect();
        if !rest.is_empty() {
            let newer = std::mem::replace(&mut self.local, rest);
            self.local.extend(newer);
        }
        if taken > 0 {
            let bytes: u64 = items[..taken].iter().map(|(_, f)| write_data_len(f)).sum();
            let large = items[..taken]
                .iter()
                .filter(|(_, f)| self.is_large(f))
                .count();
            let new = self.staged.is_none();
            let st = self.staged.get_or_insert_with(LocalGroup::default);
            st.from.extend(from);
            st.writes += taken;
            st.bytes += bytes;
            st.large += large;
            // A group started with nothing in flight keeps the last landing's wait while its
            // window lasts. Otherwise it arms its own from its first write: as long as the old
            // one waited for, or what is outstanding now (docs/issues/10, S1).
            if new
                && holding
                && self.committing.is_empty()
                && self.gather.is_none_or(|g| now >= g.until)
            {
                let target = self
                    .gather
                    .map_or(0, |g| g.target)
                    .max(self.pending_write_count());
                let wait = if large > 0 { Wait::Large } else { Wait::Small };
                self.arm(now, target, wait);
            }
            // Flush coalesce: partner's journal joins now; track it like launch().
            if self.service.coalesce_partner() {
                let st = self.staged.take().expect("staged");
                self.count_staged(st.writes);
                self.committing.push_back(st);
            }
        }
    }

    /// Starts the staged group's commit once its wait is over: holding, the wait armed at the
    /// last landing or at the group's first write; with pipelined commits, behind those in
    /// flight, after the same window as after a landing. Launching at the first staged write
    /// bypassed gathering and cut 1M sequential from about 700 to 548 MiB/s.
    fn maybe_launch_staged(&mut self, now: Instant) {
        let (staged_n, staged_large) = match &self.staged {
            Some(st) if st.writes > 0 => (st.writes, st.large),
            _ => return,
        };
        if self.async_commits && !self.gather_window.is_zero() {
            if self.gather.is_none() {
                // Behind a commit in flight that nothing can join: its landing arms the wait.
                if !self.committing.is_empty() && !self.service.pipelined() {
                    return;
                }
                let wait = if self.commit_target_bytes > 0 && staged_large > 0 {
                    Wait::Large
                } else {
                    Wait::Small
                };
                self.arm(now, staged_n.max(self.pending_write_count()), wait);
                // A group that already holds the floor's bytes need not wait a pump for it.
                if !(self.commit_target_bytes > 0
                    && self.pending_write_bytes() >= self.commit_target_bytes)
                {
                    return;
                }
            }
            let (ready, expired) = self.gather_ready(now, self.pending_write_count());
            // The wait stays until the group can start: behind a commit in flight, it may not.
            if !ready || !self.service.launch() {
                return;
            }
            if expired {
                self.commit_stats.expired += 1;
            }
            self.gather = None;
            self.take_staged(false);
            return;
        }
        if self.service.launch() {
            self.take_staged(false);
        }
    }

    /// The staged group, launched (`ReplicaService::launch` returned true), joins the commits in
    /// flight. `forced`: it started for a request queued behind it, or to settle.
    fn take_staged(&mut self, forced: bool) {
        let mut st = self.staged.take().expect("staged");
        st.forced = forced;
        self.count_staged(st.writes);
        self.progress += 1;
        self.committing.push_back(st);
    }

    fn count_staged(&mut self, writes: usize) {
        self.commit_stats.commits += 1;
        self.commit_stats.writes += writes as u64;
        self.commit_stats.staged += 1;
    }

    /// Starts the held group's commit now, whatever its wait, when nothing is in flight: before
    /// anything that settles the store (docs/issues/10, S5).
    fn launch_held(&mut self) {
        if self.committing.is_empty() && self.staged.is_some() && self.service.launch() {
            self.take_staged(true);
        }
    }

    /// Retain submitted buffers until their real CQEs arrive, without delivering success.
    fn drain_quarantined_local(&mut self) {
        self.invalidate_clock();
        // Stop/detach may wait for actual CQEs; submitted write buffers must outlive them.
        while !self.committing.is_empty() || self.staged.is_some() {
            self.launch_held();
            if self.staged.is_some() && self.service.holding() && self.service.launch() {
                self.take_staged(true);
            }
            if self.committing.is_empty() {
                break;
            }
            let _ = self.service.poll(true);
            self.committing.pop_front();
            if !self.service.holding()
                && let Some(st) = self.staged.take()
            {
                self.committing.push_back(st);
            }
        }
    }

    /// Everything queued for the local replica handled and durable, a held group included,
    /// without waiting for writes to gather: before using the replica directly, or anything
    /// that settles its store (docs/issues/10, S5).
    fn settle_local(&mut self, now: Instant) {
        if !self.clock.status().is_healthy() {
            self.drain_quarantined_local();
            return;
        }
        let settling = std::mem::replace(&mut self.settling, true);
        while !self.committing.is_empty() || !self.local.is_empty() || self.staged.is_some() {
            let before = self.progress;
            self.launch_held();
            self.finish_commit(now, true);
            self.launch_held();
            self.flush_local(now);
            if !self.clock.status().is_healthy() {
                self.drain_quarantined_local();
                break;
            }
            if self.progress == before {
                eprintln!(
                    "flaminio-node: settling the local replica made no progress ({} commits in flight, {} requests queued, a group staged: {})",
                    self.committing.len(),
                    self.local.len(),
                    self.staged.is_some()
                );
                debug_assert!(false, "settle_local made no progress");
                break;
            }
        }
        self.settling = settling;
    }

    fn deliver_local(&mut self, now: Instant, from: Vec<LocalFrom>, replies: Vec<Reply>) {
        if self.sample().is_none() {
            return;
        }
        let mut touched = Vec::new();
        for (from, reply) in from.into_iter().zip(replies) {
            match from {
                LocalFrom::Conn(c) => {
                    if let Some((_, tx)) = self.conns.get(&c) {
                        let _ = tx.try_send(reply);
                    }
                }
                LocalFrom::Target { volume, session } => {
                    if let Reply::Send(frame) = reply {
                        let local = self.targets.get(&volume).map(|rt| rt.local);
                        if let Some(local) = local {
                            self.reply_to_target(now, &volume, local, session, frame);
                            touched.push(volume);
                        }
                    }
                    // Nothing or Close: the target's timeout handles it.
                }
            }
        }
        touched.dedup();
        for v in touched {
            self.drain(now, &v);
        }
    }

    fn inspect(&mut self, volume: &str, offset: u64, len: u64) -> Option<Vec<u8>> {
        let rt = self.targets.get(volume);
        let (chunk_size, sref) = match rt {
            Some(rt) => match rt.sessions.get(&rt.local) {
                Some(Sess::Local { sref: Some(s), .. }) => (rt.chunk_size, *s),
                _ => return None,
            },
            None => {
                // Not a target here: find a replica session of the volume.
                let (s, _) = self.conns.values().next()?;
                (self.service.chunk_size_of(s.vol)?, *s)
            }
        };
        let mut out = Vec::with_capacity(len as usize);
        let mut off = offset;
        while off < offset + len {
            let chunk = (off / chunk_size) as u32;
            let within = off % chunk_size;
            let n = (chunk_size - within).min(offset + len - off);
            let f = Frame::Read {
                req: 0,
                chunk,
                ext: Extent::new((within / BLOCK as u64) as u32, (n / BLOCK as u64) as u32),
            };
            match self.service.handle(&[(sref, f)]).pop()? {
                Reply::Send(Frame::Data { data, .. }) => out.extend_from_slice(&data),
                _ => return None,
            }
            off += n;
        }
        Some(out)
    }

    // --- Targets -----------------------------------------------------------------------------

    fn on_meta(&mut self, now: Instant, id: u64, answer: Answer) {
        let Some(wait) = self.meta_waits.remove(&id) else {
            return;
        };
        match wait {
            MetaWait::Register => match answer {
                Answer::Failed { code, message } => {
                    eprintln!(
                        "flaminio-node: registration failed: {code:?}: {message}; trying again"
                    );
                    self.registration = Registration::Needed;
                    self.next_register = now + REGISTER_RETRY;
                }
                _ => {
                    self.registration = Registration::Done;
                    self.registrations += 1;
                    // Live at once: the first heartbeat goes out at the next turn.
                    self.next_heartbeat = now;
                }
            },
            MetaWait::Heartbeat { registered } => {
                if let Answer::Failed { code, message } = answer {
                    eprintln!("flaminio-node: heartbeat failed: {code:?}: {message}");
                    // The service does not know the node (its record is gone): register again,
                    // unless a registration already succeeded since this heartbeat went out.
                    if code == flaminio_metaclient::Code::NotFound
                        && registered == self.registrations
                        && self.registration == Registration::Done
                    {
                        self.registration = Registration::Needed;
                        self.next_register = now;
                    }
                }
            }
            MetaWait::Attach {
                volume,
                part,
                attempt,
            } => {
                // An answer to an attach dropped since, whether data or a failure, must neither
                // fill in nor end the attach that holds the volume now (docs/issues/6, S3).
                let Some(a) = self
                    .attaching
                    .get_mut(&volume)
                    .filter(|a| a.attempt == attempt)
                else {
                    return;
                };
                match (part, answer) {
                    (0, Answer::GetVolume(r)) => match r.volume {
                        Some(v) => a.volume = Some(v),
                        // An empty answer would leave the attach waiting for good.
                        None => {
                            self.attaching.remove(&volume);
                            self.out.push(Output::AttachFailed {
                                volume,
                                why: "the metadata service has no such volume".into(),
                            });
                            return;
                        }
                    },
                    (1, Answer::ListNodes(r)) => a.nodes = Some(r.nodes),
                    (_, Answer::Failed { message, .. }) => {
                        self.attaching.remove(&volume);
                        self.out.push(Output::AttachFailed {
                            volume,
                            why: message,
                        });
                        return;
                    }
                    _ => return,
                }
                if a.volume.is_some() && a.nodes.is_some() {
                    let a = self.attaching.remove(&volume).expect("attaching");
                    match self.start_target(now, &volume, a) {
                        Ok(epoch) => {
                            let size = self.targets[&volume].size;
                            self.out.push(Output::Attached {
                                volume: volume.clone(),
                                epoch,
                                size,
                            })
                        }
                        Err(why) => self.out.push(Output::AttachFailed {
                            volume: volume.clone(),
                            why,
                        }),
                    }
                    self.drain(now, &volume);
                }
            }
            MetaWait::Isr {
                volume,
                generation,
                sm_req,
                chunks,
            } => {
                let Some(sample) = self.sample() else {
                    return;
                };
                let us = sample.elapsed_us();
                let Some(rt) = self
                    .targets
                    .get_mut(&volume)
                    .filter(|rt| rt.generation == generation && rt.sm.accepts_meta_reply(sm_req))
                else {
                    return;
                };
                let Answer::ChangeIsr(r) = answer else { return };
                if r.epoch > rt.view.epoch {
                    // Fencing precedes payload validation: even an unusable payload must
                    // not hide a higher authenticated epoch from the current target.
                    let mut view = rt.view.clone();
                    view.epoch = r.epoch;
                    rt.sm
                        .on_meta(us, sm_req, &MetaReply::Rejected { vol: 0, view });
                    self.drain(now, &volume);
                    return;
                }
                if !valid_isr_indices(&chunks, rt.view.chunks.len())
                    || r.chunks.len() != chunks.len()
                    || !rt.valid_chunks(&r.chunks)
                {
                    return;
                }
                if r.epoch == rt.view.epoch {
                    for (&c, m) in chunks.iter().zip(&r.chunks) {
                        rt.merge_chunk(c as usize, m);
                    }
                }
                let mut view = rt.view.clone();
                view.epoch = r.epoch;
                if rt.missing_addrs() {
                    self.meta_call(
                        Call::ListNodes(pb::ListNodesRequest {}),
                        MetaWait::Nodes {
                            volume: volume.clone(),
                        },
                    );
                }
                let Some(rt) = self.targets.get_mut(&volume) else {
                    return;
                };
                let reply = if r.applied {
                    MetaReply::Applied { vol: 0, view }
                } else {
                    MetaReply::Rejected { vol: 0, view }
                };
                rt.sm.on_meta(us, sm_req, &reply);
                self.drain(now, &volume);
            }
            MetaWait::Refresh {
                volume,
                generation,
                sm_req,
            } => {
                let Some(sample) = self.sample() else {
                    return;
                };
                let us = sample.elapsed_us();
                let Some(rt) = self
                    .targets
                    .get_mut(&volume)
                    .filter(|rt| rt.generation == generation && rt.sm.accepts_meta_reply(sm_req))
                else {
                    return;
                };
                // Without an answer the target's read times out, and it reads again later.
                let Answer::GetVolume(r) = answer else { return };
                let Some(v) = r.volume else { return };
                if v.epoch > rt.view.epoch {
                    let mut view = rt.view.clone();
                    view.epoch = v.epoch;
                    rt.sm
                        .on_meta(us, sm_req, &MetaReply::Applied { vol: 0, view });
                    self.drain(now, &volume);
                    return;
                }
                if !rt.valid_refresh(&volume, &v) {
                    return;
                }
                // Keep the newest view per chunk for Hello's chunk lists: the reply may be
                // older than what an ISR change already showed.
                if v.epoch == rt.view.epoch {
                    for (i, new) in v.chunks.iter().enumerate() {
                        rt.merge_chunk(i, new);
                    }
                }
                let mut view = rt.view.clone();
                view.epoch = v.epoch;
                if rt.missing_addrs() {
                    self.meta_call(
                        Call::ListNodes(pb::ListNodesRequest {}),
                        MetaWait::Nodes {
                            volume: volume.clone(),
                        },
                    );
                }
                let Some(rt) = self.targets.get_mut(&volume) else {
                    return;
                };
                rt.sm
                    .on_meta(us, sm_req, &MetaReply::Applied { vol: 0, view });
                self.drain(now, &volume);
            }
            MetaWait::Nodes { volume } => {
                let Some(rt) = self.targets.get_mut(&volume) else {
                    return;
                };
                let Answer::ListNodes(r) = answer else { return };
                for (i, name) in rt.nodes.iter().enumerate() {
                    if rt.addrs[i].is_none()
                        && let Some(n) = r.nodes.iter().find(|n| &n.id == name)
                    {
                        rt.addrs[i] = n.address.parse().ok();
                    }
                }
            }
        }
    }

    fn start_target(&mut self, _now: Instant, volume: &str, a: Attaching) -> Result<u64, String> {
        let v = a.volume.ok_or("no such volume")?;
        let geometry = validate_volume(volume, &v)?;
        if v.target != self.cfg.node {
            return Err(format!(
                "the volume's target is {:?}, not this node",
                v.target
            ));
        }
        // The local replica takes the volume's first epoch from the token, so the token must be
        // this attach's: epochs of different volumes of a name never overlap, so the epoch fixes
        // the volume (docs/issues/30, S6). The opaque proof verified both signatures.
        if !a.proof.is_current(&self.nonce, Purpose::Normal)
            || !a.proof.matches_target(volume, &self.cfg.node, v.epoch)
        {
            return Err(format!(
                "handoff is for epoch {} on {}; current metadata epoch {} requires a matching current-process agent/admin Attach",
                a.proof.claims().epoch,
                a.proof.claims().node,
                v.epoch
            ));
        }
        let claims = a.proof.claims();
        if self
            .ran_epochs
            .get(volume)
            .is_some_and(|&old| v.epoch <= old)
        {
            return Err(
                "this process already ran that epoch; obtain a new-epoch Attach handoff".into(),
            );
        }
        // Validated record/member caps bound this map to 1 + N*(R+1), below u32::MAX.
        let mut nodes: Vec<String> = Vec::new();
        let id = |name: &str, nodes: &mut Vec<String>| -> NodeId {
            match nodes.iter().position(|n| n == name) {
                Some(i) => i as NodeId,
                None => {
                    nodes.push(name.to_string());
                    (nodes.len() - 1) as NodeId
                }
            }
        };
        let local = id(&self.cfg.node, &mut nodes);
        let chunks: Vec<ChunkMeta> = v
            .chunks
            .iter()
            .map(|c| ChunkMeta {
                replicas: c.replicas.iter().map(|r| id(r, &mut nodes)).collect(),
                isr: c.isr.iter().map(|r| id(r, &mut nodes)).collect(),
                isr_ver: c.isr_ver,
                moving: (!c.moving_from.is_empty())
                    .then(|| (id(&c.moving_from, &mut nodes), id(&c.moving_to, &mut nodes))),
            })
            .collect();
        let all = a.nodes.unwrap_or_default();
        let addrs = nodes
            .iter()
            .map(|n| {
                all.iter()
                    .find(|x| &x.id == n)
                    .and_then(|x| x.address.parse().ok())
            })
            .collect();
        let view = VolumeMeta {
            epoch: v.epoch,
            target: Some(local),
            min_isr: v.min_isr as usize,
            blocks_per_chunk: geometry.blocks_per_chunk,
            chunks,
        };
        let sample = self
            .sample()
            .ok_or("clock quarantined; operator restart required")?;
        let us = sample.elapsed_us();
        self.clock.admit(sample).map_err(|e| e.to_string())?;
        self.ran_epochs.insert(volume.to_string(), v.epoch);
        let sm = Target::new(local, 0, &view, self.cfg.timing, self.cfg.max_io, us);
        self.targets.insert(
            volume.to_string(),
            TargetRt {
                sm,
                view,
                generation: a.attempt,
                token: a.proof.capability().to_vec(),
                first_epoch: claims.first_epoch,
                size: v.size_bytes,
                chunk_size: v.chunk_bytes,
                replica_count: v.replicas as usize,
                nodes,
                addrs,
                local,
                sessions: BTreeMap::new(),
                ops: BTreeMap::new(),
                subs: BTreeMap::new(),
                reads: BTreeMap::new(),
                deposed_reported: false,
            },
        );
        Ok(v.epoch)
    }

    fn submit(&mut self, now: Instant, volume: &str, op: u64, io: Io) {
        let Some(sample) = self.sample() else {
            // This operation is not in rt.ops yet, so invalidation cannot complete it.
            self.out.push(Output::Done {
                op,
                result: Err(IoError::ClockQuarantined),
                permit: None,
            });
            return;
        };
        let us = sample.elapsed_us();
        let Some(rt) = self.targets.get_mut(volume) else {
            self.out.push(Output::Done {
                op,
                result: Err(IoError::NotAttached),
                permit: None,
            });
            return;
        };
        let (offset, len) = match &io {
            Io::Read { offset, len } | Io::ReadDirect { offset, len } => (*offset, *len),
            Io::Write { offset, data } => (*offset, data.len() as u64),
        };
        let b = BLOCK as u64;
        if len == 0
            || !offset.is_multiple_of(b)
            || !len.is_multiple_of(b)
            || offset.checked_add(len).is_none_or(|e| e > rt.size)
        {
            self.out.push(Output::Done {
                op,
                result: Err(IoError::Invalid),
                permit: None,
            });
            return;
        }
        let mut pieces = Vec::new();
        let mut off = offset;
        while off < offset + len {
            let chunk = (off / rt.chunk_size) as u32;
            let within = off % rt.chunk_size;
            let n = (rt.chunk_size - within).min(offset + len - off);
            pieces.push((
                chunk,
                Extent::new((within / b) as u32, (n / b) as u32),
                (off - offset) as usize,
                n as usize,
            ));
            off += n;
        }
        rt.ops.insert(
            op,
            ClientOp {
                chunks: pieces.iter().map(|(chunk, ..)| *chunk).collect(),
                pending: pieces.len(),
                failed: None,
                read: matches!(io, Io::Read { .. }).then(|| vec![0u8; len as usize]),
                direct: matches!(io, Io::ReadDirect { .. }),
            },
        );
        for (chunk, ext, at, n) in pieces {
            let sub = self.next_sub;
            self.next_sub += 1;
            rt.subs.insert(sub, (op, at, n));
            let kind = match &io {
                Io::Read { .. } | Io::ReadDirect { .. } => Op::Read { chunk, ext },
                Io::Write { data, .. } => Op::Write {
                    chunk,
                    ext,
                    data: Payload::Bytes(data.slice(at..at + n)),
                },
            };
            rt.sm.submit(us, sub, kind);
        }
        self.drain(now, volume);
    }

    /// Routes a target's outputs. Not reentrant: routing can produce replies that make the
    /// same or another target output more, and those wait for the current outputs to be
    /// routed, so every session's messages leave in the order the target produced them.
    fn drain(&mut self, now: Instant, volume: &str) {
        self.pending_drain.insert(volume.to_string());
        if self.draining {
            return;
        }
        self.draining = true;
        while let Some(v) = self.pending_drain.pop_first() {
            self.drain_one(now, &v);
        }
        self.draining = false;
    }

    fn drain_one(&mut self, now: Instant, volume: &str) {
        if self.sample().is_none() {
            return;
        }
        let Some(rt) = self.targets.get_mut(volume) else {
            return;
        };
        for e in rt.sm.take_events() {
            let flaminio_repl::target::Event::SessionFailed { node, reason } = e;
            let name = rt.nodes.get(node as usize).map_or("?", String::as_str);
            eprintln!("flaminio-node: {volume}: session to {name} failed: {reason}");
        }
        let generation = rt.generation;
        let outs = rt.sm.take_output();
        for o in outs {
            if self.sample().is_none() {
                return;
            }
            match o {
                TOut::Send { to, msg } => self.send_from_target(now, volume, to, msg),
                TOut::Meta {
                    req,
                    body: MetaRequest::Get { .. },
                } => self.meta_call(
                    Call::GetVolume(pb::GetVolumeRequest {
                        id: volume.to_string(),
                    }),
                    MetaWait::Refresh {
                        volume: volume.to_string(),
                        generation,
                        sm_req: req,
                    },
                ),
                TOut::Meta { req, body } => {
                    let MetaRequest::ChangeIsr { epoch, changes, .. } = body else {
                        continue;
                    };
                    if changes.is_empty() || changes.len() > MAX_CHANGES_PER_REQUEST {
                        // Keep a producer regression from forwarding an unsupported RPC.
                        // The core retains its pending work and retries after its timeout.
                        continue;
                    }
                    let rt = self.targets.get(volume).expect("target");
                    let chunks: Vec<u32> = changes.iter().map(|c| c.chunk).collect();
                    if !valid_isr_indices(&chunks, rt.view.chunks.len()) {
                        continue;
                    }
                    let names = |s: &std::collections::BTreeSet<NodeId>| -> Vec<String> {
                        s.iter().map(|n| rt.nodes[*n as usize].clone()).collect()
                    };
                    let mut summary: BTreeMap<Vec<String>, usize> = BTreeMap::new();
                    for c in &changes {
                        *summary.entry(names(&c.new_isr)).or_default() += 1;
                    }
                    for (isr, n) in summary {
                        eprintln!(
                            "flaminio-node: {volume}: proposing ISR {} for {n} chunks",
                            isr.join("+")
                        );
                    }
                    let call = pb::ChangeIsrRequest {
                        volume: volume.to_string(),
                        epoch,
                        changes: changes
                            .iter()
                            .map(|c| pb::IsrChange {
                                chunk: c.chunk,
                                expected_ver: c.expected_ver,
                                new_isr: names(&c.new_isr),
                                change_replicas: c.new_replicas.is_some(),
                                new_replicas: c
                                    .new_replicas
                                    .iter()
                                    .flatten()
                                    .map(|n| rt.nodes[*n as usize].clone())
                                    .collect(),
                            })
                            .collect(),
                    };
                    self.meta_call(
                        Call::ChangeIsr(call),
                        MetaWait::Isr {
                            volume: volume.to_string(),
                            generation,
                            sm_req: req,
                            chunks,
                        },
                    );
                }
                TOut::Done { op: sub, result } => self.sub_done(volume, sub, result),
            }
        }
        let Some(rt) = self.targets.get_mut(volume) else {
            return;
        };
        if rt.sm.is_deposed() && !rt.deposed_reported {
            rt.deposed_reported = true;
            for key in rt.sessions.values().filter_map(|s| match s {
                Sess::Remote { key, .. } => Some(*key),
                _ => None,
            }) {
                self.net.send(NetCommand::Close { key });
            }
            self.out.push(Output::Deposed {
                volume: volume.to_string(),
            });
            self.targets.remove(volume);
        }
    }

    fn detach(&mut self, volume: &str) {
        // An attach still waiting for the metadata service ends too: its answers are ignored.
        self.attaching.remove(volume);
        let Some(rt) = self.targets.remove(volume) else {
            return;
        };
        for s in rt.sessions.values() {
            if let Sess::Remote { key, .. } = s {
                self.net.send(NetCommand::Close { key: *key });
            }
        }
        for op in rt.ops.keys() {
            self.out.push(Output::Done {
                op: *op,
                result: Err(IoError::NotAttached),
                permit: None,
            });
        }
        self.keys.retain(|_, (v, _, _)| v != volume);
        self.local_reads.retain(|_, p| p.volume != volume);
        self.out.push(Output::Detached {
            volume: volume.to_string(),
        });
    }

    fn sub_done(&mut self, volume: &str, sub: u64, result: OpResult<Payload>) {
        let Some(sample) = self.sample() else {
            return;
        };
        let Some(rt) = self.targets.get_mut(volume) else {
            return;
        };
        let Some((op, at, n)) = rt.subs.remove(&sub) else {
            return;
        };
        let Some(c) = rt.ops.get_mut(&op) else { return };
        match result {
            OpResult::Written => {}
            OpResult::Read(p) if p.len() != n => c.failed = Some(IoError::Invalid),
            // Already in the client's buffer.
            OpResult::Read(Payload::Placed(_)) if c.direct => {}
            OpResult::Read(Payload::Bytes(b)) if c.direct => self.out.push(Output::Piece {
                generation: sample.generation(),
                op,
                at,
                data: b,
            }),
            OpResult::Read(Payload::Bytes(b)) => match c.read.as_mut() {
                Some(buf) => buf[at..at + n].copy_from_slice(&b),
                None => c.failed = Some(IoError::Invalid),
            },
            OpResult::Read(Payload::Placed(_)) => c.failed = Some(IoError::Invalid),
            OpResult::Failed => c.failed = Some(IoError::Deposed),
            OpResult::Unreadable => c.failed = Some(IoError::Unreadable),
        }
        c.pending -= 1;
        if c.pending == 0 {
            let c = rt.ops.remove(&op).expect("op");
            let result = match c.failed {
                Some(e) => Err(e),
                None => Ok(c.read),
            };
            let mut result = result;
            let permit = if result.is_ok() {
                if rt.sm.revalidate_success(sample.elapsed_us(), &c.chunks) {
                    self.clock.success_permit(sample).ok()
                } else {
                    None
                }
            } else {
                None
            };
            if result.is_ok() && permit.is_none() {
                result = Err(if self.clock.status().is_healthy() {
                    IoError::Deposed
                } else {
                    IoError::ClockQuarantined
                });
            }
            self.out.push(Output::Done { op, result, permit });
        }
    }

    fn send_from_target(
        &mut self,
        now: Instant,
        volume: &str,
        to: NodeId,
        msg: ToReplica<Payload>,
    ) {
        let Some(sample) = self.sample() else {
            return;
        };
        let generation = sample.generation();
        let rt = self.targets.get_mut(volume).expect("target");
        if let Request::Read { ext, .. } = &msg.body {
            rt.reads.insert((to, msg.req), ext.len as usize * BLOCK);
        }
        if let Request::Hello { chunks } = msg.body {
            if to == rt.local {
                let a = Accepted {
                    peer_node: self.cfg.node.clone(),
                    req: msg.req,
                    epoch: msg.epoch,
                    session: msg.session,
                    claims: Claims {
                        volume: volume.to_string(),
                        epoch: msg.epoch,
                        node: self.cfg.node.clone(),
                        issued_at: 0,
                        first_epoch: rt.first_epoch,
                    },
                    size: rt.size,
                    chunk_size: rt.chunk_size,
                    chunks,
                };
                // A Hello fences the store, which settles it: a held group starts first.
                self.settle_local(now);
                if !self.admit() {
                    return;
                }
                let Some(rt) = self.targets.get_mut(volume) else {
                    return;
                };
                match self.service.hello(&a) {
                    Ok((sref, frame)) => {
                        rt.sessions.insert(
                            rt.local,
                            Sess::Local {
                                session: msg.session,
                                sref: Some(sref),
                            },
                        );
                        let local = rt.local;
                        self.reply_to_target(now, volume, local, msg.session, frame);
                    }
                    Err(why) => {
                        eprintln!(
                            "flaminio-node: {volume}: local replica refused the session: {why}"
                        );
                        rt.sessions.insert(
                            rt.local,
                            Sess::Local {
                                session: msg.session,
                                sref: None,
                            },
                        );
                    }
                }
                return;
            }
            let Some(addr) = rt.addrs.get(to as usize).copied().flatten() else {
                return; // no address: the request times out
            };
            let key = self.next_key;
            self.next_key += 1;
            if let Some(Sess::Remote { key: old, .. }) = rt.sessions.insert(
                to,
                Sess::Remote {
                    session: msg.session,
                    key,
                },
            ) {
                self.keys.remove(&old);
                self.net.send(NetCommand::Close { key: old });
            }
            self.keys.insert(key, (volume.to_string(), to, msg.session));
            let hello = Frame::Hello {
                req: msg.req,
                epoch: msg.epoch,
                session: msg.session,
                volume: volume.to_string(),
                token: rt.token.clone(),
                size: rt.size,
                chunk_size: rt.chunk_size,
                chunks,
            };
            self.net.send(NetCommand::Connect {
                generation,
                key,
                addr,
                node: rt.nodes[to as usize].clone(),
                hello: Box::new(hello),
            });
            return;
        }
        // A direct client read from the local replica: admitted by the replica rules here,
        // carried out by the frontend straight into the client's buffer.
        if let Request::Read { chunk, ext } = msg.body
            && to == rt.local
            && let Some(Sess::Local {
                session,
                sref: Some(sref),
            }) = rt.sessions.get(&to)
            && *session == msg.session
            && let Some(sub) = rt.sm.client_read_of(to, msg.req)
            && let Some(&(op, at, n)) = rt.subs.get(&sub)
            && rt.ops.get(&op).is_some_and(|c| c.direct)
        {
            let sref = *sref;
            // No flush of earlier local requests is needed: the target holds the blocks'
            // lock, so any earlier write to them has completed here already.
            match self.service.admit_read(sref, msg.req, chunk, ext) {
                crate::replica::Admit::Read {
                    key, offset, epoch, ..
                } => {
                    let token = self.next_token;
                    self.next_token += 1;
                    self.local_reads.insert(
                        token,
                        PendingLocal {
                            generation,
                            volume: volume.to_string(),
                            sref,
                            session: msg.session,
                            req: msg.req,
                            key,
                            offset,
                            epoch,
                        },
                    );
                    self.out.push(Output::LocalRead(LocalRead {
                        op,
                        at,
                        len: n,
                        token,
                        generation,
                    }));
                }
                crate::replica::Admit::Reply(Reply::Send(frame)) => {
                    let local = self.targets[volume].local;
                    self.reply_to_target(now, volume, local, msg.session, frame);
                }
                crate::replica::Admit::Reply(_) => {} // the target's timeout handles it
            }
            return;
        }
        let frame = to_frame(&msg);
        match rt.sessions.get(&to) {
            Some(Sess::Local {
                session,
                sref: Some(sref),
            }) if *session == msg.session => {
                let sref = *sref;
                self.local.push((
                    LocalFrom::Target {
                        volume: volume.to_string(),
                        session: msg.session,
                    },
                    sref,
                    frame,
                ));
            }
            Some(Sess::Remote { session, key }) if *session == msg.session => {
                self.net.send(NetCommand::Send {
                    generation,
                    key: *key,
                    frame,
                });
            }
            _ => {} // no such session any more: the request times out
        }
    }

    fn reply_to_target(
        &mut self,
        _now: Instant,
        volume: &str,
        from: NodeId,
        session: u64,
        frame: Frame,
    ) {
        let Some(sample) = self.sample() else {
            return;
        };
        let us = sample.elapsed_us();
        let Some(rt) = self.targets.get_mut(volume) else {
            return;
        };
        let Some((req, body)) = from_frame(frame) else {
            return;
        };
        if let Response::Data(p) = &body {
            // Exactly the length asked for, or the reply is dropped.
            match rt.reads.remove(&(from, req)) {
                Some(n) if n == p.len() => {}
                _ => return,
            }
        }
        let m = FromReplica {
            vol: 0,
            epoch: rt.sm.epoch(),
            session,
            req,
            body,
        };
        rt.sm.on_reply(us, from, &m);
    }
}

impl<D: BlockDevice> Engine<D> {
    /// The fast path for a client read (spec §5, fast reads): if the volume's target admits
    /// every piece at once, by the same rules as its queue, and the local replica admits each
    /// on the target's session, the frontend reads the pieces from the store itself
    /// ([`Self::plan_fast_read`]) and ends with [`Self::finish_fast_read`]. `None`: use
    /// [`Input::Io`], which applies the same rules through the queue.
    pub fn try_fast_read(&mut self, volume: &str, offset: u64, len: u64) -> Option<FastLocal> {
        let now = Instant::now();
        let sample = self.sample()?;
        let us = sample.elapsed_us();
        let rt = self.targets.get_mut(volume)?;
        let b = BLOCK as u64;
        if len == 0
            || !offset.is_multiple_of(b)
            || !len.is_multiple_of(b)
            || offset.checked_add(len).is_none_or(|e| e > rt.size)
        {
            return None;
        }
        let Some(Sess::Local {
            session,
            sref: Some(sref),
        }) = rt.sessions.get(&rt.local)
        else {
            return None;
        };
        let (session, sref) = (*session, *sref);
        let mut pieces = Vec::with_capacity(1);
        let mut off = offset;
        let mut refused = false;
        while off < offset + len {
            let chunk = (off / rt.chunk_size) as u32;
            let within = off % rt.chunk_size;
            let n = (rt.chunk_size - within).min(offset + len - off);
            let ext = Extent::new((within / b) as u32, (n / b) as u32);
            let Some(fr) = rt.sm.try_fast_read(us, chunk, ext) else {
                refused = true;
                break;
            };
            // The local replica's rules on the target's session: fencing above all.
            let admitted = fr.session == session
                && match self.service.admit_read(sref, 0, chunk, ext) {
                    crate::replica::Admit::Read {
                        key,
                        offset: in_chunk,
                        epoch,
                        ..
                    } => {
                        pieces.push(FastPiece {
                            fr,
                            sref,
                            at: (off - offset) as usize,
                            len: n as usize,
                            key,
                            offset: in_chunk,
                            epoch,
                        });
                        true
                    }
                    crate::replica::Admit::Reply(_) => false,
                };
            if !admitted {
                rt.sm.finish_fast_read(us, fr, false);
                refused = true;
                break;
            }
            off += n;
        }
        if refused {
            for p in pieces {
                rt.sm.finish_fast_read(us, p.fr, false);
            }
            self.drain(now, volume);
            return None;
        }
        Some(FastLocal {
            pieces,
            generation: sample.generation(),
        })
    }

    /// Plans piece `i` of a fast read into `buf` (exactly its length): the device reads to
    /// perform. Never-written ranges are zero-filled now.
    pub fn plan_fast_read(
        &mut self,
        fast: &FastLocal,
        i: usize,
        buf: &mut [u8],
    ) -> Result<flaminio_store::ReadPlan, IoError> {
        if !self.admit() {
            return Err(IoError::ClockQuarantined);
        }
        self.clock
            .check_generation(fast.generation)
            .map_err(|_| IoError::ClockQuarantined)?;
        let p = fast.pieces.get(i).ok_or(IoError::Invalid)?;
        if buf.len() != p.len {
            return Err(IoError::Invalid);
        }
        self.service
            .plan_read(p.key, p.offset, buf, p.epoch)
            .map_err(|e| {
                eprintln!("flaminio-node: fast read plan: {e}");
                IoError::Invalid
            })
    }

    /// Ends a fast read: `ok` if every piece was read (and verified). A final permit means its data
    /// counts; otherwise the frontend submits the read again through [`Input::Io`]. The data
    /// counts only if no newer target fenced the local replica while the reads were under way:
    /// a newer target writes only to replicas it has fenced (spec §5, fast reads).
    pub fn finish_fast_read(
        &mut self,
        volume: &str,
        fast: FastLocal,
        ok: bool,
    ) -> Option<SuccessPermit> {
        let now = Instant::now();
        let sample = self.sample()?;
        self.clock.check_generation(fast.generation).ok()?;
        let us = sample.elapsed_us();
        let ok = ok
            && fast
                .pieces
                .iter()
                .all(|p| self.service.still_fenced_at(p.sref));
        let rt = self.targets.get_mut(volume)?;
        let mut all = ok;
        for p in fast.pieces {
            all &= rt.sm.finish_fast_read(us, p.fr, ok);
        }
        let permit = all
            .then(|| self.clock.success_permit(sample).ok())
            .flatten();
        self.drain(now, volume);
        permit
    }

    /// Plans a local read into `buf` (exactly `lr.len` bytes): the device reads for the
    /// frontend to perform. Unmapped ranges are zero-filled now.
    pub fn plan_local_read(
        &mut self,
        lr: &LocalRead,
        buf: &mut [u8],
    ) -> Result<flaminio_store::ReadPlan, IoError> {
        if !self.admit() {
            return Err(IoError::ClockQuarantined);
        }
        self.clock
            .check_generation(lr.generation)
            .map_err(|_| IoError::ClockQuarantined)?;
        let p = self.local_reads.get(&lr.token).ok_or(IoError::Invalid)?;
        if buf.len() != lr.len {
            return Err(IoError::Invalid);
        }
        self.service
            .plan_read(p.key, p.offset, buf, p.epoch)
            .map_err(|e| {
                eprintln!("flaminio-node: {}: read plan: {e}", p.volume);
                IoError::Invalid
            })
    }

    /// The target epoch, session and request of a pending local read, for the tests.
    #[doc(hidden)]
    pub fn local_read_ids(&self, lr: &LocalRead) -> Option<(u64, u64, u64)> {
        let p = self.local_reads.get(&lr.token)?;
        Some((p.sref.epoch, p.session, p.req))
    }

    /// Ends a local read: `ok` if its device reads completed and verified. A failed read is
    /// sent again to another ISR member, and the local replica stays a member (docs/issues/5).
    pub fn local_read_done(
        &mut self,
        lr: &LocalRead,
        plan: Option<flaminio_store::ReadPlan>,
        ok: bool,
    ) {
        let now = Instant::now();
        if let Some(plan) = plan {
            self.service.finish_read(plan);
        }
        let Some(p) = self.local_reads.remove(&lr.token) else {
            return;
        };
        if self.clock.check_generation(p.generation).is_err() {
            let _ = self.sample();
            return;
        }
        if !ok {
            eprintln!("flaminio-node: {}: local read failed", p.volume);
        }
        let Some(sample) = self.sample() else {
            return;
        };
        let us = sample.elapsed_us();
        let Some(rt) = self.targets.get_mut(&p.volume) else {
            return;
        };
        // Pending reads outlive a deposed target until the detach, and a new target numbers its
        // sessions and requests from 1 again: a stale read must not touch the new one's. The
        // read's session is the target's local session, opened at the target's epoch.
        if p.sref.epoch != rt.sm.epoch() {
            return;
        }
        let local = rt.local;
        let expected = rt.reads.remove(&(local, p.req));
        // The read was admitted before the device read: if a newer target fenced the replica
        // meanwhile, it may have written to it, and the answer is what the replica would have
        // given at the time of the read (spec §5, fast reads). That holds for a failed read
        // too, which a fence can cause (the store refuses to plan a read at an older epoch):
        // the target learns it was deposed, instead of trying another member.
        let fenced = self.service.fenced_epoch(p.sref);
        if !ok && fenced == p.sref.epoch {
            rt.sm
                .on_read_failed(us, local, p.sref.epoch, p.session, p.req);
            self.drain(now, &p.volume);
            return;
        }
        if expected != Some(lr.len) {
            return;
        }
        let body = if fenced == p.sref.epoch {
            Response::Data(Payload::Placed(lr.len))
        } else {
            Response::Stale { fenced }
        };
        let m = FromReplica {
            vol: 0,
            epoch: p.sref.epoch,
            session: p.session,
            req: p.req,
            body,
        };
        rt.sm.on_reply(us, local, &m);
        self.drain(now, &p.volume);
    }

    /// Releases a plan without ending its local read, to plan it again.
    pub fn finish_plan(&self, plan: flaminio_store::ReadPlan) {
        self.service.finish_read(plan);
    }

    /// The device the store reads from, for the frontend's own reads.
    pub fn device(&self) -> &D {
        self.service.store().device()
    }
}

/// Whether a frame for the local replica writes to its store.
fn is_write(f: &Frame) -> bool {
    matches!(f, Frame::Write { .. } | Frame::Copy { .. })
}

fn write_data_len(f: &Frame) -> u64 {
    match f {
        Frame::Write { data, .. } | Frame::Copy { data, .. } => data.len() as u64,
        _ => 0,
    }
}

fn to_frame(m: &ToReplica<Payload>) -> Frame {
    let req = m.req;
    match &m.body {
        Request::Hello { .. } => unreachable!("Hello frames carry the session's token"),
        Request::Heartbeat => Frame::Heartbeat { req },
        Request::Write {
            chunk,
            ext,
            data,
            version,
            prune,
        } => Frame::Write {
            req,
            chunk: *chunk,
            ext: *ext,
            version: *version,
            prune: *prune,
            data: data.shared(),
        },
        Request::Read { chunk, ext } => Frame::Read {
            req,
            chunk: *chunk,
            ext: *ext,
        },
        Request::Digest { chunk, ext } => Frame::Digest {
            req,
            chunk: *chunk,
            ext: *ext,
        },
        Request::Mapped { chunk, ext } => Frame::Mapped {
            req,
            chunk: *chunk,
            ext: *ext,
        },
        Request::Discard { chunk, ext } => Frame::Discard {
            req,
            chunk: *chunk,
            ext: *ext,
        },
        Request::BeginResync { chunk } => Frame::BeginResync { req, chunk: *chunk },
        Request::Settle { chunk, below } => Frame::Settle {
            req,
            chunk: *chunk,
            below: *below,
        },
        Request::Copy { chunk, ext, data } => Frame::Copy {
            req,
            chunk: *chunk,
            ext: *ext,
            data: data.shared(),
        },
    }
}

fn from_frame(f: Frame) -> Option<(u64, Response<Payload>)> {
    Some(match f {
        Frame::Fenced { req, uncertain } => (req, Response::Fenced { uncertain }),
        Frame::Ok { req } => (req, Response::Ok),
        Frame::Data { req, data } => (req, Response::Data(Payload::Bytes(Bytes::from(data)))),
        Frame::Digests { req, sums } => (req, Response::Digests(sums)),
        Frame::Mappings { req, exts } => (req, Response::Mapped(exts)),
        Frame::Stale { req, fenced } => (req, Response::Stale { fenced }),
        _ => return None,
    })
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    use flaminio_format::Uuid;
    use flaminio_format::layout::LayoutParams;
    use flaminio_format::mkfs::FormatOptions;
    use flaminio_store::{MemDevice, Store, StoreConfig};

    const ISR: u64 = 100;
    const REFRESH: u64 = 101;

    struct TestClock(
        std::sync::atomic::AtomicU64,
        std::sync::atomic::AtomicUsize,
        std::sync::atomic::AtomicUsize,
    );
    impl TestClock {
        fn fail_after(&self, successful_reads: usize) {
            self.2.store(
                self.1.load(Ordering::SeqCst) + successful_reads + 1,
                Ordering::SeqCst,
            );
        }
    }
    impl crate::clock::ElapsedClock for TestClock {
        fn read(&self) -> Result<crate::clock::ClockReading, crate::clock::ClockFault> {
            let read = self.1.fetch_add(1, Ordering::SeqCst) + 1;
            if self.2.load(Ordering::SeqCst) == read {
                return Err(crate::clock::ClockFault::SampleFailed);
            }
            let ns = self.0.load(Ordering::Relaxed) as u128 * 1000;
            Ok(crate::clock::ClockReading {
                monotonic_before_ns: ns,
                boottime_ns: ns,
                monotonic_after_ns: ns,
            })
        }
    }
    fn proof() -> VerifiedHandoff {
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let key = Ed25519KeyPair::from_seed_unchecked(&[83; 32]).unwrap();
        let mut token = b"FLCT\x02\x00\x01v".to_vec();
        token.extend_from_slice(&7u64.to_be_bytes());
        token.extend_from_slice(b"\x00\x04fl-a");
        token.extend_from_slice(&0i64.to_be_bytes());
        token.extend_from_slice(&1u64.to_be_bytes());
        token.extend_from_slice(key.sign(&token).as_ref());
        let mut handoff = b"FLCH\x01\x01".to_vec();
        handoff.extend_from_slice(&[83; 32]);
        handoff.extend_from_slice(ring::digest::digest(&ring::digest::SHA256, &token).as_ref());
        handoff.extend_from_slice(key.sign(&handoff).as_ref());
        flaminio_net::token::TokenKey::from_raw(key.public_key().as_ref())
            .unwrap()
            .verify_handoff(&token, &handoff, &[83; 32], Purpose::Normal)
            .unwrap()
    }

    struct NoMeta;
    impl MetaPort for NoMeta {
        fn call(&self, _: u64, _: Call) {}
    }

    struct Fixture {
        engine: Engine<MemDevice>,
        volume: pb::Volume,
        isr_req: u64,
        refresh_req: u64,
        clock: Arc<TestClock>,
    }

    impl Fixture {
        fn new(generation: u64) -> Self {
            let timing = Timing {
                lease: 10,
                drift_ppm: 0,
                heartbeat: 1_000_000,
                request_timeout: 1_000_000,
                reconnect: 1_000_000,
                meta_timeout: 1_000,
                refresh: 30,
            };
            let view = VolumeMeta {
                epoch: 7,
                target: Some(0),
                min_isr: 1,
                blocks_per_chunk: 256,
                chunks: (0..2)
                    .map(|_| ChunkMeta {
                        replicas: vec![0, 1],
                        isr: [0, 1].into(),
                        isr_ver: 0,
                        moving: None,
                    })
                    .collect(),
            };
            let mut sm = Target::new(0, 0, &view, timing, 1, 0);
            let hello = sm
                .take_output()
                .into_iter()
                .find_map(|o| match o {
                    TOut::Send { to: 0, msg } if matches!(msg.body, Request::Hello { .. }) => {
                        Some(msg)
                    }
                    _ => None,
                })
                .unwrap();
            sm.on_reply(
                1,
                0,
                &FromReplica {
                    vol: 0,
                    epoch: 7,
                    session: hello.session,
                    req: hello.req,
                    body: Response::Fenced {
                        uncertain: vec![(0, vec![]), (1, vec![])],
                    },
                },
            );
            sm.tick(1 + timing.takeover_wait().unwrap());
            let isr_req = sm
                .take_output()
                .into_iter()
                .find_map(|o| match o {
                    TOut::Meta {
                        req,
                        body: MetaRequest::ChangeIsr { changes, .. },
                    } => {
                        assert_eq!(changes.len(), 2);
                        Some(req)
                    }
                    _ => None,
                })
                .expect("the missing peer requires a real shrink proposal");
            sm.tick(30);
            let refresh_req = sm
                .take_output()
                .into_iter()
                .find_map(|o| match o {
                    TOut::Meta {
                        req,
                        body: MetaRequest::Get { .. },
                    } => Some(req),
                    _ => None,
                })
                .unwrap();
            let device = MemDevice::new(256 << 20);
            Store::format(
                &device,
                &FormatOptions {
                    disk_uuid: Uuid([81; 16]),
                    cluster_uuid: Uuid::NIL,
                    format_nonce: 1,
                    created_unix_secs: 0,
                    layout: LayoutParams {
                        journal_size: Some(16 << 20),
                        ..Default::default()
                    },
                    label: String::new(),
                },
            )
            .unwrap();
            let clock = Arc::new(TestClock(
                std::sync::atomic::AtomicU64::new(0),
                std::sync::atomic::AtomicUsize::new(0),
                std::sync::atomic::AtomicUsize::new(0),
            ));
            let domain = Arc::new(
                ClockDomain::new(
                    clock.clone(),
                    crate::clock::ClockQualification::new(1, 1, 1000).unwrap(),
                )
                .unwrap(),
            );
            let net = crate::net::spawn(
                crate::net::NetConfig {
                    clock: domain.clone(),
                    trust_domain: "test".into(),
                    server: None,
                    client: None,
                },
                Arc::new(|_| {}),
            )
            .unwrap();
            let mut engine = Engine::new(
                EngineConfig {
                    node: "fl-a".into(),
                    address: "127.0.0.1:1".into(),
                    timing,
                    max_io: 1,
                    heartbeat: None,
                },
                ReplicaService::new(Store::open(device, StoreConfig::default()).unwrap()),
                net,
                Box::new(NoMeta),
                domain,
                [83; 32],
            );
            engine.targets.insert(
                "v".into(),
                TargetRt {
                    sm,
                    view,
                    generation,
                    token: vec![],
                    first_epoch: 7,
                    size: 2 << 20,
                    chunk_size: 1 << 20,
                    replica_count: 2,
                    nodes: vec!["fl-a".into(), "fl-b".into()],
                    addrs: vec![Some("127.0.0.1:1".parse().unwrap()); 2],
                    local: 0,
                    sessions: BTreeMap::new(),
                    ops: BTreeMap::new(),
                    subs: BTreeMap::new(),
                    reads: BTreeMap::new(),
                    deposed_reported: false,
                },
            );
            engine.meta_waits.insert(
                ISR,
                MetaWait::Isr {
                    volume: "v".into(),
                    generation,
                    sm_req: isr_req,
                    chunks: vec![0, 1],
                },
            );
            engine.meta_waits.insert(
                REFRESH,
                MetaWait::Refresh {
                    volume: "v".into(),
                    generation,
                    sm_req: refresh_req,
                },
            );
            let volume = pb::Volume {
                id: "v".into(),
                size_bytes: 2 << 20,
                chunk_bytes: 1 << 20,
                replicas: 2,
                min_isr: 1,
                epoch: 7,
                target: "fl-a".into(),
                chunks: (0..2)
                    .map(|_| pb::ChunkMeta {
                        replicas: vec!["fl-a".into(), "fl-b".into()],
                        isr: vec!["fl-a".into(), "fl-b".into()],
                        ..Default::default()
                    })
                    .collect(),
            };
            Self {
                engine,
                volume,
                isr_req,
                refresh_req,
                clock,
            }
        }

        fn answer(&mut self, id: u64, answer: Answer) {
            self.clock.0.store(31, Ordering::Relaxed);
            self.engine.on_meta(Instant::now(), id, answer);
        }

        fn isr(&mut self, chunks: Vec<pb::ChunkMeta>, epoch: u64) {
            self.answer(
                ISR,
                Answer::ChangeIsr(pb::ChangeIsrResponse {
                    applied: true,
                    epoch,
                    chunks,
                }),
            );
        }

        fn refresh(&mut self, volume: pb::Volume) {
            self.answer(
                REFRESH,
                Answer::GetVolume(pb::GetVolumeResponse {
                    volume: Some(volume),
                }),
            );
        }

        fn shrunk(&self, version: u64) -> Vec<pb::ChunkMeta> {
            self.volume
                .chunks
                .iter()
                .cloned()
                .map(|mut c| {
                    c.isr = vec!["fl-a".into()];
                    c.isr_ver = version;
                    c
                })
                .collect()
        }
    }

    #[test]
    fn stale_isr_reply_does_not_regress_cache() {
        let mut f = Fixture::new(7);
        let mut fresh = f.volume.clone();
        fresh.chunks = f.shrunk(5);
        f.refresh(fresh);
        f.isr(f.shrunk(1), 7);
        let rt = &f.engine.targets["v"];
        assert!(
            rt.view
                .chunks
                .iter()
                .all(|c| c.isr_ver == 5 && c.isr == [0].into())
        );
        assert!(!rt.sm.is_deposed());
        assert!(!rt.sm.accepts_meta_reply(f.isr_req));
    }

    #[test]
    fn stale_refresh_reply_does_not_regress_cache() {
        let mut f = Fixture::new(7);
        f.isr(f.shrunk(5), 7);
        f.refresh(f.volume.clone());
        let rt = &f.engine.targets["v"];
        assert!(
            rt.view
                .chunks
                .iter()
                .all(|c| c.isr_ver == 5 && c.isr == [0].into())
        );
        assert!(!rt.sm.accepts_meta_reply(f.refresh_req));
    }

    #[test]
    fn expired_isr_reply_cannot_answer_or_depose_the_next_batch() {
        let mut f = Fixture::new(7);
        let rt = f.engine.targets.get_mut("v").unwrap();
        rt.sm.tick(1_100);
        let next = rt
            .sm
            .take_output()
            .into_iter()
            .find_map(|o| match o {
                TOut::Meta {
                    req,
                    body: MetaRequest::ChangeIsr { .. },
                } => Some(req),
                _ => None,
            })
            .unwrap();
        assert_ne!(next, f.isr_req);
        assert!(rt.sm.accepts_meta_reply(next));
        let before = rt.view.clone();
        let mut delayed = f.shrunk(999);
        delayed[0].replicas = vec!["fl-a".into(), "new-node".into()];
        f.isr(delayed, 7);
        let rt = &f.engine.targets["v"];
        assert_eq!(rt.view, before);
        assert_eq!(rt.nodes, ["fl-a", "fl-b"]);
        assert!(rt.sm.accepts_meta_reply(next));
        assert!(!rt.sm.is_deposed());
    }

    #[test]
    fn expired_refresh_reply_cannot_pollute_the_next_request() {
        let mut f = Fixture::new(7);
        let rt = f.engine.targets.get_mut("v").unwrap();
        rt.sm.tick(1_100);
        let next = rt
            .sm
            .take_output()
            .into_iter()
            .find_map(|o| match o {
                TOut::Meta {
                    req,
                    body: MetaRequest::Get { .. },
                } => Some(req),
                _ => None,
            })
            .unwrap();
        assert_ne!(next, f.refresh_req);
        let before = rt.view.clone();
        let mut delayed = f.volume.clone();
        delayed.chunks = f.shrunk(999);
        delayed.chunks[0].replicas = vec!["fl-a".into(), "new-node".into()];
        f.refresh(delayed);
        let rt = &f.engine.targets["v"];
        assert_eq!(rt.view, before);
        assert_eq!(rt.nodes, ["fl-a", "fl-b"]);
        assert!(rt.sm.accepts_meta_reply(next));
        assert!(!rt.sm.is_deposed());
    }

    #[test]
    fn reattach_ignores_previous_generation() {
        for id in [ISR, REFRESH] {
            let mut f = Fixture::new(7);
            let mut replacement = Fixture::new(8);
            assert_eq!(f.isr_req, replacement.isr_req);
            assert_eq!(f.refresh_req, replacement.refresh_req);
            f.engine
                .targets
                .insert("v".into(), replacement.engine.targets.remove("v").unwrap());
            let answer = if id == ISR {
                Answer::ChangeIsr(pb::ChangeIsrResponse {
                    applied: true,
                    epoch: 999,
                    chunks: vec![],
                })
            } else {
                let mut v = f.volume.clone();
                v.epoch = 999;
                v.chunks.clear();
                Answer::GetVolume(pb::GetVolumeResponse { volume: Some(v) })
            };
            f.answer(id, answer);
            let rt = &f.engine.targets["v"];
            assert_eq!(rt.generation, 8);
            assert_eq!(rt.view.epoch, 7);
            assert_eq!(rt.nodes, ["fl-a", "fl-b"]);
            assert!(rt.sm.accepts_meta_reply(f.isr_req));
            assert!(rt.sm.accepts_meta_reply(f.refresh_req));
        }
    }

    #[test]
    fn isr_reply_requires_complete_count() {
        for count in [0, 1, 3] {
            let mut f = Fixture::new(7);
            let before = f.engine.targets["v"].view.clone();
            let mut c = f.shrunk(4).remove(0);
            c.replicas = vec!["fl-a".into(), "fl-c".into()];
            f.isr(vec![c; count], 7);
            let rt = &f.engine.targets["v"];
            assert_eq!(rt.view, before);
            assert_eq!(rt.nodes, ["fl-a", "fl-b"]);
            assert!(rt.sm.accepts_meta_reply(f.isr_req));
        }
    }

    #[test]
    fn higher_epoch_malformed_metadata_deposes_before_conversion() {
        for id in [ISR, REFRESH] {
            for malformed_move in [false, true] {
                let mut f = Fixture::new(7);
                let mut v = f.volume.clone();
                v.epoch = 8;
                if malformed_move {
                    v.chunks[0].moving_from = "fl-a".into();
                    v.chunks[0].moving_to = "fl-b".into();
                } else {
                    v.size_bytes = 0;
                    v.chunks.clear();
                    v.target = "bad\nnode".into();
                }
                if id == ISR {
                    f.isr(v.chunks, v.epoch);
                } else {
                    f.refresh(v);
                }
                assert!(!f.engine.targets.contains_key("v"));
                assert!(
                    f.engine
                        .take_output()
                        .iter()
                        .any(|o| matches!(o, Output::Deposed { volume } if volume == "v"))
                );
            }
        }
    }

    #[test]
    fn invalid_membership_is_not_partially_converted_or_adopted() {
        for id in [ISR, REFRESH] {
            for epoch in [6, 7] {
                let mut f = Fixture::new(7);
                let before = f.engine.targets["v"].view.clone();
                let mut chunks = f.shrunk(4);
                chunks[0].replicas = vec!["fl-a".into(), "new-valid-node".into()];
                chunks[1].isr = vec!["fl-a".into(), "fl-a".into()];
                if id == ISR {
                    f.isr(chunks, epoch);
                } else {
                    let mut v = f.volume.clone();
                    v.chunks = chunks;
                    v.epoch = epoch;
                    f.refresh(v);
                }
                let rt = &f.engine.targets["v"];
                assert_eq!(rt.view, before);
                assert_eq!(rt.nodes, ["fl-a", "fl-b"]);
                assert!(rt.sm.accepts_meta_reply(if id == ISR {
                    f.isr_req
                } else {
                    f.refresh_req
                }));
            }
        }
    }

    #[test]
    fn malformed_refresh_geometry_preserves_cache_and_live_request() {
        for (case, epoch) in (0..19).flat_map(|case| [6, 7].map(|epoch| (case, epoch))) {
            let mut f = Fixture::new(7);
            let before = f.engine.targets["v"].view.clone();
            let mut v = f.volume.clone();
            v.epoch = epoch;
            match case {
                0 => v.id = "other".into(),
                1 => v.size_bytes += BLOCK as u64,
                2 => v.chunk_bytes *= 2,
                3 => v.replicas = 1,
                4 => v.min_isr = 2,
                5 => {
                    v.chunks.pop();
                }
                6 => v.id = "bad/id".into(),
                7 => v.size_bytes = 0,
                8 => v.size_bytes = u64::MAX,
                9 => v.chunk_bytes = 0,
                10 => v.chunk_bytes = 4096,
                11 => v.chunk_bytes = 2 << 30,
                12 => v.chunk_bytes = (1 << 20) + 4096,
                13 => v.size_bytes = 65_537 << 20,
                14 => v.size_bytes = 3882 << 20,
                15 => v.replicas = u32::MAX,
                16 => v.min_isr = 0,
                17 => v.chunks.push(v.chunks[0].clone()),
                _ => v.target = "bad\nnode".into(),
            }
            f.refresh(v);
            let rt = &f.engine.targets["v"];
            assert_eq!(rt.view, before, "case {case}");
            assert_eq!(rt.nodes, ["fl-a", "fl-b"]);
            assert!(rt.sm.accepts_meta_reply(f.refresh_req));
        }
        let mut f = Fixture::new(7);
        let mut v = f.volume.clone();
        v.target = "fl-b".into();
        f.refresh(v);
        assert!(f.engine.targets["v"].sm.accepts_meta_reply(f.refresh_req));
    }

    /// The first row introduces a legal new peer; a bad later row must not leave that
    /// peer in the runtime map before the complete payload has been validated.
    fn malformed_memberships(base: &[pb::ChunkMeta]) -> Vec<(&'static str, Vec<pb::ChunkMeta>)> {
        let mut cases = Vec::new();
        for case in 0..16 {
            let mut chunks = base.to_vec();
            chunks[0].replicas = vec!["fl-a".into(), "new-peer".into()];
            chunks[0].isr = vec!["fl-a".into()];
            chunks[0].isr_ver = 100;
            let c = &mut chunks[1];
            let name = match case {
                0 => {
                    c.replicas.clear();
                    "empty replicas"
                }
                1 => {
                    c.replicas.pop();
                    "short replicas"
                }
                2 => {
                    c.replicas.push("extra".into());
                    "extra replicas without move"
                }
                3 => {
                    c.replicas[1] = c.replicas[0].clone();
                    "duplicate replicas"
                }
                4 => {
                    c.replicas[1] = "bad\nnode".into();
                    "invalid replica id"
                }
                5 => {
                    c.isr.clear();
                    "empty ISR"
                }
                6 => {
                    c.isr.push("fl-a".into());
                    "duplicate ISR"
                }
                7 => {
                    c.isr = vec!["outsider".into()];
                    "ISR outside replicas"
                }
                8 => {
                    c.moving_from = "fl-b".into();
                    "missing move destination"
                }
                9 => {
                    c.moving_to = "fl-c".into();
                    "missing move source"
                }
                10 => {
                    c.moving_from = "fl-b".into();
                    c.moving_to = "fl-b".into();
                    "self move"
                }
                11 => {
                    c.moving_from = "outsider".into();
                    c.moving_to = "fl-c".into();
                    "unplaced move source"
                }
                12 => {
                    c.moving_from = "fl-b".into();
                    c.moving_to = "bad/node".into();
                    "invalid move id"
                }
                13 => {
                    c.replicas.push("fl-c".into());
                    c.moving_from = "fl-b".into();
                    c.moving_to = "fl-d".into();
                    "grown move omits destination"
                }
                14 => {
                    c.moving_from = "fl-a".into();
                    c.moving_to = "fl-b".into();
                    "planned move destination already placed"
                }
                _ => {
                    c.replicas[1] = "x".repeat(129);
                    "overlong node id"
                }
            };
            cases.push((name, chunks));
        }
        cases
    }

    #[test]
    fn every_metadata_membership_refusal_precedes_peer_cache_and_core_mutation() {
        let base = Fixture::new(7).volume;
        for (case, chunks) in malformed_memberships(&base.chunks) {
            let mut initial = base.clone();
            initial.chunks = chunks.clone();
            assert!(validate_volume("v", &initial).is_err(), "initial {case}");
            let mut f = Fixture::new(7);
            f.engine.targets.clear();
            let before = f.engine.service.store().device().mutations();
            let error = f
                .engine
                .start_target(
                    Instant::now(),
                    "v",
                    Attaching {
                        attempt: 1,
                        proof: proof(),
                        volume: Some(initial.clone()),
                        nodes: Some(vec![]),
                    },
                )
                .unwrap_err();
            assert!(error.contains("membership"), "initial {case}: {error}");
            assert!(f.engine.targets.is_empty());
            assert_eq!(f.engine.service.store().device().mutations(), before);
            for id in [ISR, REFRESH] {
                for epoch in [6, 7] {
                    let mut f = Fixture::new(7);
                    let rt = &f.engine.targets["v"];
                    let before = (rt.view.clone(), rt.nodes.clone(), rt.addrs.clone());
                    if id == ISR {
                        f.isr(chunks.clone(), epoch);
                    } else {
                        let mut v = initial.clone();
                        v.epoch = epoch;
                        f.refresh(v);
                    }
                    let rt = &f.engine.targets["v"];
                    assert_eq!(
                        (rt.view.clone(), rt.nodes.clone(), rt.addrs.clone()),
                        before,
                        "{case}, reply {id}, epoch {epoch}"
                    );
                    assert!(rt.sm.accepts_meta_reply(if id == ISR {
                        f.isr_req
                    } else {
                        f.refresh_req
                    }));
                    assert!(!rt.sm.is_deposed());
                    assert!(f.engine.take_output().is_empty(), "{case}");
                }
            }
        }
    }

    #[test]
    fn initial_geometry_and_identity_refuse_before_target_or_replica_creation() {
        for case in 0..15 {
            let mut f = Fixture::new(7);
            f.engine.targets.clear();
            let mut v = f.volume.clone();
            match case {
                0 => v.size_bytes = 0,
                1 => v.chunk_bytes = 0,
                2 => v.chunk_bytes = 4096,
                3 => v.chunk_bytes = 2 << 30,
                4 => v.size_bytes = u64::MAX,
                5 => v.size_bytes += 4096,
                6 => v.size_bytes = 65_537 << 20,
                7 => v.size_bytes = 3_882 << 20, // RF2's first over-budget count.
                8 => v.replicas = u32::MAX,
                9 => v.min_isr = 0,
                10 => v.min_isr = 3,
                11 => v.chunks.clear(),
                12 => v.chunks.push(v.chunks[0].clone()),
                13 => v.id = "different-volume".into(),
                _ => v.target = "bad/node".into(),
            }
            let before = f.engine.service.store().chunks();
            let result = f.engine.start_target(
                Instant::now(),
                "v",
                Attaching {
                    attempt: 1,
                    proof: proof(),
                    volume: Some(v),
                    nodes: Some(vec![]),
                },
            );
            let error = result.unwrap_err();
            assert!(
                !error.contains("token"),
                "case {case}: geometry must fail first: {error}"
            );
            assert!(f.engine.targets.is_empty());
            assert_eq!(f.engine.service.store().chunks(), before);
        }
    }

    #[test]
    fn generic_refresh_rejects_even_self_consistent_immutable_geometry_changes() {
        for case in 0..4 {
            let mut f = Fixture::new(7);
            let before = f.engine.targets["v"].view.clone();
            let mut v = f.volume.clone();
            match case {
                0 => {
                    v.size_bytes *= 2;
                    v.chunks.extend(v.chunks.clone());
                }
                1 => {
                    v.chunk_bytes *= 2;
                    v.chunks.pop();
                }
                2 => {
                    v.replicas = 3;
                    for c in &mut v.chunks {
                        c.replicas.push("fl-c".into());
                    }
                }
                _ => v.min_isr = 2,
            }
            assert!(
                validate_volume("v", &v).is_ok(),
                "valid geometry case {case}"
            );
            f.refresh(v);
            let rt = &f.engine.targets["v"];
            assert_eq!(rt.view, before);
            assert_eq!(rt.nodes, ["fl-a", "fl-b"]);
            assert!(rt.sm.accepts_meta_reply(f.refresh_req));
        }
    }

    #[test]
    fn isr_saved_indices_are_unique_bounded_and_reply_rows_remain_positional() {
        for indices in [
            vec![],
            vec![0, 0],
            vec![0, 2],
            vec![u32::MAX, 0],
            vec![0; MAX_CHANGES_PER_REQUEST + 1],
        ] {
            let mut f = Fixture::new(7);
            let before = f.engine.targets["v"].view.clone();
            let MetaWait::Isr { chunks, .. } = f.engine.meta_waits.get_mut(&ISR).unwrap() else {
                unreachable!()
            };
            *chunks = indices.clone();
            f.isr(vec![f.shrunk(4)[0].clone(); indices.len()], 7);
            let rt = &f.engine.targets["v"];
            assert_eq!(rt.view, before, "indices {indices:?}");
            assert!(rt.sm.accepts_meta_reply(f.isr_req));
        }
        let mut f = Fixture::new(7);
        let MetaWait::Isr { chunks, .. } = f.engine.meta_waits.get_mut(&ISR).unwrap() else {
            unreachable!()
        };
        *chunks = vec![1, 0];
        let mut rows = f.shrunk(4);
        rows[1].isr_ver = 5;
        f.isr(rows, 7);
        let rt = &f.engine.targets["v"];
        assert_eq!(rt.view.chunks[1].isr_ver, 4);
        assert_eq!(rt.view.chunks[0].isr_ver, 5);
        assert!(!rt.sm.accepts_meta_reply(f.isr_req));
    }

    #[test]
    fn legal_move_stages_are_adopted_from_refresh_and_isr_replies() {
        for id in [ISR, REFRESH] {
            for stage in 0..3 {
                let mut f = Fixture::new(7);
                let mut v = f.volume.clone();
                for c in &mut v.chunks {
                    c.isr_ver = 100;
                    c.moving_from = "fl-b".into();
                    c.moving_to = "fl-c".into();
                    if stage >= 1 {
                        c.replicas.push(c.moving_to.clone());
                    }
                    if stage == 2 {
                        c.isr.push(c.moving_to.clone());
                    }
                }
                assert!(validate_volume("v", &v).is_ok());
                if id == ISR {
                    f.isr(v.chunks, v.epoch);
                } else {
                    f.refresh(v);
                }
                let rt = &f.engine.targets["v"];
                assert_eq!(rt.nodes, ["fl-a", "fl-b", "fl-c"]);
                for c in &rt.view.chunks {
                    assert_eq!(c.isr_ver, 100);
                    assert_eq!(c.moving, Some((1, 2)));
                    assert_eq!(
                        c.replicas,
                        if stage == 0 {
                            vec![0, 1]
                        } else {
                            vec![0, 1, 2]
                        }
                    );
                    assert_eq!(
                        c.isr,
                        if stage == 2 {
                            [0, 1, 2].into()
                        } else {
                            [0, 1].into()
                        }
                    );
                }
                assert!(!rt.sm.accepts_meta_reply(if id == ISR {
                    f.isr_req
                } else {
                    f.refresh_req
                }));
                assert!(!rt.sm.is_deposed());
            }
        }
    }

    #[test]
    fn valid_concentrated_and_moving_views_at_every_rf_cap() {
        for (rf, count) in (1..=5).zip([5137, 3881, 3119, 2607, 2239]) {
            let mut v = Fixture::new(7).volume;
            v.size_bytes = count * (1 << 30);
            v.chunk_bytes = 1 << 30;
            v.replicas = rf;
            v.min_isr = rf;
            let members: Vec<_> = (0..rf).map(|i| format!("n{i}")).collect();
            let mut c = pb::ChunkMeta {
                replicas: members.clone(),
                isr: members,
                isr_ver: u64::MAX,
                ..Default::default()
            };
            v.chunks = vec![c.clone(); count as usize];
            assert_eq!(validate_volume("v", &v).unwrap().chunks, count as u32);
            // Requested moves may have only R members; in-progress moves have R+1.
            c.moving_from = "n0".into();
            c.moving_to = format!("n{rf}");
            v.chunks.fill(c.clone());
            assert!(validate_volume("v", &v).is_ok());
            c.replicas.push(c.moving_to.clone());
            v.chunks.fill(c.clone());
            assert!(validate_volume("v", &v).is_ok());
            c.isr = c.replicas.clone();
            v.chunks.fill(c.clone());
            assert!(validate_volume("v", &v).is_ok());
            v.chunks.push(c);
            v.size_bytes += 1 << 30;
            assert!(validate_volume("v", &v).unwrap_err().contains("budget"));
        }
    }

    #[test]
    fn equal_version_refresh_changes_only_the_requested_move() {
        let mut f = Fixture::new(7);
        let mut v = f.volume.clone();
        v.chunks[0].isr = vec!["fl-b".into()];
        v.chunks[0].moving_from = "fl-b".into();
        v.chunks[0].moving_to = "fl-c".into();
        f.refresh(v);
        let rt = &f.engine.targets["v"];
        assert_eq!(rt.view.chunks[0].isr, [0, 1].into());
        assert_eq!(rt.view.chunks[0].replicas, vec![0, 1]);
        assert_eq!(rt.view.chunks[0].moving, Some((1, 2)));
    }

    #[test]
    fn lower_epoch_metadata_cannot_install_newer_chunk_versions() {
        for id in [ISR, REFRESH] {
            let mut f = Fixture::new(7);
            let before = f.engine.targets["v"].view.clone();
            let chunks = f.shrunk(999);
            if id == ISR {
                f.isr(chunks, 6);
            } else {
                let mut v = f.volume.clone();
                v.epoch = 6;
                v.chunks = chunks;
                f.refresh(v);
            }
            assert_eq!(f.engine.targets["v"].view, before);
        }
    }

    #[test]
    fn hello_adapters_preserve_the_captured_chunk_set_instead_of_cached_placement() {
        for peer in [0, 1] {
            let mut f = Fixture::new(7);
            let (net, commands) = crate::net::Net::test_channel(f.engine.clock.clone());
            f.engine.net = net;
            let rt = f.engine.targets.get_mut("v").unwrap();
            let mut original = rt.view.clone();
            original.chunks[0].replicas = vec![1 - peer];
            original.chunks[0].isr = [1 - peer].into();
            let mut sm = Target::new(0, 0, &original, f.engine.cfg.timing, 1, 0);
            let hello = sm
                .take_output()
                .into_iter()
                .find_map(|out| match out {
                    TOut::Send { to, msg }
                        if to == peer && matches!(msg.body, Request::Hello { .. }) =>
                    {
                        Some(msg)
                    }
                    _ => None,
                })
                .unwrap();
            let Request::Hello { chunks } = &hello.body else {
                unreachable!()
            };
            assert_eq!(chunks, &[1]);
            let captured = chunks.clone();
            let ids = (hello.req, hello.epoch, hello.session);
            rt.sm = sm;
            // Its cached view now places this peer on both chunks, after the Hello was made.
            assert!(rt.view.chunks.iter().all(|c| c.replicas.contains(&peer)));
            f.engine.send_from_target(Instant::now(), "v", peer, hello);
            if peer == 0 {
                let inventory: Vec<_> = f
                    .engine
                    .service
                    .store()
                    .chunks()
                    .into_iter()
                    .map(|info| info.key.index)
                    .collect();
                assert_eq!(
                    inventory,
                    vec![1],
                    "local Accepted must preserve the captured set"
                );
                assert!(matches!(
                    f.engine.targets["v"].sessions.get(&0),
                    Some(Sess::Local { sref: Some(_), .. })
                ));
                assert!(
                    commands.try_recv().is_err(),
                    "local Hello does not use the network"
                );
            } else {
                let NetCommand::Connect { node, hello, .. } =
                    commands.try_recv().expect("actual outgoing Connect")
                else {
                    panic!("not a Connect command")
                };
                assert_eq!(node, "fl-b");
                assert!(
                    matches!(*hello, Frame::Hello { req, epoch, session, size, chunk_size, ref chunks, .. }
                    if (req, epoch, session) == ids && size == 2 << 20 && chunk_size == 1 << 20 && *chunks == captured)
                );
                assert!(commands.try_recv().is_err());
            }
        }
    }
    #[test]
    fn first_clock_fault_while_settling_a_queued_request_does_not_panic() {
        let mut f = Fixture::new(1);
        f.engine.out.clear();
        let accepted = Accepted {
            peer_node: "fl-a".into(),
            req: 1,
            epoch: 8,
            session: 1,
            claims: Claims {
                volume: "v".into(),
                epoch: 8,
                node: "fl-a".into(),
                issued_at: 0,
                first_epoch: 1,
            },
            size: 2 << 20,
            chunk_size: 1 << 20,
            chunks: vec![0],
        };
        let (tx, _rx) = channel::unbounded();
        f.engine.handle(Input::Net(NetEvent::Opened {
            conn: 90,
            accepted: Box::new(accepted.clone()),
            replies: tx,
        }));
        f.engine.handle(Input::Net(NetEvent::Request {
            conn: 90,
            frame: Frame::Read {
                req: 2,
                chunk: 0,
                ext: Extent::new(0, 1),
            },
        }));
        assert_eq!(f.engine.local.len(), 1);
        assert!(f.engine.committing.is_empty() && f.engine.staged.is_none());
        f.clock.fail_after(1); // outer handle admits; flush_local first observes the fault.
        let (tx, _rx) = channel::unbounded();
        f.engine.handle(Input::Net(NetEvent::Opened {
            conn: 91,
            accepted: Box::new(accepted),
            replies: tx,
        }));
        assert!(!f.engine.clock.status().is_healthy());
        assert!(f.engine.local.is_empty() && f.engine.conns.is_empty());
        assert_eq!(f.engine.clock.status().generation.number(), 1);
    }

    #[test]
    fn clock_fault_keeps_only_consistently_prepermitted_piece_done_pairs() {
        let mut f = Fixture::new(1);
        f.engine.out.clear();
        let domain = f.engine.clock_domain().clone();
        let sample = domain.observer().sample().unwrap();
        let generation = sample.generation();
        let permit = domain.success_permit(sample).unwrap();
        f.engine.out.push(Output::Piece {
            generation,
            op: 9001,
            at: 0,
            data: vec![1; 4096].into(),
        });
        f.engine.out.push(Output::Done {
            op: 9001,
            result: Ok(None),
            permit: Some(permit),
        });
        f.engine.out.push(Output::Piece {
            generation,
            op: 9002,
            at: 0,
            data: vec![2; 4096].into(),
        });
        domain.quarantine(crate::clock::ClockFault::SampleFailed);
        let outputs = f.engine.take_output();
        assert_eq!(outputs.len(), 2);
        assert!(matches!(&outputs[0], Output::Piece { op: 9001, .. }));
        assert!(matches!(
            &outputs[1],
            Output::Done {
                op: 9001,
                result: Ok(_),
                permit: Some(_)
            }
        ));
        assert!(f.engine.targets.is_empty());
        assert!(f.engine.meta_waits.is_empty());
        f.engine.handle(Input::Meta {
            id: ISR,
            answer: Answer::ChangeIsr(pb::ChangeIsrResponse {
                applied: true,
                epoch: 7,
                chunks: f.shrunk(99),
            }),
        });
        assert!(f.engine.targets.is_empty());
        assert!(f.engine.take_output().is_empty());
        assert_eq!(domain.status().generation.number(), 1);
    }
}
