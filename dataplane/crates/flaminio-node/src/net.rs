//! The network thread: replication sessions in both directions.
//!
//! One thread runs one async executor. It does three jobs:
//! - **replica side:** it accepts TLS connections and opens their sessions (certificate and
//!   token checks in `flaminio_net::session`);
//! - **target side:** it opens sessions to other nodes' replicas when the engine asks;
//! - **both sides:** it moves frames between the connections and the engine.
//!
//! The engine never blocks on the network, and the network never touches the engine's state.

use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::rc::Rc;
use std::sync::Arc;
use std::thread;

use flaminio_net::frame::Frame;
use flaminio_net::session::{self, Accepted, SessionError};
use flaminio_net::token::TokenKey;
use futures_rustls::rustls::{ClientConfig, ServerConfig};
use futures_rustls::{TlsAcceptor, TlsConnector};
use smol::channel;

use crate::replica::Reply;

/// Identifies an accepted connection (replica side).
pub type ConnId = u64;
/// Identifies an outgoing session (target side), chosen by the engine.
pub type Key = u64;

/// From the network thread to the engine.
pub enum NetEvent {
    /// Replica side: a session passed its certificate and token checks. The engine answers on
    /// `replies`, in order.
    Opened {
        conn: ConnId,
        accepted: Box<Accepted>,
        replies: channel::Sender<Reply>,
    },
    /// Replica side: a request on an open session.
    Request { conn: ConnId, frame: Frame },
    /// Replica side: the connection is gone.
    Closed { conn: ConnId },
    /// Target side: a reply on an outgoing session.
    Reply { key: Key, frame: Frame },
    /// Target side: the outgoing session failed or was refused.
    Lost { key: Key, why: String },
}

/// From the engine to the network thread.
pub enum NetCommand {
    /// Opens a session to the data plane of `node` at `addr`, starting with `hello`.
    Connect {
        key: Key,
        addr: SocketAddr,
        node: String,
        hello: Box<Frame>,
    },
    /// Sends on an outgoing session. Frames sent before it is connected wait for it.
    Send { key: Key, frame: Frame },
    /// Closes an outgoing session.
    Close { key: Key },
}

/// Accepting sessions: where, with which certificate, and the token key.
pub struct ServerSide {
    pub listener: TcpListener,
    pub tls: Arc<ServerConfig>,
    pub token_key: TokenKey,
}

/// Settings of the network thread.
pub struct NetConfig {
    pub trust_domain: String,
    /// None: this node accepts no sessions.
    pub server: Option<ServerSide>,
    /// None: this node opens no sessions.
    pub client: Option<Arc<ClientConfig>>,
}

/// Where the network thread delivers events. It must not block.
pub type Sink = Arc<dyn Fn(NetEvent) + Send + Sync>;

/// The engine's handle on the network thread.
#[derive(Clone)]
pub struct Net {
    cmds: channel::Sender<NetCommand>,
}

impl Net {
    pub fn send(&self, c: NetCommand) {
        let _ = self.cmds.try_send(c);
    }
}

/// Starts the network thread.
pub fn spawn(cfg: NetConfig, sink: Sink) -> std::io::Result<Net> {
    let (cmds, rx) = channel::unbounded::<NetCommand>();
    let listener = match &cfg.server {
        Some(s) => Some(smol::Async::new(s.listener.try_clone()?)?),
        None => None,
    };
    thread::Builder::new()
        .name("flaminio-net".into())
        .spawn(move || {
            let ex = Rc::new(smol::LocalExecutor::new());
            smol::block_on(ex.run(async {
                if let (Some(listener), Some(server)) = (listener, cfg.server.as_ref()) {
                    let acceptor = TlsAcceptor::from(server.tls.clone());
                    let (td, key) = (cfg.trust_domain.clone(), server.token_key.clone());
                    ex.spawn(accept_loop(
                        ex.clone(),
                        listener,
                        acceptor,
                        td,
                        key,
                        sink.clone(),
                    ))
                    .detach();
                }
                let connector = cfg.client.clone().map(TlsConnector::from);
                let mut outgoing: HashMap<Key, (channel::Sender<Frame>, smol::Task<()>)> =
                    HashMap::new();
                while let Ok(c) = rx.recv().await {
                    match c {
                        NetCommand::Connect {
                            key,
                            addr,
                            node,
                            hello,
                        } => {
                            let Some(connector) = connector.clone() else {
                                sink(NetEvent::Lost {
                                    key,
                                    why: "this node opens no sessions".into(),
                                });
                                continue;
                            };
                            let (tx, frames) = channel::unbounded();
                            let task = ex.spawn(outgoing_session(
                                key,
                                addr,
                                node,
                                *hello,
                                connector,
                                cfg.trust_domain.clone(),
                                frames,
                                sink.clone(),
                            ));
                            // Replacing an entry drops (cancels) the old session's task.
                            outgoing.insert(key, (tx, task));
                        }
                        NetCommand::Send { key, frame } => {
                            if let Some((tx, _)) = outgoing.get(&key) {
                                let _ = tx.try_send(frame);
                            }
                        }
                        NetCommand::Close { key } => {
                            outgoing.remove(&key);
                        }
                    }
                }
            }));
        })?;
    Ok(Net { cmds })
}

