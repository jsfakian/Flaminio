//! Two nodes, each a full engine (store, TLS network thread, engine thread), and a model of
//! the metadata service: a replicated volume end to end, through a replica failure and its
//! resync.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use flaminio_format::Uuid;
use flaminio_format::layout::LayoutParams;
use flaminio_format::mkfs::FormatOptions;
use flaminio_metaclient::{Answer, Call, pb};
use flaminio_net::tls::Files;
use flaminio_net::token::TokenKey;
use flaminio_node::engine::{Engine, EngineConfig, Input, Io, MetaPort, Output};
use flaminio_node::net::{self, NetConfig, ServerSide};
use flaminio_node::replica::ReplicaService;
use flaminio_repl::meta::MAX_CHANGES_PER_REQUEST;
use flaminio_repl::types::Timing;
use flaminio_store::{MemDevice, Store, StoreConfig};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, SanType,
};
use ring::signature::{Ed25519KeyPair, KeyPair as _};

const NONCE: [u8; 32] = [83; 32];
struct TestClock(Instant);
impl flaminio_node::clock::ElapsedClock for TestClock {
    fn read(&self) -> Result<flaminio_node::clock::ClockReading, flaminio_node::clock::ClockFault> {
        let ns = self.0.elapsed().as_nanos();
        Ok(flaminio_node::clock::ClockReading {
            monotonic_before_ns: ns,
            boottime_ns: ns,
            monotonic_after_ns: ns,
        })
    }
}
fn clock() -> Arc<flaminio_node::clock::ClockDomain> {
    Arc::new(
        flaminio_node::clock::ClockDomain::new(
            Arc::new(TestClock(Instant::now())),
            flaminio_node::clock::ClockQualification::new(1, 1, 1000).unwrap(),
        )
        .unwrap(),
    )
}
fn proof(
    key: &Ed25519KeyPair,
    token: Vec<u8>,
) -> Result<flaminio_net::handoff::VerifiedHandoff, String> {
    let mut handoff = b"FLCH\x01\x01".to_vec();
    handoff.extend_from_slice(&NONCE);
    handoff.extend_from_slice(ring::digest::digest(&ring::digest::SHA256, &token).as_ref());
    handoff.extend_from_slice(key.sign(&handoff).as_ref());
    TokenKey::from_raw(key.public_key().as_ref())
        .unwrap()
        .verify_handoff(
            &token,
            &handoff,
            &NONCE,
            flaminio_net::handoff::Purpose::Normal,
        )
        .map_err(|_| "invalid capability token or target handoff".into())
}

const TD: &str = "lab";
const MIB: u64 = 1 << 20;
const CHUNK: u64 = 4 * MIB;
const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// The metadata service, as a model with the same rules for the calls the engine makes.
#[derive(Default)]
struct Model {
    nodes: BTreeMap<String, String>,
    volume: Option<pb::Volume>,
    /// Registrations to refuse, as a contended or unreachable service does.
    refuse_registrations: u32,
    batches: Vec<pb::ChangeIsrRequest>,
    hold_isr_replies: bool,
    held_isr: Vec<(mpsc::Sender<Input>, u64, Answer)>,
}

struct FakeMeta {
    model: Arc<Mutex<Model>>,
    inbox: mpsc::Sender<Input>,
}

impl MetaPort for FakeMeta {
    fn call(&self, id: u64, call: Call) {
        let mut m = self.model.lock().unwrap();
        let hold = m.hold_isr_replies && matches!(&call, Call::ChangeIsr(_));
        let answer = match call {
            Call::RegisterNode(_) if m.refuse_registrations > 0 => {
                m.refuse_registrations -= 1;
                Answer::Failed {
                    code: flaminio_metaclient::Code::Aborted,
                    message: "too much contention".into(),
                }
            }
            Call::RegisterNode(r) => {
                let n = r.node.unwrap();
                m.nodes.insert(n.id, n.address);
                Answer::RegisterNode(pb::RegisterNodeResponse {})
            }
            Call::NodeHeartbeat(h) if !m.nodes.contains_key(&h.node) => Answer::Failed {
                code: flaminio_metaclient::Code::NotFound,
                message: format!("node {:?} is not registered", h.node),
            },
            Call::NodeHeartbeat(_) => {
                Answer::NodeHeartbeat(pb::NodeHeartbeatResponse { ttl_seconds: 10 })
            }
            Call::ListNodes(_) => Answer::ListNodes(pb::ListNodesResponse {
                nodes: m
                    .nodes
                    .iter()
                    .map(|(id, a)| pb::Node {
                        id: id.clone(),
                        address: a.clone(),
                        ..Default::default()
                    })
                    .collect(),
            }),
            Call::GetVolume(_) => Answer::GetVolume(pb::GetVolumeResponse {
                volume: m.volume.clone(),
            }),
            Call::ChangeIsr(r) => {
                m.batches.push(r.clone());
                if r.changes.is_empty() || r.changes.len() > MAX_CHANGES_PER_REQUEST {
                    let _ = self.inbox.send(Input::Meta {
                        id,
                        answer: Answer::Failed {
                            code: flaminio_metaclient::Code::InvalidArgument,
                            message: "unsupported ISR batch size".into(),
                        },
                    });
                    return;
                }
                let v = m.volume.as_mut().unwrap();
                let distinct = r.changes.iter().map(|c| c.chunk).collect::<BTreeSet<_>>();
                if r.changes.iter().any(|c| c.chunk as usize >= v.chunks.len()) {
                    let _ = self.inbox.send(Input::Meta {
                        id,
                        answer: Answer::Failed {
                            code: flaminio_metaclient::Code::InvalidArgument,
                            message: "invalid chunk list".into(),
                        },
                    });
                    return;
                }
                let ok = r.epoch == v.epoch
                    && distinct.len() == r.changes.len()
                    && r.changes.iter().all(|c| {
                        let ch = &v.chunks[c.chunk as usize];
                        let replicas = if c.change_replicas {
                            &c.new_replicas
                        } else {
                            &ch.replicas
                        };
                        ch.isr_ver == c.expected_ver
                            && c.new_isr.len() as u32 >= v.min_isr
                            && c.new_isr.iter().all(|n| replicas.contains(n))
                            && c.new_isr.iter().collect::<BTreeSet<_>>().len() == c.new_isr.len()
                            && (!c.change_replicas || valid_move(ch, c))
                    });
                if ok {
                    for c in &r.changes {
                        let ch = &mut v.chunks[c.chunk as usize];
                        let mut isr = c.new_isr.clone();
                        isr.sort();
                        isr.dedup();
                        ch.isr = isr;
                        ch.isr_ver += 1;
                        if c.change_replicas {
                            ch.replicas = c.new_replicas.clone();
                            if !ch.replicas.contains(&ch.moving_from) {
                                ch.moving_from.clear();
                                ch.moving_to.clear();
                            }
                        }
                    }
                }
                Answer::ChangeIsr(pb::ChangeIsrResponse {
                    applied: ok,
                    epoch: v.epoch,
                    chunks: r
                        .changes
                        .iter()
                        .map(|c| v.chunks[c.chunk as usize].clone())
                        .collect(),
                })
            }
        };
        if hold {
            m.held_isr.push((self.inbox.clone(), id, answer));
            return;
        }
        let _ = self.inbox.send(Input::Meta { id, answer });
    }
}

fn valid_move(ch: &pb::ChunkMeta, c: &pb::IsrChange) -> bool {
    if ch.moving_from.is_empty()
        || ch.moving_to.is_empty()
        || ch.moving_from == ch.moving_to
        || !ch.replicas.contains(&ch.moving_from)
    {
        return false;
    }
    let old: BTreeSet<_> = ch.replicas.iter().cloned().collect();
    let new: BTreeSet<_> = c.new_replicas.iter().cloned().collect();
    if new.len() != c.new_replicas.len() {
        return false;
    }
    let mut add = old.clone();
    add.insert(ch.moving_to.clone());
    let mut remove = old;
    remove.remove(&ch.moving_from);
    (!ch.replicas.contains(&ch.moving_to) && new == add)
        || (ch.replicas.contains(&ch.moving_from)
            && c.new_isr.contains(&ch.moving_to)
            && new == remove)
}

fn identities(dir: &Path) -> BTreeMap<&'static str, Files> {
    let mut ca = CertificateParams::new(vec![]).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_key = KeyPair::generate().unwrap();
    std::fs::write(dir.join("ca.pem"), ca.self_signed(&ca_key).unwrap().pem()).unwrap();
    ["fl-a", "fl-b", "fl-c"]
        .into_iter()
        .map(|name| {
            let mut p = CertificateParams::new(vec![]).unwrap();
            p.subject_alt_names = vec![
                SanType::URI(
                    format!("spiffe://{TD}/flaminio/node/{name}/io")
                        .try_into()
                        .unwrap(),
                ),
                SanType::IpAddress(LOCALHOST),
            ];
            p.extended_key_usages = vec![
                ExtendedKeyUsagePurpose::ServerAuth,
                ExtendedKeyUsagePurpose::ClientAuth,
            ];
            let key = KeyPair::generate().unwrap();
            let cert = p
                .signed_by(&key, &Issuer::from_params(&ca, &ca_key))
                .unwrap();
            std::fs::write(dir.join(format!("{name}.crt")), cert.pem()).unwrap();
            std::fs::write(dir.join(format!("{name}.key")), key.serialize_pem()).unwrap();
            (
                name,
                Files {
                    cert: dir.join(format!("{name}.crt")),
                    key: dir.join(format!("{name}.key")),
                    ca: dir.join("ca.pem"),
                },
            )
        })
        .collect()
}

fn token(key: &Ed25519KeyPair, volume: &str, epoch: u64, node: &str) -> Vec<u8> {
    let mut b = b"FLCT\x02".to_vec();
    b.extend_from_slice(&(volume.len() as u16).to_be_bytes());
    b.extend_from_slice(volume.as_bytes());
    b.extend_from_slice(&epoch.to_be_bytes());
    b.extend_from_slice(&(node.len() as u16).to_be_bytes());
    b.extend_from_slice(node.as_bytes());
    b.extend_from_slice(&0i64.to_be_bytes());
    b.extend_from_slice(&1u64.to_be_bytes());
    let sig = key.sign(&b);
    b.extend_from_slice(sig.as_ref());
    b
}

fn store() -> Store<MemDevice> {
    store_of_size(256 * MIB)
}

fn store_of_size(size: u64) -> Store<MemDevice> {
    let dev = MemDevice::new(size);
    Store::format(
        &dev,
        &FormatOptions {
            disk_uuid: Uuid([1; 16]),
            cluster_uuid: Uuid::NIL,
            format_nonce: 1,
            created_unix_secs: 0,
            layout: LayoutParams {
                journal_size: Some(16 * MIB),
                ..Default::default()
            },
            label: String::new(),
        },
    )
    .unwrap();
    Store::open(dev, StoreConfig::default()).unwrap()
}

