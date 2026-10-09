//! Sessions over mutual TLS: one connection, one session.
//!
//! **Accepting** (the replica side):
//! 1. The TLS handshake verifies the target's certificate against the CA.
//! 2. The peer must be a node's data plane (`.../node/<node>/io`).
//! 3. The first frame must be a `Hello`, and its capability token must:
//!    - verify under the metadata service's key;
//!    - name the `Hello`'s volume and epoch, and the peer's own node.
//!
//!    A valid node certificate alone grants nothing.
//! 4. Any later `Hello` on the connection is refused.
//!
//! Fencing (refusing an epoch older than the volume's) is the replica core's job.
//!
//! **Connecting** (the target side): the server must prove it is the data plane of the node
//! the target meant to reach.

use std::net::IpAddr;

use futures_lite::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use futures_rustls::{TlsAcceptor, TlsConnector};
use rustls_pki_types::ServerName;

use crate::frame::{DecodeError, Frame};
use crate::identity::{IdError, Identity};
use crate::tls::peer_identity;
use crate::token::{Claims, TokenKey};

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Decode(#[from] DecodeError),
    #[error("peer identity: {0}")]
    Identity(#[from] IdError),
    #[error("refused: {0}")]
    Refused(String),
}

/// A framed stream.
pub struct Conn<S> {
    stream: S,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Conn<S> {
    pub fn new(stream: S) -> Self {
        Self { stream }
    }

    pub async fn send(&mut self, f: &Frame) -> Result<(), SessionError> {
        self.stream.write_all(&f.encode()).await?;
        self.stream.flush().await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Frame, SessionError> {
        let mut prefix = [0u8; 4];
        self.stream.read_exact(&mut prefix).await?;
        let n = Frame::body_len(prefix)?;
        let mut body = vec![0u8; n];
        self.stream.read_exact(&mut body).await?;
        Ok(Frame::decode(&body)?)
    }

    /// Sends a refusal, best effort, and returns the error to report.
    async fn refuse(&mut self, why: String) -> SessionError {
        let _ = self
            .send(&Frame::Refused {
                reason: why.clone(),
            })
            .await;
        SessionError::Refused(why)
    }

    pub fn into_inner(self) -> S {
        self.stream
    }
}

/// An accepted session: who the peer is and what its token grants.
#[derive(Debug, Clone)]
pub struct Accepted {
    pub peer_node: String,
    /// The `Hello`'s request id, to answer it.
    pub req: u64,
    pub epoch: u64,
    pub session: u64,
    pub claims: Claims,
}

/// Accepts a session on an incoming connection (see the module documentation).
pub async fn accept<IO>(
    io: IO,
    acceptor: &TlsAcceptor,
    trust_domain: &str,
    key: &TokenKey,
) -> Result<(Accepted, Conn<futures_rustls::server::TlsStream<IO>>), SessionError>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let tls = acceptor.accept(io).await?;
    let peer = peer_identity(tls.get_ref().1.peer_certificates(), trust_domain);
    let mut conn = Conn::new(tls);
    let peer_node = match peer {
        Ok(id) => match id.node_with("io") {
            Some(n) => n.to_string(),
            None => {
                return Err(conn
                    .refuse(format!("{id} is not a node's data plane"))
                    .await);
            }
        },
        Err(e) => return Err(conn.refuse(e.to_string()).await),
    };
    let Frame::Hello {
        req,
        epoch,
        session,
        volume,
        token,
    } = conn.recv().await?
    else {
        return Err(conn.refuse("the first frame must be Hello".into()).await);
    };
    let claims = match key.verify(&token) {
        Ok(c) => c,
        Err(e) => return Err(conn.refuse(e.to_string()).await),
    };
    if claims.node != peer_node || claims.volume != volume || claims.epoch != epoch {
        let why = format!(
            "token grants {} epoch {} to {}, not {volume} epoch {epoch} to {peer_node}",
            claims.volume, claims.epoch, claims.node
        );
        return Err(conn.refuse(why).await);
    }
    Ok((
        Accepted {
            peer_node,
            req,
            epoch,
            session,
            claims,
        },
        conn,
    ))
}

/// Refuses requests that cannot appear after the session's `Hello`.
pub fn check_request(f: &Frame) -> Result<(), &'static str> {
    match f {
        Frame::Heartbeat { .. }
        | Frame::Write { .. }
        | Frame::Read { .. }
        | Frame::BeginResync { .. }
        | Frame::Copy { .. } => Ok(()),
        Frame::Hello { .. } => Err("one session per connection"),
        _ => Err("not a request"),
    }
}

/// Connects to the data plane of node `node` at `ip` and opens a session with `hello`.
pub async fn connect<IO>(
    io: IO,
    connector: &TlsConnector,
    ip: IpAddr,
    trust_domain: &str,
    node: &str,
    hello: &Frame,
) -> Result<Conn<futures_rustls::client::TlsStream<IO>>, SessionError>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let tls = connector
        .connect(ServerName::IpAddress(ip.into()), io)
        .await?;
    let id: Identity = peer_identity(tls.get_ref().1.peer_certificates(), trust_domain)?;
    if id.node_with("io") != Some(node) {
        return Err(SessionError::Refused(format!(
            "expected the data plane of {node}, reached {id}"
        )));
    }
    let mut conn = Conn::new(tls);
    conn.send(hello).await?;
    Ok(conn)
}
