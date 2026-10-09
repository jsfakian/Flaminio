//! The data plane's client of the metadata service: gRPC over mutual TLS, authenticated as the
//! node's data plane (`.../node/<node>/io`).
//!
//! The data plane's engine is a single synchronous thread, so the client runs on a thread of
//! its own with a small tokio runtime. [`MetaClient::call`] queues a call; its answer comes to
//! the callback given at start, on the client's thread. Calls go to the endpoints in turn until
//! one answers: any instance of the service will do.

pub mod pb;

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tonic::codegen::http::uri::PathAndQuery;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};

/// Where the metadata service is, and how to authenticate to it.
#[derive(Debug, Clone)]
pub struct Config {
    /// `ip:port` of each instance.
    pub endpoints: Vec<String>,
    /// PEM certificate and key of this node's data plane, and the CA bundle.
    pub cert_pem: String,
    pub key_pem: String,
    pub ca_pem: String,
    /// Deadline of one call on one endpoint.
    pub timeout: Duration,
}

/// A call to the metadata service.
#[derive(Debug, Clone)]
pub enum Call {
    GetVolume(pb::GetVolumeRequest),
    ChangeIsr(pb::ChangeIsrRequest),
    RegisterNode(pb::RegisterNodeRequest),
    NodeHeartbeat(pb::NodeHeartbeatRequest),
}

/// The answer to a call.
#[derive(Debug, Clone)]
pub enum Answer {
    GetVolume(pb::GetVolumeResponse),
    ChangeIsr(pb::ChangeIsrResponse),
    RegisterNode(pb::RegisterNodeResponse),
    NodeHeartbeat(pb::NodeHeartbeatResponse),
    /// The service refused the call, or no endpoint answered.
    Failed {
        code: tonic::Code,
        message: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("no endpoints")]
    NoEndpoints,
    #[error("endpoint {0}: {1}")]
    Endpoint(String, String),
    #[error("cannot start the client thread: {0}")]
    Thread(#[from] std::io::Error),
}

/// A running client. Dropping it stops its thread once queued calls are done.
pub struct MetaClient {
    tx: mpsc::Sender<(u64, Call)>,
}

const SERVICE: &str = "/flaminio.meta.v1.MetadataService/";

impl MetaClient {
    /// Starts the client thread. `on_answer(id, answer)` runs on that thread for each call.
    pub fn spawn(
        cfg: Config,
        on_answer: impl Fn(u64, Answer) + Send + 'static,
    ) -> Result<Self, ClientError> {
        if cfg.endpoints.is_empty() {
            return Err(ClientError::NoEndpoints);
        }
        let tls = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(&cfg.ca_pem))
            .identity(Identity::from_pem(&cfg.cert_pem, &cfg.key_pem));
        let mut channels = Vec::new();
        for e in &cfg.endpoints {
            let host = e.rsplit_once(':').map_or(e.as_str(), |(h, _)| h);
            let ep = Endpoint::from_shared(format!("https://{e}"))
                .and_then(|ep| ep.tls_config(tls.clone().domain_name(host)))
                .map_err(|err| ClientError::Endpoint(e.clone(), err.to_string()))?
                .timeout(cfg.timeout)
                .connect_timeout(cfg.timeout);
            channels.push(ep);
        }
        let (tx, rx) = mpsc::channel::<(u64, Call)>();
        thread::Builder::new()
            .name("flaminio-meta".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                // Lazy connections: one per endpoint, kept for the client's life.
                let chans: Vec<Channel> =
                    rt.block_on(async { channels.into_iter().map(|e| e.connect_lazy()).collect() });
                let mut preferred = 0;
                while let Ok((id, call)) = rx.recv() {
                    let answer = rt.block_on(call_any(&chans, &mut preferred, call));
                    on_answer(id, answer);
                }
            })?;
        Ok(Self { tx })
    }

    /// Queues a call. Its answer goes to the callback, with `id`.
    pub fn call(&self, id: u64, call: Call) {
        let _ = self.tx.send((id, call));
    }
}

/// Tries the preferred endpoint, then the others, until one answers.
async fn call_any(chans: &[Channel], preferred: &mut usize, call: Call) -> Answer {
    let mut last = Answer::Failed {
        code: tonic::Code::Unavailable,
        message: "no endpoint answered".into(),
    };
    for k in 0..chans.len() {
        let i = (*preferred + k) % chans.len();
        match call_one(chans[i].clone(), call.clone()).await {
            Answer::Failed { code, message }
                if matches!(
                    code,
                    tonic::Code::Unavailable | tonic::Code::DeadlineExceeded
                ) =>
            {
                last = Answer::Failed { code, message };
            }
            a => {
                *preferred = i;
                return a;
            }
        }
    }
    last
}

async fn unary<Req, Resp>(
    ch: Channel,
    method: &'static str,
    req: Req,
) -> Result<Resp, tonic::Status>
where
    Req: prost::Message + Send + Sync + 'static,
    Resp: prost::Message + Default + Send + Sync + 'static,
{
    let mut grpc = tonic::client::Grpc::new(ch);
    grpc.ready()
        .await
        .map_err(|e| tonic::Status::unavailable(e.to_string()))?;
    let path = PathAndQuery::from_maybe_shared(format!("{SERVICE}{method}"))
        .map_err(|e| tonic::Status::internal(e.to_string()))?;
    let codec = tonic_prost::ProstCodec::<Req, Resp>::default();
    grpc.unary(tonic::Request::new(req), path, codec)
        .await
        .map(|r| r.into_inner())
}

async fn call_one(ch: Channel, call: Call) -> Answer {
    let r = match call {
        Call::GetVolume(r) => unary(ch, "GetVolume", r).await.map(Answer::GetVolume),
        Call::ChangeIsr(r) => unary(ch, "ChangeIsr", r).await.map(Answer::ChangeIsr),
        Call::RegisterNode(r) => unary(ch, "RegisterNode", r).await.map(Answer::RegisterNode),
        Call::NodeHeartbeat(r) => unary(ch, "NodeHeartbeat", r)
            .await
            .map(Answer::NodeHeartbeat),
    };
    r.unwrap_or_else(|s| Answer::Failed {
        code: s.code(),
        message: s.message().to_string(),
    })
}

/// Makes one call and waits for its answer. For tools and tests, not the engine.
pub fn call_blocking(client_cfg: &Config, call: Call) -> Result<Answer, ClientError> {
    let (tx, rx) = mpsc::channel();
    let c = MetaClient::spawn(client_cfg.clone(), move |_, a| {
        let _ = tx.send(a);
    })?;
    c.call(0, call);
    drop(c);
    Ok(rx.recv().unwrap_or(Answer::Failed {
        code: tonic::Code::Unavailable,
        message: "client stopped".into(),
    }))
}
