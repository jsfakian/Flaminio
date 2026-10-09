//! Sessions over real TLS on loopback: who may open one, and with what token.

use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};

use flaminio_net::frame::Frame;
use flaminio_net::session::{self, SessionError};
use flaminio_net::tls::Files;
use flaminio_net::token::TokenKey;
use futures_rustls::{TlsAcceptor, TlsConnector};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, SanType,
};
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use smol::Async;

const TD: &str = "lab";
const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

struct Ca {
    params: CertificateParams,
    key: KeyPair,
    pem: String,
    dir: PathBuf,
}

impl Ca {
    fn new(name: &str) -> Ca {
        let dir = std::env::temp_dir().join(format!("flaminio-net-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut params = CertificateParams::new(vec![]).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let key = KeyPair::generate().unwrap();
        let pem = params.self_signed(&key).unwrap().pem();
        std::fs::write(dir.join("ca.pem"), &pem).unwrap();
        Ca {
            params,
            key,
            pem,
            dir,
        }
    }

    /// Issues `id` with an IP SAN for loopback; returns its files, verified by `trust` (a CA
    /// bundle path).
    fn issue(&self, name: &str, id: &str, trust: &Path) -> Files {
        let mut p = CertificateParams::new(vec![]).unwrap();
        p.subject_alt_names = vec![
            SanType::URI(id.try_into().unwrap()),
            SanType::IpAddress(LOCALHOST),
        ];
        p.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        let key = KeyPair::generate().unwrap();
        let issuer = Issuer::from_params(&self.params, &self.key);
        let cert = p.signed_by(&key, &issuer).unwrap();
        let (c, k) = (
            self.dir.join(format!("{name}.crt")),
            self.dir.join(format!("{name}.key")),
        );
        std::fs::write(&c, cert.pem()).unwrap();
        std::fs::write(&k, key.serialize_pem()).unwrap();
        let _ = &self.pem;
        Files {
            cert: c,
            key: k,
            ca: trust.to_path_buf(),
        }
    }

    fn bundle(&self) -> PathBuf {
        self.dir.join("ca.pem")
    }
}

fn node_id(node: &str, component: &str) -> String {
    format!("spiffe://{TD}/flaminio/node/{node}/{component}")
}

/// A token as Go's pkg/captoken makes them.
fn token(key: &Ed25519KeyPair, volume: &str, epoch: u64, node: &str) -> Vec<u8> {
    let mut b = b"FLCT\x01".to_vec();
    b.extend_from_slice(&(volume.len() as u16).to_be_bytes());
    b.extend_from_slice(volume.as_bytes());
    b.extend_from_slice(&epoch.to_be_bytes());
    b.extend_from_slice(&(node.len() as u16).to_be_bytes());
    b.extend_from_slice(node.as_bytes());
    b.extend_from_slice(&1_791_500_000i64.to_be_bytes());
    let sig = key.sign(&b);
    b.extend_from_slice(sig.as_ref());
    b
}

fn hello(token: Vec<u8>, volume: &str, epoch: u64) -> Frame {
    Frame::Hello {
        req: 1,
        epoch,
        session: 3,
        volume: volume.into(),
        token,
    }
}

struct Setup {
    ca: Ca,
    server: Files,
    signer: Ed25519KeyPair,
}

fn setup(name: &str) -> Setup {
    let ca = Ca::new(name);
    let server = ca.issue("server", &node_id("fl-b", "io"), &ca.bundle());
    let signer = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    Setup { ca, server, signer }
}

/// Runs a server that accepts one session (answering a heartbeat), and a client that opens
/// one. Returns both outcomes.
fn run(
    s: &Setup,
    client: &Files,
    expect_node: &str,
    hello: Frame,
) -> (
    Result<session::Accepted, SessionError>,
    Result<Frame, SessionError>,
) {
    let key = TokenKey::from_raw(s.signer.public_key().as_ref()).unwrap();
    let acceptor = TlsAcceptor::from(s.server.server_config().unwrap());
    let connector = TlsConnector::from(client.client_config().unwrap());
    let listener = Async::<TcpListener>::bind(([127, 0, 0, 1], 0)).unwrap();
    let addr = listener.get_ref().local_addr().unwrap();
    smol::block_on(async {
        let server = smol::spawn(async move {
            let (io, _) = listener.accept().await?;
            let (accepted, mut conn) = session::accept(io, &acceptor, TD, &key).await?;
            conn.send(&Frame::Fenced {
                req: accepted.req,
                uncertain: vec![],
            })
            .await?;
            let f = conn.recv().await?;
            if let Err(why) = session::check_request(&f) {
                conn.send(&Frame::Refused { reason: why.into() }).await?;
                return Err(SessionError::Refused(why.into()));
            }
            conn.send(&Frame::Ok { req: 2 }).await?;
            Ok(accepted)
        });
        let client = async {
            let io = Async::<TcpStream>::connect(addr).await?;
            let mut conn =
                session::connect(io, &connector, LOCALHOST, TD, expect_node, &hello).await?;
            match conn.recv().await? {
                Frame::Fenced { .. } => {}
                other => return Ok(other),
            }
            conn.send(&Frame::Heartbeat { req: 2 }).await?;
            conn.recv().await
        };
        let c = client.await;
        let s = server.await;
        (s, c)
    })
}

fn refused<T: std::fmt::Debug>(r: &Result<T, SessionError>, what: &str) {
    match r {
        Err(SessionError::Refused(why)) => assert!(why.contains(what), "{why}"),
        other => panic!("expected a refusal about {what:?}, got {other:?}"),
    }
}

#[test]
fn a_valid_session() {
    let s = setup("valid");
    let client = s.ca.issue("client", &node_id("fl-a", "io"), &s.ca.bundle());
    let (server, client) = run(
        &s,
        &client,
        "fl-b",
        hello(token(&s.signer, "pvc-1", 4, "fl-a"), "pvc-1", 4),
    );
    let a = server.unwrap();
    assert_eq!((a.peer_node.as_str(), a.epoch, a.session), ("fl-a", 4, 3));
    assert_eq!(a.claims.volume, "pvc-1");
    assert_eq!(client.unwrap(), Frame::Ok { req: 2 });
}

#[test]
fn a_certificate_from_another_ca_is_refused() {
    let s = setup("foreign");
    let other = Ca::new("foreign-other");
    let client = other.issue("client", &node_id("fl-a", "io"), &s.ca.bundle());
    let (server, client) = run(
        &s,
        &client,
        "fl-b",
        hello(token(&s.signer, "v", 1, "fl-a"), "v", 1),
    );
    assert!(server.is_err(), "server accepted a foreign certificate");
    assert!(client.is_err());
}

#[test]
fn only_a_node_data_plane_may_open_a_session() {
    let s = setup("component");
    let client =
        s.ca.issue("client", &node_id("fl-a", "meta"), &s.ca.bundle());
    let (server, client) = run(
        &s,
        &client,
        "fl-b",
        hello(token(&s.signer, "v", 1, "fl-a"), "v", 1),
    );
    refused(&server, "not a node's data plane");
    assert!(
        matches!(client, Ok(Frame::Refused { .. }) | Err(_)),
        "{client:?}"
    );
}

#[test]
fn the_token_must_name_this_node_volume_and_epoch() {
    let s = setup("token");
    let client = s.ca.issue("client", &node_id("fl-a", "io"), &s.ca.bundle());
    for (tok, volume, epoch) in [
        (token(&s.signer, "v", 1, "fl-c"), "v", 1), // another node's token
        (token(&s.signer, "w", 1, "fl-a"), "v", 1), // another volume's
        (token(&s.signer, "v", 1, "fl-a"), "v", 2), // another epoch's
    ] {
        let (server, client) = run(&s, &client, "fl-b", hello(tok, volume, epoch));
        refused(&server, "token grants");
        assert!(matches!(client, Ok(Frame::Refused { .. })), "{client:?}");
    }
    let forger = Ed25519KeyPair::from_seed_unchecked(&[8; 32]).unwrap();
    let (server, _) = run(
        &s,
        &client,
        "fl-b",
        hello(token(&forger, "v", 1, "fl-a"), "v", 1),
    );
    refused(&server, "invalid capability token");
}

#[test]
fn the_client_checks_whom_it_reached() {
    let s = setup("server-id");
    let client = s.ca.issue("client", &node_id("fl-a", "io"), &s.ca.bundle());
    let (_, client) = run(
        &s,
        &client,
        "fl-c",
        hello(token(&s.signer, "v", 1, "fl-a"), "v", 1),
    );
    refused(&client, "expected the data plane of fl-c");
}

#[test]
fn one_session_per_connection() {
    assert!(session::check_request(&hello(vec![], "v", 1)).is_err());
    assert!(session::check_request(&Frame::Ok { req: 1 }).is_err());
    assert!(session::check_request(&Frame::Heartbeat { req: 1 }).is_ok());
}
