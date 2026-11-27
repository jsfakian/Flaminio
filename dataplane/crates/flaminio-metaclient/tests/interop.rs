//! Against the Go metadata service: run with `make meta-interop`, which starts one and sets
//! `FLAMINIO_META_INTEROP` (its address) and `FLAMINIO_META_INTEROP_DIR` (certificates). The
//! script has registered fl-a and fl-b, created volume `v` (2 chunks, RF 2) and attached it to
//! fl-a. A separate `bounded-isr` volume has 129 chunks. These tests are fl-a's data plane.

use std::collections::BTreeSet;
use std::time::Duration;

use flaminio_metaclient::{Answer, Call, Config, call_blocking, pb};
use flaminio_repl::meta::{ChunkMeta, MAX_CHANGES_PER_REQUEST, MetaReply, MetaRequest, VolumeMeta};
use flaminio_repl::replica::Replica;
use flaminio_repl::target::{Output, Target};
use flaminio_repl::types::{NodeId, Timing, Value};

fn config(endpoints: Vec<String>) -> Config {
    let dir = std::env::var("FLAMINIO_META_INTEROP_DIR").expect("set by make meta-interop");
    let read = |f: &str| std::fs::read_to_string(format!("{dir}/{f}")).unwrap();
    Config {
        endpoints,
        cert_pem: read("fl-a.crt"),
        key_pem: read("fl-a.key"),
        ca_pem: read("ca/ca.crt"),
        timeout: Duration::from_secs(3),
    }
}

#[test]
#[ignore = "needs a metadata service: make meta-interop"]
fn interop_with_the_go_service() {
    let addr = std::env::var("FLAMINIO_META_INTEROP").expect("set by make meta-interop");
    // Two dead endpoints first, the client moves on to the live one: one that never answers
    // (a partition: the connection is accepted by the kernel, then nothing) and one that refuses.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let cfg = config(vec![
        silent.local_addr().unwrap().to_string(),
        "127.0.0.1:1".into(),
        addr,
    ]);

    let Answer::GetVolume(r) = call_blocking(
        &cfg,
        Call::GetVolume(pb::GetVolumeRequest { id: "v".into() }),
    )
    .unwrap() else {
        panic!("GetVolume failed");
    };
    let v = r.volume.unwrap();
    // Attached once: the epoch after its first, the revision of its claim, which the in-memory
    // store starts at the clock (docs/issues/30).
    let epoch = v.epoch;
    assert!(epoch > 1, "epoch {epoch}");
    assert_eq!((v.target.as_str(), v.chunks.len()), ("fl-a", 2));
    assert_eq!(
        (v.size_bytes, v.chunk_bytes, v.replicas, v.min_isr),
        (2 << 20, 1 << 20, 2, 1)
    );
    let c0 = v.chunks[0].clone();
    assert_eq!(c0.isr.len(), 2);

    // As the target, shrink chunk 0's ISR.
    let change = |expected_ver: u64, new_isr: Vec<String>| {
        Call::ChangeIsr(pb::ChangeIsrRequest {
            volume: "v".into(),
            epoch,
            changes: vec![pb::IsrChange {
                chunk: 0,
                expected_ver,
                new_isr,
                ..Default::default()
            }],
        })
    };
    let Answer::ChangeIsr(r) =
        call_blocking(&cfg, change(0, vec![c0.replicas[0].clone()])).unwrap()
    else {
        panic!("ChangeIsr failed");
    };
    assert!(r.applied);
    assert_eq!(
        (r.epoch, r.chunks[0].isr_ver, r.chunks[0].isr.len()),
        (epoch, 1, 1)
    );
    // A retry is refused and shows the result.
    let Answer::ChangeIsr(r) =
        call_blocking(&cfg, change(0, vec![c0.replicas[0].clone()])).unwrap()
    else {
        panic!("ChangeIsr failed");
    };
    assert!(!r.applied);
    assert_eq!(r.chunks[0].isr_ver, 1);

    // fl-a heartbeats itself, and may not heartbeat fl-b.
    let hb = |node: &str| Call::NodeHeartbeat(pb::NodeHeartbeatRequest { node: node.into() });
    assert!(
        matches!(call_blocking(&cfg, hb("fl-a")).unwrap(), Answer::NodeHeartbeat(h) if h.ttl_seconds == 10)
    );
    assert!(matches!(
        call_blocking(&cfg, hb("fl-b")).unwrap(),
        Answer::Failed {
            code: tonic::Code::PermissionDenied,
            ..
        }
    ));
    let Answer::ListNodes(l) =
        call_blocking(&cfg, Call::ListNodes(pb::ListNodesRequest {})).unwrap()
    else {
        panic!("ListNodes failed");
    };
    let mut nodes: Vec<(String, String)> = l.nodes.into_iter().map(|n| (n.id, n.address)).collect();
    nodes.sort();
    assert_eq!(
        nodes,
        vec![
            ("fl-a".into(), "fl-a:7410".into()),
            ("fl-b".into(), "fl-b:7410".into())
        ]
    );
    eprintln!("interop: GetVolume, ChangeIsr (applied, then refused), heartbeats, ListNodes: ok");
}

