//! The network thread: replication sessions in both directions.
//!
//! One thread runs one async executor. It does three jobs:
//! - **replica side:** it accepts TLS connections and opens their sessions (certificate and
//!   token checks in `flaminio_net::session`);
//! - **target side:** it opens sessions to other nodes' replicas when the engine asks;
//! - **both sides:** it moves frames between the connections and the engine.
//!
//! The engine never blocks on the network, and the network never touches the engine's state.

use std::cell::RefCell;
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::thread;

use flaminio_net::frame::Frame;
use flaminio_net::session::{self, Accepted, SessionError};
use flaminio_net::token::TokenKey;
use futures_rustls::rustls::{ClientConfig, ServerConfig};
use futures_rustls::{TlsAcceptor, TlsConnector};
use smol::channel;

use crate::clock::{ClockDomain, ClockGeneration, ClockObserver};
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
    /// Cancel both directions and the listener permanently after the shared gate closes.
    Quarantine,
    /// Opens a session to the data plane of `node` at `addr`, starting with `hello`.
    Connect {
        generation: ClockGeneration,
        key: Key,
        addr: SocketAddr,
        node: String,
        hello: Box<Frame>,
    },
    /// Sends on an outgoing session. Frames sent before it is connected wait for it.
    Send {
        generation: ClockGeneration,
        key: Key,
        frame: Frame,
    },
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
    pub clock: Arc<ClockDomain>,
    pub trust_domain: String,
    /// None: this node accepts no sessions.
    pub server: Option<ServerSide>,
    /// None: this node opens no sessions.
    pub client: Option<Arc<ClientConfig>>,
}

/// Where the network thread delivers events. It must not block.
pub type Sink = Arc<dyn Fn(NetEvent) + Send + Sync>;

/// The engine's handle on the network thread.
///
/// Cloning shares the command channel. When the last `Net` is dropped the command channel
/// closes and the worker thread is joined, so TLS/accept state is gone before ASan/LSan runs
/// (engine tests used to leak the thread past process teardown).
#[derive(Clone)]
pub struct Net {
    cmds: channel::Sender<NetCommand>,
    clock: Arc<ClockDomain>,
    /// Shared so every clone keeps the worker alive; the last drop joins it.
    #[allow(dead_code)] // held for Arc drop → NetWorker::drop → join
    worker: Arc<NetWorker>,
}

/// Owns the join handle; dropped only after every `Net` sender is gone.
struct NetWorker {
    join: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Drop for NetWorker {
    fn drop(&mut self) {
        if let Some(h) = self.join.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = h.join();
        }
    }
}

impl Net {
    pub fn clock_domain(&self) -> &Arc<ClockDomain> {
        &self.clock
    }

    pub fn send(&self, c: NetCommand) {
        let _ = self.cmds.try_send(c);
    }

    /// Inspect engine adapter commands without starting a network worker in unit tests.
    #[cfg(test)]
    pub(crate) fn test_channel(clock: Arc<ClockDomain>) -> (Self, channel::Receiver<NetCommand>) {
        let (cmds, receiver) = channel::unbounded();
        (
            Self {
                cmds,
                clock,
                worker: Arc::new(NetWorker {
                    join: Mutex::new(None),
                }),
            },
            receiver,
        )
    }
}

