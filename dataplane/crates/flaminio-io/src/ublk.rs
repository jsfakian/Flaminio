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

use std::cell::RefCell;
use std::os::fd::{AsRawFd, RawFd};
use std::rc::Rc;

use flaminio_format::Uuid;
use flaminio_format::consts::GIB;
use flaminio_store::{Store, StoreConfig};
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

fn serve(c: Config) -> Result<(), String> {
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
            io_ok &= completion.await == expected;
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

// --- Phase 1: a volume served by the node engine (flaminio-io node --attach) ---------------

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use flaminio_node::engine::{Input, Io, IoError, LocalRead, Output};
use flaminio_uring::EventFd;

use crate::node::{NodeConfig, Parts, drive_until, log_output, read_token};

/// What a tag task waits for while the engine serves its request.
enum Event {
    /// A piece to read from the local store, into the tag's buffer.
    Local(LocalRead),
    /// A piece that came from another replica.
    Piece {
        at: usize,
        data: Arc<Vec<u8>>,
    },
    Done(Result<Option<Vec<u8>>, IoError>),
}

type Waiter = smol::channel::Sender<Event>;

/// What the queue thread's tasks share: the engine and the completions they wait for.
struct EngineCtx {
    engine: RefCell<flaminio_node::engine::Engine<UringDevice>>,
    /// The store's device, for the local reads the tag tasks perform.
    fd: RawFd,
    inputs: std::sync::mpsc::Receiver<Input>,
    waiters: RefCell<HashMap<u64, Waiter>>,
    next_op: Cell<u64>,
    volume: String,
}

impl EngineCtx {
    /// Feeds the engine what other threads sent, flushes it (a full turn with ticks when
    /// `tick`), and delivers completions.
    fn pump_with(&self, tick: bool) {
        let now = Instant::now();
        let outputs = {
            let mut e = self.engine.borrow_mut();
            while let Ok(i) = self.inputs.try_recv() {
                e.handle(now, i);
            }
            if tick {
                e.turn(now);
            } else {
                e.flush(now);
            }
            e.take_output()
        };
        for o in outputs {
            match o {
                Output::Done { op, result } => {
                    if let Some(tx) = self.waiters.borrow_mut().remove(&op) {
                        let _ = tx.try_send(Event::Done(result));
                    }
                }
                Output::LocalRead(lr) => {
                    if let Some(tx) = self.waiters.borrow().get(&lr.op) {
                        let _ = tx.try_send(Event::Local(lr));
                    }
                }
                Output::Piece { op, at, data } => {
                    if let Some(tx) = self.waiters.borrow().get(&op) {
                        let _ = tx.try_send(Event::Piece { at, data });
                    }
                }
                other => log_output(&other),
            }
        }
    }

    /// After an I/O event: no tick.
    fn pump(&self) {
        self.pump_with(false);
    }

    fn submit(&self, io: Io, waiter: Waiter) {
        let op = self.next_op.get();
        self.next_op.set(op + 1);
        self.waiters.borrow_mut().insert(op, waiter);
        self.engine.borrow_mut().handle(
            Instant::now(),
            Input::Io {
                volume: self.volume.clone(),
                op,
                io,
            },
        );
        self.pump();
    }
}

/// Attaches `volume` through the engine, then serves it as a ublk device whose queue thread
/// runs the engine.
pub fn serve_volume(
    c: &NodeConfig,
    mut parts: Parts<UringDevice>,
    volume: String,
    efd: Arc<EventFd>,
) -> Result<(), String> {
    let token = read_token(c.token.as_ref().expect("checked with --attach"))?;
    parts.engine.handle(
        Instant::now(),
        Input::Attach {
            volume: volume.clone(),
            token,
        },
    );
    let attached = drive_until(&mut parts, Duration::from_secs(60), |o| {
        matches!(o, Output::Attached { .. } | Output::AttachFailed { .. })
    });
    let size = match attached {
        Some(Output::Attached { size, .. }) => size,
        Some(Output::AttachFailed { why, .. }) => return Err(format!("attach {volume}: {why}")),
        _ => {
            return Err(format!(
                "attach {volume}: no answer from the metadata service"
            ));
        }
    };

    let ctrl = UblkCtrlBuilder::default()
        .name("flaminio")
        .id(c.id)
        .depth(c.depth)
        .nr_queues(1)
        .io_buf_bytes(c.io_buf)
        .dev_flags(UblkFlags::UBLK_DEV_F_ADD_DEV)
        .build()
        .map_err(|e| format!("create ublk device (is ublk_drv loaded, and are we root?): {e}"))?;
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
    let parts = Arc::new(Mutex::new(Some(parts)));
    let queue = move |qid: u16, dev: &UblkDev| {
        let Some(parts) = parts.lock().expect("parts").take() else {
            eprintln!("flaminio-io: only one queue is supported");
            return;
        };
        if let Err(e) = serve_engine_queue(qid, dev, parts, volume.clone(), efd.clone()) {
            eprintln!("flaminio-io: queue {qid} stopped: {e}");
        }
    };
    let ready = move |ctrl: &UblkCtrl| {
        println!(
            "flaminio-io: serving {} MiB at {} (ublk id {})",
            size >> 20,
            ctrl.get_bdev_path(),
            ctrl.dev_info().dev_id
        );
    };
    ctrl.run_target(tgt_init, queue, ready)
        .map_err(|e| format!("ublk: {e}"))?;
    Ok(())
}

fn serve_engine_queue(
    qid: u16,
    dev: &UblkDev,
    parts: Parts<UringDevice>,
    volume: String,
    efd: Arc<EventFd>,
) -> Result<(), String> {
    let Parts {
        engine,
        inputs,
        sender: _keep_open,
    } = parts;
    let fd = engine.device().as_raw_fd();
    let ctx = EngineCtx {
        engine: RefCell::new(engine),
        fd,
        inputs,
        waiters: RefCell::new(HashMap::new()),
        next_op: Cell::new(1),
        volume,
    };
    let ctx = &ctx;
    let q = Rc::new(UblkQueue::new(qid, dev).map_err(|e| format!("ublk queue: {e}"))?);
    let depth = dev.dev_info.queue_depth;
    let buf_bytes = dev.dev_info.max_io_buf_bytes as usize;
    let bufs: Rc<Vec<RefCell<IoBuf<u8>>>> = Rc::new(
        (0..depth)
            .map(|_| RefCell::new(IoBuf::<u8>::new(buf_bytes)))
            .collect(),
    );
    let exe = Rc::new(smol::LocalExecutor::new());
    let mut tasks = Vec::with_capacity(depth as usize);
    for tag in 0..depth {
        let (q, bufs) = (q.clone(), bufs.clone());
        tasks.push(exe.spawn(async move {
            match engine_io_task(&q, tag, &bufs, ctx).await {
                Ok(()) | Err(UblkError::QueueIsDown) => {}
                Err(e) => eprintln!("flaminio-io: tag {tag}: {e}"),
            }
        }));
    }
    // Other threads' inputs: wait for the eventfd to become readable on this ring.
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
    // Timeouts, heartbeats and leases: a turn every 10 ms.
    let _ticker = {
        let q = q.clone();
        exe.spawn(async move {
            let ts = types::Timespec::new().nsec(10_000_000);
            loop {
                let _ = q.ublk_submit_sqe(opcode::Timeout::new(&ts).build()).await;
                ctx.pump_with(true);
            }
        })
    };
    let runner = exe.clone();
    let result = smol::block_on(exe.run(async move {
        let run_ops = || while runner.try_tick() {};
        let is_done = || tasks.iter().all(|t| t.is_finished());
        libublk::wait_and_handle_io_events(&q, Some(20), run_ops, is_done).await
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
    let (done_tx, done_rx) = smol::channel::unbounded();
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
        let n = len as usize;
        let res = match op {
            // Reads land in the tag's buffer directly: pieces from the local replica are read
            // here, on this ring, as the engine admits them; other pieces are copied.
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
                    Io::Write {
                        offset,
                        data: bufs[tag as usize].borrow()[..n].to_vec(),
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
            sys::UBLK_IO_OP_FLUSH | sys::UBLK_IO_OP_DISCARD => 0,
            sys::UBLK_IO_OP_READ | sys::UBLK_IO_OP_WRITE => -libc::EINVAL,
            _ => -libc::EOPNOTSUPP,
        };
        let buf = bufs[tag as usize].borrow();
        q.submit_io_commit_cmd(tag, BufDesc::Slice(buf.as_slice()), res)
            .await?;
    }
}

/// Reads one piece from the local store into the tag's buffer, on this ring, as the MVP's
/// reads did: the engine plans it, the device reads run while the event loop keeps going, and
/// the checksums are verified at the end. A mismatch is planned and read once more before the
/// read counts as failed (which makes the target treat the local replica as failed).
async fn local_read(q: &UblkQueue<'_>, buf: &RefCell<IoBuf<u8>>, ctx: &EngineCtx, lr: LocalRead) {
    let (at, n) = (lr.at, lr.len);
    for attempt in 0..2 {
        let plan = {
            let mut b = buf.borrow_mut();
            match ctx.engine.borrow().plan_local_read(&lr, &mut b[at..at + n]) {
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
                types::Fd(ctx.fd),
                base.wrapping_add(r.buf_offset),
                r.len as u32,
            )
            .offset(r.device_offset)
            .build();
            pending.push((q.ublk_submit_sqe(sqe), r.len as i32));
        }
        let mut io_ok = true;
        for (completion, expected) in pending {
            io_ok &= completion.await == expected;
        }
        let verified = io_ok && plan.verify(&buf.borrow()[at..at + n]).is_ok();
        if verified || !io_ok || attempt == 1 {
            ctx.engine
                .borrow_mut()
                .local_read_done(Instant::now(), &lr, Some(plan), verified);
            ctx.pump();
            return;
        }
        ctx.engine.borrow().finish_plan(plan);
    }
    ctx.engine
        .borrow_mut()
        .local_read_done(Instant::now(), &lr, None, false);
    ctx.pump();
}
