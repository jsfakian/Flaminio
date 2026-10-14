//! Two nodes, each a full engine (store, TLS network thread, engine thread), and a model of
//! the metadata service: a replicated volume end to end, through a replica failure and its
//! resync.

use std::collections::BTreeMap;
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
use flaminio_repl::types::Timing;
use flaminio_store::{MemDevice, Store, StoreConfig};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, SanType,
};
use ring::signature::{Ed25519KeyPair, KeyPair as _};

const TD: &str = "lab";
const MIB: u64 = 1 << 20;
const CHUNK: u64 = 4 * MIB;
const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// The metadata service, as a model with the same rules for the calls the engine makes.
#[derive(Default)]
struct Model {
    nodes: BTreeMap<String, String>,
    volume: Option<pb::Volume>,
}

struct FakeMeta {
    model: Arc<Mutex<Model>>,
    inbox: mpsc::Sender<Input>,
}

impl MetaPort for FakeMeta {
    fn call(&self, id: u64, call: Call) {
        let mut m = self.model.lock().unwrap();
        let answer = match call {
            Call::RegisterNode(r) => {
                let n = r.node.unwrap();
                m.nodes.insert(n.id, n.address);
                Answer::RegisterNode(pb::RegisterNodeResponse {})
            }
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
                let v = m.volume.as_mut().unwrap();
                let ok = r.epoch == v.epoch
                    && r.changes.iter().all(|c| {
                        let ch = &v.chunks[c.chunk as usize];
                        ch.isr_ver == c.expected_ver
                            && c.new_isr.len() as u32 >= v.min_isr
                            && c.new_isr.iter().all(|n| ch.replicas.contains(n))
                    });
                if ok {
                    for c in &r.changes {
                        let ch = &mut v.chunks[c.chunk as usize];
                        let mut isr = c.new_isr.clone();
                        isr.sort();
                        isr.dedup();
                        ch.isr = isr;
                        ch.isr_ver += 1;
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
        let _ = self.inbox.send(Input::Meta { id, answer });
    }
}

fn identities(dir: &Path) -> BTreeMap<&'static str, Files> {
    let mut ca = CertificateParams::new(vec![]).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_key = KeyPair::generate().unwrap();
    std::fs::write(dir.join("ca.pem"), ca.self_signed(&ca_key).unwrap().pem()).unwrap();
    ["fl-a", "fl-b"]
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
    let mut b = b"FLCT\x01".to_vec();
    b.extend_from_slice(&(volume.len() as u16).to_be_bytes());
    b.extend_from_slice(volume.as_bytes());
    b.extend_from_slice(&epoch.to_be_bytes());
    b.extend_from_slice(&(node.len() as u16).to_be_bytes());
    b.extend_from_slice(node.as_bytes());
    b.extend_from_slice(&0i64.to_be_bytes());
    let sig = key.sign(&b);
    b.extend_from_slice(sig.as_ref());
    b
}

fn store() -> Store<MemDevice> {
    let dev = MemDevice::new(256 * MIB);
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

struct Node {
    inputs: mpsc::Sender<Input>,
    outputs: mpsc::Receiver<Output>,
}

fn start_node(name: &str, files: &Files, token_key: TokenKey, model: Arc<Mutex<Model>>) -> Node {
    let listener = TcpListener::bind((LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel::<Input>();
    let sink_tx = Mutex::new(tx.clone());
    let net = net::spawn(
        NetConfig {
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
    let engine = Engine::new(cfg, ReplicaService::new(store()), net, meta);
    let (otx, orx) = mpsc::channel();
    thread::spawn(move || {
        engine.run(rx, move |o| {
            let _ = otx.send(o);
        });
    });
    Node {
        inputs: tx,
        outputs: orx,
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
            epoch: 1,
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
            token: token(&signer, "v", 1, "fl-a"),
        })
        .unwrap();
    a.wait("attach", |o| matches!(o, Output::Attached { epoch: 1, .. }));

    // Writes, one across the chunk boundary, then read them back on the target, and from the
    // other node's own replica.
    let writes = [(0, 64 << 10, 1u8), (CHUNK - 4096, 8192, 2), (MIB, MIB, 3)];
    for (i, &(off, len, seed)) in writes.iter().enumerate() {
        a.io(
            i as u64,
            Io::Write {
                offset: off,
                data: pattern(seed, len),
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
            data: pattern(4, 64 << 10),
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

/// One node, its engine driven by hand, with a single-replica volume: direct reads, as the
/// ublk frontend performs them, planned by the engine and read from the store's device into
/// the caller's buffer.
#[test]
fn direct_reads_from_the_local_replica() {
    use flaminio_node::engine::LocalRead;
    use flaminio_store::BlockDevice;

    let dir = std::env::temp_dir().join(format!("flaminio-direct-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ids = identities(&dir);
    let files = &ids["fl-a"];
    let model = Arc::new(Mutex::new(Model::default()));
    let listener = TcpListener::bind((LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel::<Input>();
    let sink_tx = Mutex::new(tx.clone());
    let signer = Ed25519KeyPair::from_seed_unchecked(&[6; 32]).unwrap();
    let net = net::spawn(
        NetConfig {
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
        timing: Timing::default(),
        max_io: 64,
        heartbeat: None,
    };
    let meta = Box::new(FakeMeta {
        model: model.clone(),
        inbox: tx.clone(),
    });
    let mut engine = Engine::new(cfg, ReplicaService::new(store()), net, meta);
    model.lock().unwrap().volume = Some(pb::Volume {
        id: "v".into(),
        size_bytes: 2 * CHUNK,
        chunk_bytes: CHUNK,
        replicas: 1,
        min_isr: 1,
        epoch: 1,
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
            let now = Instant::now();
            while let Ok(i) = rx.try_recv() {
                engine.handle(now, i);
            }
            engine.turn(now);
            for o in engine.take_output() {
                if let Output::LocalRead(lr) = &o {
                    let lr: LocalRead = lr.clone();
                    let piece = &mut buf[lr.at..lr.at + lr.len];
                    let plan = engine.plan_local_read(&lr, piece).unwrap();
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
                    let ok = plan.verify(&buf[lr.at..lr.at + lr.len]).is_ok();
                    engine.local_read_done(Instant::now(), &lr, Some(plan), ok);
                    continue;
                }
                if want(&o) {
                    return o;
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
    };

    engine.handle(
        Instant::now(),
        Input::Attach {
            volume: "v".into(),
            token: token(&signer, "v", 1, "fl-a"),
        },
    );
    run(&mut engine, &mut buf, &|o| {
        matches!(o, Output::Attached { .. })
    });
    let off = CHUNK - MIB; // across the chunk boundary
    engine.handle(
        Instant::now(),
        Input::Io {
            volume: "v".into(),
            op: 1,
            io: Io::Write {
                offset: off,
                data: pattern(9, 2 * MIB),
            },
        },
    );
    let w = run(&mut engine, &mut buf, &|o| {
        matches!(o, Output::Done { op: 1, .. })
    });
    assert_eq!(
        w,
        Output::Done {
            op: 1,
            result: Ok(None)
        }
    );
    engine.handle(
        Instant::now(),
        Input::Io {
            volume: "v".into(),
            op: 2,
            io: Io::ReadDirect {
                offset: off,
                len: 2 * MIB,
            },
        },
    );
    let r = run(&mut engine, &mut buf, &|o| {
        matches!(o, Output::Done { op: 2, .. })
    });
    assert_eq!(
        r,
        Output::Done {
            op: 2,
            result: Ok(None)
        },
        "data placed, not returned"
    );
    assert!(buf == pattern(9, 2 * MIB), "both pieces read into place");

    // The fast path: the same read, admitted at once and served without the queue.
    let fast_read = |engine: &mut Engine<MemDevice>, buf: &mut Vec<u8>| -> Option<bool> {
        let fast = engine.try_fast_read(Instant::now(), "v", off, 2 * MIB)?;
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
        Some(engine.finish_fast_read(Instant::now(), "v", fast, ok))
    };
    let mut fast_buf = vec![0u8; 2 * MIB as usize];
    assert_eq!(fast_read(&mut engine, &mut fast_buf), Some(true));
    assert!(
        fast_buf == pattern(9, 2 * MIB),
        "the fast path reads the same data"
    );

    // A write in flight holds its blocks: an overlapping fast read is refused, and admitted
    // again once the write is done.
    engine.handle(
        Instant::now(),
        Input::Io {
            volume: "v".into(),
            op: 3,
            io: Io::Write {
                offset: off,
                data: pattern(10, 2 * MIB),
            },
        },
    );
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
    let net = net::spawn(
        NetConfig {
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
        timing: Timing::default(),
        max_io: 64,
        heartbeat: None,
    };
    let meta = Box::new(FakeMeta {
        model: model.clone(),
        inbox: tx.clone(),
    });
    let dev = store().into_device();
    let deferred = Store::open(DeferredDevice::new(dev), StoreConfig::default()).unwrap();
    let mut engine = Engine::new(cfg, ReplicaService::new(deferred), net, meta);
    engine.set_async_commits(Instant::now(), true);
    model.lock().unwrap().volume = Some(pb::Volume {
        id: "v".into(),
        size_bytes: 2 * CHUNK,
        chunk_bytes: CHUNK,
        replicas: 1,
        min_isr: 1,
        epoch: 1,
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
            let now = Instant::now();
            while let Ok(i) = rx.try_recv() {
                engine.handle(now, i);
            }
            engine.turn(now);
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
                    engine.local_read_done(Instant::now(), &lr, Some(plan), ok);
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
        engine.handle(
            Instant::now(),
            Input::Io {
                volume: "v".into(),
                op,
                io,
            },
        );
    };
    let done = |op: u64| move |o: &Output| matches!(o, Output::Done { op: d, .. } if *d == op);

    engine.handle(
        Instant::now(),
        Input::Attach {
            volume: "v".into(),
            token: token(&signer, "v", 1, "fl-a"),
        },
    );
    run(&mut engine, &mut buf, until, &|o| {
        matches!(o, Output::Attached { .. })
    });

    io(
        &mut engine,
        1,
        Io::Write {
            offset: 0,
            data: pattern(1, 64 * 1024),
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
            data: pattern(2, 64 * 1024),
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
            result: Ok(None)
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
            result: Ok(None)
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
    engine.set_async_commits(Instant::now(), false);
    std::fs::remove_dir_all(&dir).unwrap();
}