/// Starts the network thread.
pub fn spawn(cfg: NetConfig, sink: Sink) -> std::io::Result<Net> {
    let (cmds, rx) = channel::unbounded::<NetCommand>();
    let cancellation = cfg.clock.subscribe();
    let clock = cfg.clock.clone();
    let listener = match &cfg.server {
        Some(s) => Some(smol::Async::new(s.listener.try_clone()?)?),
        None => None,
    };
    let join = thread::Builder::new()
        .name("flaminio-net".into())
        .spawn(move || {
            let ex = Rc::new(smol::LocalExecutor::new());
            let observer = Rc::new(RefCell::new(cfg.clock.observer()));
            smol::block_on(ex.run(futures_lite::future::or(
                async {
                    if let (Some(listener), Some(server)) = (listener, cfg.server.as_ref()) {
                        let acceptor = TlsAcceptor::from(server.tls.clone());
                        let (td, key) = (cfg.trust_domain.clone(), server.token_key.clone());
                        // Weak: a strong Rc in a task the executor holds is a cycle, and the
                        // executor, the listener and the TLS config would never be freed.
                        ex.spawn(accept_loop(
                            Rc::downgrade(&ex),
                            listener,
                            acceptor,
                            td,
                            key,
                            sink.clone(),
                            observer.clone(),
                        ))
                        .detach();
                    }
                    let connector = cfg.client.clone().map(TlsConnector::from);
                    let mut outgoing: HashMap<Key, (channel::Sender<Frame>, smol::Task<()>)> =
                        HashMap::new();
                    while let Ok(c) = rx.recv().await {
                        if !admit(&observer) {
                            break;
                        }
                        match c {
                            NetCommand::Quarantine => break,
                            NetCommand::Connect {
                                generation,
                                key,
                                addr,
                                node,
                                hello,
                            } => {
                                if cfg.clock.check_generation(generation).is_err() {
                                    continue;
                                }
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
                                    observer.clone(),
                                ));
                                // Replacing an entry drops (cancels) the old session's task.
                                outgoing.insert(key, (tx, task));
                            }
                            NetCommand::Send {
                                generation,
                                key,
                                frame,
                            } => {
                                if cfg.clock.check_generation(generation).is_err() {
                                    continue;
                                }
                                if let Some((tx, _)) = outgoing.get(&key) {
                                    let _ = tx.try_send(frame);
                                }
                            }
                            NetCommand::Close { key } => {
                                outgoing.remove(&key);
                            }
                        }
                    }
                },
                async {
                    let _ = cancellation.recv().await;
                },
            )));
            // Dropping the executor drops its tasks: the accept loop with its listener, and
            // the sessions.
            drop(ex);
        })?;
    Ok(Net {
        cmds,
        clock,
        worker: Arc::new(NetWorker {
            join: Mutex::new(Some(join)),
        }),
    })
}

async fn accept_loop(
    ex: Weak<smol::LocalExecutor<'_>>,
    listener: smol::Async<TcpListener>,
    acceptor: TlsAcceptor,
    trust_domain: String,
    key: TokenKey,
    sink: Sink,
    observer: Rc<RefCell<ClockObserver>>,
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
        if !admit(&observer) {
            return;
        }
        let _ = io.get_ref().set_nodelay(true);
        let conn = next;
        next += 1;
        let (acceptor, td, key, sink) = (
            acceptor.clone(),
            trust_domain.clone(),
            key.clone(),
            sink.clone(),
        );
        // The executor runs this task, so it is alive whenever this runs.
        let Some(ex) = ex.upgrade() else { return };
        let observer = observer.clone();
        ex.spawn(async move {
            if let Err(e) = serve_conn(conn, io, &acceptor, &td, &key, sink, observer).await {
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
    observer: Rc<RefCell<ClockObserver>>,
) -> Result<(), SessionError> {
    let (accepted, stream) = session::accept(io, acceptor, trust_domain, key).await?;
    require_admission(&observer)?;
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
            require_admission(&observer)?;
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
            require_admission(&observer)?;
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
    observer: Rc<RefCell<ClockObserver>>,
) {
    let result: Result<(), SessionError> = async {
        let io = smol::Async::<TcpStream>::connect(addr).await?;
        let _ = io.get_ref().set_nodelay(true);
        let conn = session::connect_with_admission(
            io,
            &connector,
            addr.ip(),
            &trust_domain,
            &node,
            &hello,
            || admit(&observer),
        )
        .await?;
        let (reader, writer) = futures_lite::io::split(conn.into_inner());
        let mut reader = session::Conn::new(reader);
        let mut writer = session::Conn::new(writer);
        let out = async {
            while let Ok(f) = frames.recv().await {
                require_admission(&observer)?;
                writer.send(&f).await?;
            }
            Ok(())
        };
        let inp = async {
            loop {
                let frame = reader.recv().await?;
                require_admission(&observer)?;
                match frame {
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

// Sampling and permit creation contain no await, so this executor's observer is never
// borrowed across a context switch. Every frame is ordered against domain quarantine.
fn admit(observer: &RefCell<ClockObserver>) -> bool {
    let mut observer = observer.borrow_mut();
    let Ok(sample) = observer.sample() else {
        return false;
    };
    match observer.success_permit(sample) {
        Ok(permit) => {
            permit.consume();
            true
        }
        Err(_) => false,
    }
}

fn require_admission(observer: &RefCell<ClockObserver>) -> Result<(), SessionError> {
    if admit(observer) {
        Ok(())
    } else {
        Err(SessionError::Refused(
            "clock quarantined; operator restart required".into(),
        ))
    }
}