const BATCH_VOLUME: &str = "bounded-isr";
const CORE_VOLUME: u32 = 1;

fn node_id(node: &str) -> NodeId {
    match node {
        "fl-a" => 0,
        "fl-b" => 1,
        _ => panic!("unexpected fixture node {node}"),
    }
}

fn node_name(node: NodeId) -> String {
    match node {
        0 => "fl-a".into(),
        1 => "fl-b".into(),
        _ => panic!("unexpected core node {node}"),
    }
}

fn chunk_meta(chunk: &pb::ChunkMeta) -> ChunkMeta {
    ChunkMeta {
        replicas: chunk.replicas.iter().map(|n| node_id(n)).collect(),
        isr: chunk.isr.iter().map(|n| node_id(n)).collect(),
        isr_ver: chunk.isr_ver,
        moving: (!chunk.moving_from.is_empty())
            .then(|| (node_id(&chunk.moving_from), node_id(&chunk.moving_to))),
    }
}

fn volume_meta(volume: &pb::Volume) -> VolumeMeta {
    VolumeMeta {
        epoch: volume.epoch,
        target: Some(node_id(&volume.target)),
        min_isr: volume.min_isr as usize,
        blocks_per_chunk: u32::try_from(volume.chunk_bytes / 4096).unwrap(),
        chunks: volume.chunks.iter().map(chunk_meta).collect(),
    }
}

fn batch_volume(cfg: &Config) -> pb::Volume {
    let Answer::GetVolume(reply) = call_blocking(
        cfg,
        Call::GetVolume(pb::GetVolumeRequest {
            id: BATCH_VOLUME.into(),
        }),
    )
    .unwrap() else {
        panic!("GetVolume failed for batching fixture");
    };
    reply.volume.unwrap()
}

fn change_isr(cfg: &Config, request: pb::ChangeIsrRequest) -> pb::ChangeIsrResponse {
    let Answer::ChangeIsr(reply) = call_blocking(cfg, Call::ChangeIsr(request)).unwrap() else {
        panic!("bounded ChangeIsr failed");
    };
    reply
}

struct BatchRecovery {
    cfg: Config,
    target: Target<Vec<Value>>,
    replicas: [Replica; 2],
    live: [bool; 2],
    view: VolumeMeta,
    now: u64,
    sizes: Vec<usize>,
    hide_first: bool,
    hidden: Option<(u64, MetaReply)>,
    refusals: usize,
    hold: bool,
    held: Option<(u64, MetaRequest)>,
}

impl BatchRecovery {
    fn new(cfg: Config, volume: &pb::Volume) -> Self {
        let view = volume_meta(volume);
        let timing = Timing {
            lease: 100,
            drift_ppm: 100_000,
            heartbeat: 10,
            request_timeout: 20,
            reconnect: 10,
            meta_timeout: 30,
            refresh: 1_000,
        };
        let mut replicas = std::array::from_fn(|_| Replica::with_chunk_size(view.blocks_per_chunk));
        for (chunk, meta) in view.chunks.iter().enumerate() {
            for node in &meta.replicas {
                replicas[*node as usize].create_chunk(
                    CORE_VOLUME,
                    chunk as u32,
                    view.blocks_per_chunk,
                );
            }
        }
        Self {
            cfg,
            target: Target::new(0, CORE_VOLUME, &view, timing, 256, 0),
            replicas,
            live: [true, false],
            view,
            now: 0,
            sizes: Vec::new(),
            hide_first: true,
            hidden: None,
            refusals: 0,
            hold: false,
            held: None,
        }
    }

