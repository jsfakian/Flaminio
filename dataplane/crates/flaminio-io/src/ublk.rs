//! The ublk frontend: one queue, served by one thread that owns the replica store.
//!
//! Each ublk tag has an async task that waits for the kernel to deliver a request.
//!
//! - **Reads** are handled by the tag task itself, fully asynchronously: the store plans the
//!   device reads, the task queues them on the same io_uring as ublk, and verifies the
//!   checksums when they complete. Meanwhile the event loop keeps accepting requests, so the
//!   device queue stays full.
//! - **Writes, flushes and discards** go to the driver task. Every round of the event loop, it
//!   takes all of them as one batch: one group commit. Writes are acknowledged only once
//!   durable, so the device advertises no volatile write cache and flushes are no-ops.

use std::cell::{Cell, RefCell};
use std::os::fd::{AsRawFd, RawFd};
use std::rc::Rc;

use flaminio_format::Uuid;
use flaminio_format::consts::GIB;
use flaminio_store::{BlockDevice, Store, StoreConfig};
use flaminio_uring::UringDevice;
use io_uring::{opcode, types};
use libublk::ctrl::{UblkCtrl, UblkCtrlBuilder};
use libublk::helpers::IoBuf;
use libublk::io::{UblkDev, UblkQueue};
use libublk::{BufDesc, UblkError, UblkFlags, sys};

use std::time::Duration;

use crate::cli::{format_if_asked, parse_size, volume_uuid};
use crate::volume::{Op, Request, Volume, VolumeLayout};

const USAGE: &str = "\
usage:
  flaminio-io serve --device PATH --size SIZE [options]
      --chunk-size SIZE   chunk size (default 1G); SIZE must be a multiple of it
      --volume NAME       volume name (default vol0)
      --epoch N           epoch for all I/O (default 1)
      --depth N           ublk queue depth (default 128)
      --io-buf SIZE       largest single I/O (default 512K)
      --id N              ublk device id (default: first free)
      --format            format the device first (refuses an existing Flaminio disk
                          unless --force)
      --force             allow --format to destroy an existing Flaminio disk
      --buffered          use the page cache instead of O_DIRECT
      --replica ADDR      mirror writes to a remote replica (flaminio-io replica) at ADDR
      --replica-timeout S give up on an unresponsive replica after S seconds (default 5)
  flaminio-io del --id N      stop and remove ublk device N

Runs in the foreground; stop it with `flaminio-io del --id N` from another shell.";

#[derive(Clone)]
struct Config {
    device: String,
    size: u64,
    chunk_size: u64,
    volume: Uuid,
    epoch: u64,
    depth: u16,
    io_buf: u32,
    id: i32,
    format: bool,
    force: bool,
    buffered: bool,
    replica: Option<String>,
    replica_timeout: Duration,
}

impl Config {
    fn layout(&self) -> VolumeLayout {
        VolumeLayout {
            volume: self.volume,
            size: self.size,
            chunk_size: self.chunk_size,
        }
    }
}

