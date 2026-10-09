//! Serving replication sessions over the network.
//!
//! Two threads:
//! - a **network thread** runs one async executor:
//!   - it accepts TLS connections and opens their sessions (certificate and token checks in
//!     `flaminio_net::session`);
//!   - it moves decoded frames to the engine and the engine's replies back.
//! - the **engine** owns the [`ReplicaService`]. [`run_engine`] is its loop for a node without
//!   volume targets: it takes everything that has arrived as one batch, so writes from all
//!   sessions share a group commit.

use std::net::TcpListener;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;

use flaminio_net::frame::Frame;
use flaminio_net::session::{self, Accepted};
use flaminio_net::token::TokenKey;
use flaminio_store::BlockDevice;
use futures_rustls::TlsAcceptor;
use futures_rustls::rustls::ServerConfig;
use smol::channel;

use crate::replica::{ReplicaService, Reply, SessionRef};

/// Identifies a connection between the network thread and the engine.
pub type ConnId = u64;

/// From the network thread to the engine.
pub enum Event {
    /// A session passed its certificate and token checks. The engine answers on `replies`.
    Opened {
        conn: ConnId,
        accepted: Box<Accepted>,
        replies: channel::Sender<Reply>,
    },
    /// A request on an open session.
    Request { conn: ConnId, frame: Frame },
    /// The connection is gone.
    Closed { conn: ConnId },
}

/// Settings of the network thread.
#[derive(Clone)]
pub struct NetConfig {
    pub tls: Arc<ServerConfig>,
    pub trust_domain: String,
    pub token_key: TokenKey,
}

/// Starts the network thread: accepts sessions on `listener` and sends their events to the
/// engine.
pub fn spawn_network(
    listener: TcpListener,
    cfg: NetConfig,
    engine: mpsc::Sender<Event>,
) -> std::io::Result<thread::JoinHandle<()>> {
    let listener = smol::Async::new(listener)?;
    thread::Builder::new()
        .name("flaminio-net".into())
        .spawn(move || {
            let ex = smol::LocalExecutor::new();
            smol::block_on(ex.run(async {
                let acceptor = TlsAcceptor::from(cfg.tls.clone());
                let mut next: ConnId = 1;
                loop {
                    let (io, peer) = match listener.accept().await {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("replica: accept failed: {e}");
                            continue;
                        }
                    };
                    let _ = io.get_ref().set_nodelay(true);
                    let conn = next;
                    next += 1;
                    let (acceptor, cfg, engine) = (acceptor.clone(), cfg.clone(), engine.clone());
                    ex.spawn(async move {
                        if let Err(e) = serve_conn(conn, io, &acceptor, &cfg, engine).await {
                            eprintln!("replica: connection {conn} from {peer}: {e}");
                        }
                    })
                    .detach();
                }
            }));
        })
}

async fn serve_conn(
    conn: ConnId,
    io: smol::Async<std::net::TcpStream>,
    acceptor: &TlsAcceptor,
    cfg: &NetConfig,
    engine: mpsc::Sender<Event>,
) -> Result<(), session::SessionError> {
    let (accepted, stream) =
        session::accept(io, acceptor, &cfg.trust_domain, &cfg.token_key).await?;
    let (tx, rx) = channel::unbounded();
    let peer = accepted.peer_node.clone();
    if engine
        .send(Event::Opened {
            conn,
            accepted: Box::new(accepted),
            replies: tx,
        })
        .is_err()
    {
        return Ok(()); // engine gone: shutting down
    }
    let (reader, writer) = futures_lite::io::split(stream.into_inner());
    let mut reader = session::Conn::new(reader);
    let mut writer = session::Conn::new(writer);

    // Replies go out in the order the engine produced them.
    let out = smol::spawn(async move {
        while let Ok(reply) = rx.recv().await {
            match reply {
                Reply::Send(f) => writer.send(&f).await?,
                Reply::Nothing => {}
                Reply::Close(reason) => {
                    writer.send(&Frame::Refused { reason }).await?;
                    break;
                }
            }
        }
        Ok::<_, session::SessionError>(())
    });
    let result = loop {
        let frame = match reader.recv().await {
            Ok(f) => f,
            Err(e) => break Err(e),
        };
        if let Err(why) = session::check_request(&frame) {
            break Err(session::SessionError::Refused(format!(
                "{why} (from {peer})"
            )));
        }
        if engine.send(Event::Request { conn, frame }).is_err() {
            break Ok(());
        }
    };
    let _ = engine.send(Event::Closed { conn });
    out.cancel().await;
    match result {
        // The peer hung up.
        Err(session::SessionError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            Ok(())
        }
        other => other,
    }
}

/// The engine loop of a node that only holds replicas: runs until every network sender is
/// gone.
pub fn run_engine<D: BlockDevice>(mut service: ReplicaService<D>, events: mpsc::Receiver<Event>) {
    let mut conns: std::collections::BTreeMap<ConnId, (SessionRef, channel::Sender<Reply>)> =
        Default::default();
    while let Ok(first) = events.recv() {
        // Everything that has arrived is one batch.
        let mut batch = vec![first];
        while let Ok(e) = events.try_recv() {
            batch.push(e);
            if batch.len() >= 1024 {
                break;
            }
        }
        let mut requests: Vec<(SessionRef, Frame)> = Vec::new();
        let mut owners: Vec<ConnId> = Vec::new();
        for e in batch {
            match e {
                Event::Opened {
                    conn,
                    accepted,
                    replies,
                } => {
                    // Earlier requests first: a Hello can change what they mean.
                    flush(&mut service, &mut requests, &mut owners, &conns);
                    match service.hello(&accepted) {
                        Ok((s, reply)) => {
                            let _ = replies.try_send(Reply::Send(reply));
                            conns.insert(conn, (s, replies));
                        }
                        Err(why) => {
                            let _ = replies.try_send(Reply::Close(why));
                        }
                    }
                }
                Event::Request { conn, frame } => {
                    if let Some((s, _)) = conns.get(&conn) {
                        requests.push((*s, frame));
                        owners.push(conn);
                    }
                }
                Event::Closed { conn } => {
                    flush(&mut service, &mut requests, &mut owners, &conns);
                    conns.remove(&conn);
                }
            }
        }
        flush(&mut service, &mut requests, &mut owners, &conns);
    }
}

fn flush<D: BlockDevice>(
    service: &mut ReplicaService<D>,
    requests: &mut Vec<(SessionRef, Frame)>,
    owners: &mut Vec<ConnId>,
    conns: &std::collections::BTreeMap<ConnId, (SessionRef, channel::Sender<Reply>)>,
) {
    if requests.is_empty() {
        return;
    }
    let replies = service.handle(requests);
    for (conn, reply) in owners.iter().zip(replies) {
        if let Some((_, tx)) = conns.get(conn) {
            let _ = tx.try_send(reply);
        }
    }
    requests.clear();
    owners.clear();
}