    fn metadata(&mut self, req: u64, body: MetaRequest) {
        let reply = match body {
            MetaRequest::Get { .. } => {
                self.view = volume_meta(&batch_volume(&self.cfg));
                MetaReply::Applied {
                    vol: CORE_VOLUME,
                    view: self.view.clone(),
                }
            }
            MetaRequest::ChangeIsr { epoch, changes, .. } => {
                assert!(!changes.is_empty());
                assert!(changes.len() <= MAX_CHANGES_PER_REQUEST);
                assert_eq!(
                    changes
                        .iter()
                        .map(|c| c.chunk)
                        .collect::<BTreeSet<_>>()
                        .len(),
                    changes.len()
                );
                self.sizes.push(changes.len());
                let wire = pb::ChangeIsrRequest {
                    volume: BATCH_VOLUME.into(),
                    epoch,
                    changes: changes
                        .iter()
                        .map(|change| pb::IsrChange {
                            chunk: change.chunk,
                            expected_ver: change.expected_ver,
                            new_isr: change.new_isr.iter().map(|&n| node_name(n)).collect(),
                            change_replicas: change.new_replicas.is_some(),
                            new_replicas: change
                                .new_replicas
                                .as_ref()
                                .map(|nodes| nodes.iter().map(|&n| node_name(n)).collect())
                                .unwrap_or_default(),
                        })
                        .collect(),
                };
                let response = change_isr(&self.cfg, wire.clone());
                assert_eq!(response.chunks.len(), changes.len());
                let persisted = batch_volume(&self.cfg);
                for (named, change) in response.chunks.iter().zip(&changes) {
                    assert_eq!(named, &persisted.chunks[change.chunk as usize]);
                }
                if self.hide_first {
                    self.hide_first = false;
                    assert!(response.applied);
                    // Model an applied request whose answer was lost. Replaying that
                    // exact wire request is refused and cannot apply the transition twice.
                    let replay = change_isr(&self.cfg, wire);
                    assert!(!replay.applied);
                    assert_eq!(replay.chunks, response.chunks);
                    assert_eq!(batch_volume(&self.cfg), persisted);
                    self.hidden = Some((
                        req,
                        MetaReply::Rejected {
                            vol: CORE_VOLUME,
                            view: volume_meta(&persisted),
                        },
                    ));
                    return;
                }
                if let Some((old_req, old_reply)) = self.hidden.take() {
                    assert_ne!(old_req, req, "the hidden answer must have timed out");
                    self.target.on_meta(self.now, old_req, &old_reply);
                    assert!(self.target.accepts_meta_reply(req));
                }
                self.view.epoch = response.epoch;
                for (named, change) in response.chunks.iter().zip(&changes) {
                    self.view.chunks[change.chunk as usize] = chunk_meta(named);
                }
                if response.applied {
                    MetaReply::Applied {
                        vol: CORE_VOLUME,
                        view: self.view.clone(),
                    }
                } else {
                    self.refusals += 1;
                    MetaReply::Rejected {
                        vol: CORE_VOLUME,
                        view: self.view.clone(),
                    }
                }
            }
            _ => panic!("target emitted a non-recovery metadata request"),
        };
        self.target.on_meta(self.now, req, &reply);
    }

    fn pump(&mut self) {
        for _ in 0..10_000 {
            let outputs = self.target.take_output();
            if outputs.is_empty() {
                return;
            }
            for output in outputs {
                match output {
                    Output::Send { to, msg } => {
                        if self.live[to as usize]
                            && let Some(reply) = self.replicas[to as usize].handle(&msg)
                        {
                            self.target.on_reply(self.now, to, &reply);
                        }
                    }
                    Output::Meta { req, body }
                        if self.hold && matches!(body, MetaRequest::ChangeIsr { .. }) =>
                    {
                        assert!(self.held.replace((req, body)).is_none());
                    }
                    Output::Meta { req, body } => self.metadata(req, body),
                    Output::Done { .. } => panic!("no client I/O was submitted"),
                }
            }
        }
        panic!("recovery did not become quiet");
    }

    fn until(&mut self, description: &str, condition: impl Fn(&Self) -> bool) {
        self.pump();
        for _ in 0..1_000 {
            if condition(self) {
                return;
            }
            self.now += 10;
            self.target.tick(self.now);
            self.pump();
        }
        panic!("{description}: {}", self.target.describe());
    }
}