pub fn main(args: Vec<String>) -> Result<(), String> {
    let (cmd, rest) = args.split_first().ok_or(USAGE)?;
    let mut c = Config {
        device: String::new(),
        size: 0,
        chunk_size: GIB,
        volume: volume_uuid("vol0")?,
        epoch: 1,
        depth: 128,
        io_buf: 512 << 10,
        id: -1,
        format: false,
        force: false,
        buffered: false,
        replica: None,
        replica_timeout: Duration::from_secs(5),
    };
    let mut it = rest.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--device" => c.device = value()?,
            "--size" => c.size = parse_size(&value()?)?,
            "--chunk-size" => c.chunk_size = parse_size(&value()?)?,
            "--volume" => c.volume = volume_uuid(&value()?)?,
            "--epoch" => c.epoch = value()?.parse().map_err(|_| "invalid --epoch")?,
            "--depth" => c.depth = value()?.parse().map_err(|_| "invalid --depth")?,
            "--io-buf" => {
                c.io_buf =
                    u32::try_from(parse_size(&value()?)?).map_err(|_| "--io-buf too large")?
            }
            "--id" => c.id = value()?.parse().map_err(|_| "invalid --id")?,
            "--format" => c.format = true,
            "--force" => c.force = true,
            "--buffered" => c.buffered = true,
            "--replica" => c.replica = Some(value()?),
            "--replica-timeout" => {
                c.replica_timeout =
                    Duration::from_secs(value()?.parse().map_err(|_| "invalid --replica-timeout")?)
            }
            _ => return Err(format!("unknown argument {flag}\n\n{USAGE}")),
        }
    }
    match cmd.as_str() {
        "serve" => serve(c),
        "del" => {
            let ctrl =
                UblkCtrl::new_simple(c.id).map_err(|e| format!("ublk device {}: {e}", c.id))?;
            let _ = ctrl.kill_dev();
            ctrl.del_dev()
                .map_err(|e| format!("delete ublk device {}: {e}", c.id))?;
            println!("deleted ublk device {}", c.id);
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn open_device(c: &Config) -> Result<UringDevice, String> {
    UringDevice::open(&c.device, !c.buffered).map_err(|e| format!("open {}: {e}", c.device))
}

/// Formats if asked, then opens the store and the volume once, in this thread, so that
/// configuration errors are reported before a ublk device is created.
fn prepare(c: &Config) -> Result<(), String> {
    if c.device.is_empty() || c.size == 0 {
        return Err(USAGE.into());
    }
    let dev = open_device(c)?;
    format_if_asked(&dev, &c.device, c.format, c.force)?;
    let store = Store::open(dev, StoreConfig::default()).map_err(|e| format!("open store: {e}"))?;
    let volume =
        Volume::open(store, c.layout(), c.epoch).map_err(|e| format!("open volume: {e}"))?;
    let stats = volume.store().stats();
    eprintln!(
        "volume ready: {} GiB in {} chunks; disk {} of {} clusters used",
        c.size >> 30,
        c.layout().chunks(),
        stats.data_used,
        stats.data_clusters
    );
    Ok(())
}

fn refuse_legacy_target() -> Result<(), String> {
    Err("legacy serve cannot authorize a target; use node --clock-contract PATH and an authorized agent/admin Attach challenge handoff".into())
}

fn serve(c: Config) -> Result<(), String> {
    refuse_legacy_target()?;
    prepare(&c)?;
    let ctrl = UblkCtrlBuilder::default()
        .name("flaminio")
        .id(c.id)
        .depth(c.depth)
        .nr_queues(1)
        .io_buf_bytes(c.io_buf)
        .dev_flags(UblkFlags::UBLK_DEV_F_ADD_DEV)
        .build()
        .map_err(|e| format!("create ublk device (is ublk_drv loaded, and are we root?): {e}"))?;

    let (size, chunk) = (c.size, c.chunk_size);
    let tgt_init = move |dev: &mut UblkDev| {
        let max = dev.dev_info.max_io_buf_bytes;
        dev.tgt.dev_size = size;
        dev.tgt.params = sys::ublk_params {
            types: sys::UBLK_PARAM_TYPE_BASIC | sys::UBLK_PARAM_TYPE_DISCARD,
            basic: sys::ublk_param_basic {
                // No volatile cache: a completed write is durable.
                attrs: 0,
                logical_bs_shift: 12,
                physical_bs_shift: 12,
                io_min_shift: 12,
                io_opt_shift: 16,
                max_sectors: max >> 9,
                dev_sectors: size >> 9,
                ..Default::default()
            },
            discard: sys::ublk_param_discard {
                discard_granularity: 64 << 10,
                max_discard_sectors: u32::try_from(chunk >> 9).unwrap_or(u32::MAX),
                max_discard_segments: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        Ok(())
    };
    let qc = c.clone();
    let queue = move |qid: u16, dev: &UblkDev| {
        if let Err(e) = serve_queue(qid, dev, &qc) {
            eprintln!("flaminio-io: queue {qid} stopped: {e}");
        }
    };
    let ready = move |ctrl: &UblkCtrl| {
        println!(
            "flaminio-io: serving volume of {} GiB at {} (ublk id {})",
            size >> 30,
            ctrl.get_bdev_path(),
            ctrl.dev_info().dev_id
        );
    };
    ctrl.run_target(tgt_init, queue, ready)
        .map_err(|e| format!("ublk: {e}"))?;
    Ok(())
}

/// Work for the driver: a request from a tag task, or a tick for background replica work.
enum Job {
    Io {
        tag: u16,
        done: smol::channel::Sender<i32>,
    },
    Tick,
}

fn serve_queue(qid: u16, dev: &UblkDev, c: &Config) -> Result<(), String> {
    let store = Store::open(open_device(c)?, StoreConfig::default())
        .map_err(|e| format!("open store: {e}"))?;
    let volume = RefCell::new(
        Volume::open(store, c.layout(), c.epoch).map_err(|e| format!("open volume: {e}"))?,
    );
    if let Some(addr) = &c.replica {
        eprintln!("mirroring writes to replica {addr}");
        volume
            .borrow_mut()
            .attach_remote(addr.clone(), c.replica_timeout);
    }
    let q = Rc::new(UblkQueue::new(qid, dev).map_err(|e| format!("ublk queue: {e}"))?);
    let depth = dev.dev_info.queue_depth;
    let buf_bytes = dev.dev_info.max_io_buf_bytes as usize;
    let bufs: Rc<Vec<RefCell<IoBuf<u8>>>> = Rc::new(
        (0..depth)
            .map(|_| RefCell::new(IoBuf::<u8>::new(buf_bytes)))
            .collect(),
    );
    let exe = Rc::new(smol::LocalExecutor::new());
    let (tx, rx) = smol::channel::unbounded::<Job>();

    let fd = volume.borrow().store().device().as_raw_fd();
    let volume = &volume;
    let mut tasks = Vec::with_capacity(depth as usize);
    for tag in 0..depth {
        let (q, bufs, tx) = (q.clone(), bufs.clone(), tx.clone());
        tasks.push(exe.spawn(async move {
            match io_task(&q, tag, &bufs, tx, volume, fd).await {
                Ok(()) | Err(UblkError::QueueIsDown) => {}
                Err(e) => eprintln!("flaminio-io: tag {tag}: {e}"),
            }
        }));
    }
    // Background replica work runs on ticks: a 50 ms io_uring timeout on the queue's own ring,
    // so the event loop wakes up for it even when the device is idle.
    let _ticker = c.replica.is_some().then(|| {
        let (q, tx) = (q.clone(), tx.clone());
        exe.spawn(async move {
            let ts = types::Timespec::new().nsec(50_000_000);
            loop {
                let _ = q.ublk_submit_sqe(opcode::Timeout::new(&ts).build()).await;
                if tx.send(Job::Tick).await.is_err() {
                    break;
                }
            }
        })
    });
    drop(tx);
    let driver = {
        let (q, bufs) = (q.clone(), bufs.clone());
        exe.spawn(async move { drive(&q, &bufs, rx, volume).await })
    };

    let ticker = exe.clone();
    let result = smol::block_on(exe.run(async move {
        let run_ops = || while ticker.try_tick() {};
        let is_done = || tasks.iter().all(|t| t.is_finished());
        libublk::wait_and_handle_io_events(&q, Some(20), run_ops, is_done).await
    }));
    drop(driver);
    result.map_err(|e| format!("event loop: {e}"))
}

/// One tag: fetch a request, serve it (reads here, the rest through the driver), commit its
/// result, repeat.
///
/// The tag's buffer stays borrowed across the prep and commit awaits on purpose: while those
/// commands are outstanding the buffer is lent to the kernel, which fills it with the next
/// write's data. Holding the borrow makes any other access panic instead of racing the kernel.
#[allow(clippy::await_holding_refcell_ref)]
async fn io_task(
    q: &UblkQueue<'_>,
    tag: u16,
    bufs: &[RefCell<IoBuf<u8>>],
    tx: smol::channel::Sender<Job>,
    volume: &RefCell<Volume<UringDevice>>,
    fd: RawFd,
) -> Result<(), UblkError> {
    let (done_tx, done_rx) = smol::channel::bounded(1);
    {
        let buf = bufs[tag as usize].borrow();
        q.submit_io_prep_cmd(tag, BufDesc::Slice(buf.as_slice()), 0, Some(&*buf))
            .await?;
    }
    loop {
        let (op, offset, len) = {
            let iod = q.get_iod(tag);
            (
                iod.op_flags & 0xff,
                iod.start_sector << 9,
                u64::from(iod.nr_sectors) << 9,
            )
        };
        let res = if op == sys::UBLK_IO_OP_READ {
            read(q, &bufs[tag as usize], volume, fd, offset, len).await
        } else {
            if tx
                .send(Job::Io {
                    tag,
                    done: done_tx.clone(),
                })
                .await
                .is_err()
            {
                return Ok(());
            }
            done_rx.recv().await.unwrap_or(-libc::EIO)
        };
        // The buffer stays borrowed until the kernel delivers this tag's next request; the
        // driver only touches a tag's buffer while it holds that tag's request.
        let buf = bufs[tag as usize].borrow();
        q.submit_io_commit_cmd(tag, BufDesc::Slice(buf.as_slice()), res)
            .await?;
    }
}

/// Serves one read: plan it, queue its device reads on the ublk io_uring (so they run while the
/// event loop keeps accepting requests), then verify. A checksum mismatch can come from a write
/// that changed the range while the read was in flight; it is planned and read once more, and
/// only a second mismatch is an error.
async fn read(
    q: &UblkQueue<'_>,
    buf: &RefCell<IoBuf<u8>>,
    volume: &RefCell<Volume<UringDevice>>,
    fd: RawFd,
    offset: u64,
    len: u64,
) -> i32 {
    let n = len as usize;
    if n > buf.borrow().len() {
        return -libc::EINVAL;
    }
    for _attempt in 0..2 {
        let plans = match volume
            .borrow()
            .plan_read(offset, &mut buf.borrow_mut()[..n])
        {
            Ok(plans) => plans,
            Err(errno) => return errno,
        };
        // The kernel writes into this tag's buffer. Nothing else touches it until the request
        // is committed, and every read queued here is awaited before the buffer is used.
        let base = buf.borrow_mut().as_mut_ptr();
        let mut pending = Vec::new();
        for (piece, plan) in &plans {
            for r in plan.reads() {
                let dst = base.wrapping_add(piece + r.buf_offset);
                let sqe = opcode::Read::new(types::Fd(fd), dst, r.len as u32)
                    .offset(r.device_offset)
                    .build();
                pending.push((q.ublk_submit_sqe(sqe), r.len as i32));
            }
        }
        let mut io_ok = true;
        for (completion, expected) in pending {
            let completed = completion.await == expected;
            io_ok &= completed;
        }
        let verified = io_ok && {
            let data = buf.borrow();
            plans
                .iter()
                .all(|(piece, plan)| plan.verify(&data[*piece..]).is_ok())
        };
        volume.borrow().finish_read(plans);
        if !io_ok {
            return -libc::EIO;
        }
        if verified {
            return len as i32;
        }
    }
    -libc::EIO
}

/// Collects every request delivered in this round and processes them as one batch (one group
/// commit for the writes). A tick runs one step of background replica work.
async fn drive(
    q: &UblkQueue<'_>,
    bufs: &[RefCell<IoBuf<u8>>],
    rx: smol::channel::Receiver<Job>,
    volume: &RefCell<Volume<UringDevice>>,
) {
    while let Ok(first) = rx.recv().await {
        // Let the other tag tasks woken in this round queue their requests too.
        smol::future::yield_now().await;
        let mut jobs = vec![first];
        while let Ok(j) = rx.try_recv() {
            jobs.push(j);
        }
        let mut batch = Vec::with_capacity(jobs.len());
        let mut tick = false;
        for j in jobs {
            match j {
                Job::Io { tag, done } => batch.push((tag, done)),
                Job::Tick => tick = true,
            }
        }
        if !batch.is_empty() {
            let mut results = vec![0i32; batch.len()];
            {
                let mut guards: Vec<_> = batch
                    .iter()
                    .map(|(tag, _)| bufs[*tag as usize].borrow_mut())
                    .collect();
                let mut reqs = Vec::with_capacity(batch.len());
                let mut index = Vec::with_capacity(batch.len());
                for (i, ((tag, _), g)) in batch.iter().zip(guards.iter_mut()).enumerate() {
                    let iod = q.get_iod(*tag);
                    let op = match iod.op_flags & 0xff {
                        sys::UBLK_IO_OP_READ => Op::Read,
                        sys::UBLK_IO_OP_WRITE => Op::Write,
                        sys::UBLK_IO_OP_FLUSH => Op::Flush,
                        sys::UBLK_IO_OP_DISCARD => Op::Discard,
                        _ => {
                            results[i] = -libc::EOPNOTSUPP;
                            continue;
                        }
                    };
                    reqs.push(Request {
                        op,
                        offset: iod.start_sector << 9,
                        len: u64::from(iod.nr_sectors) << 9,
                        buf: &mut g[..],
                    });
                    index.push(i);
                }
                let rs = volume.borrow_mut().process(&mut reqs);
                for (r, i) in rs.into_iter().zip(index) {
                    results[i] = r;
                }
            }
            for ((_, done), r) in batch.into_iter().zip(results) {
                let _ = done.send(r).await;
            }
        }
        if tick {
            volume.borrow_mut().maintain();
        }
    }
}

// --- Phase 1: a volume served by the node engine after the authorized control handshake -----

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use flaminio_net::handoff::VerifiedHandoff;
use flaminio_node::clock::{ClockObserver, SuccessPermit};
use flaminio_node::engine::{Input, Io, IoError, LocalRead, Output};
use flaminio_uring::{AlignedBuf, EventFd, Staging};

use crate::node::{NodeConfig, Parts, drive_until, log_output};

/// What a tag task waits for while the engine serves its request.
enum Event {
    /// A piece to read from the local store, into the tag's buffer.
    Local(LocalRead),
    /// A piece that came from another replica.
    Piece {
        at: usize,
        data: bytes::Bytes,
    },
    Done(Result<Option<Vec<u8>>, IoError>),
}

type Waiter = smol::channel::Sender<Event>;

/// What every queue thread of a device shares: the engine, and the completions its tag tasks
/// wait for. Locks are held only between awaits: admission, planning, completion.
struct Shared<D: BlockDevice = UringDevice> {
    engine: Mutex<flaminio_node::engine::Engine<D>>,
    inputs: Mutex<mpsc::Receiver<Input>>,
    sender: mpsc::Sender<Input>,
    /// The store's device, for the local reads the tag tasks perform, and its path, for logs.
    fd: RawFd,
    device: String,
    /// A zero-copy read was copied through the staging area, and logged (docs/issues/5): once per
    /// device, as a line per read would flood the log under a workload misaligned on purpose.
    staged: AtomicBool,
    live: Arc<AtomicBool>,
    /// Op id → the waiting tag task's queue and channel.
    waiters: Mutex<HashMap<u64, (u16, Waiter)>>,
    next_op: AtomicU64,
    volume: String,
    /// The ublk device's id: a deposed target stops its device.
    dev_id: i32,
    /// Local reads are checked against their checksums (off only for experiments).
    verify: bool,
    /// How long a queue thread checks its ring without sleeping after a completion.
    poll: Duration,
    /// Queue rings with SINGLE_ISSUER and DEFER_TASKRUN (an experiment).
    defer_taskrun: bool,
    /// Per queue: what wakes its thread when another thread completes one of its operations.
    /// Queue 0's is the node's eventfd, which the network and metadata threads signal too.
    wakes: Vec<Arc<EventFd>>,
    /// The engine holds staged groups (docs/issues/10): large writes are handed over as soon as
    /// each is copied in, and counted in `arrivals` meanwhile.
    holding: bool,
    /// The store's cluster size, for telling large writes.
    cluster_size: u64,
    /// The large writes being copied in, shared with the engine.
    arrivals: Arc<AtomicUsize>,
}

/// A large write being copied in (docs/issues/10, S4): counted in the device's arrivals while it
/// lives, so that a commit waiting for writes to gather waits for it too. Dropped when the
/// write is handed to the engine, or its copy failed, or its task stops.
struct Arrival<'a>(&'a AtomicUsize);

impl<'a> Arrival<'a> {
    fn new(arrivals: &'a AtomicUsize) -> Self {
        arrivals.fetch_add(1, Ordering::Relaxed);
        Self(arrivals)
    }
}

impl Drop for Arrival<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A queue thread's handle on the shared engine.
struct EngineCtx<D: BlockDevice = UringDevice> {
    observer: RefCell<ClockObserver>,
    shared: Arc<Shared<D>>,
    qid: u16,
    /// Requests were submitted since the last pump. The event loop pumps once per round, after
    /// running the tag tasks, so that the writes of a round reach the store together and share
    /// one group commit.
    due: Cell<bool>,
    /// Writes whose data is still being copied into the staging area (zero copy). The pump
    /// waits for them, so that they join the same group commit as the writes before them.
    /// Large writes, while the engine holds groups, are not counted here but in arrivals: they
    /// go to the engine one by one (docs/issues/10, S3).
    staging: Cell<u32>,
    /// Arms this queue's timer for the end of a commit's wait for writes to gather.
    gather: std::cell::OnceCell<smol::channel::Sender<Instant>>,
}

impl<D: BlockDevice> EngineCtx<D> {
    fn observe(&self) -> bool {
        let healthy = self.observer.borrow_mut().sample().is_ok();
        if !healthy {
            self.shared.live.store(false, Ordering::SeqCst);
            self.shared.wakes[0].signal();
        }
        healthy
    }

    fn noop_permit(&self) -> Option<SuccessPermit> {
        let mut observer = self.observer.borrow_mut();
        let sample = observer.sample().ok()?;
        observer.success_permit(sample).ok()
    }

    fn engine(&self) -> std::sync::MutexGuard<'_, flaminio_node::engine::Engine<D>> {
        self.shared.engine.lock().expect("engine")
    }

    /// Feeds the engine what other threads sent, flushes it (a full turn with ticks when
    /// `tick`), and delivers completions, waking the queue threads they belong to.
    fn pump_with(&self, tick: bool) {
        let outputs = {
            let mut e = self.engine();
            if let Ok(inputs) = self.shared.inputs.try_lock() {
                while let Ok(i) = inputs.try_recv() {
                    e.handle(i);
                }
            }
            if tick {
                e.turn();
            } else {
                e.flush();
            }
            if let (Some(at), Some(tx)) = (e.take_gather_timer(), self.gather.get()) {
                let _ = tx.try_send(at);
            }
            e.take_output()
        };
        let mut wake = vec![false; self.shared.wakes.len()];
        for o in outputs {
            let mut waiters = self.shared.waiters.lock().expect("waiters");
            let mut deliver = |op: u64, ev: Event, last: bool| {
                let w = if last {
                    waiters.remove(&op)
                } else {
                    waiters.get(&op).cloned()
                };
                if let Some((q, tx)) = w {
                    let _ = tx.try_send(ev);
                    wake[q as usize] |= q != self.qid;
                }
            };
            let _ = self.observe();
            match o {
                Output::Done { op, result, permit } => {
                    if let Some(permit) = permit {
                        permit.consume();
                    }
                    deliver(op, Event::Done(result), true)
                }
                Output::LocalRead(lr) => deliver(lr.op, Event::Local(lr), false),
                Output::Piece { op, at, data, .. } => deliver(op, Event::Piece { at, data }, false),
                Output::Deposed { ref volume } if *volume == self.shared.volume => {
                    self.shared.live.store(false, Ordering::SeqCst);
                    log_output(&o);
                    // Every I/O would fail from now on. Stopping the device frees the node
                    // for its next attach; a queue thread cannot wait for its own end, so
                    // another thread asks.
                    let id = self.shared.dev_id;
                    thread::spawn(move || {
                        if let Err(e) = UblkCtrl::new_simple(id).and_then(|c| c.kill_dev()) {
                            eprintln!("flaminio-io: stop ublk device {id} after deposition: {e}");
                        }
                    });
                }
                Output::Detached { ref volume } if *volume == self.shared.volume => {
                    self.shared.live.store(false, Ordering::SeqCst);
                    log_output(&o);
                }
                other => log_output(&other),
            }
        }
        for (q, w) in wake.into_iter().enumerate() {
            if w {
                self.shared.wakes[q].signal();
            }
        }
    }

    /// After an I/O event: no tick.
    fn pump(&self) {
        self.due.set(false);
        self.pump_with(false);
    }

    /// Pumps if requests were submitted since the last pump and no write is still being
    /// staged. Returns whether it did.
    fn pump_if_due(&self) -> bool {
        let due = self.due.get() && self.staging.get() == 0;
        if due {
            self.pump();
        }
        due
    }

    fn submit(&self, io: Io, waiter: Waiter) {
        let op = self.shared.next_op.fetch_add(1, Ordering::Relaxed);
        self.shared
            .waiters
            .lock()
            .expect("waiters")
            .insert(op, (self.qid, waiter));
        self.engine().handle(Input::Io {
            volume: self.shared.volume.clone(),
            op,
            io,
        });
        // Pumped at the end of the event loop's round: see `due`.
        self.due.set(true);
    }
}

/// The engine's slot: whoever drives the engine holds it, the main thread while no volume is
/// attached and the device's queue thread while one is.
pub type Slot = Arc<Mutex<Option<Parts<UringDevice>>>>;

/// An existing device's exact same-process lifetime. Its proof can answer a lost reply,
/// but a newly issued epoch must never reuse this device while it serves the old one.
#[derive(Clone)]
pub struct LiveDevice {
    pub volume: String,
    pub id: i32,
    pub proof: VerifiedHandoff,
    pub live: Arc<AtomicBool>,
    engine: std::sync::Weak<dyn EngineAccess>,
}
impl LiveDevice {
    pub fn retry_permit(&self, proof: &VerifiedHandoff) -> Option<SuccessPermit> {
        if !self.live.load(Ordering::SeqCst) || self.proof.capability() != proof.capability() {
            return None;
        }
        let shared = self.engine.upgrade()?;
        shared.attachment_permit(&self.volume, proof)
    }
}
pub type Attached = Arc<Mutex<Option<LiveDevice>>>;

trait EngineAccess: Send + Sync {
    fn attachment_permit(&self, volume: &str, proof: &VerifiedHandoff) -> Option<SuccessPermit>;
}
impl<D: BlockDevice + Send> EngineAccess for Shared<D> {
    fn attachment_permit(&self, volume: &str, proof: &VerifiedHandoff) -> Option<SuccessPermit> {
        self.engine.lock().ok()?.attachment_permit(volume, proof)
    }
}

/// The real tag loop and deterministic completion tests share this submission boundary.
/// A returned future owns the submission until its CQE; plans stay in the caller meanwhile.
trait QueueIo {
    fn submit(&self, sqe: io_uring::squeue::Entry) -> impl std::future::Future<Output = i32>;
    fn commit_copy(
        &self,
        tag: u16,
        bytes: &[u8],
        result: i32,
    ) -> impl std::future::Future<Output = Result<i32, UblkError>>;
    fn commit_zc(
        &self,
        tag: u16,
        result: i32,
    ) -> impl std::future::Future<Output = Result<i32, UblkError>>;
}
impl QueueIo for UblkQueue<'_> {
    fn submit(&self, sqe: io_uring::squeue::Entry) -> impl std::future::Future<Output = i32> {
        self.ublk_submit_sqe(sqe)
    }
    async fn commit_copy(&self, tag: u16, bytes: &[u8], result: i32) -> Result<i32, UblkError> {
        self.submit_io_commit_cmd(tag, BufDesc::Slice(bytes), result)
            .await
    }
    async fn commit_zc(&self, tag: u16, result: i32) -> Result<i32, UblkError> {
        let reg = sys::ublk_auto_buf_reg {
            index: tag,
            ..Default::default()
        };
        self.submit_io_commit_cmd(tag, BufDesc::AutoReg(reg), result)
            .await
    }
}

/// A command from the control socket to the main thread.
pub enum Control {
    Attach {
        volume: String,
        proof: VerifiedHandoff,
        reply: mpsc::Sender<String>,
        /// When the caller stops waiting. The node skips a request that it would begin after
        /// it, and bounds the attach's steps by it (docs/issues/6, S2).
        deadline: Instant,
    },
}

/// How long an attach waits for the metadata service to answer the target's start (docs/issues/6).
pub(crate) const METADATA_LIMIT: Duration = Duration::from_secs(30);
/// How long adding the ublk device is retried while a previous device with its id goes away.
pub(crate) const ADD_DEVICE_LIMIT: Duration = Duration::from_secs(10);
/// An allowance for starting the device, not enforced: libublk waits up to 3 s for its
/// character device, and a kernel that scans partitions inside the start waits for the first
/// reads, up to a takeover (5.5 s).
pub(crate) const START_LIMIT: Duration = Duration::from_secs(10);

/// Runs a node: the main thread drives the engine (replicas only) until an attach command
/// arrives, then serves that volume as a ublk device until it is detached, and so on. The
/// engine and its state survive attaches and detaches; one volume at a time.
pub fn serve_node(
    c: &NodeConfig,
    parts: Parts<UringDevice>,
    efd: Arc<EventFd>,
    nonce: [u8; 32],
) -> Result<(), String> {
    parts
        .engine
        .device()
        .notify_commits(efd.as_raw_fd())
        .map_err(|e| format!("commit notifications: {e}"))?;
    let engine_tx = parts.sender.clone();
    let slot: Slot = Arc::new(Mutex::new(Some(parts)));
    let attached: Attached = Arc::new(Mutex::new(None));
    let (ctl_tx, ctl_rx) = mpsc::channel::<Control>();
    let wake: Arc<dyn Fn() + Send + Sync> = {
        let (efd, tx) = (efd.clone(), Mutex::new(engine_tx.clone()));
        Arc::new(move || {
            let _ = tx.lock().expect("sender").send(Input::Nop);
            efd.signal();
        })
    };
    let clock = {
        let guard = slot.lock().expect("slot");
        let engine = &guard.as_ref().expect("engine").engine;
        crate::control::ClockControl {
            domain: engine.clock_domain().clone(),
            nonce,
            key: flaminio_net::token::TokenKey::from_pem(
                &std::fs::read_to_string(c.pki.join("token.pub")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?,
        }
    };
    crate::control::spawn(
        &c.control,
        ctl_tx.clone(),
        engine_tx,
        wake,
        attached.clone(),
        clock,
    )?;
    eprintln!(
        "flaminio-io: serving replicas; control socket {}",
        c.control.display()
    );
    loop {
        {
            let mut guard = slot.lock().expect("slot");
            let parts = guard.as_mut().expect("the main thread holds the engine");
            drive_until(parts, Duration::from_millis(50), |_| false);
        }
        match ctl_rx.try_recv() {
            Ok(Control::Attach {
                volume,
                proof,
                reply,
                deadline,
            }) => {
                let request = AttachRequest {
                    volume,
                    proof,
                    // The target's start and adding the device end by then: the rest is left
                    // for starting the device (docs/issues/6).
                    steps_by: deadline.checked_sub(START_LIMIT).unwrap_or(deadline),
                };
                let result = if Instant::now() >= request.steps_by {
                    // No step would have any time left.
                    Err(format!(
                        "attach {}: its caller's time ran out before it could begin",
                        request.volume
                    ))
                } else {
                    attach_and_serve(c, &slot, &attached, efd.clone(), &reply, request)
                };
                if let Err(e) = result {
                    // The caller may have given up waiting: the log keeps the reason.
                    eprintln!("flaminio-io: {e}");
                    let _ = reply.send(format!("error: {e}"));
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

/// An attach taken from the control socket.
struct AttachRequest {
    volume: String,
    proof: VerifiedHandoff,
    /// When the target's start and adding the device must have ended: the caller's deadline
    /// less `START_LIMIT`, which is left for starting the device.
    steps_by: Instant,
}

/// Attaches the request's volume, then serves it as a ublk device until the device stops (on
/// detach). Replies "ok /dev/ublkbN" once the device is up. Its steps end by `steps_by`, so
/// that the answer, or the reason there is none, reaches the caller before it stops waiting.
fn attach_and_serve(
    c: &NodeConfig,
    slot: &Slot,
    attached: &Attached,
    efd: Arc<EventFd>,
    reply: &mpsc::Sender<String>,
    request: AttachRequest,
) -> Result<(), String> {
    let AttachRequest {
        volume,
        proof,
        steps_by,
    } = request;
    let size = {
        let mut guard = slot.lock().expect("slot");
        let parts = guard.as_mut().expect("the main thread holds the engine");
        parts.engine.handle(Input::Attach {
            volume: volume.clone(),
            proof: proof.clone(),
        });
        let wait = METADATA_LIMIT.min(steps_by.saturating_duration_since(Instant::now()));
        match drive_until(parts, wait, |o| {
            matches!(o, Output::Attached { .. } | Output::AttachFailed { .. })
        }) {
            Some(Output::Attached { size, .. }) => size,
            Some(Output::AttachFailed { why, .. }) => {
                return Err(format!("attach {volume}: {why}"));
            }
            _ => {
                // Drop the attach in the engine: an answer that comes later must not start a
                // target that no device serves (docs/issues/6, S3).
                parts.engine.handle(Input::Detach {
                    volume: volume.clone(),
                });
                // Say so when the wait was cut short: then the caller's time ran out too.
                let cut = if wait < METADATA_LIMIT {
                    ", all the time its caller had left"
                } else {
                    ""
                };
                return Err(format!(
                    "attach {volume}: no answer from the metadata service within {}{cut}",
                    crate::control::seconds(wait)
                ));
            }
        }
    };
    let result = run_device(
        c,
        slot,
        attached,
        (&volume, size, proof),
        efd,
        reply,
        steps_by,
    );
    *attached.lock().expect("attached") = None;
    // Whatever stopped the device, the target ends with it.
    if let Some(parts) = slot.lock().expect("slot").as_mut() {
        parts.engine.handle(Input::Detach { volume });
    }
    result
}

/// Adds the node's ublk device. The previous device with the same id may still be going away
/// (a deposed target's device is stopped by another thread): the kernel refuses the id until
/// it is gone, so adding is tried again for a few seconds, and not past `by`.
fn add_device(
    c: &NodeConfig,
    by: Instant,
    clock: &Arc<flaminio_node::clock::ClockDomain>,
) -> Result<UblkCtrl, String> {
    let mut observer = clock.observer();
    let deadline = (Instant::now() + ADD_DEVICE_LIMIT).min(by);
    // Zero copy: the kernel registers each request's pages as fixed buffer `tag` of the
    // queue's ring before handing the request over (see `engine_io_task_zc`).
    let ctrl_flags = if c.zero_copy {
        (sys::UBLK_F_AUTO_BUF_REG | sys::UBLK_F_SUPPORT_ZERO_COPY) as u64
    } else {
        0
    };
    loop {
        let sample = observer.sample().map_err(|e| e.to_string())?;
        clock.admit(sample).map_err(|e| e.to_string())?;
        match UblkCtrlBuilder::default()
            .name("flaminio")
            .id(c.id)
            .depth(c.depth)
            .nr_queues(c.queues)
            .io_buf_bytes(c.io_buf)
            .ctrl_flags(ctrl_flags)
            .dev_flags(UblkFlags::UBLK_DEV_F_ADD_DEV)
            .build()
        {
            Ok(ctrl) => return Ok(ctrl),
            Err(e) if Instant::now() < deadline => {
                eprintln!("flaminio-io: add ublk device {}: {e}; trying again", c.id);
                thread::sleep(Duration::from_millis(250));
            }
            Err(e) => {
                return Err(format!(
                    "create ublk device (is ublk_drv loaded, and are we root?): {e}"
                ));
            }
        }
    }
}

fn run_device(
    c: &NodeConfig,
    slot: &Slot,
    attached: &Attached,
    (volume, size, proof): (&str, u64, VerifiedHandoff),
    efd: Arc<EventFd>,
    reply: &mpsc::Sender<String>,
    add_by: Instant,
) -> Result<(), String> {
    let clock = slot
        .lock()
        .expect("slot")
        .as_ref()
        .ok_or("engine unavailable")?
        .engine
        .clock_domain()
        .clone();
    let ctrl = add_device(c, add_by, &clock)?;
    let tgt_init = move |dev: &mut UblkDev| {
        let max = dev.dev_info.max_io_buf_bytes;
        dev.tgt.dev_size = size;
        dev.tgt.params = sys::ublk_params {
            types: sys::UBLK_PARAM_TYPE_BASIC,
            basic: sys::ublk_param_basic {
                // No volatile cache: a completed write is durable on the whole ISR.
                attrs: 0,
                logical_bs_shift: 12,
                physical_bs_shift: 12,
                io_min_shift: 12,
                io_opt_shift: 16,
                max_sectors: max >> 9,
                dev_sectors: size >> 9,
                ..Default::default()
            },
            ..Default::default()
        };
        Ok(())
    };
    // The queue threads share the engine while the device runs; the main thread gets it back
    // after.
    let Some(Parts {
        mut engine,
        inputs,
        sender,
    }) = slot.lock().expect("slot").take()
    else {
        return Err("the engine is not with the main thread".into());
    };
    // The queue threads pump the engine when the store's device signals a commit done (the
    // node's eventfd, queue 0's wake-up), so commits need not block them.
    engine.set_async_commits(!c.sync_commits);
    if let Some(us) = c.gather_us {
        engine.set_commit_gather(Duration::from_micros(us));
    }
    if let Some(mib) = c.commit_target_mib {
        engine.set_commit_target_bytes(mib << 20);
    }
    // Read after every engine setting above (docs/issues/10, S3). A new counter per device: a count
    // left by a device that stopped mid-copy cannot hold the next one's commits.
    let (holding, cluster_size) = (engine.holding(), engine.cluster_size());
    let arrivals = Arc::new(AtomicUsize::new(0));
    engine.set_arrivals(arrivals.clone());
    let mut wakes = vec![efd];
    for _ in 1..c.queues {
        wakes.push(Arc::new(
            EventFd::new().map_err(|e| format!("eventfd: {e}"))?,
        ));
    }
    let verify = !c.no_verify;
    if !verify {
        eprintln!("flaminio-io: EXPERIMENT: local reads are not checked against their checksums");
    }
    let shared = Arc::new(Shared {
        fd: engine.device().as_raw_fd(),
        device: c.device.clone(),
        staged: AtomicBool::new(false),
        live: Arc::new(AtomicBool::new(false)),
        engine: Mutex::new(engine),
        inputs: Mutex::new(inputs),
        sender,
        waiters: Mutex::new(HashMap::new()),
        next_op: AtomicU64::new(1),
        volume: volume.to_string(),
        dev_id: ctrl.dev_info().dev_id as i32,
        verify,
        poll: Duration::from_micros(c.poll_us),
        defer_taskrun: c.defer_taskrun,
        wakes,
        holding,
        cluster_size,
        arrivals,
    });
    let (qshared, zero_copy) = (shared.clone(), c.zero_copy);
    let queue = move |qid: u16, dev: &UblkDev| {
        let observer = qshared
            .engine
            .lock()
            .expect("engine")
            .clock_domain()
            .observer();
        let ctx = EngineCtx {
            observer: RefCell::new(observer),
            shared: qshared.clone(),
            qid,
            due: Cell::new(false),
            staging: Cell::new(0),
            gather: std::cell::OnceCell::new(),
        };
        if let Err(e) = serve_with(qid, dev, &ctx, zero_copy) {
            eprintln!("flaminio-io: queue {qid} stopped: {e}");
        }
    };
    let (ready_reply, ready_attached, ready_vol) = (
        Mutex::new(reply.clone()),
        attached.clone(),
        volume.to_string(),
    );
    let ready_shared = shared.clone();
    let ready = move |ctrl: &UblkCtrl| {
        let permit = ready_shared
            .engine
            .lock()
            .expect("engine")
            .attachment_permit(&ready_vol, &proof);
        let Some(permit) = permit else {
            let id = ctrl.dev_info().dev_id as i32;
            thread::spawn(move || {
                let _ = UblkCtrl::new_simple(id).and_then(|c| c.kill_dev());
            });
            let _ = ready_reply
                .lock()
                .expect("reply")
                .send("error: target lost authority while starting device".into());
            return;
        };
        let path = ctrl.get_bdev_path();
        let id = ctrl.dev_info().dev_id as i32;
        println!(
            "flaminio-io: serving {} MiB at {path} (ublk id {id})",
            size >> 20
        );
        ready_shared.live.store(true, Ordering::SeqCst);
        *ready_attached.lock().expect("attached") = Some(LiveDevice {
            volume: ready_vol.clone(),
            id,
            proof: proof.clone(),
            live: ready_shared.live.clone(),
            engine: Arc::downgrade(&(ready_shared.clone() as Arc<dyn EngineAccess>)),
        });
        permit.consume();
        let _ = ready_reply
            .lock()
            .expect("reply")
            .send(format!("ok {path}"));
    };
    let result = ctrl
        .run_target(tgt_init, queue, ready)
        .map_err(|e| format!("ublk: {e}"));
    // Every queue thread has ended: hand the engine back, whatever happened to the device.
    let Ok(shared) = Arc::try_unwrap(shared) else {
        return Err("a queue thread still holds the engine".into());
    };
    let mut engine = shared.engine.into_inner().expect("engine");
    engine.set_async_commits(false);
    *slot.lock().expect("slot") = Some(Parts {
        engine,
        inputs: shared.inputs.into_inner().expect("inputs"),
        sender: shared.sender,
    });
    result?;
    eprintln!("flaminio-io: {volume} detached; serving replicas");
    Ok(())
}

/// Serves one ublk queue on this thread: its tag tasks, its wake-ups, and on queue 0 the
/// engine's ticks.
fn serve_with(qid: u16, dev: &UblkDev, ctx: &EngineCtx, zero_copy: bool) -> Result<(), String> {
    if ctx.shared.defer_taskrun {
        // The queue's ring, as libublk would build it (its sizes), plus SINGLE_ISSUER and
        // DEFER_TASKRUN: completion work runs only when this thread asks for events, instead
        // of interrupting it.
        let (sq, cq) = (u32::from(dev.tgt.sq_depth), u32::from(dev.tgt.cq_depth));
        libublk::ublk_init_task_ring(|cell| {
            if cell.get().is_none() {
                let ring = io_uring::IoUring::builder()
                    .setup_cqsize(cq)
                    .setup_coop_taskrun()
                    .setup_single_issuer()
                    .setup_defer_taskrun()
                    .build(sq)
                    .map_err(UblkError::IOError)?;
                cell.set(RefCell::new(ring))
                    .map_err(|_| UblkError::OtherError(-libc::EEXIST))?;
            }
            Ok(())
        })
        .map_err(|e| format!("queue ring: {e}"))?;
    }
    let q = Rc::new(UblkQueue::new(qid, dev).map_err(|e| format!("ublk queue: {e}"))?);
    let depth = dev.dev_info.queue_depth;
    let buf_bytes = dev.dev_info.max_io_buf_bytes as usize;
    let exe = Rc::new(smol::LocalExecutor::new());
    let mut tasks = Vec::with_capacity(depth as usize);
    if zero_copy {
        eprintln!(
            "flaminio-io: zero copy: local reads go straight into the requests, not verified"
        );
        let staging =
            Rc::new(Staging::new(depth as usize, buf_bytes).map_err(|e| format!("staging: {e}"))?);
        for tag in 0..depth {
            let (q, staging) = (q.clone(), staging.clone());
            tasks.push(exe.spawn(async move {
                match engine_io_task_zc(&q, tag, &staging, ctx).await {
                    Ok(()) | Err(UblkError::QueueIsDown) => {}
                    Err(e) => eprintln!("flaminio-io: tag {tag}: {e}"),
                }
            }));
        }
    } else {
        let bufs: Rc<Vec<RefCell<IoBuf<u8>>>> = Rc::new(
            (0..depth)
                .map(|_| RefCell::new(IoBuf::<u8>::new(buf_bytes)))
                .collect(),
        );
        for tag in 0..depth {
            let (q, bufs) = (q.clone(), bufs.clone());
            tasks.push(exe.spawn(async move {
                match engine_io_task(&q, tag, &bufs, ctx).await {
                    Ok(()) | Err(UblkError::QueueIsDown) => {}
                    Err(e) => eprintln!("flaminio-io: tag {tag}: {e}"),
                }
            }));
        }
    }
    // Other threads' inputs, and completions of this queue's operations that another thread
    // delivered: wait for the queue's eventfd to become readable on this ring.
    let efd = ctx.shared.wakes[qid as usize].clone();
    let _waker = {
        let q = q.clone();
        let fd = efd.as_raw_fd();
        exe.spawn(async move {
            loop {
                let sqe = opcode::PollAdd::new(types::Fd(fd), libc::POLLIN as u32).build();
                let _ = q.ublk_submit_sqe(sqe).await;
                efd.drain();
                ctx.pump();
            }
        })
    };
    // A commit waiting for writes to gather: pump when its wait ends. A timeout per wait, each
    // in its own task, so that a short wait never queues behind a long one (docs/issues/10).
    let (gather_tx, gather_rx) = smol::channel::unbounded::<Instant>();
    let _ = ctx.gather.set(gather_tx);
    let _gather = {
        // A weak handle: a task of the executor holding it strongly would keep the executor,
        // and every task's queue handle, alive past the device (`UblkQueue::drop` unmaps the
        // device's command buffer, which deleting the device waits for).
        let (q, timers) = (q.clone(), Rc::downgrade(&exe));
        exe.spawn(async move {
            while let Ok(at) = gather_rx.recv().await {
                let Some(d) = at
                    .checked_duration_since(Instant::now())
                    .filter(|d| !d.is_zero())
                else {
                    ctx.pump();
                    continue;
                };
                let Some(timers) = timers.upgrade() else {
                    break;
                };
                let q = q.clone();
                timers
                    .spawn(async move {
                        let ts = types::Timespec::new()
                            .sec(d.as_secs())
                            .nsec(d.subsec_nanos());
                        let _ = q.ublk_submit_sqe(opcode::Timeout::new(&ts).build()).await;
                        ctx.pump();
                    })
                    .detach();
            }
        })
    };
    // Timeouts, heartbeats and leases: a turn every 10 ms, on queue 0.
    let _ticker = (qid == 0).then(|| {
        let q = q.clone();
        exe.spawn(async move {
            let ts = types::Timespec::new().nsec(10_000_000);
            loop {
                let _ = q.ublk_submit_sqe(opcode::Timeout::new(&ts).build()).await;
                ctx.pump_with(true);
            }
        })
    });
    let runner = exe.clone();
    let result = smol::block_on(exe.run(async move {
        // Run the tasks until they wait, then pump what they submitted, which can wake them.
        let run_ops = || loop {
            while runner.try_tick() {}
            if !ctx.pump_if_due() {
                break;
            }
        };
        let is_done = || tasks.iter().all(|t| t.is_finished());
        event_loop(&q, ctx.shared.poll, run_ops, is_done).await
    }));
    result.map_err(|e| format!("event loop: {e}"))
}

/// One tag: fetch a request, hand it to the engine, commit the result, repeat. The buffer
/// discipline is `io_task`'s: see there.
#[allow(clippy::await_holding_refcell_ref)]
async fn engine_io_task(
    q: &UblkQueue<'_>,
    tag: u16,
    bufs: &[RefCell<IoBuf<u8>>],
    ctx: &EngineCtx,
) -> Result<(), UblkError> {
    let events = smol::channel::unbounded();
    {
        let buf = bufs[tag as usize].borrow();
        q.submit_io_prep_cmd(tag, BufDesc::Slice(buf.as_slice()), 0, Some(&*buf))
            .await?;
    }
    loop {
        let (op, offset, len) = {
            let iod = q.get_iod(tag);
            (
                iod.op_flags & 0xff,
                iod.start_sector << 9,
                u64::from(iod.nr_sectors) << 9,
            )
        };
        serve_copy_request(q, tag, bufs, ctx, op, offset, len, &events).await?;
    }
}

#[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
async fn serve_copy_request<Q: QueueIo, D: BlockDevice>(
    q: &Q,
    tag: u16,
    bufs: &[RefCell<IoBuf<u8>>],
    ctx: &EngineCtx<D>,
    op: u32,
    offset: u64,
    len: u64,
    events: &(Waiter, smol::channel::Receiver<Event>),
) -> Result<(), UblkError> {
    let (done_tx, done_rx) = events;
    let n = len as usize;
    let admitted = ctx.observe();
    let res = if !admitted {
        -libc::EIO
    } else {
        match op {
            // Reads land in the tag's buffer directly: pieces from the local replica are read
            // here, on this ring, as the engine admits them; other pieces are copied.
            sys::UBLK_IO_OP_READ
                if n <= bufs[tag as usize].borrow().len()
                    && fast_read_copy(q, &bufs[tag as usize], ctx, offset, len).await =>
            {
                len as i32
            }
            sys::UBLK_IO_OP_READ if n <= bufs[tag as usize].borrow().len() => {
                ctx.submit(Io::ReadDirect { offset, len }, done_tx.clone());
                loop {
                    match done_rx.recv().await {
                        Ok(Event::Local(lr)) => local_read(q, &bufs[tag as usize], ctx, lr).await,
                        Ok(Event::Piece { at, data }) => {
                            bufs[tag as usize].borrow_mut()[at..at + data.len()]
                                .copy_from_slice(&data);
                        }
                        Ok(Event::Done(Ok(_))) => break len as i32,
                        Ok(Event::Done(Err(IoError::Invalid))) => break -libc::EINVAL,
                        _ => break -libc::EIO,
                    }
                }
            }
            sys::UBLK_IO_OP_READ | sys::UBLK_IO_OP_WRITE
                if n <= bufs[tag as usize].borrow().len() =>
            {
                let io = if op == sys::UBLK_IO_OP_READ {
                    Io::Read { offset, len }
                } else {
                    // One copy, into an aligned shared buffer: the store and the replicas take
                    // slices of it.
                    Io::Write {
                        offset,
                        data: AlignedBuf::shared_copy_of(&bufs[tag as usize].borrow()[..n]),
                    }
                };
                ctx.submit(io, done_tx.clone());
                match done_rx.recv().await {
                    Ok(Event::Done(Ok(Some(data)))) if data.len() == n => {
                        bufs[tag as usize].borrow_mut()[..n].copy_from_slice(&data);
                        len as i32
                    }
                    Ok(Event::Done(Ok(None))) => len as i32,
                    Ok(Event::Done(Err(IoError::Invalid))) => -libc::EINVAL,
                    _ => -libc::EIO,
                }
            }
            // Writes are durable when acknowledged; the protocol has no discard yet, and a
            // discard is only a hint.
            sys::UBLK_IO_OP_FLUSH | sys::UBLK_IO_OP_DISCARD => match ctx.noop_permit() {
                Some(p) => {
                    p.consume();
                    0
                }
                None => -libc::EIO,
            },
            sys::UBLK_IO_OP_READ | sys::UBLK_IO_OP_WRITE => -libc::EINVAL,
            _ => -libc::EOPNOTSUPP,
        }
    };
    let buf = bufs[tag as usize].borrow();
    q.commit_copy(tag, buf.as_slice(), res).await?;
    let _ = ctx.observe();
    Ok(())
}

/// Reads one piece from the local store into the tag's buffer, on this ring, as the MVP's
/// reads did: the engine plans it, the device reads run while the event loop keeps going, and
/// the checksums are verified at the end. A mismatch is planned and read once more before the
/// read counts as failed, which makes the target read it from another ISR member (docs/issues/5).
async fn local_read<Q: QueueIo, D: BlockDevice>(
    q: &Q,
    buf: &RefCell<IoBuf<u8>>,
    ctx: &EngineCtx<D>,
    lr: LocalRead,
) {
    let (at, n) = (lr.at, lr.len);
    for attempt in 0..2 {
        let plan = {
            let mut b = buf.borrow_mut();
            match ctx.engine().plan_local_read(&lr, &mut b[at..at + n]) {
                Ok(p) => p,
                Err(_) => break,
            }
        };
        // The kernel gave us this tag's buffer; nothing else touches it until the request is
        // committed, and every read queued here is awaited before it is used.
        let base = buf.borrow_mut().as_mut_ptr().wrapping_add(at);
        let mut pending = Vec::new();
        for r in plan.reads() {
            let sqe = opcode::Read::new(
                types::Fd(ctx.shared.fd),
                base.wrapping_add(r.buf_offset),
                r.len as u32,
            )
            .offset(r.device_offset)
            .build();
            pending.push((q.submit(sqe), r.len as i32));
        }
        let mut io_ok = true;
        for (completion, expected) in pending {
            let completed = completion.await == expected;
            let healthy = ctx.observe();
            io_ok &= completed && healthy;
        }
        let verified =
            io_ok && (!ctx.shared.verify || plan.verify(&buf.borrow()[at..at + n]).is_ok());
        if io_ok && !verified {
            eprintln!(
                "flaminio-io: {}: checksum mismatch on a local read (attempt {})",
                ctx.shared.volume,
                attempt + 1
            );
        }
        if verified || !io_ok || attempt == 1 {
            ctx.engine().local_read_done(&lr, Some(plan), verified);
            ctx.pump();
            return;
        }
        ctx.engine().finish_plan(plan);
    }
    ctx.engine().local_read_done(&lr, None, false);
    ctx.pump();
}

/// Copies `len` bytes at `at` from the tag's staging slot into the request's pages.
fn stage_to_request<Q: QueueIo>(
    q: &Q,
    st: &Staging,
    tag: u16,
    at: usize,
    len: usize,
) -> impl std::future::Future<Output = bool> {
    // The request's registered buffer is addressed by offset from its start.
    let sqe = opcode::ReadFixed::new(types::Fd(st.fd()), at as *mut u8, len as u32, tag)
        .offset(st.offset(tag as usize, at))
        .build();
    let done = q.submit(sqe);
    async move { done.await == len as i32 }
}

/// One tag, with zero copy: the request's pages are fixed buffer `tag` of this ring.
async fn engine_io_task_zc(
    q: &UblkQueue<'_>,
    tag: u16,
    st: &Staging,
    ctx: &EngineCtx,
) -> Result<(), UblkError> {
    let events = smol::channel::unbounded();
    let reg = sys::ublk_auto_buf_reg {
        index: tag,
        ..Default::default()
    };
    q.submit_io_prep_cmd(tag, BufDesc::AutoReg(reg), 0, None)
        .await?;
    loop {
        let (op, offset, len, flags) = {
            let iod = q.get_iod(tag);
            (
                iod.op_flags & 0xff,
                iod.start_sector << 9,
                u64::from(iod.nr_sectors) << 9,
                iod.op_flags,
            )
        };
        serve_zc_request(q, tag, st, ctx, op, offset, len, flags, &events).await?;
    }
}

#[allow(clippy::too_many_arguments)]
async fn serve_zc_request<Q: QueueIo, D: BlockDevice>(
    q: &Q,
    tag: u16,
    st: &Staging,
    ctx: &EngineCtx<D>,
    op: u32,
    offset: u64,
    len: u64,
    flags: u32,
    events: &(Waiter, smol::channel::Receiver<Event>),
) -> Result<(), UblkError> {
    let (done_tx, done_rx) = events;
    let n = len as usize;
    let res = if !ctx.observe() || flags & sys::UBLK_IO_F_NEED_REG_BUF != 0 {
        // The kernel could not register the request's pages.
        -libc::EIO
    } else {
        match op {
            sys::UBLK_IO_OP_READ
                if n <= st.slot_len() && fast_read_zc(q, tag, st, ctx, offset, len).await =>
            {
                len as i32
            }
            sys::UBLK_IO_OP_READ if n <= st.slot_len() => {
                ctx.submit(Io::ReadDirect { offset, len }, done_tx.clone());
                let mut failed = false;
                loop {
                    match done_rx.recv().await {
                        Ok(Event::Local(lr)) => local_read_zc(q, tag, st, ctx, lr).await,
                        Ok(Event::Piece { at, data }) => {
                            st.with_slot(tag as usize, |b| {
                                b[at..at + data.len()].copy_from_slice(&data)
                            });
                            let copied = stage_to_request(q, st, tag, at, data.len()).await;
                            let _ = ctx.observe();
                            failed |= !copied;
                        }
                        Ok(Event::Done(Ok(_))) if !failed => break len as i32,
                        Ok(Event::Done(Err(IoError::Invalid))) => break -libc::EINVAL,
                        _ => break -libc::EIO,
                    }
                }
            }
            sys::UBLK_IO_OP_WRITE if n <= st.slot_len() => {
                // A large write, while the engine holds groups, goes to it as soon as its
                // own data is copied in, and is counted in arrivals meanwhile. The others
                // wait for one another, which gathers them (docs/issues/10, S3).
                let large = ctx.shared.holding
                    && flaminio_store::large(ctx.shared.cluster_size, offset, len);
                // The write's data, from the request's pages into the staging slot.
                let sqe =
                    opcode::WriteFixed::new(types::Fd(st.fd()), std::ptr::null(), n as u32, tag)
                        .offset(st.offset(tag as usize, 0))
                        .build();
                let arrival = large.then(|| Arrival::new(&ctx.shared.arrivals));
                if !large {
                    ctx.staging.set(ctx.staging.get() + 1);
                }
                let copied = q.submit(sqe).await;
                let healthy = ctx.observe();
                if !large {
                    ctx.staging.set(ctx.staging.get() - 1);
                }
                if copied != n as i32 || !healthy {
                    drop(arrival);
                    // A wait for writes may have held for this one: the engine looks again.
                    ctx.due.set(true);
                    -libc::EIO
                } else {
                    // The one copy of the write's data in user space: into an aligned
                    // shared buffer, which the store writes as is and the replicas share.
                    let data = st.with_slot(tag as usize, |b| AlignedBuf::shared_copy_of(&b[..n]));
                    ctx.submit(Io::Write { offset, data }, done_tx.clone());
                    drop(arrival);
                    match done_rx.recv().await {
                        Ok(Event::Done(Ok(_))) => len as i32,
                        Ok(Event::Done(Err(IoError::Invalid))) => -libc::EINVAL,
                        _ => -libc::EIO,
                    }
                }
            }
            sys::UBLK_IO_OP_FLUSH | sys::UBLK_IO_OP_DISCARD => match ctx.noop_permit() {
                Some(p) => {
                    p.consume();
                    0
                }
                None => -libc::EIO,
            },
            sys::UBLK_IO_OP_READ | sys::UBLK_IO_OP_WRITE => -libc::EINVAL,
            _ => -libc::EOPNOTSUPP,
        }
    };
    q.commit_zc(tag, res).await?;
    let _ = ctx.observe();
    Ok(())
}

/// A local read with zero copy: the device reads land in the request's pages, and the store's
/// zero-filled ranges (never written) are copied in from the staging slot. The checksums are
/// not verified: the data never passes through this process.
async fn local_read_zc<Q: QueueIo, D: BlockDevice>(
    q: &Q,
    tag: u16,
    st: &Staging,
    ctx: &EngineCtx<D>,
    lr: LocalRead,
) {
    let (at, n) = (lr.at, lr.len);
    // The plan zero-fills the never-written ranges of the slot; they are copied in below.
    let plan = st.with_slot(tag as usize, |b| {
        ctx.engine().plan_local_read(&lr, &mut b[at..at + n])
    });
    let Ok(plan) = plan else {
        ctx.engine().local_read_done(&lr, None, false);
        ctx.pump();
        return;
    };
    let ok = zc_read_plan(q, tag, st, ctx, &plan, at, n).await;
    ctx.engine().local_read_done(&lr, Some(plan), ok);
    ctx.pump();
}

/// Carries out a planned read with zero copy: the device reads land in the request's pages at
/// `at`, and the plan's zero-filled ranges are copied in from the tag's staging slot. A device
/// read the disk refuses with EINVAL, because the request's pages are not aligned for direct
/// I/O on it, is done again through the staging slot (docs/issues/5, S1).
async fn zc_read_plan<Q: QueueIo, D: BlockDevice>(
    q: &Q,
    tag: u16,
    st: &Staging,
    ctx: &EngineCtx<D>,
    plan: &flaminio_store::ReadPlan,
    at: usize,
    n: usize,
) -> bool {
    let mut pending = Vec::new();
    let mut zeros = Vec::new();
    let mut pos = 0;
    for r in plan.reads() {
        if r.buf_offset > pos {
            zeros.push((pos, r.buf_offset - pos));
        }
        // Where this read goes in the request, and in the slot if it is staged: one value for
        // both, so neither can lose a term.
        let to = at + r.buf_offset;
        let sqe =
            opcode::ReadFixed::new(types::Fd(ctx.shared.fd), to as *mut u8, r.len as u32, tag)
                .offset(r.device_offset)
                .build();
        pending.push((q.submit(sqe), to, r.len, r.device_offset));
        pos = r.buf_offset + r.len;
    }
    if pos < n {
        zeros.push((pos, n - pos));
    }
    let mut ok = true;
    for (start, len) in zeros {
        let copied = stage_to_request(q, st, tag, at + start, len).await;
        let healthy = ctx.observe();
        if !copied || !healthy {
            eprintln!("flaminio-io: local read: zero fill of {len} bytes at {start} failed");
            ok = false;
        }
    }
    let mut refused = Vec::new();
    for (completion, to, len, device_offset) in pending {
        let got = completion.await;
        let healthy = ctx.observe();
        ok &= healthy;
        if got == -libc::EINVAL {
            refused.push((to, len, device_offset));
        } else if got != len as i32 {
            eprintln!("flaminio-io: local read: device read returned {got}, not {len}");
            ok = false;
        }
    }
    // Once a read failed, the plan fails anyway: nothing is worth staging.
    for (to, len, device_offset) in refused {
        if !ok {
            break;
        }
        ok = stage_device_read(q, st, tag, ctx, to, len, device_offset).await;
        // Once a staged read worked: a load first, as every queue thread checks the flag.
        if ok
            && !ctx.shared.staged.load(Ordering::Relaxed)
            && !ctx.shared.staged.swap(true, Ordering::Relaxed)
        {
            eprintln!(
                "flaminio-io: {}: zero copy: a read's pages are not aligned for {} (its DMA \
                 alignment); such reads are copied through the staging area",
                ctx.shared.volume, ctx.shared.device
            );
        }
    }
    ok
}

/// A device read the disk refused into the request's pages: from the disk into the tag's
/// staging slot, which is page-aligned, then from the slot into the request's pages at the same
/// place. True if both completed.
async fn stage_device_read<Q: QueueIo, D: BlockDevice>(
    q: &Q,
    st: &Staging,
    tag: u16,
    ctx: &EngineCtx<D>,
    to: usize,
    len: usize,
    device_offset: u64,
) -> bool {
    // No `with_slot` runs on this tag's slot meanwhile: only this tag's task uses it, and it
    // awaits the read here. The zero fills used other ranges of it.
    let sqe = opcode::Read::new(
        types::Fd(ctx.shared.fd),
        st.addr(tag as usize, to, len),
        len as u32,
    )
    .offset(device_offset)
    .build();
    let got = q.submit(sqe).await;
    let healthy = ctx.observe();
    if got != len as i32 || !healthy {
        eprintln!("flaminio-io: local read: device read returned {got}, not {len} (staged)");
        return false;
    }
    let copied = stage_to_request(q, st, tag, to, len).await;
    let healthy = ctx.observe();
    if !copied || !healthy {
        eprintln!("flaminio-io: local read: copy of {len} staged bytes at {to} failed");
        return false;
    }
    true
}

/// The fast path, with zero copy. True if the read was served; false to take the slow path.
async fn fast_read_zc<Q: QueueIo, D: BlockDevice>(
    q: &Q,
    tag: u16,
    st: &Staging,
    ctx: &EngineCtx<D>,
    offset: u64,
    len: u64,
) -> bool {
    let Some(fast) = ctx.engine().try_fast_read(&ctx.shared.volume, offset, len) else {
        return false;
    };
    let mut ok = true;
    let pieces: Vec<(usize, usize)> = fast.pieces().collect();
    for (i, (at, n)) in pieces.into_iter().enumerate() {
        let plan = st.with_slot(tag as usize, |b| {
            ctx.engine().plan_fast_read(&fast, i, &mut b[at..at + n])
        });
        let Ok(plan) = plan else {
            ok = false;
            break;
        };
        ok &= zc_read_plan(q, tag, st, ctx, &plan, at, n).await;
        ctx.engine().finish_plan(plan);
        if !ok {
            break;
        }
    }
    let served = ctx.engine().finish_fast_read(&ctx.shared.volume, fast, ok);
    ctx.pump();
    if let Some(permit) = served {
        permit.consume();
        true
    } else {
        false
    }
}

/// The fast path, into the tag's buffer: the pieces are planned, read and verified here,
/// without the engine's queue. True if the read was served; false to take the slow path.
async fn fast_read_copy<Q: QueueIo, D: BlockDevice>(
    q: &Q,
    buf: &RefCell<IoBuf<u8>>,
    ctx: &EngineCtx<D>,
    offset: u64,
    len: u64,
) -> bool {
    let Some(fast) = ctx.engine().try_fast_read(&ctx.shared.volume, offset, len) else {
        return false;
    };
    let mut ok = true;
    let mut plans = Vec::new();
    for (i, (at, n)) in fast.pieces().enumerate() {
        let plan = {
            let mut b = buf.borrow_mut();
            ctx.engine().plan_fast_read(&fast, i, &mut b[at..at + n])
        };
        match plan {
            Ok(p) => plans.push((p, at, n)),
            Err(_) => {
                ok = false;
                break;
            }
        }
    }
    // The kernel gave us this tag's buffer; nothing else touches it until the request is
    // committed, and every read queued here is awaited before it is used.
    let base = buf.borrow_mut().as_mut_ptr();
    let mut pending = Vec::new();
    if ok {
        for (plan, at, _) in &plans {
            for r in plan.reads() {
                let sqe = opcode::Read::new(
                    types::Fd(ctx.shared.fd),
                    base.wrapping_add(at + r.buf_offset),
                    r.len as u32,
                )
                .offset(r.device_offset)
                .build();
                pending.push((q.submit(sqe), r.len as i32));
            }
        }
    }
    for (completion, expected) in pending {
        let completed = completion.await == expected;
        let healthy = ctx.observe();
        ok &= completed && healthy;
    }
    if ok && ctx.shared.verify {
        let b = buf.borrow();
        ok = plans
            .iter()
            .all(|(plan, at, n)| plan.verify(&b[*at..*at + *n]).is_ok());
        if !ok {
            // Served again by the slow path, which checks once more; logged even if that
            // succeeds, as integrity runs count these.
            eprintln!(
                "flaminio-io: {}: checksum mismatch on a fast read at {offset}+{len}; retrying",
                ctx.shared.volume
            );
        }
    }
    for (plan, _, _) in plans {
        ctx.engine().finish_plan(plan);
    }
    let served = ctx.engine().finish_fast_read(&ctx.shared.volume, fast, ok);
    ctx.pump();
    if let Some(permit) = served {
        permit.consume();
        true
    } else {
        false
    }
}

/// libublk's event loop, with polling: for `poll` after the last completion, the ring is
/// checked without sleeping, so that the next request or device completion costs no wake-up of
/// the thread (in a VM, a halted vCPU's wake-up costs microseconds). Then it blocks, as
/// libublk's own loop always does (`poll` zero).
async fn event_loop(
    q: &UblkQueue<'_>,
    poll: Duration,
    run_ops: impl Fn(),
    is_done: impl Fn() -> bool,
) -> Result<(), UblkError> {
    use libublk::uring_async::{
        ublk_reap_io_events_with_update_queue, ublk_wake_task, uring_poll_io_fn,
    };
    run_ops();
    let mut last = Instant::now();
    loop {
        let spinning = !poll.is_zero() && last.elapsed() < poll;
        // Submit what the tasks queued; wait for a completion only when not spinning.
        let polled = if spinning {
            uring_poll_io_fn::<io_uring::squeue::Entry>(q, None, 0)
        } else {
            uring_poll_io_fn::<io_uring::squeue::Entry>(q, Some(types::Timespec::new().sec(20)), 1)
        };
        let (poll_timeout, failed) = match polled {
            Ok(t) => (t, false),
            Err(_) => (false, true),
        };
        let mut reaped = 0usize;
        let aborted = ublk_reap_io_events_with_update_queue(q, poll_timeout, None, |cqe| {
            reaped += 1;
            ublk_wake_task(cqe.user_data(), cqe)
        })?;
        run_ops();
        if reaped > 0 {
            last = Instant::now();
        } else if spinning {
            std::hint::spin_loop();
        }
        if (aborted || failed) && is_done() {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Arrival;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn an_arrival_counts_while_it_lives() {
        let n = AtomicUsize::new(0);
        let a = Arrival::new(&n);
        let b = Arrival::new(&n);
        assert_eq!(n.load(Ordering::Relaxed), 2);
        drop(a);
        assert_eq!(n.load(Ordering::Relaxed), 1);
        // A task stopped mid-copy drops its arrival with it.
        let task = move || drop(b);
        task();
        assert_eq!(n.load(Ordering::Relaxed), 0);
        // An optional one, as the write path takes it.
        let none = false.then(|| Arrival::new(&n));
        assert!(none.is_none());
        assert_eq!(n.load(Ordering::Relaxed), 0);
    }
}

#[cfg(test)]
mod clock_acceptance {
    use super::*;
    use crate::test_support::{Clock, Fixture};
    use flaminio_store::MemDevice;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    #[derive(Default)]
    struct Queue {
        pending: RefCell<Vec<smol::channel::Sender<i32>>>,
        offered: RefCell<Vec<i32>>,
        delivered: RefCell<Vec<i32>>,
        hold_commit: Cell<bool>,
        commit: RefCell<Option<smol::channel::Sender<()>>>,
    }
    impl QueueIo for Queue {
        fn submit(&self, _: io_uring::squeue::Entry) -> impl Future<Output = i32> {
            let (tx, rx) = smol::channel::bounded(1);
            self.pending.borrow_mut().push(tx);
            async move { rx.recv().await.unwrap() }
        }
        async fn commit_copy(&self, _: u16, _: &[u8], result: i32) -> Result<i32, UblkError> {
            self.commit_zc(0, result).await
        }
        async fn commit_zc(&self, _: u16, result: i32) -> Result<i32, UblkError> {
            self.offered.borrow_mut().push(result);
            if self.hold_commit.get() {
                let (tx, rx) = smol::channel::bounded(1);
                *self.commit.borrow_mut() = Some(tx);
                rx.recv().await.unwrap();
            }
            self.delivered.borrow_mut().push(result);
            Ok(0)
        }
    }
    impl Queue {
        fn complete(&self, result: i32) {
            for tx in self.pending.borrow_mut().drain(..) {
                tx.try_send(result).unwrap();
            }
        }
        fn deliver(&self) {
            self.commit
                .borrow_mut()
                .take()
                .unwrap()
                .try_send(())
                .unwrap();
        }
    }
    fn poll<F: Future + ?Sized>(f: Pin<&mut F>) -> Poll<F::Output> {
        f.poll(&mut Context::from_waker(Waker::noop()))
    }
    fn context() -> (EngineCtx<MemDevice>, Arc<Clock>) {
        let f = Fixture::new();
        let domain = f.engine.clock_domain().clone();
        let observer = domain.observer();
        let arrivals = Arc::new(AtomicUsize::new(0));
        let shared = Arc::new(Shared {
            engine: Mutex::new(f.engine),
            inputs: Mutex::new(f.rx),
            sender: f.tx,
            fd: -1,
            device: "memory".into(),
            staged: AtomicBool::new(false),
            live: Arc::new(AtomicBool::new(true)),
            waiters: Mutex::new(HashMap::new()),
            next_op: AtomicU64::new(1000),
            volume: "v".into(),
            dev_id: -1,
            verify: true,
            poll: Duration::ZERO,
            defer_taskrun: false,
            wakes: vec![Arc::new(EventFd::new().unwrap())],
            holding: false,
            cluster_size: 65536,
            arrivals,
        });
        (
            EngineCtx {
                observer: RefCell::new(observer),
                shared,
                qid: 0,
                due: Cell::new(false),
                staging: Cell::new(0),
                gather: std::cell::OnceCell::new(),
            },
            f.clock,
        )
    }
    fn drive<F: Future<Output = Result<(), UblkError>> + ?Sized>(
        mut f: Pin<&mut F>,
        ctx: &EngineCtx<MemDevice>,
    ) {
        for _ in 0..128 {
            ctx.pump();
            if let Poll::Ready(r) = poll(f.as_mut()) {
                r.unwrap();
                return;
            }
        }
        panic!("request made no progress");
    }
    #[test]
    fn clock_fault_between_io_admissions_completes_the_registered_frontend_waiter() {
        let (ctx, clock) = context();
        let (tx, rx) = smol::channel::unbounded();
        clock
            .fail_on_read
            .store(clock.reads.load(Ordering::SeqCst) + 2, Ordering::SeqCst);
        ctx.submit(
            Io::Read {
                offset: 0,
                len: 4096,
            },
            tx,
        );
        assert!(
            !ctx.shared
                .engine
                .lock()
                .unwrap()
                .clock_domain()
                .status()
                .is_healthy()
        );
        ctx.pump();
        assert!(matches!(
            rx.try_recv(),
            Ok(Event::Done(Err(IoError::ClockQuarantined)))
        ));
        assert!(rx.try_recv().is_err(), "exactly one completion");
        assert!(ctx.shared.waiters.lock().unwrap().is_empty());
    }

    #[test]
    fn copied_and_registered_requests_terminate_when_submit_detects_the_fault() {
        for registered in [false, true] {
            let (ctx, clock) = context();
            let q = Queue::default();
            let bufs = [RefCell::new(IoBuf::<u8>::new(4096))];
            let st = Staging::new(1, 4096).unwrap();
            let events = smol::channel::unbounded();
            let mut work: Pin<Box<dyn Future<Output = Result<(), UblkError>>>> = if registered {
                Box::pin(serve_zc_request(
                    &q,
                    0,
                    &st,
                    &ctx,
                    sys::UBLK_IO_OP_WRITE,
                    0,
                    4096,
                    0,
                    &events,
                ))
            } else {
                Box::pin(serve_copy_request(
                    &q,
                    0,
                    &bufs,
                    &ctx,
                    sys::UBLK_IO_OP_WRITE,
                    0,
                    4096,
                    &events,
                ))
            };
            if registered {
                assert!(poll(work.as_mut()).is_pending());
                q.complete(4096); // source-page copy ends before Engine submission.
            }
            clock
                .fail_on_read
                .store(clock.reads.load(Ordering::SeqCst) + 3, Ordering::SeqCst);
            assert!(poll(work.as_mut()).is_pending());
            assert!(
                !ctx.shared
                    .engine
                    .lock()
                    .unwrap()
                    .clock_domain()
                    .status()
                    .is_healthy()
            );
            drive(work.as_mut(), &ctx);
            assert_eq!(*q.delivered.borrow(), [-libc::EIO]);
            assert!(ctx.shared.waiters.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn copied_fast_read_retains_work_until_cqe_then_explicit_eio() {
        let (ctx, clock) = context();
        let q = Queue::default();
        let bufs = [RefCell::new(IoBuf::<u8>::new(4096))];
        bufs[0].borrow_mut().fill(0x55);
        let events = smol::channel::unbounded();
        let mut future = Box::pin(serve_copy_request(
            &q,
            0,
            &bufs,
            &ctx,
            sys::UBLK_IO_OP_READ,
            0,
            4096,
            &events,
        ));
        assert!(poll(future.as_mut()).is_pending());
        assert_eq!(q.pending.borrow().len(), 1);
        assert!(q.offered.borrow().is_empty());
        clock.failed.store(true, Ordering::SeqCst);
        ctx.pump();
        assert!(
            poll(future.as_mut()).is_pending(),
            "fault must not drop a submitted device read"
        );
        assert!(q.offered.borrow().is_empty());
        q.complete(4096);
        drive(future.as_mut(), &ctx);
        assert_eq!(*q.delivered.borrow(), [-libc::EIO]);
        assert!(bufs[0].borrow().iter().all(|&b| b == 0x55));
    }
    #[test]
    fn registered_fast_read_retains_plan_until_cqe_then_explicit_eio() {
        let (ctx, clock) = context();
        let q = Queue::default();
        let st = Staging::new(1, 4096).unwrap();
        let events = smol::channel::unbounded();
        let mut future = Box::pin(serve_zc_request(
            &q,
            0,
            &st,
            &ctx,
            sys::UBLK_IO_OP_READ,
            0,
            4096,
            0,
            &events,
        ));
        assert!(poll(future.as_mut()).is_pending());
        assert_eq!(q.pending.borrow().len(), 1);
        clock.failed.store(true, Ordering::SeqCst);
        ctx.pump();
        assert!(poll(future.as_mut()).is_pending());
        assert!(q.offered.borrow().is_empty());
        q.complete(4096);
        drive(future.as_mut(), &ctx);
        assert_eq!(*q.delivered.borrow(), [-libc::EIO]);
    }
    #[test]
    fn final_permit_can_deliver_after_fault_but_next_request_cannot_succeed() {
        let (ctx, clock) = context();
        let q = Queue::default();
        q.hold_commit.set(true);
        let bufs = [RefCell::new(IoBuf::<u8>::new(4096))];
        bufs[0].borrow_mut().fill(0x55);
        let events = smol::channel::unbounded();
        let mut future = Box::pin(serve_copy_request(
            &q,
            0,
            &bufs,
            &ctx,
            sys::UBLK_IO_OP_READ,
            0,
            4096,
            &events,
        ));
        assert!(poll(future.as_mut()).is_pending());
        q.complete(4096);
        assert!(poll(future.as_mut()).is_pending());
        assert_eq!(*q.offered.borrow(), [4096]);
        assert!(q.delivered.borrow().is_empty());
        clock.failed.store(true, Ordering::SeqCst);
        ctx.pump();
        q.deliver();
        drive(future.as_mut(), &ctx);
        drop(future);
        assert_eq!(*q.delivered.borrow(), [4096]);
        q.hold_commit.set(false);
        let mut next = Box::pin(serve_copy_request(
            &q,
            0,
            &bufs,
            &ctx,
            sys::UBLK_IO_OP_READ,
            0,
            4096,
            &events,
        ));
        drive(next.as_mut(), &ctx);
        assert_eq!(*q.delivered.borrow(), [4096, -libc::EIO]);
    }
    #[test]
    fn staged_write_fault_at_cqe_never_reaches_replication() {
        let (ctx, clock) = context();
        let q = Queue::default();
        let st = Staging::new(1, 4096).unwrap();
        let events = smol::channel::unbounded();
        let before = ctx.engine().service().store().stats().journal_head;
        let mut future = Box::pin(serve_zc_request(
            &q,
            0,
            &st,
            &ctx,
            sys::UBLK_IO_OP_WRITE,
            0,
            4096,
            0,
            &events,
        ));
        assert!(poll(future.as_mut()).is_pending());
        assert_eq!(ctx.staging.get(), 1);
        clock.failed.store(true, Ordering::SeqCst);
        q.complete(4096);
        drive(future.as_mut(), &ctx);
        assert_eq!(ctx.staging.get(), 0);
        assert_eq!(*q.delivered.borrow(), [-libc::EIO]);
        assert_eq!(before, ctx.engine().service().store().stats().journal_head);
    }
    #[test]
    fn slow_local_copy_and_registered_reads_wait_for_actual_device_completion() {
        for registered in [false, true] {
            let (ctx, clock) = context();
            let q = Queue::default();
            let bufs = [RefCell::new(IoBuf::<u8>::new(4096))];
            bufs[0].borrow_mut().fill(0x55);
            let st = Staging::new(1, 4096).unwrap();
            let (tx, rx) = smol::channel::unbounded();
            ctx.submit(
                Io::ReadDirect {
                    offset: 0,
                    len: 4096,
                },
                tx,
            );
            ctx.pump();
            let lr = match rx.try_recv().unwrap() {
                Event::Local(lr) => lr,
                _ => panic!("slow local read not dispatched"),
            };
            let mut work: Pin<Box<dyn Future<Output = ()>>> = if registered {
                Box::pin(local_read_zc(&q, 0, &st, &ctx, lr))
            } else {
                Box::pin(local_read(&q, &bufs[0], &ctx, lr))
            };
            assert!(poll(work.as_mut()).is_pending());
            assert_eq!(q.pending.borrow().len(), 1);
            clock.failed.store(true, Ordering::SeqCst);
            ctx.pump();
            assert!(poll(work.as_mut()).is_pending());
            q.complete(4096);
            assert!(poll(work.as_mut()).is_ready());
            assert!(matches!(
                rx.try_recv().unwrap(),
                Event::Done(Err(IoError::ClockQuarantined))
            ));
        }
    }
    #[test]
    fn registered_zero_fill_and_alignment_fallback_cqes_cannot_authorize_after_fault() {
        for zero_fill in [false, true] {
            let (ctx, clock) = context();
            let q = Queue::default();
            let st = Staging::new(1, 4096).unwrap();
            let events = smol::channel::unbounded();
            let mut work = Box::pin(serve_zc_request(
                &q,
                0,
                &st,
                &ctx,
                sys::UBLK_IO_OP_READ,
                if zero_fill { 4096 } else { 0 },
                4096,
                0,
                &events,
            ));
            assert!(poll(work.as_mut()).is_pending());
            if !zero_fill {
                q.complete(-libc::EINVAL);
                assert!(poll(work.as_mut()).is_pending());
                assert_eq!(
                    q.pending.borrow().len(),
                    1,
                    "fallback read must be in flight"
                );
            }
            clock.failed.store(true, Ordering::SeqCst);
            ctx.pump();
            assert!(poll(work.as_mut()).is_pending());
            assert!(q.offered.borrow().is_empty());
            q.complete(4096);
            drive(work.as_mut(), &ctx);
            assert_eq!(*q.delivered.borrow(), [-libc::EIO]);
        }
    }
    #[test]
    fn pipelined_read_cqes_are_all_drained_after_the_first_fault() {
        let (ctx, clock) = context();
        {
            let mut e = ctx.engine();
            e.handle(Input::Io {
                volume: "v".into(),
                op: 101,
                io: Io::Write {
                    offset: 1 << 20,
                    data: vec![0x55; 4096].into(),
                },
            });
        }
        ctx.pump();
        let q = Queue::default();
        let bufs = [RefCell::new(IoBuf::<u8>::new((1 << 20) + 4096))];
        let events = smol::channel::unbounded();
        let mut work = Box::pin(serve_copy_request(
            &q,
            0,
            &bufs,
            &ctx,
            sys::UBLK_IO_OP_READ,
            0,
            (1 << 20) + 4096,
            &events,
        ));
        assert!(poll(work.as_mut()).is_pending());
        assert_eq!(q.pending.borrow().len(), 2);
        clock.failed.store(true, Ordering::SeqCst);
        q.pending.borrow_mut().remove(0).try_send(4096).unwrap();
        assert!(poll(work.as_mut()).is_pending());
        assert_eq!(q.pending.borrow().len(), 1);
        assert!(q.offered.borrow().is_empty());
        q.complete(4096);
        drive(work.as_mut(), &ctx);
        assert_eq!(*q.delivered.borrow(), [-libc::EIO]);
    }
    #[test]
    fn fast_completion_revalidates_exact_lease_boundary_without_a_ticker() {
        for us in [4_999_999, 5_000_000, 5_000_001] {
            let mut f = Fixture::new();
            let fast = f.engine.try_fast_read("v", 0, 4096).unwrap();
            f.clock.ns.store(us * 1000, Ordering::SeqCst);
            let permit = f.engine.finish_fast_read("v", fast, true);
            assert_eq!(
                permit.is_some(),
                us < 5_000_000,
                "send-time lease boundary {us}"
            );
        }
    }
    #[test]
    fn quarantine_preserves_real_store_pins_until_the_planned_read_finishes() {
        let mut f = Fixture::new();
        let fast = f.engine.try_fast_read("v", 0, 4096).unwrap();
        let mut bytes = vec![0; 4096];
        let plan = f.engine.plan_fast_read(&fast, 0, &mut bytes).unwrap();
        assert!(!plan.reads().is_empty());
        f.clock.failed.store(true, Ordering::SeqCst);
        f.engine.flush();
        // Move the closed engine's memory store into the test and force reclamation
        // pressure directly; this grants no client/session admission after quarantine.
        let mut store = f.engine.into_service().into_store();
        let key = store
            .chunks()
            .into_iter()
            .find(|c| c.key.index == 0)
            .unwrap()
            .key;
        store.write(key, 0, &vec![0x99; 65536], 999, 5).unwrap();
        store.checkpoint().unwrap();
        assert!(
            store.stats().data_quarantined > 0,
            "quarantine released an outstanding read pin"
        );
        for read in plan.reads() {
            store
                .device()
                .read_at(
                    read.device_offset,
                    &mut bytes[read.buf_offset..read.buf_offset + read.len],
                )
                .unwrap();
        }
        assert_eq!(bytes, vec![0x55; 4096]);
        assert!(plan.verify(&bytes).is_ok());
        store.finish_read(plan);
        store.checkpoint().unwrap();
        assert_eq!(
            store.stats().data_quarantined,
            0,
            "actual completion must release pin ownership"
        );
    }
    #[test]
    fn engine_entry_rejects_validly_signed_wrong_identity_nonce_and_stale_metadata() {
        use crate::test_support::{NONCE, credentials, token_key};
        use flaminio_net::handoff::Purpose;
        for (volume, node, nonce) in [
            ("other", "fl-a", NONCE),
            ("v", "fl-b", NONCE),
            ("v", "fl-a", [84; 32]),
            ("v", "fl-a", NONCE),
        ] {
            let mut f = Fixture::new();
            f.engine.handle(Input::Detach { volume: "v".into() });
            f.drain();
            let before = f.engine.service().store().chunks();
            // Every envelope and nested capability has a valid signature/digest. Epoch6
            // is greater than this process's last start but mismatches current metadata5.
            let (token, handoff) = credentials(volume, node, 6, 1, &nonce);
            let proof = token_key()
                .verify_handoff(&token, &handoff, &nonce, Purpose::Normal)
                .unwrap();
            f.engine.handle(Input::Attach {
                volume: "v".into(),
                proof,
            });
            let outputs = f.drain();
            assert!(
                outputs
                    .iter()
                    .any(|o| matches!(o, Output::AttachFailed { .. }))
            );
            assert!(!outputs.iter().any(|o| matches!(o, Output::Attached { .. })));
            let after = f.engine.service().store().chunks();
            assert_eq!(after.len(), before.len());
            for (a, b) in after.iter().zip(before.iter()) {
                assert_eq!(
                    a.fenced_epoch, b.fenced_epoch,
                    "rejected proof reached Hello/fence effects"
                );
            }
        }
    }
}

#[cfg(test)]
mod native_handoff_tests;
