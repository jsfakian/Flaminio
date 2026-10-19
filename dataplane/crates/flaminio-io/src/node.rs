//! `flaminio-io node`: a phase-1 data-plane node (docs/phase1-plan.md, step 4).
//!
//! **Every node** serves replicas of its local store to the volume targets of other nodes
//! (and its own), over mutual TLS. It registers with the metadata service and sends liveness
//! heartbeats.
//!
//! **With `--attach VOLUME --token FILE`** (Linux), the node also becomes the volume's target,
//! with the capability token from the volume's attach. It serves the volume as
//! `/dev/ublkbN`, replicated as the metadata service placed it.
//!
//! One thread, the engine, owns the store and every state machine:
//! - **Network and metadata threads** hand it their events through a channel, and wake it.
//! - **With a ublk device,** the engine lives on the device's queue thread. That thread waits on
//!   its io_uring, so the wake-up is an eventfd read queued on the same ring, and a 10 ms timeout
//!   drives the engine's ticks.

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flaminio_metaclient::MetaClient;
use flaminio_net::tls::Files;
use flaminio_net::token::TokenKey;
use flaminio_node::engine::{Engine, EngineConfig, Input, Output};
use flaminio_node::net::{self, NetConfig, ServerSide};
use flaminio_node::replica::ReplicaService;
use flaminio_repl::types::Timing;
use flaminio_store::{Allocation, BlockDevice, Store, StoreConfig};

use crate::cli::{format_if_asked, parse_size};

const USAGE: &str = "\
usage: flaminio-io node --device PATH --node NAME --listen IP:PORT --meta IP:PORT[,IP:PORT...]
                        --pki DIR --trust-domain TD [options]
      --advertise IP:PORT  replication address registered for this node (default: --listen)
      --format             format the device first (refuses an existing Flaminio disk
                           unless --force)
      --force              allow --format to destroy an existing Flaminio disk
      --buffered           use the page cache instead of O_DIRECT
      --attach VOLUME      also be VOLUME's target, serving it as /dev/ublkbN (Linux)
      --token FILE         the capability token from VOLUME's attach (raw bytes)
      --depth N            ublk queue depth (default 128)
      --io-buf SIZE        largest single I/O, at most 1M (default 512K)
      --poll-us N          after any completion, a queue thread checks its ring without sleeping
                           for N microseconds before it blocks (default 0: never). Spends CPU
                           under load to save the wake-ups of low queue depths
      --queues N           ublk queues, each served by a thread of its own (default 1). The
                           kernel maps each submitting CPU to one queue: more queues serve more
                           submitting threads at once
      --no-verify          EXPERIMENT ONLY: skip the checksum check of local reads (to measure
                           what verification costs)
      --zero-copy          local reads go straight from the disk into the request's pages
                           (ublk auto buffer registration): no copy, and so no checksum
                           verification of those reads. For measuring; see docs/performance-plan.md
      --gather-us N        after a commit, the next waits up to N microseconds for the writes
                           just acknowledged to come back, so that they commit together
                           (default 300; 0: never waits)
      --sync-commits       commits block the queue thread until durable (as before commits
                           went asynchronous): for comparing
      --pipelined-commits  up to 4 commits in flight (spec §6.2): for measuring. Off by
                           default; opening with this sets the on-disk feature. A disk that
                           already has it still recovers by its rules without this flag
      --first-fit          new data goes to the lowest free clusters, reusing freed space,
                           instead of continuing from the last allocation: for comparing
      --id N               ublk device id (default: first free)
      --control PATH       control socket for attach and detach (default /run/flaminio/node.sock)