async fn accept_loop(
    ex: Rc<smol::LocalExecutor<'_>>,
    listener: smol::Async<TcpListener>,
    acceptor: TlsAcceptor,
    trust_domain: String,
    key: TokenKey,
    sink: Sink,
) {
    let mut next: ConnId = 1;
    loop {
        let (io, peer) = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("flaminio-net: accept failed: {e}");
                continue;
            }
        };
        let _ = io.get_ref().set_nodelay(true);
        let conn = next;
        next += 1;
        let (acceptor, td, key, sink) = (
            acceptor.clone(),
            trust_domain.clone(),
            key.clone(),
            sink.clone(),
        );
        ex.spawn(async move {
            if let Err(e) = serve_conn(conn, io, &acceptor, &td, &key, sink).await {
                eprintln!("flaminio-net: session from {peer}: {e}");
            }
        })
        .detach();
    }
}

async fn serve_conn(
    conn: ConnId,
    io: smol::Async<TcpStream>,
    acceptor: &TlsAcceptor,
    trust_domain: &str,
    key: &TokenKey,
    sink: Sink,
) -> Result<(), SessionError> {
    let (accepted, stream) = session::accept(io, acceptor, trust_domain, key).await?;
    let (tx, rx) = channel::unbounded();
    let peer = accepted.peer_node.clone();
    sink(NetEvent::Opened {
        conn,
        accepted: Box::new(accepted),
        replies: tx,
    });
    let (reader, writer) = futures_lite::io::split(stream.into_inner());
    let mut reader = session::Conn::new(reader);
    let mut writer = session::Conn::new(writer);
    // Replies go out in the order the engine produced them. Whichever direction ends first
    // ends the connection.
    let out = async {
        while let Ok(reply) = rx.recv().await {
            match reply {
                Reply::Send(f) => writer.send(&f).await?,
                Reply::Nothing => {}
                Reply::Close(reason) => {
                    writer.send(&Frame::Refused { reason }).await?;
                    return Err(SessionError::Refused("closed by the engine".into()));
                }
            }
        }
        Ok(())
    };
    let inp = async {
        loop {
            let frame = reader.recv().await?;
            if let Err(why) = session::check_request(&frame) {
                return Err(SessionError::Refused(format!("{why} (from {peer})")));
            }
            sink(NetEvent::Request { conn, frame });
        }
    };
    let result = futures_lite::future::or(inp, out).await;
    sink(NetEvent::Closed { conn });
    match result {
        Err(SessionError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(()),
        other => other,
    }
}

#[allow(clippy::too_many_arguments)]
async fn outgoing_session(
    key: Key,
    addr: SocketAddr,
    node: String,
    hello: Frame,
    connector: TlsConnector,
    trust_domain: String,
    frames: channel::Receiver<Frame>,
    sink: Sink,
) {
    let result: Result<(), SessionError> = async {
        let io = smol::Async::<TcpStream>::connect(addr).await?;
        let _ = io.get_ref().set_nodelay(true);
        let conn =
            session::connect(io, &connector, addr.ip(), &trust_domain, &node, &hello).await?;
        let (reader, writer) = futures_lite::io::split(conn.into_inner());
        let mut reader = session::Conn::new(reader);
        let mut writer = session::Conn::new(writer);
        let out = async {
            while let Ok(f) = frames.recv().await {
                writer.send(&f).await?;
            }
            Ok(())
        };
        let inp = async {
            loop {
                match reader.recv().await? {
                    Frame::Refused { reason } => return Err(SessionError::Refused(reason)),
                    frame => sink(NetEvent::Reply { key, frame }),
                }
            }
        };
        futures_lite::future::or(inp, out).await
    }
    .await;
    let why = match result {
        Ok(()) => "closed".to_string(),
        Err(e) => e.to_string(),
    };
    sink(NetEvent::Lost { key, why });
}
