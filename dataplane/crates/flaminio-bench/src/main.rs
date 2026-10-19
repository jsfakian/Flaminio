//! `flaminio-bench`: drives the replica store directly, with no frontend or network, to
//! measure what the store itself costs compared with the raw device.
//!
//! "Batch" plays the role of fio's queue depth: each iteration submits `batch` requests
//! together (one group commit for writes, one io_uring submission for reads) and waits for all
//! of them. Latency is per batch, which equals per request since they complete together.

use std::fs::File;
use std::io::{self, Read};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flaminio_format::Uuid;
use flaminio_format::consts::{BLOCK_SIZE, BLOCK_SIZE_U64, MIB};
use flaminio_format::layout::LayoutParams;
use flaminio_format::mkfs::FormatOptions;
use flaminio_format::superblock::select_superblock;
use flaminio_store::{BlockDevice, ChunkKey, ReadReq, Store, StoreConfig, WriteReq};
use flaminio_uring::AlignedBuf;

const USAGE: &str = "\
usage: flaminio-bench --device PATH [options]

  --format             format the device first (refuses to overwrite a Flaminio disk
                       unless --force)
  --force              allow --format to destroy an existing Flaminio disk
  --journal-size SIZE  journal size for --format (default: the format's default for the disk)
  --chunks N           chunks in the working set (default 16)
  --chunk-size SIZE    chunk size (default 256M)
  --workload W         randread | randwrite | read | write (default randwrite)
  --bs SIZE            request size, a multiple of 4K (default 4K)
  --batch N            requests submitted together (default 1)
  --runtime SECONDS    measured duration (default 30)
  --prefill            write the whole working set once first, if it is still empty
  --buffered           use the page cache instead of O_DIRECT (Linux)
  --name NAME          label for the output
  --json PATH          also write the result as JSON

SIZE accepts K, M and G suffixes.";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Workload {
    RandRead,
    RandWrite,
    Read,
    Write,
}

impl Workload {
    fn is_write(self) -> bool {
        matches!(self, Workload::RandWrite | Workload::Write)
    }

    fn is_random(self) -> bool {
        matches!(self, Workload::RandRead | Workload::RandWrite)
    }
}

struct Args {
    device: String,
    format: bool,
    force: bool,
    chunks: u64,
    chunk_size: u64,
    workload: Workload,
    bs: u64,
    batch: usize,
    runtime: Duration,
    prefill: bool,
    buffered: bool,
    journal_size: Option<u64>,
    name: Option<String>,
    json: Option<String>,
}

type Result<T> = std::result::Result<T, String>;

fn main() -> ExitCode {
    match parse_args().and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_size(s: &str) -> Result<u64> {
    let (num, mult) = match s.chars().last() {
        Some('K' | 'k') => (&s[..s.len() - 1], 1u64 << 10),
        Some('M' | 'm') => (&s[..s.len() - 1], 1 << 20),
        Some('G' | 'g') => (&s[..s.len() - 1], 1 << 30),
        _ => (s, 1),
    };
    num.parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(mult))
        .ok_or_else(|| format!("invalid size {s}"))
}

fn parse_args() -> Result<Args> {
    let mut a = Args {
        device: String::new(),
        format: false,
        force: false,
        chunks: 16,
        chunk_size: 256 * MIB,
        workload: Workload::RandWrite,
        bs: 4096,
        batch: 1,
        runtime: Duration::from_secs(30),
        prefill: false,
        buffered: false,
        journal_size: None,
        name: None,
        json: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--device" => a.device = value()?,
            "--format" => a.format = true,
            "--force" => a.force = true,
            "--chunks" => a.chunks = value()?.parse().map_err(|_| "invalid --chunks")?,
            "--chunk-size" => a.chunk_size = parse_size(&value()?)?,
            "--workload" => {
                a.workload = match value()?.as_str() {
                    "randread" => Workload::RandRead,
                    "randwrite" => Workload::RandWrite,
                    "read" => Workload::Read,
                    "write" => Workload::Write,
                    w => return Err(format!("unknown workload {w}")),
                }
            }
            "--bs" => a.bs = parse_size(&value()?)?,
            "--batch" => a.batch = value()?.parse().map_err(|_| "invalid --batch")?,
            "--runtime" => {
                a.runtime = Duration::from_secs(value()?.parse().map_err(|_| "invalid --runtime")?)
            }
            "--prefill" => a.prefill = true,
            "--journal-size" => a.journal_size = Some(parse_size(&value()?)?),
            "--buffered" => a.buffered = true,
            "--name" => a.name = Some(value()?),
            "--json" => a.json = Some(value()?),
            "-h" | "--help" => return Err(USAGE.into()),
            f => return Err(format!("unknown argument {f}\n\n{USAGE}")),
        }
    }
    if a.device.is_empty() {
        return Err(USAGE.into());
    }
    if a.bs == 0
        || !a.bs.is_multiple_of(BLOCK_SIZE_U64)
        || a.bs > a.chunk_size
        || a.batch == 0
        || a.chunks == 0
    {
        return Err("--bs must be a non-zero multiple of 4K no larger than a chunk; --batch and --chunks must be positive".into());
    }
    Ok(a)
}