PKI directory: io.crt and io.key (this node's data-plane identity), ca.crt (the CA bundle) and
token.pub (the metadata service's token key). Runs in the foreground.";

#[derive(Clone)]
pub(crate) struct NodeConfig {
    pub device: String,
    pub format: bool,
    pub force: bool,
    pub buffered: bool,
    pub node: String,
    pub listen: Option<SocketAddr>,
    pub advertise: Option<SocketAddr>,
    pub meta: Vec<String>,
    pub pki: PathBuf,
    pub trust_domain: String,
    pub attach: Option<String>,
    pub token: Option<PathBuf>,
    pub depth: u16,
    pub io_buf: u32,
    pub id: i32,
    pub control: PathBuf,
    pub zero_copy: bool,
    pub no_verify: bool,
    pub queues: u16,
    pub poll_us: u64,
    pub gather_us: Option<u64>,
    pub sync_commits: bool,
    pub pipelined_commits: bool,
    pub first_fit: bool,
}

fn parse(args: &[String]) -> Result<NodeConfig, String> {
    let mut c = NodeConfig {
        device: String::new(),
        format: false,
        force: false,
        buffered: false,
        node: String::new(),
        listen: None,
        advertise: None,
        meta: Vec::new(),
        pki: PathBuf::from("/etc/flaminio/pki"),
        trust_domain: String::new(),
        attach: None,
        token: None,
        depth: 128,
        io_buf: 512 << 10,
        id: -1,
        control: PathBuf::from("/run/flaminio/node.sock"),
        zero_copy: false,
        no_verify: false,
        queues: 1,
        poll_us: 0,
        gather_us: None,
        sync_commits: false,
        pipelined_commits: false,
        first_fit: false,
    };
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        let addr = |s: String| s.parse::<SocketAddr>().map_err(|e| format!("{s}: {e}"));
        match flag.as_str() {
            "--device" => c.device = value()?,
            "--format" => c.format = true,
            "--force" => c.force = true,
            "--buffered" => c.buffered = true,
            "--node" => c.node = value()?,
            "--listen" => c.listen = Some(addr(value()?)?),
            "--advertise" => c.advertise = Some(addr(value()?)?),
            "--meta" => c.meta = value()?.split(',').map(str::to_string).collect(),
            "--pki" => c.pki = value()?.into(),
            "--trust-domain" => c.trust_domain = value()?,
            "--attach" => c.attach = Some(value()?),
            "--token" => c.token = Some(value()?.into()),
            "--depth" => c.depth = value()?.parse().map_err(|_| "invalid --depth")?,
            "--io-buf" => {
                c.io_buf =
                    u32::try_from(parse_size(&value()?)?).map_err(|_| "--io-buf too large")?
            }
            "--id" => c.id = value()?.parse().map_err(|_| "invalid --id")?,
            "--control" => c.control = value()?.into(),
            "--zero-copy" => c.zero_copy = true,
            "--no-verify" => c.no_verify = true,
            "--poll-us" => c.poll_us = value()?.parse().map_err(|_| "invalid --poll-us")?,
            "--gather-us" => {
                c.gather_us = Some(value()?.parse().map_err(|_| "invalid --gather-us")?)
            }
            "--sync-commits" => c.sync_commits = true,
            "--pipelined-commits" => c.pipelined_commits = true,
            "--first-fit" => c.first_fit = true,
            "--queues" => {
                c.queues = value()?.parse().map_err(|_| "invalid --queues")?;
                if c.queues == 0 || c.queues > 64 {
                    return Err("--queues must be 1 to 64".into());
                }
            }
            _ => return Err(format!("unknown argument {flag}\n\n{USAGE}")),
        }
    }
    if c.device.is_empty()
        || c.node.is_empty()
        || c.listen.is_none()
        || c.meta.is_empty()
        || c.trust_domain.is_empty()
    {
        return Err(USAGE.into());
    }
    if c.attach.is_some() != c.token.is_some() {
        return Err("--attach and --token go together".into());
    }
    // A write or read travels as one frame of at most 1 MiB of data.
    if c.io_buf == 0 || c.io_buf > (1 << 20) || !c.io_buf.is_multiple_of(4096) {
        return Err("--io-buf must be a multiple of 4K, at most 1M".into());
    }
    Ok(c)
}

/// What wakes the engine thread when another thread hands it an input.
pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

/// The engine and the channel that feeds it.
pub(crate) struct Parts<D: BlockDevice> {
    pub engine: Engine<D>,
    pub inputs: mpsc::Receiver<Input>,
    pub sender: mpsc::Sender<Input>,
}

/// Starts everything but the engine's own thread: store, replica service, network thread,
/// metadata client.
pub(crate) fn build<D: BlockDevice + 'static>(
    c: &NodeConfig,
    dev: D,
    wake: Wake,
) -> Result<Parts<D>, String> {
    format_if_asked(&dev, &c.device, c.format, c.force)?;
    let config = StoreConfig {
        allocation: if c.first_fit {
            Allocation::FirstFit
        } else {
            Allocation::NextFit
        },
        pipelined_commits: c.pipelined_commits,
        ..StoreConfig::default()
    };
    let store = Store::open(dev, config).map_err(|e| format!("open store: {e}"))?;
    let stats = store.stats();
    eprintln!(
        "flaminio-io: store ready, {} chunk replicas, {} of {} clusters used",
        stats.chunks, stats.data_used, stats.data_clusters
    );
    let service = ReplicaService::new(store);

    let read = |f: &str| {
        std::fs::read_to_string(c.pki.join(f))
            .map_err(|e| format!("{}: {e}", c.pki.join(f).display()))
    };
    let files = Files {
        cert: c.pki.join("io.crt"),
        key: c.pki.join("io.key"),
        ca: c.pki.join("ca.crt"),
    };
    let token_key =
        TokenKey::from_pem(&read("token.pub")?).map_err(|e| format!("token.pub: {e}"))?;
    let listen = c.listen.expect("checked");
    let listener = TcpListener::bind(listen).map_err(|e| format!("listen on {listen}: {e}"))?;

    let (tx, rx) = mpsc::channel::<Input>();
    let (net_tx, net_wake) = (Mutex::new(tx.clone()), wake.clone());
    let net = net::spawn(
        NetConfig {
            trust_domain: c.trust_domain.clone(),
            server: Some(ServerSide {
                listener,
                tls: files.server_config().map_err(|e| e.to_string())?,
                token_key,
            }),
            client: Some(files.client_config().map_err(|e| e.to_string())?),
        },
        Arc::new(move |e| {
            let _ = net_tx.lock().expect("sink").send(Input::Net(e));
            net_wake();
        }),
    )
    .map_err(|e| format!("network thread: {e}"))?;

    let (meta_tx, meta_wake) = (Mutex::new(tx.clone()), wake);
    let meta = MetaClient::spawn(
        flaminio_metaclient::Config {
            endpoints: c.meta.clone(),
            cert_pem: read("io.crt")?,
            key_pem: read("io.key")?,
            ca_pem: read("ca.crt")?,
            timeout: Duration::from_secs(3),
        },
        move |id, answer| {
            let _ = meta_tx
                .lock()
                .expect("sink")
                .send(Input::Meta { id, answer });
            meta_wake();
        },
    )
    .map_err(|e| format!("metadata client: {e}"))?;

    let cfg = EngineConfig {
        node: c.node.clone(),
        address: c.advertise.unwrap_or(listen).to_string(),
        timing: Timing::default(),
        max_io: 256,
        heartbeat: Some(Duration::from_secs(3)),
    };
    let engine = Engine::new(cfg, service, net, Box::new(meta));
    Ok(Parts {
        engine,
        inputs: rx,
        sender: tx,
    })
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// Drives the engine on this thread until `stop` says so, or until `limit` passes. Returns
/// the output that stopped it.
pub(crate) fn drive_until<D: BlockDevice>(
    p: &mut Parts<D>,
    limit: Duration,
    mut stop: impl FnMut(&Output) -> bool,
) -> Option<Output> {
    let end = Instant::now() + limit;
    loop {
        let now = Instant::now();
        p.engine.turn(now);
        for o in p.engine.take_output() {
            log_output(&o);
            if stop(&o) {
                return Some(o);
            }
        }
        if now >= end {
            return None;
        }
        let wait = p
            .engine
            .next_wakeup()
            .min(end)
            .saturating_duration_since(Instant::now())
            .max(Duration::from_micros(100));
        if let Ok(i) = p.inputs.recv_timeout(wait) {
            let now = Instant::now();
            p.engine.handle(now, i);
            while let Ok(i) = p.inputs.try_recv() {
                p.engine.handle(now, i);
            }
        }
    }
}

pub(crate) fn log_output(o: &Output) {
    match o {
        Output::Attached {
            volume,
            epoch,
            size,
        } => {
            eprintln!(
                "flaminio-io: target of {volume} at epoch {epoch} ({} MiB)",
                size >> 20
            )
        }
        Output::AttachFailed { volume, why } => eprintln!("flaminio-io: attach {volume}: {why}"),
        Output::Deposed { volume } => {
            eprintln!("flaminio-io: deposed as the target of {volume}: it moved to another node")
        }
        Output::Detached { volume } => eprintln!("flaminio-io: detached {volume}"),
        Output::Done { .. } | Output::LocalRead(_) | Output::Piece { .. } => {}
    }
}

pub fn main(args: &[String]) -> Result<(), String> {
    let c = parse(args)?;
    #[cfg(target_os = "linux")]
    {
        let dev = flaminio_uring::UringDevice::open(&c.device, !c.buffered)
            .map_err(|e| format!("open {}: {e}", c.device))?;
        let efd = Arc::new(flaminio_uring::EventFd::new().map_err(|e| format!("eventfd: {e}"))?);
        let wake: Wake = {
            let efd = efd.clone();
            Arc::new(move || efd.signal())
        };
        let parts = build(&c, dev, wake)?;
        crate::ublk::serve_node(&c, parts, efd)
    }
    #[cfg(not(target_os = "linux"))]
    {
        if c.attach.is_some() {
            return Err("--attach needs Linux (ublk)".into());
        }
        let dev = flaminio_store::FileDevice::open(&c.device)
            .map_err(|e| format!("open {}: {e}", c.device))?;
        serve_replicas(build(&c, dev, Arc::new(|| {}))?)
    }
}

/// A node without a volume target: the engine serves replicas until the process ends.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn serve_replicas<D: BlockDevice>(p: Parts<D>) -> Result<(), String> {
    eprintln!("flaminio-io: serving replicas");
    let Parts {
        engine,
        inputs,
        sender,
    } = p;
    let _keep = sender;
    engine.run(inputs, |o| log_output(&o));
    Ok(())
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// The token file: the raw token bytes.
pub(crate) fn read_token(path: &PathBuf) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// Decodes standard base64 (with or without padding), as the metadata service's JSON
/// carries tokens. None if it is not valid base64.
pub(crate) fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let s = s.trim_end_matches('=').as_bytes();
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for group in s.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in group.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..group.len()]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::base64_decode;

    #[test]
    fn base64() {
        assert_eq!(base64_decode("").unwrap(), b"");
        assert_eq!(base64_decode("Zg==").unwrap(), b"f");
        assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
        assert_eq!(base64_decode("Zm9v").unwrap(), b"foo");
        assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");
        assert_eq!(base64_decode("Zm9vYg").unwrap(), b"foob");
        assert_eq!(base64_decode("/+8=").unwrap(), [0xff, 0xef]);
        assert!(base64_decode("Zm9v!").is_none());
        assert!(base64_decode("Z").is_none());
    }
}
