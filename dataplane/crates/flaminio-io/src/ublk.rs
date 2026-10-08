//! The ublk frontend: one queue, served by one thread that owns the replica store.
//!
//! Each ublk tag has an async task that waits for the kernel to deliver a request, hands it to
//! the driver task and waits for the result. Every round of the event loop, the driver collects
//! all requests delivered so far and processes them as one batch: one group commit for the
//! writes, one submission for the reads. Writes are acknowledged only once durable, so the
//! device advertises no volatile write cache and flushes are no-ops.

use std::cell::RefCell;
use std::fs::File;
use std::io::Read;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use flaminio_format::Uuid;
use flaminio_format::consts::{BLOCK_SIZE, BLOCK_SIZE_U64, GIB};
use flaminio_format::layout::LayoutParams;
use flaminio_format::mkfs::FormatOptions;
use flaminio_format::superblock::select_superblock;
use flaminio_store::{BlockDevice, Store, StoreConfig};
use flaminio_uring::{AlignedBuf, UringDevice};
use libublk::ctrl::{UblkCtrl, UblkCtrlBuilder};
use libublk::helpers::IoBuf;
use libublk::io::{UblkDev, UblkQueue};
use libublk::{BufDesc, UblkError, UblkFlags, sys};

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

fn parse_size(s: &str) -> Result<u64, String> {
    let (num, mult) = match s.chars().last() {
        Some('K' | 'k') => (&s[..s.len() - 1], 1u64 << 10),
        Some('M' | 'm') => (&s[..s.len() - 1], 1 << 20),
        Some('G' | 'g') => (&s[..s.len() - 1], 1 << 30),
        Some('T' | 't') => (&s[..s.len() - 1], 1 << 40),
        _ => (s, 1),
    };
    num.parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(mult))
        .ok_or_else(|| format!("invalid size {s}"))
}

/// Volume names become UUIDs by padding: readable in `flaminio-disk inspect --slots`.
fn volume_uuid(name: &str) -> Result<Uuid, String> {
    if name.is_empty() || name.len() > 16 {
        return Err("volume name must be 1 to 16 bytes".into());
    }
    let mut b = [0u8; 16];
    b[..name.len()].copy_from_slice(name.as_bytes());
    Ok(Uuid(b))
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

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| e.to_string())?;
    Ok(b)
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
    if c.format {
        let size = dev.size();
        let (mut first, mut last) = (
            AlignedBuf::zeroed(BLOCK_SIZE),
            AlignedBuf::zeroed(BLOCK_SIZE),
        );
        dev.read_at(0, &mut first).map_err(|e| e.to_string())?;
        dev.read_at((size & !(BLOCK_SIZE_U64 - 1)) - BLOCK_SIZE_U64, &mut last)
            .map_err(|e| e.to_string())?;
        if select_superblock(&first, &last).is_ok() && !c.force {
            return Err(format!(
                "{} already holds a Flaminio disk; pass --force to destroy it",
                c.device
            ));
        }
        let options = FormatOptions {
            disk_uuid: Uuid(random_bytes()?),
            cluster_uuid: Uuid::NIL,
            format_nonce: u64::from_le_bytes(random_bytes()?) | 1,
            created_unix_secs: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            layout: LayoutParams::default(),
            label: "flaminio-io".into(),
        };
        Store::format(&dev, &options).map_err(|e| format!("format: {e}"))?;
        eprintln!("formatted {}", c.device);
    }
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

/// A request handed from a tag task to the driver.
struct Pending {
    tag: u16,
    done: smol::channel::Sender<i32>,
}

fn serve_queue(qid: u16, dev: &UblkDev, c: &Config) -> Result<(), String> {
    let store = Store::open(open_device(c)?, StoreConfig::default())
        .map_err(|e| format!("open store: {e}"))?;
    let volume = RefCell::new(
        Volume::open(store, c.layout(), c.epoch).map_err(|e| format!("open volume: {e}"))?,
    );
    let q = Rc::new(UblkQueue::new(qid, dev).map_err(|e| format!("ublk queue: {e}"))?);
    let depth = dev.dev_info.queue_depth;
    let buf_bytes = dev.dev_info.max_io_buf_bytes as usize;
    let bufs: Rc<Vec<RefCell<IoBuf<u8>>>> = Rc::new(
        (0..depth)
            .map(|_| RefCell::new(IoBuf::<u8>::new(buf_bytes)))
            .collect(),
    );
    let exe = Rc::new(smol::LocalExecutor::new());
    let (tx, rx) = smol::channel::unbounded::<Pending>();

    let mut tasks = Vec::with_capacity(depth as usize);
    for tag in 0..depth {
        let (q, bufs, tx) = (q.clone(), bufs.clone(), tx.clone());
        tasks.push(exe.spawn(async move {
            match io_task(&q, tag, &bufs, tx).await {
                Ok(()) | Err(UblkError::QueueIsDown) => {}
                Err(e) => eprintln!("flaminio-io: tag {tag}: {e}"),
            }
        }));
    }
    drop(tx);
    let driver = {
        let (q, bufs) = (q.clone(), bufs.clone());
        let volume = &volume;
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

/// One tag: fetch a request, hand it to the driver, commit its result, repeat.
async fn io_task(
    q: &UblkQueue<'_>,
    tag: u16,
    bufs: &[RefCell<IoBuf<u8>>],
    tx: smol::channel::Sender<Pending>,
) -> Result<(), UblkError> {
    let (done_tx, done_rx) = smol::channel::bounded(1);
    {
        let buf = bufs[tag as usize].borrow();
        q.submit_io_prep_cmd(tag, BufDesc::Slice(buf.as_slice()), 0, Some(&*buf))
            .await?;
    }
    loop {
        if tx
            .send(Pending {
                tag,
                done: done_tx.clone(),
            })
            .await
            .is_err()
        {
            return Ok(());
        }
        let res = done_rx.recv().await.unwrap_or(-libc::EIO);
        // The buffer stays borrowed until the kernel delivers this tag's next request; the
        // driver only touches a tag's buffer while it holds that tag's request.
        let buf = bufs[tag as usize].borrow();
        q.submit_io_commit_cmd(tag, BufDesc::Slice(buf.as_slice()), res)
            .await?;
    }
}

/// Collects every request delivered in this round and processes them as one batch.
async fn drive(
    q: &UblkQueue<'_>,
    bufs: &[RefCell<IoBuf<u8>>],
    rx: smol::channel::Receiver<Pending>,
    volume: &RefCell<Volume<UringDevice>>,
) {
    while let Ok(first) = rx.recv().await {
        // Let the other tag tasks woken in this round queue their requests too.
        smol::future::yield_now().await;
        let mut batch = vec![first];
        while let Ok(p) = rx.try_recv() {
            batch.push(p);
        }
        let mut results = vec![0i32; batch.len()];
        {
            let mut guards: Vec<_> = batch
                .iter()
                .map(|p| bufs[p.tag as usize].borrow_mut())
                .collect();
            let mut reqs = Vec::with_capacity(batch.len());
            let mut index = Vec::with_capacity(batch.len());
            for (i, (p, g)) in batch.iter().zip(guards.iter_mut()).enumerate() {
                let iod = q.get_iod(p.tag);
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
        for (p, r) in batch.into_iter().zip(results) {
            let _ = p.done.send(r).await;
        }
    }
}