#[test]
fn authenticated_malformed_hello_cannot_erase_or_fence_old_incarnation() {
    use flaminio_net::frame::Frame;
    use flaminio_net::session::{Accepted, Conn};
    use flaminio_net::token::Claims;
    use futures_lite::AsyncWriteExt;
    use futures_rustls::{TlsAcceptor, TlsConnector};
    use smol::Async;

    let dir = std::env::temp_dir().join(format!("flaminio-hostile-hello-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let signer = Ed25519KeyPair::from_seed_unchecked(&[80; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let acceptor = TlsAcceptor::from(ids["fl-b"].server_config().unwrap());
    let connector = TlsConnector::from(ids["fl-a"].client_config().unwrap());
    let mut service = ReplicaService::new(store());
    service
        .hello(&Accepted {
            peer_node: "fl-a".into(),
            req: 1,
            epoch: 9,
            session: 1,
            claims: Claims {
                volume: "v".into(),
                epoch: 9,
                node: "fl-a".into(),
                issued_at: 0,
                first_epoch: 0,
            },
            size: 4 * CHUNK,
            chunk_size: CHUNK,
            chunks: vec![0, 3],
        })
        .unwrap();
    let before = service.store().chunks();
    let mutations = service.store().device().mutations();
    for case in 0..7 {
        let mut hello = Frame::Hello {
            req: 2,
            epoch: 21,
            session: 2,
            volume: "v".into(),
            token: token_first(&signer, "v", 21, "fl-a", 20),
            size: 4 * CHUNK,
            chunk_size: CHUNK,
            chunks: vec![0, 1],
        };
        let Frame::Hello {
            size,
            chunk_size,
            chunks,
            ..
        } = &mut hello
        else {
            unreachable!()
        };
        match case {
            0 => *size = 0,
            1 => *chunk_size = 0,
            2 => *chunk_size = 4096,
            3 => *size += 4096,
            4 => *chunks = vec![0, 0],
            5 => *chunks = vec![0, 4],
            _ => *size = 65_537 * CHUNK,
        }
        assert!(hello.encode_checked().is_err());
        let listener = Async::<TcpListener>::bind((LOCALHOST, 0)).unwrap();
        let addr = listener.get_ref().local_addr().unwrap();
        smol::block_on(async {
            let server = async {
                let (io, _) = listener.accept().await.unwrap();
                let (accepted, mut conn) = flaminio_net::session::accept(io, &acceptor, TD, &key)
                    .await
                    .unwrap();
                assert_eq!(accepted.claims.first_epoch, 20);
                assert_eq!(accepted.peer_node, "fl-a");
                let why = service.hello(&accepted).unwrap_err();
                conn.send(&Frame::Refused { reason: why }).await.unwrap();
            };
            let client = async {
                let io = Async::<std::net::TcpStream>::connect(addr).await.unwrap();
                let mut tls = connector.connect(LOCALHOST.into(), io).await.unwrap();
                // Deliberately bypass the checked sender as a hostile, authenticated peer.
                tls.write_all(&hello.encode()).await.unwrap();
                tls.flush().await.unwrap();
                let reply = Conn::new(tls).recv().await.unwrap();
                assert!(
                    matches!(reply, Frame::Refused { .. }),
                    "case {case}: {reply:?}"
                );
            };
            futures_lite::future::zip(server, client).await;
        });
        assert_eq!(service.store().chunks(), before, "case {case}");
        assert_eq!(
            service.store().device().mutations(),
            mutations,
            "case {case}"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

struct Node {
    inputs: mpsc::Sender<Input>,
    outputs: mpsc::Receiver<Output>,
    worker: Option<thread::JoinHandle<()>>,
}

fn start_node(name: &str, files: &Files, token_key: TokenKey, model: Arc<Mutex<Model>>) -> Node {
    start_node_with_store(name, files, token_key, model, store())
}

fn start_node_with_store(
    name: &str,
    files: &Files,
    token_key: TokenKey,
    model: Arc<Mutex<Model>>,
    store: Store<MemDevice>,
) -> Node {
    let listener = TcpListener::bind((LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel::<Input>();
    let sink_tx = Mutex::new(tx.clone());
    let clock = clock();
    let net = net::spawn(
        NetConfig {
            clock: clock.clone(),
            trust_domain: TD.into(),
            server: Some(ServerSide {
                listener,
                tls: files.server_config().unwrap(),
                token_key,
            }),
            client: Some(files.client_config().unwrap()),
        },
        Arc::new(move |e| {
            let _ = sink_tx.lock().unwrap().send(Input::Net(e));
        }),
    )
    .unwrap();
    let cfg = EngineConfig {
        node: name.into(),
        address: addr.to_string(),
        timing: Timing {
            lease: 1_000_000,
            drift_ppm: 100_000,
            heartbeat: 200_000,
            request_timeout: 500_000,
            reconnect: 200_000,
            meta_timeout: 1_000_000,
            refresh: 1_000_000,
        },
        max_io: 64,
        heartbeat: None,
    };
    let meta = Box::new(FakeMeta {
        model,
        inbox: tx.clone(),
    });
    let engine = Engine::new(cfg, ReplicaService::new(store), net, meta, clock, NONCE);
    let (otx, orx) = mpsc::channel();
    let worker = thread::spawn(move || {
        engine.run(rx, move |o| {
            let _ = otx.send(o);
        });
    });
    Node {
        inputs: tx,
        outputs: orx,
        worker: Some(worker),
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        let _ = self.inputs.send(Input::Stop);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !thread::panicking() {
                result.expect("engine worker panicked");
            }
        }
    }
}

impl Node {
    fn wait(&self, what: &str, f: impl Fn(&Output) -> bool) -> Output {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.outputs.recv_timeout(left) {
                Ok(o) if f(&o) => return o,
                Ok(_) => {}
                Err(_) => panic!("timed out waiting for {what}"),
            }
        }
    }

    fn io(&self, op: u64, io: Io) -> Option<Vec<u8>> {
        self.inputs
            .send(Input::Io {
                volume: "v".into(),
                op,
                io,
            })
            .unwrap();
        match self.wait(
            "I/O",
            |o| matches!(o, Output::Done { op: o, .. } if *o == op),
        ) {
            Output::Done { result, .. } => result.unwrap_or_else(|e| panic!("op {op}: {e}")),
            _ => unreachable!(),
        }
    }

    fn inspect(&self, offset: u64, len: u64) -> Vec<u8> {
        let (tx, rx) = mpsc::channel();
        self.inputs
            .send(Input::InspectLocal {
                volume: "v".into(),
                offset,
                len,
                reply: tx,
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(10))
            .unwrap()
            .expect("local replica")
    }
}

fn pattern(seed: u8, len: u64) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn isr(model: &Arc<Mutex<Model>>) -> Vec<Vec<String>> {
    model
        .lock()
        .unwrap()
        .volume
        .as_ref()
        .unwrap()
        .chunks
        .iter()
        .map(|c| c.isr.clone())
        .collect()
}

#[test]
fn replicated_volume_through_a_replica_failure() {
    let dir = std::env::temp_dir().join(format!("flaminio-engine-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let signer = Ed25519KeyPair::from_seed_unchecked(&[5; 32]).unwrap();
    let pubkey = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let model = Arc::new(Mutex::new(Model::default()));

    let a = start_node("fl-a", &ids["fl-a"], pubkey.clone(), model.clone());
    let b = start_node("fl-b", &ids["fl-b"], pubkey, model.clone());
    // Both nodes register themselves; then the volume: 2 chunks, RF 2, attached to fl-a.
    let deadline = Instant::now() + Duration::from_secs(10);
    while model.lock().unwrap().nodes.len() < 2 {
        assert!(Instant::now() < deadline, "nodes did not register");
        thread::sleep(Duration::from_millis(10));
    }
    {
        let both = vec!["fl-a".to_string(), "fl-b".to_string()];
        model.lock().unwrap().volume = Some(pb::Volume {
            id: "v".into(),
            size_bytes: 2 * CHUNK,
            chunk_bytes: CHUNK,
            replicas: 2,
            min_isr: 1,
            epoch: 2,
            target: "fl-a".into(),
            chunks: (0..2)
                .map(|_| pb::ChunkMeta {
                    replicas: both.clone(),
                    isr: both.clone(),
                    isr_ver: 0,
                    ..Default::default()
                })
                .collect(),
        });
    }
    a.inputs
        .send(Input::Attach {
            volume: "v".into(),
            proof: proof(&signer, token(&signer, "v", 2, "fl-a")).unwrap(),
        })
        .unwrap();
    a.wait("attach", |o| matches!(o, Output::Attached { epoch: 2, .. }));

    // Writes, one across the chunk boundary, then read them back on the target, and from the
    // other node's own replica.
    let writes = [(0, 64 << 10, 1u8), (CHUNK - 4096, 8192, 2), (MIB, MIB, 3)];
    for (i, &(off, len, seed)) in writes.iter().enumerate() {
        a.io(
            i as u64,
            Io::Write {
                offset: off,
                data: pattern(seed, len).into(),
            },
        );
    }
    for (i, &(off, len, seed)) in writes.iter().enumerate() {
        let got = a.io(100 + i as u64, Io::Read { offset: off, len }).unwrap();
        assert!(got == pattern(seed, len), "read back {off}+{len}");
        assert!(
            b.inspect(off, len) == pattern(seed, len),
            "fl-b's replica of {off}+{len}"
        );
    }

    // fl-b stops answering. The write still completes: fl-a removes fl-b from the ISR first.
    b.inputs
        .send(Input::Deafen {
            until: Instant::now() + Duration::from_secs(3),
        })
        .unwrap();
    let t = Instant::now();
    a.io(
        10,
        Io::Write {
            offset: 2 * MIB,
            data: pattern(4, 64 << 10).into(),
        },
    );
    eprintln!("write with fl-b deaf completed after {:?}", t.elapsed());
    assert_eq!(
        isr(&model)[0],
        vec!["fl-a".to_string()],
        "fl-b removed from chunk 0's ISR"
    );

    // fl-b comes back: it is resynced and rejoins, and holds the write it missed.
    let deadline = Instant::now() + Duration::from_secs(30);
    while isr(&model).iter().any(|c| c.len() != 2) {
        assert!(
            Instant::now() < deadline,
            "fl-b did not rejoin: {:?}",
            isr(&model)
        );
        thread::sleep(Duration::from_millis(50));
    }
    eprintln!("fl-b back in the ISR {:?} after the write", t.elapsed());
    assert!(
        b.inspect(2 * MIB, 64 << 10) == pattern(4, 64 << 10),
        "fl-b resynced"
    );
    for &(off, len, seed) in &writes {
        assert!(b.inspect(off, len) == pattern(seed, len));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Timing with no heartbeat, lease renewal or volume refresh within a test. A test that counts
/// commits or device writes needs it: a heartbeat queued behind writes changes how they go to
/// the store (it starts a held group, and the writes behind it are not staged), and at 1 s
/// heartbeats fall inside such a test once the sanitizers slow it down.
fn quiet() -> Timing {
    Timing {
        lease: 600_000_000,
        heartbeat: 300_000_000,
        refresh: 300_000_000,
        ..Timing::default()
    }
}

/// An engine on `name` for the tests to drive by hand, as the ublk frontend's threads do: its
/// inputs (network events, metadata answers) arrive on the receiver.
fn hand_driven(
    name: &str,
    files: &Files,
    token_key: TokenKey,
    model: Arc<Mutex<Model>>,
    timing: Timing,
    heartbeat: Option<Duration>,
) -> (Engine<MemDevice>, mpsc::Receiver<Input>) {
    let listener = TcpListener::bind((LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel::<Input>();
    let sink_tx = Mutex::new(tx.clone());
    let clock = clock();
    let net = net::spawn(
        NetConfig {
            clock: clock.clone(),
            trust_domain: TD.into(),
            server: Some(ServerSide {
                listener,
                tls: files.server_config().unwrap(),
                token_key,
            }),
            client: Some(files.client_config().unwrap()),
        },
        Arc::new(move |e| {
            let _ = sink_tx.lock().unwrap().send(Input::Net(e));
        }),
    )
    .unwrap();
    let cfg = EngineConfig {
        node: name.into(),
        address: addr.to_string(),
        timing,
        max_io: 64,
        heartbeat,
    };
    let meta = Box::new(FakeMeta { model, inbox: tx });
    (
        Engine::new(cfg, ReplicaService::new(store()), net, meta, clock, NONCE),
        rx,
    )
}

/// Reads a local read's piece from the store's device into `buf`, as the ublk frontend does,
/// and reports it: failed if `fail`, else as its checksums say.
fn serve_local_read(
    engine: &mut Engine<MemDevice>,
    buf: &mut [u8],
    lr: &flaminio_node::engine::LocalRead,
    fail: bool,
) {
    use flaminio_store::BlockDevice;

    let piece = &mut buf[lr.at..lr.at + lr.len];
    let plan = engine.plan_local_read(lr, piece).unwrap();
    let mut reads: Vec<(u64, &mut [u8])> = Vec::new();
    let mut rest: &mut [u8] = piece;
    let mut base = 0;
    for r in plan.reads() {
        let (_, tail) = rest.split_at_mut(r.buf_offset - base);
        let (this, tail) = tail.split_at_mut(r.len);
        reads.push((r.device_offset, this));
        rest = tail;
        base = r.buf_offset + r.len;
    }
    engine.device().read_many(&mut reads).unwrap();
    let ok = !fail && plan.verify(&buf[lr.at..lr.at + lr.len]).is_ok();
    engine.local_read_done(lr, Some(plan), ok);
}

/// What the tests do with a local read, as the ublk frontend would: read it into the buffer and
/// report it.
type Frontend<'a> =
    dyn FnMut(&mut Engine<MemDevice>, &mut [u8], flaminio_node::engine::LocalRead) + 'a;

/// One turn of `engine`, after the inputs that arrived: its outputs.
fn pump(engine: &mut Engine<MemDevice>, rx: &mpsc::Receiver<Input>) -> Vec<Output> {
    let _now = Instant::now();
    while let Ok(i) = rx.try_recv() {
        engine.handle(i);
    }
    engine.turn();
    engine.take_output()
}

/// Drives `engine` until `want` takes an output, for at most 20 s. Local reads go to
/// `local`; pieces of direct reads read elsewhere are copied into `buf`.
fn drive(
    engine: &mut Engine<MemDevice>,
    rx: &mpsc::Receiver<Input>,
    buf: &mut [u8],
    local: &mut Frontend<'_>,
    want: &dyn Fn(&Output) -> bool,
) -> Output {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < deadline, "timed out");
        for o in pump(engine, rx) {
            match o {
                Output::LocalRead(lr) => local(engine, buf, lr),
                Output::Piece { at, ref data, .. } if !want(&o) => {
                    buf[at..at + data.len()].copy_from_slice(data);
                }
                o if want(&o) => return o,
                _ => {}
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
}

/// A volume of two chunks attached on fl-a at `epoch`, with every replica in the ISR.
fn volume(replicas: &[&str], epoch: u64) -> pb::Volume {
    let replicas: Vec<String> = replicas.iter().map(|s| s.to_string()).collect();
    pb::Volume {
        id: "v".into(),
        size_bytes: 2 * CHUNK,
        chunk_bytes: CHUNK,
        replicas: replicas.len() as u32,
        min_isr: 1,
        epoch,
        target: "fl-a".into(),
        chunks: (0..2)
            .map(|_| pb::ChunkMeta {
                replicas: replicas.clone(),
                isr: replicas.clone(),
                isr_ver: 0,
                ..Default::default()
            })
            .collect(),
    }
}

/// One node, its engine driven by hand, with a single-replica volume: direct reads, as the
/// ublk frontend performs them, planned by the engine and read from the store's device into
/// the caller's buffer.
#[test]
fn direct_reads_from_the_local_replica() {
    use flaminio_store::BlockDevice;

    let dir = std::env::temp_dir().join(format!("flaminio-direct-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let model = Arc::new(Mutex::new(Model::default()));
    let signer = Ed25519KeyPair::from_seed_unchecked(&[6; 32]).unwrap();
    let (mut engine, rx) = hand_driven(
        "fl-a",
        &ids["fl-a"],
        TokenKey::from_raw(signer.public_key().as_ref()).unwrap(),
        model.clone(),
        Timing::default(),
        None,
    );
    model.lock().unwrap().volume = Some(pb::Volume {
        id: "v".into(),
        size_bytes: 2 * CHUNK,
        chunk_bytes: CHUNK,
        replicas: 1,
        min_isr: 1,
        epoch: 2,
        target: "fl-a".into(),
        chunks: (0..2)
            .map(|_| pb::ChunkMeta {
                replicas: vec!["fl-a".into()],
                isr: vec!["fl-a".into()],
                isr_ver: 0,
                ..Default::default()
            })
            .collect(),
    });

    // Drives the engine until `want` returns a value, playing the ublk frontend for local
    // reads into `buf`.
    let mut buf = vec![0u8; 2 * MIB as usize];
    let run = |engine: &mut Engine<MemDevice>,
               buf: &mut Vec<u8>,
               want: &dyn Fn(&Output) -> bool|
     -> Output {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(Instant::now() < deadline, "timed out");
            let _now = Instant::now();
            while let Ok(i) = rx.try_recv() {
                engine.handle(i);
            }
            engine.turn();
            for o in engine.take_output() {
                if let Output::LocalRead(lr) = &o {
                    serve_local_read(engine, buf, lr, false);
                    continue;
                }
                if want(&o) {
                    return o;
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
    };

    engine.handle(Input::Attach {
        volume: "v".into(),
        proof: proof(&signer, token(&signer, "v", 2, "fl-a")).unwrap(),
    });
    run(&mut engine, &mut buf, &|o| {
        matches!(o, Output::Attached { .. })
    });
    let off = CHUNK - MIB; // across the chunk boundary
    engine.handle(Input::Io {
        volume: "v".into(),
        op: 1,
        io: Io::Write {
            offset: off,
            data: pattern(9, 2 * MIB).into(),
        },
    });
    let w = run(&mut engine, &mut buf, &|o| {
        matches!(o, Output::Done { op: 1, .. })
    });
    assert_eq!(
        w,
        Output::Done {
            op: 1,
            result: Ok(None),
            permit: None
        }
    );
    engine.handle(Input::Io {
        volume: "v".into(),
        op: 2,
        io: Io::ReadDirect {
            offset: off,
            len: 2 * MIB,
        },
    });
    let r = run(&mut engine, &mut buf, &|o| {
        matches!(o, Output::Done { op: 2, .. })
    });
    assert_eq!(
        r,
        Output::Done {
            op: 2,
            result: Ok(None),
            permit: None
        },
        "data placed, not returned"
    );
    assert!(buf == pattern(9, 2 * MIB), "both pieces read into place");

    // The fast path: the same read, admitted at once and served without the queue.
    let fast_read = |engine: &mut Engine<MemDevice>, buf: &mut Vec<u8>| -> Option<bool> {
        let fast = engine.try_fast_read("v", off, 2 * MIB)?;
        let pieces: Vec<(usize, usize)> = fast.pieces().collect();
        assert_eq!(pieces.len(), 2, "one piece per chunk");
        let mut ok = true;
        for (i, (at, n)) in pieces.into_iter().enumerate() {
            let piece = &mut buf[at..at + n];
            let plan = engine.plan_fast_read(&fast, i, piece).unwrap();
            let mut reads: Vec<(u64, &mut [u8])> = Vec::new();
            let mut rest: &mut [u8] = piece;
            let mut base = 0;
            for r in plan.reads() {
                let (_, tail) = rest.split_at_mut(r.buf_offset - base);
                let (this, tail) = tail.split_at_mut(r.len);
                reads.push((r.device_offset, this));
                rest = tail;
                base = r.buf_offset + r.len;
            }
            engine.device().read_many(&mut reads).unwrap();
            ok &= plan.verify(&buf[at..at + n]).is_ok();
            engine.finish_plan(plan);
        }
        Some(
            engine
                .finish_fast_read("v", fast, ok)
                .map(|p| p.consume())
                .is_some(),
        )
    };
    let mut fast_buf = vec![0u8; 2 * MIB as usize];
    assert_eq!(fast_read(&mut engine, &mut fast_buf), Some(true));
    assert!(
        fast_buf == pattern(9, 2 * MIB),
        "the fast path reads the same data"
    );

    // A write in flight holds its blocks: an overlapping fast read is refused, and admitted
    // again once the write is done.
    engine.handle(Input::Io {
        volume: "v".into(),
        op: 3,
        io: Io::Write {
            offset: off,
            data: pattern(10, 2 * MIB).into(),
        },
    });
    assert_eq!(
        fast_read(&mut engine, &mut fast_buf),
        None,
        "refused under a write"
    );
    run(&mut engine, &mut buf, &|o| {
        matches!(o, Output::Done { op: 3, .. })
    });
    assert_eq!(fast_read(&mut engine, &mut fast_buf), Some(true));
    assert!(
        fast_buf == pattern(10, 2 * MIB),
        "the write's data, after it completed"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The request timeout of the hand-driven engines: a failed local read must be answered well
/// within it, not after it (docs/issues/5).
fn request_timeout() -> Duration {
    Duration::from_micros(Timing::default().request_timeout)
}

/// Writes `data` at 0 through `engine` as operation `op`, serving its local reads.
fn write_at_0(engine: &mut Engine<MemDevice>, rx: &mpsc::Receiver<Input>, op: u64, data: Vec<u8>) {
    engine.handle(Input::Io {
        volume: "v".into(),
        op,
        io: Io::Write {
            offset: 0,
            data: data.into(),
        },
    });
    let mut serve =
        |e: &mut Engine<MemDevice>, b: &mut [u8], lr| serve_local_read(e, b, &lr, false);
    let done = drive(
        engine,
        rx,
        &mut [],
        &mut serve,
        &|o| matches!(o, Output::Done { op: o, .. } if *o == op),
    );
    assert!(
        matches!(
            done,
            Output::Done {
                result: Ok(None),
                ..
            }
        ),
        "{done:?}"
    );
}

/// Submits a direct read of `len` bytes at 0 as operation `op`.
fn read_direct_at_0(engine: &mut Engine<MemDevice>, op: u64, len: u64) {
    engine.handle(Input::Io {
        volume: "v".into(),
        op,
        io: Io::ReadDirect { offset: 0, len },
    });
}

/// Attaches the volume on a hand-driven engine at `epoch`.
fn attach(
    engine: &mut Engine<MemDevice>,
    rx: &mpsc::Receiver<Input>,
    signer: &Ed25519KeyPair,
    epoch: u64,
) {
    engine.handle(Input::Attach {
        volume: "v".into(),
        proof: proof(signer, token(signer, "v", epoch, "fl-a")).unwrap(),
    });
    let mut serve =
        |e: &mut Engine<MemDevice>, b: &mut [u8], lr| serve_local_read(e, b, &lr, false);
    drive(
        engine,
        rx,
        &mut [],
        &mut serve,
        &|o| matches!(o, Output::Attached { epoch: e, .. } if *e == epoch),
    );
}

/// Reports a local read as failed, as the frontend does when its device read fails.
fn fail_local_read(
    engine: &mut Engine<MemDevice>,
    _: &mut [u8],
    lr: flaminio_node::engine::LocalRead,
) {
    engine.local_read_done(&lr, None, false);
}

/// A failed local read on a single-replica volume ends at once, with an error (docs/issues/5), not
/// after the request timeout, and the local replica stays in the ISR.
#[test]
fn a_failed_local_read_on_the_only_replica_fails_at_once() {
    use flaminio_node::engine::IoError;

    let dir = std::env::temp_dir().join(format!("flaminio-failed-only-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let model = Arc::new(Mutex::new(Model::default()));
    let signer = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let (mut engine, rx) = hand_driven(
        "fl-a",
        &ids["fl-a"],
        key,
        model.clone(),
        Timing::default(),
        None,
    );
    model.lock().unwrap().volume = Some(volume(&["fl-a"], 2));
    attach(&mut engine, &rx, &signer, 2);
    write_at_0(&mut engine, &rx, 1, pattern(1, 64 << 10));

    let t = Instant::now();
    read_direct_at_0(&mut engine, 2, 64 << 10);
    let mut buf = vec![0u8; 64 << 10];
    let done = drive(&mut engine, &rx, &mut buf, &mut fail_local_read, &|o| {
        matches!(o, Output::Done { op: 2, .. })
    });
    assert!(
        t.elapsed() < request_timeout() / 2,
        "took {:?}",
        t.elapsed()
    );
    assert!(
        matches!(
            done,
            Output::Done {
                result: Err(IoError::Unreadable),
                ..
            }
        ),
        "{done:?}"
    );
    assert_eq!(isr(&model), vec![vec!["fl-a".to_string()]; 2]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A failed local read on a replicated volume is read from the other replica at once
/// (docs/issues/5), and the local replica stays in the ISR.
#[test]
fn a_failed_local_read_is_read_from_the_other_replica() {
    let dir = std::env::temp_dir().join(format!("flaminio-failed-two-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let model = Arc::new(Mutex::new(Model::default()));
    let signer = Ed25519KeyPair::from_seed_unchecked(&[8; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let _b = start_node("fl-b", &ids["fl-b"], key.clone(), model.clone());
    let (mut engine, rx) = hand_driven(
        "fl-a",
        &ids["fl-a"],
        key,
        model.clone(),
        Timing::default(),
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !model.lock().unwrap().nodes.contains_key("fl-b") {
        assert!(Instant::now() < deadline, "fl-b did not register");
        thread::sleep(Duration::from_millis(10));
    }
    model.lock().unwrap().volume = Some(volume(&["fl-a", "fl-b"], 2));
    attach(&mut engine, &rx, &signer, 2);
    write_at_0(&mut engine, &rx, 1, pattern(2, 64 << 10));

    let t = Instant::now();
    read_direct_at_0(&mut engine, 2, 64 << 10);
    // The failed local read leaves the buffer alone: what is there came from fl-b.
    let mut buf = vec![0u8; 64 << 10];
    let done = drive(&mut engine, &rx, &mut buf, &mut fail_local_read, &|o| {
        matches!(o, Output::Done { op: 2, .. })
    });
    assert!(
        t.elapsed() < request_timeout() / 2,
        "took {:?}",
        t.elapsed()
    );
    assert!(
        matches!(
            done,
            Output::Done {
                result: Ok(None),
                ..
            }
        ),
        "{done:?}"
    );
    assert!(buf == pattern(2, 64 << 10), "fl-b's data");
    let both = vec!["fl-a".to_string(), "fl-b".to_string()];
    assert_eq!(isr(&model), vec![both; 2]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A failed local read reported after its target was deposed and replaced is ignored, even
/// when the new target's read in flight has the same session and request numbers: that read
/// completes, from one local read, well within the request timeout (docs/issues/5). Requests are
/// numbered from 1 by each target, so the old target sends a few more before its read, and the
/// new one reads until its read gets the same number.
#[test]
fn a_stale_failed_local_read_leaves_the_new_target_alone() {
    let dir = std::env::temp_dir().join(format!("flaminio-failed-stale-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let model = Arc::new(Mutex::new(Model::default()));
    let signer = Ed25519KeyPair::from_seed_unchecked(&[9; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let (mut engine, rx) = hand_driven(
        "fl-a",
        &ids["fl-a"],
        key,
        model.clone(),
        Timing::default(),
        None,
    );
    model.lock().unwrap().volume = Some(volume(&["fl-a"], 2));

    // A direct read whose local read is held back.
    let held = |engine: &mut Engine<MemDevice>, op: u64| {
        read_direct_at_0(engine, op, 64 << 10);
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(Instant::now() < deadline, "no local read for op {op}");
            for o in pump(engine, &rx) {
                if let Output::LocalRead(lr) = o {
                    assert_eq!(lr.op, op);
                    return lr;
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
    };
    attach(&mut engine, &rx, &signer, 2);
    for op in 10..16 {
        write_at_0(&mut engine, &rx, op, pattern(3, 64 << 10));
    }
    let stale = held(&mut engine, 2);
    let (_, session, req) = engine.local_read_ids(&stale).unwrap();

    // A newer epoch deposes the target, which the engine drops without purging its local
    // reads; then the volume is attached again here.
    model.lock().unwrap().volume.as_mut().unwrap().epoch = 3;
    let mut serve =
        |e: &mut Engine<MemDevice>, b: &mut [u8], lr| serve_local_read(e, b, &lr, false);
    drive(&mut engine, &rx, &mut [], &mut serve, &|o| {
        matches!(o, Output::Deposed { .. })
    });
    attach(&mut engine, &rx, &signer, 3);
    write_at_0(&mut engine, &rx, 3, pattern(4, 64 << 10));
    let mut buf = vec![0u8; 64 << 10];
    let mut op = 100;
    let fresh = loop {
        let lr = held(&mut engine, op);
        let (epoch, s, r) = engine.local_read_ids(&lr).unwrap();
        assert_eq!((epoch, s), (3, session), "the new target's first session");
        assert!(r <= req, "request {r} went past the stale one, {req}");
        if r == req {
            break lr;
        }
        serve_local_read(&mut engine, &mut buf, &lr, false);
        let mut serve =
            |e: &mut Engine<MemDevice>, b: &mut [u8], lr| serve_local_read(e, b, &lr, false);
        drive(
            &mut engine,
            &rx,
            &mut buf,
            &mut serve,
            &|o| matches!(o, Output::Done { op: o, .. } if *o == op),
        );
        op += 1;
    };
    let fresh_op = op;

    let t = Instant::now();
    engine.local_read_done(&stale, None, false);
    buf.fill(0);
    serve_local_read(&mut engine, &mut buf, &fresh, false);
    let mut again = 0;
    let mut count = |e: &mut Engine<MemDevice>, b: &mut [u8], lr| {
        again += 1;
        serve_local_read(e, b, &lr, false);
    };
    let done = drive(
        &mut engine,
        &rx,
        &mut buf,
        &mut count,
        &|o| matches!(o, Output::Done { op: o, .. } if *o == fresh_op),
    );
    assert!(
        t.elapsed() < request_timeout() / 2,
        "took {:?}",
        t.elapsed()
    );
    assert!(
        matches!(
            done,
            Output::Done {
                result: Ok(None),
                ..
            }
        ),
        "{done:?}"
    );
    assert_eq!(again, 0, "the read was not sent again");
    assert!(buf == pattern(4, 64 << 10), "the new target's data");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A local read that fails because a newer target fenced the replica meanwhile is answered
/// `Stale`, which deposes the target, rather than sent to another member or ended as
/// unreadable (docs/issues/5). fl-b becomes the volume's target at epoch 3 and fences fl-a's
/// replica while fl-a's target, at epoch 2, has a local read in flight. fl-a's target neither
/// heartbeats nor refreshes during the test, so the failed read is what tells it.
#[test]
fn a_local_read_failed_by_a_fence_deposes_the_target() {
    use flaminio_node::engine::IoError;

    let dir = std::env::temp_dir().join(format!("flaminio-failed-fenced-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let model = Arc::new(Mutex::new(Model::default()));
    let signer = Ed25519KeyPair::from_seed_unchecked(&[10; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let (mut engine, rx) = hand_driven(
        "fl-a",
        &ids["fl-a"],
        key.clone(),
        model.clone(),
        quiet(),
        None,
    );
    let b = start_node("fl-b", &ids["fl-b"], key, model.clone());
    let deadline = Instant::now() + Duration::from_secs(10);
    while model.lock().unwrap().nodes.len() < 2 {
        assert!(Instant::now() < deadline, "nodes did not register");
        pump(&mut engine, &rx);
        thread::sleep(Duration::from_millis(10));
    }
    model.lock().unwrap().volume = Some(volume(&["fl-a"], 2));
    attach(&mut engine, &rx, &signer, 2);
    write_at_0(&mut engine, &rx, 1, pattern(5, 64 << 10));
    read_direct_at_0(&mut engine, 2, 64 << 10);
    let held = loop {
        let o = pump(&mut engine, &rx);
        if let Some(lr) = o.into_iter().find_map(|o| match o {
            Output::LocalRead(lr) => Some(lr),
            _ => None,
        }) {
            break lr;
        }
        thread::sleep(Duration::from_millis(1));
    };

    // fl-b attaches at epoch 3 and fences fl-a's replica, which fl-a's engine serves.
    {
        let mut m = model.lock().unwrap();
        let v = m.volume.as_mut().unwrap();
        v.epoch = 3;
        v.target = "fl-b".into();
    }
    b.inputs
        .send(Input::Attach {
            volume: "v".into(),
            proof: proof(&signer, token(&signer, "v", 3, "fl-b")).unwrap(),
        })
        .unwrap();
    // Attached comes as fl-b's target starts; a read through it completes only once its Hello
    // has fenced fl-a's replica.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut attached = false;
    loop {
        assert!(
            Instant::now() < deadline,
            "fl-b did not read from fl-a's replica"
        );
        pump(&mut engine, &rx);
        match b.outputs.try_recv() {
            Ok(Output::Attached { epoch: 3, .. }) => {
                attached = true;
                b.inputs
                    .send(Input::Io {
                        volume: "v".into(),
                        op: 50,
                        io: Io::Read {
                            offset: 0,
                            len: 4096,
                        },
                    })
                    .unwrap();
            }
            Ok(Output::Done { op: 50, result, .. }) => {
                assert!(attached);
                assert_eq!(result.unwrap(), Some(pattern(5, 4096)));
                break;
            }
            _ => thread::sleep(Duration::from_millis(1)),
        }
    }

    // The store refuses to plan a read at epoch 2 now, so the frontend reports it failed.
    engine.local_read_done(&held, None, false);
    let (mut done, mut deposed) = (None, false);
    let deadline = Instant::now() + Duration::from_secs(20);
    while done.is_none() || !deposed {
        assert!(
            Instant::now() < deadline,
            "done {done:?}, deposed {deposed}"
        );
        for o in pump(&mut engine, &rx) {
            match o {
                Output::Done { op: 2, result, .. } => done = Some(result),
                Output::Deposed { .. } => deposed = true,
                _ => {}
            }
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(done, Some(Err(IoError::Deposed))), "{done:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An attach dropped before the metadata service answered (its frontend gave up waiting) must
/// leave nothing behind: its late answers, data or failures, neither start a target nor touch a
/// new attach of the volume (docs/issues/6, S3). The metadata answers wait in the engine's input
/// channel until the test hands them over, so they can come late, or be swapped for failures.
enum Late {
    /// Hand the answers over after the detach, then attach again.
    Ignored,
    /// Attach again at a newer epoch first, then hand all the answers over.
    BeforeNewAttach,
    /// As `BeforeNewAttach`, with the dropped attach's answers turned into failures.
    FailedBeforeNewAttach,
}

fn dropped_attach(late: Late) {
    use flaminio_node::engine::IoError;

    let dir = std::env::temp_dir().join(format!(
        "flaminio-dropped-attach-{}-{}",
        std::process::id(),
        match late {
            Late::Ignored => 0,
            Late::BeforeNewAttach => 1,
            Late::FailedBeforeNewAttach => 2,
        }
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let model = Arc::new(Mutex::new(Model::default()));
    let signer = Ed25519KeyPair::from_seed_unchecked(&[11; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let (mut engine, rx) = hand_driven(
        "fl-a",
        &ids["fl-a"],
        key,
        model.clone(),
        Timing::default(),
        None,
    );
    model.lock().unwrap().volume = Some(volume(&["fl-a"], 2));
    let attach_input = |epoch| Input::Attach {
        volume: "v".into(),
        proof: proof(&signer, token(&signer, "v", epoch, "fl-a")).unwrap(),
    };
    // Hands every waiting input to the engine, after `edit`, and returns its outputs.
    let deliver = |engine: &mut Engine<MemDevice>, edit: &dyn Fn(&mut Vec<Input>)| {
        let mut inputs: Vec<Input> = rx.try_iter().collect();
        edit(&mut inputs);
        let _now = Instant::now();
        for i in inputs {
            engine.handle(i);
        }
        engine.turn();
        engine.take_output()
    };
    let attached = |outs: &[Output]| -> Vec<u64> {
        outs.iter()
            .filter_map(|o| match o {
                Output::Attached { epoch, .. } => Some(*epoch),
                _ => None,
            })
            .collect()
    };
    let failed = |outs: &[Output]| {
        outs.iter()
            .any(|o| matches!(o, Output::AttachFailed { .. }))
    };

    // The attach's metadata calls are answered into the channel at once, at epoch 2.
    engine.handle(attach_input(2));
    engine.handle(Input::Detach { volume: "v".into() });
    match late {
        Late::Ignored => {
            let outs = deliver(&mut engine, &|_| {});
            assert!(attached(&outs).is_empty() && !failed(&outs), "{outs:?}");
            engine.handle(Input::Io {
                volume: "v".into(),
                op: 1,
                io: Io::Read {
                    offset: 0,
                    len: 4096,
                },
            });
            let outs = engine.take_output();
            assert!(
                outs.iter().any(|o| matches!(
                    o,
                    Output::Done {
                        op: 1,
                        result: Err(IoError::NotAttached),
                        permit: None
                    }
                )),
                "no target: {outs:?}"
            );
            engine.handle(attach_input(2));
            let outs = deliver(&mut engine, &|_| {});
            assert_eq!(attached(&outs), [2], "{outs:?}");
        }
        Late::BeforeNewAttach | Late::FailedBeforeNewAttach => {
            model.lock().unwrap().volume.as_mut().unwrap().epoch = 3;
            engine.handle(attach_input(3));
            let fail = matches!(late, Late::FailedBeforeNewAttach);
            // The dropped attach's answers come first in the channel: the first GetVolume and
            // the first ListNodes (the engine's own RegisterNode comes before them).
            let outs = deliver(&mut engine, &|inputs| {
                if !fail {
                    return;
                }
                let mut seen = (false, false);
                for i in inputs.iter_mut() {
                    let Input::Meta { answer, .. } = i else {
                        continue;
                    };
                    let first = match answer {
                        Answer::GetVolume(_) => !std::mem::replace(&mut seen.0, true),
                        Answer::ListNodes(_) => !std::mem::replace(&mut seen.1, true),
                        _ => false,
                    };
                    if first {
                        *answer = Answer::Failed {
                            code: flaminio_metaclient::Code::Unavailable,
                            message: "the dropped attach's call".into(),
                        };
                    }
                }
                assert_eq!(seen, (true, true));
            });
            assert_eq!(attached(&outs), [3], "{outs:?}");
            assert!(!failed(&outs), "{outs:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// An attach of a volume the metadata service does not have fails at once, rather than waiting
/// for an answer that will not come.
#[test]
fn an_attach_of_an_unknown_volume_fails_at_once() {
    let dir = std::env::temp_dir().join(format!("flaminio-unknown-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let model = Arc::new(Mutex::new(Model::default()));
    let signer = Ed25519KeyPair::from_seed_unchecked(&[12; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let (mut engine, rx) = hand_driven("fl-a", &ids["fl-a"], key, model, Timing::default(), None);
    engine.handle(Input::Attach {
        volume: "v".into(),
        proof: proof(&signer, token(&signer, "v", 2, "fl-a")).unwrap(),
    });
    let t = Instant::now();
    let mut serve =
        |e: &mut Engine<MemDevice>, b: &mut [u8], lr| serve_local_read(e, b, &lr, false);
    let out = drive(&mut engine, &rx, &mut [], &mut serve, &|o| {
        matches!(o, Output::Attached { .. } | Output::AttachFailed { .. })
    });
    assert!(matches!(out, Output::AttachFailed { .. }), "{out:?}");
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "took {:?}",
        t.elapsed()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn late_answers_to_a_dropped_attach_start_no_target() {
    dropped_attach(Late::Ignored);
}

#[test]
fn late_answers_to_a_dropped_attach_leave_a_new_attach_alone() {
    dropped_attach(Late::BeforeNewAttach);
}

#[test]
fn late_failures_of_a_dropped_attach_leave_a_new_attach_alone() {
    dropped_attach(Late::FailedBeforeNewAttach);
}

/// A registration the metadata service refuses (contended, or unreachable at start) is sent
/// again, heartbeats or not, and so is one the service lost (docs/issues/7): the node does not stay
/// unregistered, and never live, until a restart.
#[test]
fn a_refused_or_lost_registration_is_sent_again() {
    let dir = std::env::temp_dir().join(format!("flaminio-register-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let model = Arc::new(Mutex::new(Model {
        refuse_registrations: 2,
        ..Model::default()
    }));
    let signer = Ed25519KeyPair::from_seed_unchecked(&[13; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let (mut engine, rx) = hand_driven(
        "fl-a",
        &ids["fl-a"],
        key,
        model.clone(),
        Timing::default(),
        Some(Duration::from_millis(20)),
    );
    let registered = |engine: &mut Engine<MemDevice>| {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !model.lock().unwrap().nodes.contains_key("fl-a") {
            assert!(Instant::now() < deadline, "fl-a never registered");
            pump(engine, &rx);
            thread::sleep(Duration::from_millis(5));
        }
    };
    registered(&mut engine);
    assert_eq!(model.lock().unwrap().refuse_registrations, 0);
    // The service loses the record: the next heartbeat gets NotFound, and the node registers again.
    model.lock().unwrap().nodes.clear();
    registered(&mut engine);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A single-node engine whose local replica's commits stay in flight until the test lands
/// them: writes are acknowledged only once durable, the next group waits for the commit, and
/// reads go on meanwhile.
#[test]
fn commits_in_flight_hold_writes_but_not_reads() {
    use flaminio_node::engine::LocalRead;
    use flaminio_store::{BlockDevice, DeferredDevice};

    let dir = std::env::temp_dir().join(format!("flaminio-async-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let files = &ids["fl-a"];
    let model = Arc::new(Mutex::new(Model::default()));
    let listener = TcpListener::bind((LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel::<Input>();
    let sink_tx = Mutex::new(tx.clone());
    let signer = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let clock = clock();
    let net = net::spawn(
        NetConfig {
            clock: clock.clone(),
            trust_domain: TD.into(),
            server: Some(ServerSide {
                listener,
                tls: files.server_config().unwrap(),
                token_key: TokenKey::from_raw(signer.public_key().as_ref()).unwrap(),
            }),
            client: Some(files.client_config().unwrap()),
        },
        Arc::new(move |e| {
            let _ = sink_tx.lock().unwrap().send(Input::Net(e));
        }),
    )
    .unwrap();
    let cfg = EngineConfig {
        node: "fl-a".into(),
        address: addr.to_string(),
        timing: quiet(),
        max_io: 64,
        heartbeat: None,
    };
    let meta = Box::new(FakeMeta {
        model: model.clone(),
        inbox: tx.clone(),
    });
    let dev = store().into_device();
    let deferred = Store::open(DeferredDevice::new(dev), StoreConfig::default()).unwrap();
    let mut engine = Engine::new(cfg, ReplicaService::new(deferred), net, meta, clock, NONCE);
    engine.set_async_commits(true);
    // Count/time gather for the paths below; the byte-floor section turns this back on.
    engine.set_commit_target_bytes(0);
    model.lock().unwrap().volume = Some(pb::Volume {
        id: "v".into(),
        size_bytes: 4 * CHUNK,
        chunk_bytes: CHUNK,
        replicas: 1,
        min_isr: 1,
        epoch: 2,
        target: "fl-a".into(),
        chunks: (0..4)
            .map(|_| pb::ChunkMeta {
                replicas: vec!["fl-a".into()],
                isr: vec!["fl-a".into()],
                isr_ver: 0,
                ..Default::default()
            })
            .collect(),
    });

    // Drives the engine for `turns` turns (or until `want` matches), playing the frontend for
    // local reads into `buf`. Returns the outputs that matched.
    let mut buf = vec![0u8; MIB as usize];
    let run = |engine: &mut Engine<DeferredDevice>,
               buf: &mut Vec<u8>,
               turns: usize,
               want: &dyn Fn(&Output) -> bool|
     -> Vec<Output> {
        let mut got = Vec::new();
        for _ in 0..turns {
            let _now = Instant::now();
            while let Ok(i) = rx.try_recv() {
                engine.handle(i);
            }
            engine.turn();
            for o in engine.take_output() {
                if let Output::LocalRead(lr) = &o {
                    let lr: LocalRead = lr.clone();
                    let piece = &mut buf[lr.at..lr.at + lr.len];
                    let plan = engine.plan_local_read(&lr, piece).unwrap();
                    for r in plan.reads() {
                        let (o, n) = (r.buf_offset, r.len);
                        engine
                            .device()
                            .read_at(r.device_offset, &mut piece[o..o + n])
                            .unwrap();
                    }
                    let ok = plan.verify(&buf[lr.at..lr.at + lr.len]).is_ok();
                    engine.local_read_done(&lr, Some(plan), ok);
                    continue;
                }
                if want(&o) {
                    got.push(o);
                }
            }
            if !got.is_empty() && turns > 1000 {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        got
    };
    let until = 20_000; // turns: a ceiling, the loop stops at the first match
    let io = |engine: &mut Engine<DeferredDevice>, op: u64, io: Io| {
        engine.handle(Input::Io {
            volume: "v".into(),
            op,
            io,
        });
    };
    let done = |op: u64| move |o: &Output| matches!(o, Output::Done { op: d, .. } if *d == op);

    engine.handle(Input::Attach {
        volume: "v".into(),
        proof: proof(&signer, token(&signer, "v", 2, "fl-a")).unwrap(),
    });
    run(&mut engine, &mut buf, until, &|o| {
        matches!(o, Output::Attached { .. })
    });

    io(
        &mut engine,
        1,
        Io::Write {
            offset: 0,
            data: pattern(1, 64 * 1024).into(),
        },
    );
    assert!(
        run(&mut engine, &mut buf, 50, &done(1)).is_empty(),
        "not durable yet"
    );
    assert!(engine.device().in_flight());
    // The next write waits for the commit in flight; a read of other blocks does not.
    io(
        &mut engine,
        2,
        Io::Write {
            offset: MIB,
            data: pattern(2, 64 * 1024).into(),
        },
    );
    io(
        &mut engine,
        3,
        Io::ReadDirect {
            offset: CHUNK,
            len: 64 * 1024,
        },
    );
    let r = run(&mut engine, &mut buf, until, &done(3));
    assert_eq!(
        r,
        vec![Output::Done {
            op: 3,
            result: Ok(None),
            permit: None
        }]
    );
    assert!(buf[..64 * 1024].iter().all(|&b| b == 0), "never written");
    assert!(run(&mut engine, &mut buf, 20, &done(2)).is_empty());

    engine.device().complete();
    let w = run(&mut engine, &mut buf, until, &done(1));
    assert_eq!(
        w,
        vec![Output::Done {
            op: 1,
            result: Ok(None),
            permit: None
        }]
    );
    // Then the second write's commit starts.
    assert!(run(&mut engine, &mut buf, 20, &done(2)).is_empty());
    assert!(engine.device().in_flight());
    engine.device().complete();
    assert_eq!(run(&mut engine, &mut buf, until, &done(2)).len(), 1);

    for (op, offset, seed) in [(4, 0, 1), (5, MIB, 2)] {
        io(
            &mut engine,
            op,
            Io::ReadDirect {
                offset,
                len: 64 * 1024,
            },
        );
        assert_eq!(run(&mut engine, &mut buf, until, &done(op)).len(), 1);
        assert!(
            buf[..64 * 1024] == pattern(seed, 64 * 1024),
            "write {seed} reads back"
        );
    }
    let cluster = 64 * 1024;
    let at = |i: u64| 2 * MIB + i * cluster;
    let write = |engine: &mut Engine<DeferredDevice>, op: u64, i: u64| {
        engine.handle(Input::Io {
            volume: "v".into(),
            op,
            io: Io::Write {
                offset: at(i),
                data: pattern(op as u8, cluster).into(),
            },
        });
    };
    let acked = |engine: &mut Engine<DeferredDevice>,
                 buf: &mut Vec<u8>,
                 ops: std::ops::RangeInclusive<u64>| {
        let mut n = 0;
        while n < ops.clone().count() {
            n += run(
                engine,
                buf,
                until,
                &|o| matches!(o, Output::Done { op, .. } if ops.contains(op)),
            )
            .len();
        }
    };

    // Gathering: after a commit of two writes, with nothing staged, the next commit waits for
    // two writes before it starts. Count/time only (byte floor off).
    engine.set_commit_target_bytes(0);
    engine.set_commit_gather(Duration::from_secs(10));
    write(&mut engine, 10, 0);
    write(&mut engine, 11, 1);
    run(&mut engine, &mut buf, 5, &done(10));
    assert_eq!(
        engine.device().pending_writes(),
        3,
        "two writes and their batch"
    );
    engine.device().complete();
    acked(&mut engine, &mut buf, 10..=11);
    write(&mut engine, 12, 2);
    run(&mut engine, &mut buf, 20, &done(12));
    assert!(!engine.device().in_flight(), "waiting for a second write");
    write(&mut engine, 13, 3);
    run(&mut engine, &mut buf, 5, &done(12));
    assert_eq!(engine.device().pending_writes(), 3, "both in one group");
    // From here, waits end after 5 ms (the wait this completion starts included).
    engine.set_commit_gather(Duration::from_millis(5));
    engine.device().complete();
    acked(&mut engine, &mut buf, 12..=13);
    write(&mut engine, 14, 4);
    run(&mut engine, &mut buf, 30, &done(14));
    assert_eq!(
        engine.device().pending_writes(),
        2,
        "started alone after 5 ms"
    );

    // Staging: writes that arrive while a commit is in flight are staged behind it. Their data
    // goes at once; when it lands, their commit is only their journal batch.
    write(&mut engine, 15, 5);
    write(&mut engine, 16, 6);
    run(&mut engine, &mut buf, 5, &done(15));
    assert_eq!(
        engine.device().pending_writes(),
        2,
        "still the commit of write 14"
    );
    assert_eq!(
        engine.device().commits_in_flight(),
        1,
        "staged group waits for the landing (pipelined commits off)"
    );
    engine.device().complete();
    assert_eq!(run(&mut engine, &mut buf, until, &done(14)).len(), 1);
    run(&mut engine, &mut buf, 5, &done(15));
    assert!(
        engine.device().in_flight(),
        "the staged group's commit started when the first landed"
    );
    assert_eq!(engine.device().pending_writes(), 1, "their batch only");
    engine.device().complete();
    acked(&mut engine, &mut buf, 15..=16);
    for (op, i) in [(15, 5), (16, 6)] {
        io(
            &mut engine,
            100 + op,
            Io::ReadDirect {
                offset: at(i),
                len: cluster,
            },
        );
        assert_eq!(run(&mut engine, &mut buf, until, &done(100 + op)).len(), 1);
        assert!(
            buf[..cluster as usize] == pattern(op as u8, cluster),
            "write {op} reads back"
        );
    }

    // Holding (docs/issues/10): after a commit of four 1 MiB whole-cluster writes, the next group
    // waits for the writes outstanding at the landing, four, not for its window (10 s here) nor for
    // 8 MiB. Their data goes to the disk as each is staged, with nothing in flight.
    engine.set_commit_target_bytes(8 << 20);
    engine.set_commit_gather(Duration::from_secs(10));
    let write_mib = |engine: &mut Engine<DeferredDevice>, op: u64, i: u64| {
        engine.handle(Input::Io {
            volume: "v".into(),
            op,
            io: Io::Write {
                offset: i * MIB,
                data: pattern(op as u8, MIB).into(),
            },
        });
    };
    for (op, i) in [(20u64, 0u64), (21, 1), (22, 2), (23, 3)] {
        write_mib(&mut engine, op, i);
    }
    run(&mut engine, &mut buf, 5, &done(20));
    assert!(engine.device().in_flight());
    engine.device().complete();
    acked(&mut engine, &mut buf, 20..=23);
    for (op, i) in [(24u64, 0u64), (25, 1), (26, 2), (27, 3)] {
        write_mib(&mut engine, op, i);
    }
    run(&mut engine, &mut buf, 20, &done(24));
    assert!(
        engine.device().in_flight(),
        "count rule: the four writes outstanding at the landing are back"
    );
    assert_eq!(
        engine.device().pending_writes(),
        1,
        "their batch only: their data was written as each was staged"
    );
    engine.device().complete();
    acked(&mut engine, &mut buf, 24..=27);

    // The 8 MiB rule on its own: eight 1 MiB writes staged behind a commit in flight start at
    // its landing, without waiting for the landed commit's writes to come back.
    for (op, i) in [(28u64, 4u64), (29, 5), (30, 6), (31, 7)] {
        write_mib(&mut engine, op, i);
    }
    run(&mut engine, &mut buf, 20, &done(28));
    assert!(engine.device().in_flight(), "four, by the count rule");
    for op in 32u64..40 {
        write_mib(&mut engine, op, op - 24);
    }
    run(&mut engine, &mut buf, 5, &done(28));
    assert_eq!(
        engine.device().commits_in_flight(),
        1,
        "the eight are staged behind it"
    );
    // Arm the next gather with a short window before these commits land.
    engine.set_commit_gather(Duration::from_millis(5));
    engine.device().complete();
    acked(&mut engine, &mut buf, 28..=31);
    run(&mut engine, &mut buf, 1, &done(32));
    assert!(
        engine.device().in_flight(),
        "8 MiB staged: their commit started at the landing"
    );
    assert_eq!(engine.device().pending_writes(), 1, "their batch only");
    engine.device().complete();
    acked(&mut engine, &mut buf, 32..=39);

    // 4K path: byte floor does not hold the commit for 8 MiB; the gather window ends it.
    engine.handle(Input::Io {
        volume: "v".into(),
        op: 40,
        io: Io::Write {
            offset: 0,
            data: pattern(40, 4096).into(),
        },
    });
    run(&mut engine, &mut buf, 30, &done(40));
    assert!(
        engine.device().in_flight(),
        "4K started alone after the gather window, without waiting for 8 MiB"
    );
    engine.device().complete();
    acked(&mut engine, &mut buf, 40..=40);

    engine.set_async_commits(false);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A token of a volume created at first epoch `first` (version 2: docs/issues/30), as the
/// metadata service signs it.
fn token_first(key: &Ed25519KeyPair, volume: &str, epoch: u64, node: &str, first: u64) -> Vec<u8> {
    let mut b = token(key, volume, epoch, node);
    b.truncate(b.len() - 64 - 8);
    b[4] = 2;
    b.extend_from_slice(&first.to_be_bytes());
    let sig = key.sign(&b);
    b.extend_from_slice(sig.as_ref());
    b
}

/// Volume "v" of `chunks` chunks on `replicas`, every replica in the ISR, attached on `target`.
fn volume_on(chunks: u64, replicas: &[&str], target: &str, epoch: u64) -> pb::Volume {
    pb::Volume {
        size_bytes: chunks * CHUNK,
        target: target.into(),
        chunks: (0..chunks)
            .map(|_| pb::ChunkMeta {
                replicas: replicas.iter().map(|s| s.to_string()).collect(),
                isr: replicas.iter().map(|s| s.to_string()).collect(),
                isr_ver: 0,
                ..Default::default()
            })
            .collect(),
        ..volume(replicas, epoch)
    }
}

/// Waits for `node`'s answer to an attach: its epoch, or why it failed.
fn attach_result(node: &Node, signer: &Ed25519KeyPair, token: Vec<u8>) -> Result<u64, String> {
    node.inputs
        .send(Input::Attach {
            volume: "v".into(),
            proof: proof(signer, token)?,
        })
        .unwrap();
    match node.wait("attach", |o| {
        matches!(o, Output::Attached { .. } | Output::AttachFailed { .. })
    }) {
        Output::Attached { epoch, .. } => Ok(epoch),
        Output::AttachFailed { why, .. } => Err(why),
        _ => unreachable!(),
    }
}

/// Registers the two nodes with the model.
fn both_registered(model: &Arc<Mutex<Model>>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while model.lock().unwrap().nodes.len() < 2 {
        assert!(Instant::now() < deadline, "nodes did not register");
        thread::sleep(Duration::from_millis(10));
    }
}

/// A volume deleted and created again with the same name, at first epoch 1000, while fl-a and
/// fl-b keep the earlier volume's replicas (nothing deleted them): it neither adopts their data
/// nor is deposed by their fences, and the earlier volume's token is refused, at the attach and
/// by fl-b (docs/issues/30, S5 to S7).
#[test]
fn a_volume_created_again_neither_adopts_nor_is_deposed() {
    let dir = std::env::temp_dir().join(format!("flaminio-again-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let signer = Ed25519KeyPair::from_seed_unchecked(&[13; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let model = Arc::new(Mutex::new(Model::default()));
    let a = start_node("fl-a", &ids["fl-a"], key.clone(), model.clone());
    let b = start_node("fl-b", &ids["fl-b"], key.clone(), model.clone());
    both_registered(&model);

    // The earlier volume, from before first epochs: epoch 2, a version 1 token.
    model.lock().unwrap().volume = Some(volume_on(2, &["fl-a", "fl-b"], "fl-a", 2));
    let old = token(&signer, "v", 2, "fl-a");
    assert_eq!(attach_result(&a, &signer, old.clone()), Ok(2));
    for (op, off) in [(1, 0), (2, CHUNK)] {
        a.io(
            op,
            Io::Write {
                offset: off,
                data: pattern(1, 64 << 10).into(),
            },
        );
    }
    assert!(b.inspect(0, 64 << 10) == pattern(1, 64 << 10));
    a.inputs.send(Input::Detach { volume: "v".into() }).unwrap();
    a.wait("detach", |o| matches!(o, Output::Detached { .. }));

    // Deleted and created again at first epoch 1000; attached at 1001. (A new geometry is
    // dropped as well: the replica service's tests.)
    model.lock().unwrap().volume = Some(volume_on(2, &["fl-a", "fl-b"], "fl-a", 1001));
    let why = attach_result(&a, &signer, old.clone()).unwrap_err();
    assert!(why.contains("epoch 2 ") && why.contains("1001"), "{why}");
    for (what, tok) in [
        (
            "another volume's",
            token_first(&signer, "w", 1001, "fl-a", 1000),
        ),
        (
            "another node's",
            token_first(&signer, "v", 1001, "fl-b", 1000),
        ),
        ("no", Vec::new()),
    ] {
        assert!(
            attach_result(&a, &signer, tok).is_err(),
            "{what} token accepted"
        );
    }
    let new = token_first(&signer, "v", 1001, "fl-a", 1000);
    // A token that does not decode: the right one with an unknown version.
    let mut garbled = new.clone();
    garbled[4] = 9;
    let why = attach_result(&a, &signer, garbled).unwrap_err();
    assert!(why.contains("invalid capability token"), "{why}");
    assert_eq!(attach_result(&a, &signer, new), Ok(1001));
    for (op, off) in [(10, 0), (11, CHUNK)] {
        let got = a
            .io(
                op,
                Io::Read {
                    offset: off,
                    len: 64 << 10,
                },
            )
            .unwrap();
        assert!(got == vec![0; 64 << 10], "the old data at {off} is gone");
    }
    a.io(
        12,
        Io::Write {
            offset: 0,
            data: pattern(2, 64 << 10).into(),
        },
    );
    let got = a
        .io(
            13,
            Io::Read {
                offset: 0,
                len: 64 << 10,
            },
        )
        .unwrap();
    assert!(got == pattern(2, 64 << 10), "read back");
    assert!(
        b.inspect(0, 64 << 10) == pattern(2, 64 << 10),
        "fl-b's replica"
    );
    assert!(
        b.inspect(CHUNK, 64 << 10) == vec![0; 64 << 10],
        "fl-b dropped the old data"
    );

    // A session to fl-b with the earlier volume's token, from a target of it on fl-a: fl-b's
    // replica is fenced at 1001 now, so it answers Stale, and that target is deposed.
    let old_model = Arc::new(Mutex::new(Model::default()));
    {
        let addr = model.lock().unwrap().nodes["fl-b"].clone();
        let mut m = old_model.lock().unwrap();
        m.nodes.insert("fl-b".into(), addr);
        m.volume = Some(volume_on(2, &["fl-a", "fl-b"], "fl-a", 2));
    }
    let (mut zombie, rx) = hand_driven(
        "fl-a",
        &ids["fl-a"],
        key,
        old_model,
        Timing::default(),
        None,
    );
    attach(&mut zombie, &rx, &signer, 2);
    zombie.handle(Input::Io {
        volume: "v".into(),
        op: 1,
        io: Io::Write {
            offset: 0,
            data: pattern(3, 4096).into(),
        },
    });
    let mut serve =
        |e: &mut Engine<MemDevice>, b: &mut [u8], lr| serve_local_read(e, b, &lr, false);
    drive(&mut zombie, &rx, &mut [], &mut serve, &|o| {
        matches!(o, Output::Deposed { .. })
    });
    let got = a
        .io(
            14,
            Io::Read {
                offset: 0,
                len: 64 << 10,
            },
        )
        .unwrap();
    assert!(got == pattern(2, 64 << 10), "the new volume is untouched");
    let _ = std::fs::remove_dir_all(&dir);
}

/// fl-b still runs a target of the earlier volume when the new one's first Hello drops its
/// replica there: that target's I/O fails, and nothing panics (docs/issues/30, R9).
#[test]
fn a_drop_under_a_target_of_the_earlier_volume_fails_its_io() {
    let dir = std::env::temp_dir().join(format!("flaminio-zombie-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let signer = Ed25519KeyPair::from_seed_unchecked(&[14; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let model = Arc::new(Mutex::new(Model::default()));
    let a = start_node("fl-a", &ids["fl-a"], key.clone(), model.clone());
    let b = start_node("fl-b", &ids["fl-b"], key, model.clone());
    both_registered(&model);

    model.lock().unwrap().volume = Some(volume_on(2, &["fl-a", "fl-b"], "fl-b", 2));
    assert_eq!(
        attach_result(&b, &signer, token(&signer, "v", 2, "fl-b")),
        Ok(2)
    );
    b.io(
        1,
        Io::Write {
            offset: 0,
            data: pattern(1, 64 << 10).into(),
        },
    );

    model.lock().unwrap().volume = Some(volume_on(2, &["fl-a", "fl-b"], "fl-a", 1001));
    let new = token_first(&signer, "v", 1001, "fl-a", 1000);
    assert_eq!(attach_result(&a, &signer, new), Ok(1001));
    a.io(
        2,
        Io::Write {
            offset: 0,
            data: pattern(2, 64 << 10).into(),
        },
    );

    b.inputs
        .send(Input::Io {
            volume: "v".into(),
            op: 3,
            io: Io::Write {
                offset: 0,
                data: pattern(3, 4096).into(),
            },
        })
        .unwrap();
    match b.wait("the old target's write", |o| {
        matches!(o, Output::Done { op: 3, .. })
    }) {
        Output::Done { result, .. } => assert!(result.is_err(), "{result:?}"),
        _ => unreachable!(),
    }
    let got = a
        .io(
            4,
            Io::Read {
                offset: 0,
                len: 64 << 10,
            },
        )
        .unwrap();
    assert!(got == pattern(2, 64 << 10), "the new volume is untouched");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An engine with asynchronous commits on a deferred device (commits complete on command), and
/// volume "v" (16 MiB) attached, for the tests of held groups (docs/issues/10). Built with the
/// engine's defaults: the 8 MiB floor, so groups are held.
struct Holding {
    engine: Engine<flaminio_store::DeferredDevice>,
    rx: mpsc::Receiver<Input>,
    dir: std::path::PathBuf,
    buf: Vec<u8>,
    arrivals: Arc<std::sync::atomic::AtomicUsize>,
    model: Arc<Mutex<Model>>,
}

impl Holding {
    fn new(name: &str) -> Self {
        use flaminio_store::DeferredDevice;
        let dir = std::env::temp_dir().join(format!("flaminio-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ids = identities(&dir);
        let files = &ids["fl-a"];
        let model = Arc::new(Mutex::new(Model::default()));
        let listener = TcpListener::bind((LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel::<Input>();
        let sink_tx = Mutex::new(tx.clone());
        let signer = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
        let clock = clock();
        let net = net::spawn(
            NetConfig {
                clock: clock.clone(),
                trust_domain: TD.into(),
                server: Some(ServerSide {
                    listener,
                    tls: files.server_config().unwrap(),
                    token_key: TokenKey::from_raw(signer.public_key().as_ref()).unwrap(),
                }),
                client: Some(files.client_config().unwrap()),
            },
            Arc::new(move |e| {
                let _ = sink_tx.lock().unwrap().send(Input::Net(e));
            }),
        )
        .unwrap();
        let cfg = EngineConfig {
            node: "fl-a".into(),
            address: addr.to_string(),
            timing: quiet(),
            max_io: 64,
            heartbeat: None,
        };
        let meta = Box::new(FakeMeta {
            model: model.clone(),
            inbox: tx.clone(),
        });
        let dev = store().into_device();
        let deferred = Store::open(DeferredDevice::new(dev), StoreConfig::default()).unwrap();
        let mut engine = Engine::new(cfg, ReplicaService::new(deferred), net, meta, clock, NONCE);
        engine.set_async_commits(true);
        let arrivals = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        engine.set_arrivals(arrivals.clone());
        model.lock().unwrap().volume = Some(pb::Volume {
            id: "v".into(),
            size_bytes: 4 * CHUNK,
            chunk_bytes: CHUNK,
            replicas: 1,
            min_isr: 1,
            epoch: 2,
            target: "fl-a".into(),
            chunks: (0..4)
                .map(|_| pb::ChunkMeta {
                    replicas: vec!["fl-a".into()],
                    isr: vec!["fl-a".into()],
                    isr_ver: 0,
                    ..Default::default()
                })
                .collect(),
        });
        let mut h = Holding {
            engine,
            rx,
            dir,
            buf: vec![0u8; MIB as usize],
            arrivals,
            model,
        };
        h.engine.handle(Input::Attach {
            volume: "v".into(),
            proof: proof(&signer, token(&signer, "v", 2, "fl-a")).unwrap(),
        });
        h.run(20_000, &|o| matches!(o, Output::Attached { .. }));
        h
    }

    /// Drives the engine for `turns` turns of about 1 ms, or until an output matches `want`,
    /// playing the frontend for local reads. Returns the outputs that matched.
    fn run(&mut self, turns: usize, want: &dyn Fn(&Output) -> bool) -> Vec<Output> {
        use flaminio_store::BlockDevice;
        let mut got = Vec::new();
        for _ in 0..turns {
            let _now = Instant::now();
            while let Ok(i) = self.rx.try_recv() {
                self.engine.handle(i);
            }
            self.engine.turn();
            for o in self.engine.take_output() {
                if let Output::LocalRead(lr) = &o {
                    let piece = &mut self.buf[lr.at..lr.at + lr.len];
                    let plan = self.engine.plan_local_read(lr, piece).unwrap();
                    for r in plan.reads() {
                        let (o, n) = (r.buf_offset, r.len);
                        self.engine
                            .device()
                            .read_at(r.device_offset, &mut piece[o..o + n])
                            .unwrap();
                    }
                    let ok = plan.verify(&self.buf[lr.at..lr.at + lr.len]).is_ok();
                    self.engine.local_read_done(lr, Some(plan), ok);
                    continue;
                }
                if want(&o) {
                    got.push(o);
                }
            }
            if !got.is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        got
    }

    fn write(&mut self, op: u64, offset: u64, len: u64) {
        self.engine.handle(Input::Io {
            volume: "v".into(),
            op,
            io: Io::Write {
                offset,
                data: pattern(op as u8, len).into(),
            },
        });
    }

    /// Drives the engine until every write of `ops` is acknowledged, completing commits as they
    /// come.
    fn acked(&mut self, ops: std::ops::RangeInclusive<u64>) {
        let mut n = 0;
        let want = ops.clone().count();
        for _ in 0..20_000 {
            if self.engine.device().in_flight() {
                self.engine.device().complete();
            }
            n += self
                .run(
                    1,
                    &|o| matches!(o, Output::Done { op, result: Ok(_), .. } if ops.contains(op)),
                )
                .len();
            if n == want {
                return;
            }
        }
        panic!("writes {ops:?} not acknowledged ({n} of {want})");
    }

    /// Turns the engine for `turns` turns without completing anything.
    fn idle(&mut self, turns: usize) {
        self.run(turns, &|_| false);
    }

    fn in_flight(&self) -> bool {
        self.engine.device().in_flight()
    }

    fn reads_back(&mut self, op: u64, offset: u64, len: u64) {
        self.engine.handle(Input::Io {
            volume: "v".into(),
            op: 1000 + op,
            io: Io::ReadDirect { offset, len },
        });
        let done = self.run(
            20_000,
            &|o| matches!(o, Output::Done { op: d, .. } if *d == 1000 + op),
        );
        assert_eq!(done.len(), 1, "read of write {op}");
        assert!(
            self.buf[..len as usize] == pattern(op as u8, len),
            "write {op} reads back"
        );
    }
}

impl Drop for Holding {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Held groups (docs/issues/10): what a default-built engine does with large writes, and every path
/// that settles the store with a group held.
#[test]
fn holding_staged_groups() {
    use std::sync::atomic::Ordering;
    let mut h = Holding::new("holding");
    assert!(h.engine.holding(), "the default floor holds groups");
    h.engine.set_commit_gather(Duration::from_millis(1)); // 2 ms for large writes

    // A stream of four 1 MiB writes at a time: each round's group is staged, its data written
    // at once, and starts by the count rule, not the 10 s window.
    for round in 0..3u64 {
        let base = 10 + round * 4;
        for k in 0..4 {
            h.write(base + k, k * MIB, MIB);
        }
        h.idle(5);
        assert!(
            h.in_flight(),
            "round {round}: started without waiting for the window"
        );
        assert_eq!(
            h.engine.device().pending_writes(),
            1,
            "round {round}: the batch only, the data written as it was staged"
        );
        h.acked(base..=base + 3);
    }
    h.reads_back(19, MIB, MIB); // the last round: writes 18-21 at 0-3 MiB

    // At depth 1: after a commit of one write, the next starts at once (the count target is 1),
    // without waiting for the window. Write 29 waits for the four of the last landing, the
    // window, then starts alone; its landing arms a wait of 10 s for one write.
    h.write(29, 4 * MIB, MIB);
    while !h.in_flight() {
        h.idle(1);
    }
    h.engine.set_commit_gather(Duration::from_secs(10));
    h.acked(29..=29);
    for op in 30..34u64 {
        h.write(op, 4 * MIB, MIB);
        h.idle(3);
        assert!(
            h.in_flight(),
            "write {op} started without waiting for the window"
        );
        h.acked(op..=op);
    }

    // The two halves of a 1 MiB write join one commit: the second is still being copied in when
    // the first reaches the engine, from a wait whose target is 1.
    h.arrivals.store(1, Ordering::Relaxed);
    h.write(40, 5 * MIB, MIB / 2);
    h.idle(5);
    assert!(
        !h.in_flight(),
        "the first half waits for its partner being copied in"
    );
    h.arrivals.store(0, Ordering::Relaxed);
    h.write(41, 5 * MIB + MIB / 2, MIB / 2);
    h.idle(3);
    assert!(h.in_flight());
    assert_eq!(
        h.engine.device().commits_in_flight(),
        1,
        "one commit for both halves"
    );
    h.acked(40..=41);

    // Past its window, a wait whose writes are not all back goes on while writes are being
    // copied in, until they are, and at most 8 ms more.
    h.engine.set_commit_gather(Duration::from_millis(1)); // 2 ms for large writes
    h.write(42, 6 * MIB, MIB);
    h.write(43, 7 * MIB, MIB);
    h.idle(3);
    assert!(h.in_flight(), "two writes, by the count rule");
    let data = pattern(44, MIB).into(); // built before the landing: it takes milliseconds
    h.engine.take_gather_timer(); // an earlier wait's
    h.acked(42..=43); // a landing of two: the next wait's target is 2
    // The landing armed the wait, and the harness takes no timer: this is the window's end.
    let until = h
        .engine
        .take_gather_timer()
        .expect("the landing arms a wait");
    h.arrivals.store(1, Ordering::Relaxed);
    h.engine.handle(Input::Io {
        volume: "v".into(),
        op: 44,
        io: Io::Write {
            offset: 6 * MIB,
            data,
        },
    });
    // Turns as `run` does, at instants of our own, until one is past the window: the hold ends
    // 8 ms after the window by the clock, not by turns, and a slow build (the sanitizers) can be
    // past that already, where the turn shows nothing.
    loop {
        let now = Instant::now();
        while let Ok(i) = h.rx.try_recv() {
            h.engine.handle(i);
        }
        h.engine.turn();
        if now >= until {
            if now < until + Duration::from_millis(8) {
                assert!(
                    !h.in_flight(),
                    "held past the 2 ms window while a write is being copied in"
                );
            }
            break;
        }
        thread::sleep(Duration::from_micros(200));
    }
    h.arrivals.store(0, Ordering::Relaxed);
    h.idle(3);
    assert!(h.in_flight(), "started once nothing is being copied in");
    h.acked(44..=44);
    h.write(45, 7 * MIB, MIB);
    h.idle(3);
    h.acked(45..=45);
    h.write(46, 6 * MIB, MIB);
    h.write(47, 7 * MIB, MIB);
    h.idle(3);
    h.acked(46..=47);
    h.arrivals.store(1, Ordering::Relaxed);
    let t = Instant::now();
    h.write(48, 6 * MIB, MIB);
    while !h.in_flight() {
        h.idle(1);
        assert!(
            t.elapsed() < Duration::from_secs(1),
            "the limit ends the wait"
        );
    }
    assert!(
        t.elapsed() >= Duration::from_millis(9),
        "not before the window and the limit ({:?})",
        t.elapsed()
    );
    h.acked(48..=48);
    // A small-write wait never holds for writes being copied in.
    h.write(49, 8 * MIB, 4096);
    h.idle(3);
    h.acked(49..=49);
    h.write(50, 8 * MIB + 4096, 4096);
    h.idle(3);
    assert!(h.in_flight(), "4K started without waiting on arrivals");
    h.acked(50..=50);
    h.arrivals.store(0, Ordering::Relaxed);

    // A small write queued behind a held group starts it, as the group can take no more writes
    // in order; the small write commits after it.
    h.engine.set_commit_gather(Duration::from_secs(10));
    h.write(51, 9 * MIB, MIB);
    h.write(52, 10 * MIB, MIB);
    h.idle(3);
    h.acked(51..=52); // the next wait's target is 2
    h.write(53, 9 * MIB, MIB);
    h.idle(3);
    assert!(!h.in_flight(), "the group waits for its second write");
    // Elsewhere: a write overlapping 53 would wait in the target for 53 to complete.
    h.write(54, 13 * MIB, 4096);
    h.idle(3);
    assert!(h.in_flight(), "the 4K behind the group started it");
    assert_eq!(
        h.engine.device().commits_in_flight(),
        1,
        "the group's commit; the 4K's comes after it"
    );
    // At the group's landing the 4K starts, with no wait (the window here is 10 s, and a
    // session's request times out after 3 s).
    let t = Instant::now();
    h.acked(53..=53);
    h.idle(3);
    assert!(
        h.in_flight(),
        "the 4K's commit started at the group's landing"
    );
    h.acked(54..=54);
    assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
    h.reads_back(53, 9 * MIB, MIB);
    h.reads_back(54, 13 * MIB, 4096);

    // Every path that settles the store, with a group held (its wait 10 s, its count not met),
    // starts it and returns.
    let hold = |h: &mut Holding, op: u64| {
        h.write(op, 11 * MIB, MIB);
        h.write(op + 1, 12 * MIB, MIB);
        h.idle(3);
        h.acked(op..=op + 1); // target 2 next
        h.write(op + 2, 11 * MIB, MIB);
        h.idle(3);
        assert!(!h.in_flight(), "write {}: held", op + 2);
    };
    // Inspecting the local replica.
    hold(&mut h, 60);
    let (tx, rx) = mpsc::channel();
    h.engine.handle(Input::InspectLocal {
        volume: "v".into(),
        offset: 11 * MIB,
        len: 4096,
        reply: tx,
    });
    let got = rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .expect("inspected");
    assert_eq!(
        got[..],
        pattern(62, MIB)[..4096],
        "the held group is durable first"
    );
    h.acked(62..=62);
    // Deleting another volume's replicas.
    hold(&mut h, 70);
    let (tx, rx) = mpsc::channel();
    h.engine.handle(Input::DeleteReplicas {
        volume: "w".into(),
        reply: tx,
    });
    rx.recv_timeout(Duration::from_secs(5))
        .unwrap()
        .expect("deleted");
    h.acked(72..=72);
    // A peer opening a session (its Hello fences the store).
    hold(&mut h, 80);
    let (replies, peer) = smol::channel::unbounded();
    h.engine.handle(Input::Net(net::NetEvent::Opened {
        conn: 99,
        accepted: Box::new(flaminio_net::session::Accepted {
            peer_node: "fl-b".into(),
            req: 1,
            epoch: 2,
            session: 7,
            claims: flaminio_net::token::Claims {
                volume: "w".into(),
                epoch: 2,
                node: "fl-b".into(),
                issued_at: 0,
                first_epoch: 0,
            },
            size: CHUNK,
            chunk_size: CHUNK,
            chunks: vec![0],
        }),
        replies,
    }));
    assert!(peer.try_recv().is_ok(), "the Hello was answered");
    h.acked(82..=82);
    // Turning asynchronous commits off (detach), with a commit in flight, a group staged
    // behind it, and a request queued behind the group.
    h.write(90, 11 * MIB, MIB);
    h.write(91, 12 * MIB, MIB);
    h.idle(3);
    assert!(h.in_flight(), "two, by the count rule");
    h.write(92, 13 * MIB, MIB); // staged behind the commit in flight
    h.write(93, 14 * MIB + MIB / 2, 4096); // queued behind the group
    h.idle(3);
    assert_eq!(h.engine.device().commits_in_flight(), 1);
    h.engine.set_async_commits(false);
    let mut answered = Vec::new();
    for _ in 0..100 {
        for o in h.run(1, &|o| matches!(o, Output::Done { op: 90..=93, .. })) {
            if let Output::Done { op, result, .. } = o {
                assert!(result.is_ok(), "write {op}: {result:?}");
                answered.push(op);
            }
        }
        if answered.len() == 4 {
            break;
        }
    }
    answered.sort();
    assert_eq!(
        answered,
        [90, 91, 92, 93],
        "every write answered, none refused"
    );
    h.reads_back(92, 13 * MIB, MIB);
    h.reads_back(93, 14 * MIB + MIB / 2, 4096);
}

/// The attach path's Hello (a target opening its local replica's session) fences the store,
/// which settles it: a held group starts first, and its writes are answered once durable, not
/// before and not stranded (docs/issues/10, S5).
#[test]
fn an_attach_settles_a_held_group() {
    let mut h = Holding::new("holding-attach");
    h.engine.set_commit_gather(Duration::from_millis(1));
    h.write(1, 0, MIB);
    while !h.in_flight() {
        h.idle(1);
    }
    h.acked(1..=1); // a landing of one: the next wait's target is 1
    h.engine.set_commit_gather(Duration::from_secs(10));
    h.write(2, 0, MIB);
    h.write(3, MIB, MIB);
    h.idle(3);
    h.acked(2..=3); // target 2 next
    h.write(4, 0, MIB);
    h.idle(3);
    assert!(!h.in_flight(), "write 4: held");

    // Attaching a second volume: its target says Hello to the local replica.
    {
        let mut m = h.model.lock().unwrap();
        let mut w = m.volume.clone().unwrap();
        w.id = "w".into();
        m.volume = Some(w);
    }
    let signer = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    h.engine.handle(Input::Attach {
        volume: "w".into(),
        proof: proof(&signer, token(&signer, "w", 2, "fl-a")).unwrap(),
    });
    // The test completes no commit: write 4 is durable only if the Hello's settle waited for it.
    let (mut attached, mut acked) = (false, false);
    for _ in 0..5_000 {
        let got = h.run(1, &|o| {
            matches!(o, Output::Attached { .. } | Output::Done { op: 4, .. })
        });
        for o in got {
            match o {
                Output::Attached { volume, .. } => {
                    assert_eq!(volume, "w");
                    attached = true;
                }
                Output::Done { result, .. } => {
                    assert!(result.is_ok(), "write 4: {result:?}");
                    assert!(!h.in_flight(), "write 4 answered once durable");
                    acked = true;
                }
                _ => {}
            }
        }
        if attached && acked {
            break;
        }
    }
    assert!(attached, "w attached");
    assert!(acked, "the held write answered, not stranded");
    h.reads_back(4, 0, MIB);
    h.write(5, MIB, MIB);
    h.acked(5..=5);
    h.reads_back(5, MIB, MIB);
}

/// Sparse memory devices keep recovery cases small with the minimum supported 1 MiB chunks.
const SMALL_CHUNK: u64 = MIB;
// Slot capacity is based on logical device size (the256MiB default has only64 slots).
// MemDevice is sparse:8GiB supplies enough slots without allocating unwritten data blocks.
const LARGE_STORE: u64 = 8 << 30;

fn small_volume(chunks: u64) -> pb::Volume {
    pb::Volume {
        size_bytes: chunks * SMALL_CHUNK,
        chunk_bytes: SMALL_CHUNK,
        ..volume_on(chunks, &["fl-a", "fl-b"], "fl-a", 2)
    }
}

fn wait_model(model: &Arc<Mutex<Model>>, what: &str, ready: impl Fn(&Model) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if ready(&model.lock().unwrap()) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn large_volume_recovers_with_production_batch_limit() {
    let dir = std::env::temp_dir().join(format!("flaminio-large-batches-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let signer = Ed25519KeyPair::from_seed_unchecked(&[81; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let model = Arc::new(Mutex::new(Model::default()));
    let a = start_node_with_store(
        "fl-a",
        &ids["fl-a"],
        key.clone(),
        model.clone(),
        store_of_size(LARGE_STORE),
    );
    let b = start_node_with_store(
        "fl-b",
        &ids["fl-b"],
        key,
        model.clone(),
        store_of_size(LARGE_STORE),
    );
    both_registered(&model);
    {
        let mut m = model.lock().unwrap();
        m.volume = Some(small_volume(129));
        m.hold_isr_replies = true;
    }
    b.inputs
        .send(Input::Deafen {
            until: Instant::now() + Duration::from_secs(60),
        })
        .unwrap();
    assert_eq!(
        attach_result(&a, &signer, token(&signer, "v", 2, "fl-a")),
        Ok(2)
    );
    wait_model(&model, "the first bounded shrink", |m| {
        !m.held_isr.is_empty()
    });
    {
        let m = model.lock().unwrap();
        assert_eq!(m.batches[0].changes.len(), MAX_CHANGES_PER_REQUEST);
        assert_eq!(m.volume.as_ref().unwrap().chunks[128].isr.len(), 2);
        a.inputs
            .send(Input::Io {
                volume: "v".into(),
                op: 81,
                io: Io::Write {
                    offset: 128 * SMALL_CHUNK,
                    data: vec![81; 4096].into(),
                },
            })
            .unwrap();
        // Metadata cannot commit another batch while this lock is held. A write to the
        // deferred tail chunk must not inherit the first batch's committed shrink.
        thread::sleep(Duration::from_millis(20));
        while let Ok(o) = a.outputs.try_recv() {
            assert!(
                !matches!(o, Output::Done { op: 81, .. }),
                "deferred write answered early"
            );
        }
    }
    let held = {
        let mut m = model.lock().unwrap();
        m.hold_isr_replies = false;
        std::mem::take(&mut m.held_isr)
    };
    for (inbox, id, answer) in held {
        inbox.send(Input::Meta { id, answer }).unwrap();
    }
    match a.wait("the deferred write after its own chunk commits", |o| {
        matches!(o, Output::Done { op: 81, .. })
    }) {
        Output::Done { result, .. } => {
            result.unwrap();
        }
        _ => unreachable!(),
    }
    wait_model(&model, "all129 shrinks", |m| {
        m.volume
            .as_ref()
            .unwrap()
            .chunks
            .iter()
            .all(|c| c.isr == ["fl-a"])
    });
    {
        let m = model.lock().unwrap();
        let changed: BTreeSet<_> = m
            .batches
            .iter()
            .flat_map(|r| r.changes.iter())
            .map(|c| c.chunk)
            .collect();
        assert_eq!(changed.len(), 129);
        assert!(m.batches.len() >= 3);
        assert!(
            m.batches
                .iter()
                .all(|r| (1..=MAX_CHANGES_PER_REQUEST).contains(&r.changes.len()))
        );
    }
    b.inputs
        .send(Input::Deafen {
            until: Instant::now(),
        })
        .unwrap();
    wait_model(&model, "all129 rejoins", |m| {
        m.volume
            .as_ref()
            .unwrap()
            .chunks
            .iter()
            .all(|c| c.isr.len() == 2 && c.isr_ver >= 2)
    });
    assert_eq!(b.inspect(128 * SMALL_CHUNK, 4096), vec![81; 4096]);
    assert_eq!(
        a.io(
            82,
            Io::Read {
                offset: 128 * SMALL_CHUNK,
                len: 4096
            }
        ),
        Some(vec![81; 4096])
    );
    {
        let m = model.lock().unwrap();
        assert!(m.batches.iter().all(|r| {
            r.changes.len() <= MAX_CHANGES_PER_REQUEST
                && r.changes
                    .iter()
                    .map(|c| c.chunk)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == r.changes.len()
        }));
        let joined: BTreeSet<_> = m
            .batches
            .iter()
            .flat_map(|r| r.changes.iter())
            .filter(|c| c.new_isr.len() == 2)
            .map(|c| c.chunk)
            .collect();
        assert_eq!(joined.len(), 129);
    }
    drop((a, b));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bounded_batches_complete_mixed_moves_and_preserve_data() {
    let dir = std::env::temp_dir().join(format!("flaminio-mixed-batches-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let signer = Ed25519KeyPair::from_seed_unchecked(&[82; 32]).unwrap();
    let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
    let model = Arc::new(Mutex::new(Model::default()));
    let a = start_node_with_store(
        "fl-a",
        &ids["fl-a"],
        key.clone(),
        model.clone(),
        store_of_size(LARGE_STORE),
    );
    let b = start_node_with_store(
        "fl-b",
        &ids["fl-b"],
        key.clone(),
        model.clone(),
        store_of_size(LARGE_STORE),
    );
    let c = start_node_with_store(
        "fl-c",
        &ids["fl-c"],
        key,
        model.clone(),
        store_of_size(LARGE_STORE),
    );
    wait_model(&model, "three registered nodes", |m| m.nodes.len() == 3);
    model.lock().unwrap().volume = Some(small_volume(129));
    assert_eq!(
        attach_result(&a, &signer, token(&signer, "v", 2, "fl-a")),
        Ok(2)
    );
    for (op, chunk) in [0, 1, 127].into_iter().enumerate() {
        a.io(
            100 + op as u64,
            Io::Write {
                offset: chunk * SMALL_CHUNK,
                data: vec![chunk as u8 + 1; 4096].into(),
            },
        );
    }
    {
        let mut m = model.lock().unwrap();
        for (i, ch) in m.volume.as_mut().unwrap().chunks.iter_mut().enumerate() {
            if i % 3 != 2 {
                ch.moving_from = if i % 3 == 0 { "fl-b" } else { "fl-a" }.into();
                ch.moving_to = "fl-c".into();
            }
        }
    }
    wait_model(&model, "mixed moves to finish", |m| {
        m.volume
            .as_ref()
            .unwrap()
            .chunks
            .iter()
            .enumerate()
            .all(|(i, ch)| {
                let expected: Vec<_> = match i % 3 {
                    0 => vec!["fl-a", "fl-c"],
                    1 => vec!["fl-b", "fl-c"],
                    _ => vec!["fl-a", "fl-b"],
                };
                ch.moving_from.is_empty()
                    && ch.moving_to.is_empty()
                    && ch
                        .replicas
                        .iter()
                        .map(String::as_str)
                        .collect::<BTreeSet<_>>()
                        == expected.iter().copied().collect()
                    && ch.isr.iter().map(String::as_str).collect::<BTreeSet<_>>()
                        == expected.into_iter().collect()
            })
    });
    for (op, chunk) in [0, 1, 127].into_iter().enumerate() {
        let expected = vec![chunk as u8 + 1; 4096];
        assert_eq!(c.inspect(chunk * SMALL_CHUNK, 4096), expected);
        assert_eq!(
            a.io(
                200 + op as u64,
                Io::Read {
                    offset: chunk * SMALL_CHUNK,
                    len: 4096
                }
            ),
            Some(expected)
        );
    }
    {
        let m = model.lock().unwrap();
        assert!(m.batches.iter().all(|r| {
            (1..=MAX_CHANGES_PER_REQUEST).contains(&r.changes.len())
                && r.changes
                    .iter()
                    .all(|ch| ch.new_isr.len() >= m.volume.as_ref().unwrap().min_isr as usize)
        }));
        let changes: Vec<_> = m.batches.iter().flat_map(|r| &r.changes).collect();
        assert!(
            changes
                .iter()
                .any(|ch| ch.change_replicas && ch.new_replicas.len() == 3),
            "destinations added"
        );
        assert!(
            changes
                .iter()
                .any(|ch| !ch.change_replicas && ch.new_isr.len() == 3),
            "synchronized destinations joined"
        );
        assert!(
            changes
                .iter()
                .any(|ch| ch.change_replicas && ch.new_replicas.len() == 2),
            "sources removed"
        );
    }
    drop((a, b, c));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fake_meta_atomically_refuses_unsupported_batches() {
    let mut volume = small_volume(65);
    for (i, ch) in volume.chunks.iter_mut().enumerate() {
        ch.isr_ver = i as u64;
    }
    let model = Arc::new(Mutex::new(Model {
        volume: Some(volume),
        ..Default::default()
    }));
    let (inbox, replies) = mpsc::channel();
    let meta = FakeMeta {
        model: model.clone(),
        inbox,
    };
    let changes: Vec<_> = (0..65)
        .map(|chunk| pb::IsrChange {
            chunk,
            expected_ver: chunk as u64,
            new_isr: vec!["fl-a".into()],
            ..Default::default()
        })
        .collect();
    let original = model.lock().unwrap().volume.clone().unwrap();
    meta.call(
        1,
        Call::ChangeIsr(pb::ChangeIsrRequest {
            volume: "v".into(),
            epoch: 2,
            changes: changes.clone(),
        }),
    );
    assert!(matches!(
        replies.recv().unwrap(),
        Input::Meta {
            answer: Answer::Failed {
                code: flaminio_metaclient::Code::InvalidArgument,
                ..
            },
            ..
        }
    ));
    assert_eq!(model.lock().unwrap().volume.as_ref().unwrap(), &original);
    let mut wrong = changes[..64].to_vec();
    wrong[63].expected_ver = 99;
    meta.call(
        2,
        Call::ChangeIsr(pb::ChangeIsrRequest {
            volume: "v".into(),
            epoch: 2,
            changes: wrong,
        }),
    );
    assert!(matches!(
        replies.recv().unwrap(),
        Input::Meta {
            answer: Answer::ChangeIsr(pb::ChangeIsrResponse { applied: false, .. }),
            ..
        }
    ));
    assert_eq!(model.lock().unwrap().volume.as_ref().unwrap(), &original);
    let mut duplicate = changes[..64].to_vec();
    duplicate[63] = duplicate[0].clone();
    let expected_chunks: Vec<_> = duplicate
        .iter()
        .map(|c| original.chunks[c.chunk as usize].clone())
        .collect();
    for (id, epoch) in [(4, original.epoch), (6, original.epoch - 1)] {
        meta.call(
            id,
            Call::ChangeIsr(pb::ChangeIsrRequest {
                volume: "v".into(),
                epoch,
                changes: duplicate.clone(),
            }),
        );
        let Input::Meta {
            id: reply_id,
            answer: Answer::ChangeIsr(answer),
        } = replies.recv().unwrap()
        else {
            panic!("duplicate chunks must receive an authoritative refusal")
        };
        assert_eq!(reply_id, id);
        assert!(!answer.applied);
        assert_eq!(answer.epoch, original.epoch);
        assert_eq!(answer.chunks, expected_chunks);
        assert_eq!(model.lock().unwrap().volume.as_ref().unwrap(), &original);
    }
    let mut out_of_range = changes[..64].to_vec();
    out_of_range[63].chunk = original.chunks.len() as u32;
    meta.call(
        7,
        Call::ChangeIsr(pb::ChangeIsrRequest {
            volume: "v".into(),
            epoch: 2,
            changes: out_of_range,
        }),
    );
    assert!(matches!(
        replies.recv().unwrap(),
        Input::Meta {
            answer: Answer::Failed {
                code: flaminio_metaclient::Code::InvalidArgument,
                ..
            },
            ..
        }
    ));
    assert_eq!(model.lock().unwrap().volume.as_ref().unwrap(), &original);
    let mut outside = changes[..64].to_vec();
    outside[63].new_isr = vec!["fl-c".into()];
    meta.call(
        5,
        Call::ChangeIsr(pb::ChangeIsrRequest {
            volume: "v".into(),
            epoch: 2,
            changes: outside,
        }),
    );
    assert!(matches!(
        replies.recv().unwrap(),
        Input::Meta {
            answer: Answer::ChangeIsr(pb::ChangeIsrResponse { applied: false, .. }),
            ..
        }
    ));
    assert_eq!(model.lock().unwrap().volume.as_ref().unwrap(), &original);
    let mut ordered = changes[..64].to_vec();
    ordered.reverse();
    meta.call(
        3,
        Call::ChangeIsr(pb::ChangeIsrRequest {
            volume: "v".into(),
            epoch: 2,
            changes: ordered,
        }),
    );
    let Input::Meta {
        answer: Answer::ChangeIsr(answer),
        ..
    } = replies.recv().unwrap()
    else {
        panic!("boundary refused")
    };
    assert!(answer.applied);
    assert_eq!(answer.chunks.len(), MAX_CHANGES_PER_REQUEST);
    assert!(
        answer
            .chunks
            .iter()
            .enumerate()
            .all(|(i, ch)| ch.isr == ["fl-a"] && ch.isr_ver == 64 - i as u64)
    );
    assert_eq!(
        model.lock().unwrap().volume.as_ref().unwrap().chunks[64],
        original.chunks[64]
    );
}