fn open_device(path: &str, buffered: bool) -> io::Result<Box<dyn BlockDevice>> {
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(flaminio_uring::UringDevice::open(
            path, !buffered,
        )?))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = buffered;
        Ok(Box::new(flaminio_store::FileDevice::open(path)?))
    }
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| e.to_string())?;
    Ok(b)
}

/// xorshift64*: fast, good enough to pick offsets.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// User + system CPU time of this process, in microseconds (Linux; None elsewhere).
fn cpu_time_us() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let ticks: u64 = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
    Some(ticks * 10_000) // USER_HZ is 100 on Linux
}

fn run(a: Args) -> Result<()> {
    let dev = open_device(&a.device, a.buffered).map_err(|e| format!("open {}: {e}", a.device))?;
    if a.format {
        let size = dev.size();
        let mut first = AlignedBuf::zeroed(BLOCK_SIZE);
        let mut last = AlignedBuf::zeroed(BLOCK_SIZE);
        dev.read_at(0, &mut first).map_err(|e| e.to_string())?;
        dev.read_at((size & !(BLOCK_SIZE_U64 - 1)) - BLOCK_SIZE_U64, &mut last)
            .map_err(|e| e.to_string())?;
        if select_superblock(&first, &last).is_ok() && !a.force {
            return Err(format!(
                "{} already holds a Flaminio disk; pass --force to destroy it",
                a.device
            ));
        }
        let options = FormatOptions {
            disk_uuid: Uuid(random_bytes()?),
            cluster_uuid: Uuid::NIL,
            format_nonce: u64::from_le_bytes(random_bytes()?) | 1,
            created_unix_secs: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            layout: LayoutParams {
                journal_size: a.journal_size,
                ..LayoutParams::default()
            },
            label: "flaminio-bench".into(),
        };
        Store::format(&dev, &options).map_err(|e| format!("format: {e}"))?;
        eprintln!("formatted {}", a.device);
    }

    let t = Instant::now();
    let mut store =
        Store::open(dev, StoreConfig::default()).map_err(|e| format!("open store: {e}"))?;
    eprintln!("store opened in {:.2}s", t.elapsed().as_secs_f64());

    let volume = Uuid(*b"flaminio-bench\0\0");
    let keys: Vec<ChunkKey> = (0..a.chunks)
        .map(|index| ChunkKey { volume, index })
        .collect();
    let volume_size = a.chunks * a.chunk_size;
    for &k in &keys {
        match store.chunk_info(k) {
            Some(info) if info.chunk_size != a.chunk_size => {
                return Err(format!(
                    "chunk {k} exists with size {}; use --format",
                    info.chunk_size
                ));
            }
            Some(_) => {}
            None => store
                .create_chunk(k, a.chunk_size, volume_size, 1)
                .map_err(|e| format!("create {k}: {e}"))?,
        }
    }
    let mut write_seq = keys
        .iter()
        .filter_map(|&k| store.chunk_info(k))
        .map(|i| i.last_write_seq)
        .max()
        .unwrap_or(0);

    if a.prefill && write_seq == 0 {
        let t = Instant::now();
        let data = AlignedBuf::zeroed(MIB as usize);
        for &k in &keys {
            for group in (0..a.chunk_size / MIB).collect::<Vec<_>>().chunks(8) {
                let reqs: Vec<WriteReq<'_>> = group
                    .iter()
                    .map(|&m| {
                        write_seq += 1;
                        WriteReq {
                            key: k,
                            offset: m * MIB,
                            data: (&data[..]).into(),
                            write_seq,
                            epoch: 1,
                        }
                    })
                    .collect();
                for r in store
                    .write_batch(&reqs)
                    .map_err(|e| format!("prefill: {e}"))?
                {
                    r.map_err(|e| format!("prefill: {e}"))?;
                }
            }
        }
        let secs = t.elapsed().as_secs_f64();
        eprintln!(
            "prefilled {} MiB in {secs:.1}s ({:.0} MiB/s)",
            volume_size / MIB,
            (volume_size / MIB) as f64 / secs
        );
    }

    let mut rng = Rng(u64::from_le_bytes(random_bytes()?) | 1);
    let mut bufs: Vec<AlignedBuf> = (0..a.batch)
        .map(|_| AlignedBuf::zeroed(a.bs as usize))
        .collect();
    for b in &mut bufs {
        for word in b.as_chunks_mut::<8>().0 {
            word.copy_from_slice(&rng.next().to_le_bytes());
        }
    }
    let per_chunk = a.chunk_size / a.bs;
    let slots = a.chunks * per_chunk;
    let mut cursor = 0u64;
    let mut latencies: Vec<u32> = Vec::new();
    let mut ios = 0u64;
    let checkpoints_before = store.stats().checkpoint_seq;
    let cpu_before = cpu_time_us();
    let start = Instant::now();

    while start.elapsed() < a.runtime {
        let positions: Vec<(ChunkKey, u64)> = (0..a.batch)
            .map(|_| {
                let slot = if a.workload.is_random() {
                    rng.next() % slots
                } else {
                    let s = cursor;
                    cursor = (cursor + 1) % slots;
                    s
                };
                (keys[(slot / per_chunk) as usize], (slot % per_chunk) * a.bs)
            })
            .collect();
        let t = Instant::now();
        let results = if a.workload.is_write() {
            for b in &mut bufs {
                b[..8].copy_from_slice(&(write_seq + 1).to_le_bytes());
            }
            let reqs: Vec<WriteReq<'_>> = positions
                .iter()
                .zip(&bufs)
                .map(|(&(key, offset), b)| {
                    write_seq += 1;
                    WriteReq {
                        key,
                        offset,
                        data: (&b[..]).into(),
                        write_seq,
                        epoch: 1,
                    }
                })
                .collect();
            store.write_batch(&reqs)
        } else {
            let mut reqs: Vec<ReadReq<'_>> = positions
                .iter()
                .zip(bufs.iter_mut())
                .map(|(&(key, offset), b)| ReadReq {
                    key,
                    offset,
                    buf: &mut b[..],
                    epoch: 1,
                })
                .collect();
            store.read_batch(&mut reqs)
        };
        for r in results.map_err(|e| format!("store failed: {e}"))? {
            r.map_err(|e| format!("request failed: {e}"))?;
        }
        latencies.push(t.elapsed().as_micros().min(u128::from(u32::MAX)) as u32);
        ios += a.batch as u64;
    }

    let secs = start.elapsed().as_secs_f64();
    let cpu_us = cpu_before.zip(cpu_time_us()).map(|(b, e)| e - b);
    latencies.sort_unstable();
    let pct = |p: f64| latencies[((latencies.len() - 1) as f64 * p).round() as usize];
    let name = a.name.clone().unwrap_or_else(|| {
        format!("{:?}-{}k-b{}", a.workload, a.bs / 1024, a.batch).to_lowercase()
    });
    let iops = ios as f64 / secs;
    let mibs = ios as f64 * a.bs as f64 / secs / MIB as f64;
    let checkpoints = store.stats().checkpoint_seq - checkpoints_before;
    let cpu_per_io = cpu_us.map(|c| c as f64 / ios as f64);
    println!(
        "{name:<18} {:>10.0} {mibs:>10.1} {:>10} {:>10}   cpu/io {} us, checkpoints {checkpoints}",
        iops,
        pct(0.5),
        pct(0.99),
        cpu_per_io.map_or("n/a".into(), |c| format!("{c:.1}")),
    );

    if let Some(path) = a.json {
        let json = format!(
            "{{\"name\":\"{name}\",\"workload\":\"{:?}\",\"bs\":{},\"batch\":{},\"chunks\":{},\"chunk_size\":{},\
             \"direct\":{},\"runtime_s\":{secs:.3},\"ios\":{ios},\"iops\":{iops:.1},\"mib_s\":{mibs:.2},\
             \"lat_us\":{{\"p50\":{},\"p99\":{},\"p999\":{},\"max\":{}}},\"cpu_us_per_io\":{},\"checkpoints\":{checkpoints}}}\n",
            a.workload,
            a.bs,
            a.batch,
            a.chunks,
            a.chunk_size,
            cfg!(target_os = "linux") && !a.buffered,
            pct(0.5),
            pct(0.99),
            pct(0.999),
            latencies[latencies.len() - 1],
            cpu_per_io.map_or("null".into(), |c| format!("{c:.2}")),
        );
        std::fs::write(&path, json).map_err(|e| format!("write {path}: {e}"))?;
    }
    Ok(())
}
