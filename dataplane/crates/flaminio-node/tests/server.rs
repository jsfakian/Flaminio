//! The replica server end to end: TLS sessions, tokens, a file-backed store.

use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use flaminio_format::Uuid;
use flaminio_format::layout::LayoutParams;
use flaminio_format::mkfs::FormatOptions;
use flaminio_net::BLOCK;
use flaminio_net::frame::Frame;
use flaminio_net::session;
use flaminio_net::tls::Files;
use flaminio_net::token::TokenKey;
use flaminio_node::replica::ReplicaService;
use flaminio_node::server::{self, NetConfig};
use flaminio_repl::extent::Extent;
use flaminio_repl::types::Version;
use flaminio_store::{FileDevice, Store, StoreConfig};
use futures_rustls::TlsConnector;
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, SanType,
};
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use smol::Async;

const TD: &str = "lab";
const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const MIB: u64 = 1 << 20;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("flaminio-node-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A CA in `dir` and two node identities issued by it: fl-b (the replica) and fl-a.
fn pki(dir: &Path) -> (Files, Files) {
    let mut ca = CertificateParams::new(vec![]).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_key = KeyPair::generate().unwrap();
    std::fs::write(dir.join("ca.pem"), ca.self_signed(&ca_key).unwrap().pem()).unwrap();
    let issue = |name: &str| {
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
        Files {
            cert: dir.join(format!("{name}.crt")),
            key: dir.join(format!("{name}.key")),
            ca: dir.join("ca.pem"),
        }
    };
    (issue("fl-b"), issue("fl-a"))
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

fn hello(tok: Vec<u8>, epoch: u64, session: u64) -> Frame {
    Frame::Hello {
        req: 0,
        epoch,
        session,
        volume: "pvc-1".into(),
        token: tok,
        size: 16 * MIB,
        chunk_size: 4 * MIB,
        chunks: vec![0, 1],
    }
}

#[test]
fn replica_server_end_to_end() {
    let dir = tmp("e2e");
    let (server_id, client_id) = pki(&dir);
    let signer = Ed25519KeyPair::from_seed_unchecked(&[3; 32]).unwrap();

    // A file-backed store, formatted.
    let path = dir.join("disk.img");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(256 * MIB)
        .unwrap();
    let dev = FileDevice::open(&path).unwrap();
    Store::format(
        &dev,
        &FormatOptions {
            disk_uuid: Uuid([1; 16]),
            cluster_uuid: Uuid::NIL,
            format_nonce: 1,
            created_unix_secs: 0,
            layout: LayoutParams {
                journal_size: Some(8 * MIB),
                ..Default::default()
            },
            label: String::new(),
        },
    )
    .unwrap();
    let service = ReplicaService::new(Store::open(dev, StoreConfig::default()).unwrap());

    let listener = TcpListener::bind((LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    server::spawn_network(
        listener,
        NetConfig {
            tls: server_id.server_config().unwrap(),
            trust_domain: TD.into(),
            token_key: TokenKey::from_raw(signer.public_key().as_ref()).unwrap(),
        },
        tx,
    )
    .unwrap();
    std::thread::spawn(move || server::run_engine(service, rx));

    let connector = TlsConnector::from(client_id.client_config().unwrap());
    smol::block_on(async {
        let open = |epoch: u64, session: u64| {
            let connector = connector.clone();
            let tok = token(&signer, "pvc-1", epoch, "fl-a");
            async move {
                let io = Async::<TcpStream>::connect(addr).await.unwrap();
                let mut c = session::connect(
                    io,
                    &connector,
                    LOCALHOST,
                    TD,
                    "fl-b",
                    &hello(tok, epoch, session),
                )
                .await
                .unwrap();
                let reply = c.recv().await.unwrap();
                (c, reply)
            }
        };
        let (mut a, reply) = open(1, 1).await;
        assert!(matches!(reply, Frame::Fenced { req: 0, .. }), "{reply:?}");

        // Pipelined writes, one per 64 KiB, then read everything back.
        let n = 64u32;
        for i in 0..n {
            a.send(&Frame::Write {
                req: u64::from(i) + 1,
                chunk: i % 2,
                ext: Extent::new(i * 16, 1),
                version: Version {
                    epoch: 1,
                    seq: u64::from(i),
                },
                prune: None,
                data: vec![i as u8; BLOCK],
            })
            .await
            .unwrap();
        }
        for i in 0..n {
            assert_eq!(
                a.recv().await.unwrap(),
                Frame::Ok {
                    req: u64::from(i) + 1
                }
            );
        }
        for i in 0..n {
            a.send(&Frame::Read {
                req: 1000 + u64::from(i),
                chunk: i % 2,
                ext: Extent::new(i * 16, 1),
            })
            .await
            .unwrap();
            match a.recv().await.unwrap() {
                Frame::Data { req, data } => {
                    assert_eq!(req, 1000 + u64::from(i));
                    assert!(data.iter().all(|&b| b == i as u8));
                }
                other => panic!("{other:?}"),
            }
        }

        // A newer epoch fences the old target off.
        let (_b, reply) = open(2, 1).await;
        assert!(matches!(reply, Frame::Fenced { .. }), "{reply:?}");
        a.send(&Frame::Heartbeat { req: 5000 }).await.unwrap();
        assert_eq!(
            a.recv().await.unwrap(),
            Frame::Stale {
                req: 5000,
                fenced: 2
            }
        );

        // And an older epoch cannot open a session at all.
        let (_c, reply) = open(1, 9).await;
        assert_eq!(reply, Frame::Stale { req: 0, fenced: 2 });
    });
    let _ = std::fs::remove_dir_all(&dir);
}