#[test]
#[ignore = "needs a metadata service: make meta-interop"]
fn bounded_isr_recovery_with_go_service() {
    let addr = std::env::var("FLAMINIO_META_INTEROP").expect("set by make meta-interop");
    let cfg = config(vec![addr.clone()]);
    let initial = batch_volume(&cfg);
    assert_eq!(initial.chunks.len(), 129);
    assert_eq!(MAX_CHANGES_PER_REQUEST, 64);

    // Check the real service boundary independently of the producer, in reverse
    // request order. A legal no-op ISR update still advances each named version.
    let request = |count: usize, view: &pb::Volume| pb::ChangeIsrRequest {
        volume: BATCH_VOLUME.into(),
        epoch: view.epoch,
        changes: (0..count)
            .rev()
            .map(|chunk| pb::IsrChange {
                chunk: chunk as u32,
                expected_ver: view.chunks[chunk].isr_ver,
                new_isr: view.chunks[chunk].isr.clone(),
                ..Default::default()
            })
            .collect(),
    };
    let boundary = change_isr(&cfg, request(64, &initial));
    assert!(boundary.applied);
    let baseline = batch_volume(&cfg);
    for (index, named) in boundary.chunks.iter().enumerate() {
        assert_eq!(named, &baseline.chunks[63 - index]);
    }
    let oversized = call_blocking(&cfg, Call::ChangeIsr(request(65, &baseline))).unwrap();
    assert!(matches!(
        oversized,
        Answer::Failed {
            code: tonic::Code::InvalidArgument,
            ..
        }
    ));
    assert_eq!(
        batch_volume(&cfg),
        baseline,
        "oversized request wrote metadata"
    );

    let mut recovery = BatchRecovery::new(cfg, &baseline);
    recovery.until("all chunks shrink after losing fl-b", |r| {
        r.view.chunks.iter().all(|c| c.isr == BTreeSet::from([0]))
    });
    assert!(recovery.sizes.contains(&64));
    assert!(
        recovery.refusals > 0,
        "lost committed batch was never relearned"
    );
    assert!(recovery.hidden.is_none());
    let shrunk = batch_volume(&recovery.cfg);
    for (chunk, before) in shrunk.chunks.iter().zip(&baseline.chunks) {
        assert_eq!(chunk.isr, ["fl-a"]);
        assert_eq!(chunk.isr_ver, before.isr_ver + 1);
    }
    let shrink_batches = recovery.sizes.len();
    recovery.live[1] = true;
    recovery.until("all chunks rejoin after fl-b returns", |r| {
        r.target.is_settled() && r.view.chunks.iter().all(|c| c.isr.len() == 2)
    });
    assert!(recovery.sizes.len() >= shrink_batches + 3);
    let rejoined = batch_volume(&recovery.cfg);
    for (chunk, before) in rejoined.chunks.iter().zip(&baseline.chunks) {
        assert_eq!(chunk.isr, ["fl-a", "fl-b"]);
        assert_eq!(chunk.replicas, before.replicas);
        assert_eq!(chunk.isr_ver, before.isr_ver + 2);
    }

    // Hold an actual next proposal, then advance the epoch through the fixture's
    // administrator CLI. The old authenticated owner cannot commit another batch.
    recovery.live[1] = false;
    recovery.hold = true;
    recovery.until("old owner's next batch", |r| r.held.is_some());
    let dir = std::env::var("FLAMINIO_META_INTEROP_DIR").unwrap();
    let attached = std::process::Command::new(format!("{dir}/flaminio-meta"))
        .args([
            "--server",
            &addr,
            "--cert",
            &format!("{dir}/admin.crt"),
            "--key",
            &format!("{dir}/admin.key"),
            "--ca",
            &format!("{dir}/ca/ca.crt"),
            "attach",
            BATCH_VOLUME,
            "fl-b",
        ])
        .output()
        .unwrap();
    assert!(
        attached.status.success(),
        "fixture takeover failed: {}",
        String::from_utf8_lossy(&attached.stderr)
    );
    let taken_over = batch_volume(&recovery.cfg);
    assert_eq!(taken_over.epoch, rejoined.epoch + 1);
    assert_eq!(taken_over.target, "fl-b");
    assert_eq!(taken_over.chunks, rejoined.chunks);
    let (req, body) = recovery.held.take().unwrap();
    recovery.metadata(req, body);
    assert!(recovery.target.is_deposed());
    assert_eq!(batch_volume(&recovery.cfg), taken_over);
}
