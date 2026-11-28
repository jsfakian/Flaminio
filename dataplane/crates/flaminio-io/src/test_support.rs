//! Test-only memory engine with real verified credentials and independently controlled clocks.
use flaminio_format::{Uuid, layout::LayoutParams, mkfs::FormatOptions};
use flaminio_metaclient::{Answer, Call, pb};
use flaminio_net::handoff::{Purpose, VerifiedHandoff};
use flaminio_net::token::TokenKey;
use flaminio_node::clock::{
    ClockDomain, ClockFault, ClockQualification, ClockReading, ElapsedClock,
};
use flaminio_node::engine::{Engine, EngineConfig, Input, Io, MetaPort, Output};
use flaminio_node::replica::ReplicaService;
use flaminio_repl::types::Timing;
use flaminio_store::{MemDevice, Store, StoreConfig};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
pub const NONCE: [u8; 32] = [83; 32];
#[derive(Default)]
pub struct Clock {
    pub ns: AtomicU64,
    pub failed: AtomicBool,
}
impl ElapsedClock for Clock {
    fn read(&self) -> Result<ClockReading, ClockFault> {
        if self.failed.load(Ordering::SeqCst) {
            return Err(ClockFault::SampleFailed);
        }
        let n = self.ns.load(Ordering::SeqCst) as u128;
        Ok(ClockReading {
            monotonic_before_ns: n,
            boottime_ns: n,
            monotonic_after_ns: n,
        })
    }
}
pub fn signer() -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[83; 32]).unwrap()
}
pub fn credentials(
    volume: &str,
    node: &str,
    epoch: u64,
    first: u64,
    nonce: &[u8; 32],
) -> (Vec<u8>, Vec<u8>) {
    let key = signer();
    let mut token = b"FLCT\x02".to_vec();
    token.extend_from_slice(&(volume.len() as u16).to_be_bytes());
    token.extend_from_slice(volume.as_bytes());
    token.extend_from_slice(&epoch.to_be_bytes());
    token.extend_from_slice(&(node.len() as u16).to_be_bytes());
    token.extend_from_slice(node.as_bytes());
    token.extend_from_slice(&0i64.to_be_bytes());
    token.extend_from_slice(&first.to_be_bytes());
    token.extend_from_slice(key.sign(&token).as_ref());
    let mut handoff = b"FLCH\x01\x01".to_vec();
    handoff.extend_from_slice(nonce);
    handoff.extend_from_slice(ring::digest::digest(&ring::digest::SHA256, &token).as_ref());
    handoff.extend_from_slice(key.sign(&handoff).as_ref());
    (token, handoff)
}
pub fn proof(volume: &str, epoch: u64, nonce: &[u8; 32]) -> VerifiedHandoff {
    let (token, handoff) = credentials(volume, "fl-a", epoch, 1, nonce);
    token_key()
        .verify_handoff(&token, &handoff, nonce, Purpose::Normal)
        .unwrap()
}
pub fn token_key() -> TokenKey {
    TokenKey::from_raw(signer().public_key().as_ref()).unwrap()
}
struct Meta {
    volume: Arc<Mutex<pb::Volume>>,
    sender: mpsc::Sender<Input>,
}
impl MetaPort for Meta {
    fn call(&self, id: u64, call: Call) {
        let answer = match call {
            Call::GetVolume(_) => Answer::GetVolume(pb::GetVolumeResponse {
                volume: Some(self.volume.lock().unwrap().clone()),
            }),
            Call::ListNodes(_) => Answer::ListNodes(pb::ListNodesResponse {
                nodes: vec![pb::Node {
                    id: "fl-a".into(),
                    address: "127.0.0.1:1".into(),
                    ..Default::default()
                }],
            }),
            Call::RegisterNode(_) => Answer::RegisterNode(pb::RegisterNodeResponse {}),
            Call::NodeHeartbeat(_) => {
                Answer::NodeHeartbeat(pb::NodeHeartbeatResponse { ttl_seconds: 10 })
            }
            Call::ChangeIsr(_) => panic!("RF1 fixture never needs to shrink ISR"),
        };
        self.sender.send(Input::Meta { id, answer }).unwrap();
    }
}
pub struct Fixture {
    pub engine: Engine<MemDevice>,
    pub rx: mpsc::Receiver<Input>,
    pub tx: mpsc::Sender<Input>,
    pub clock: Arc<Clock>,
    pub proof: VerifiedHandoff,
}
impl Fixture {
    pub fn new() -> Self {
        let clock = Arc::new(Clock::default());
        let domain = Arc::new(
            ClockDomain::new(clock.clone(), ClockQualification::new(1, 1, 1000).unwrap()).unwrap(),
        );
        let net = flaminio_node::net::spawn(
            flaminio_node::net::NetConfig {
                clock: domain.clone(),
                trust_domain: "test".into(),
                server: None,
                client: None,
            },
            Arc::new(|_| {}),
        )
        .unwrap();
        let device = MemDevice::new(256 << 20);
        Store::format(
            &device,
            &FormatOptions {
                disk_uuid: Uuid([83; 16]),
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
        let (tx, rx) = mpsc::channel();
        let volume = Arc::new(Mutex::new(pb::Volume {
            id: "v".into(),
            size_bytes: 2 << 20,
            chunk_bytes: 1 << 20,
            replicas: 1,
            min_isr: 1,
            epoch: 5,
            target: "fl-a".into(),
            chunks: (0..2)
                .map(|_| pb::ChunkMeta {
                    replicas: vec!["fl-a".into()],
                    isr: vec!["fl-a".into()],
                    ..Default::default()
                })
                .collect(),
        }));
        let engine = Engine::new(
            EngineConfig {
                node: "fl-a".into(),
                address: "127.0.0.1:1".into(),
                timing: Timing::default(),
                max_io: 64,
                heartbeat: None,
            },
            ReplicaService::new(Store::open(device, StoreConfig::default()).unwrap()),
            net,
            Box::new(Meta {
                volume,
                sender: tx.clone(),
            }),
            domain,
            NONCE,
        );
        let proof = proof("v", 5, &NONCE);
        let mut f = Self {
            engine,
            rx,
            tx,
            clock,
            proof,
        };
        f.engine.handle(Input::Attach {
            volume: "v".into(),
            proof: f.proof.clone(),
        });
        let out = f.drain();
        assert!(
            out.iter().any(|o| matches!(o, Output::Attached { .. })),
            "{out:?}"
        );
        f.engine.handle(Input::Io {
            volume: "v".into(),
            op: 100,
            io: Io::Write {
                offset: 0,
                data: vec![0x55; 4096].into(),
            },
        });
        let out = f.drain();
        assert!(
            out.iter().any(|o| matches!(
                o,
                Output::Done {
                    op: 100,
                    result: Ok(_),
                    permit: Some(_)
                }
            )),
            "{out:?}"
        );
        f
    }
    pub fn drain(&mut self) -> Vec<Output> {
        let mut out = Vec::new();
        for _ in 0..12 {
            while let Ok(i) = self.rx.try_recv() {
                self.engine.handle(i);
            }
            self.engine.turn();
            out.extend(self.engine.take_output());
        }
        out
    }
}
