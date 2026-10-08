//! `flaminio-disk`: format and inspect Flaminio disks.
//!
//! A development tool for phase 0. It works on regular files as well as block devices, so the
//! on-disk format can be exercised on any machine. In production the node agent formats disks.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::FileExt;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use flaminio_format::consts::{
    BITS_PER_BITMAP_PAGE, BLOCK_SIZE, BLOCK_SIZE_U64, DEFAULT_CLUSTER_SIZE,
};
use flaminio_format::journal::{JournalPosition, JournalScanner};
use flaminio_format::layout::LayoutParams;
use flaminio_format::meta::{BitmapKind, BitmapPage, CheckpointHeader, ChunkHeader, ChunkState};
use flaminio_format::mkfs::{self, FormatOptions};
use flaminio_format::page::{PageType, PairChoice, select_pair};
use flaminio_format::superblock::{Superblock, SuperblockCopy, select_superblock};
use flaminio_format::{ReadAt, Uuid};

const USAGE: &str = "\
usage:
  flaminio-disk format <path> [--size <SIZE>] [--cluster-size <SIZE>] [--journal-size <SIZE>]
                              [--label <TEXT>] [--cluster-uuid <UUID>] [--force]
  flaminio-disk inspect <path> [--slots]

SIZE accepts K, M, G and T suffixes (powers of 1024). --size creates or resizes a regular file.";

type Result<T> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("format") => format(&args[1..]),
        Some("inspect") => inspect(&args[1..]),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Splits `args` into one positional path and `--flag [value]` options.
fn parse<'a>(
    args: &'a [String],
    flags: &[&str],
    switches: &[&str],
) -> Result<(&'a str, Vec<(&'a str, &'a str)>)> {
    let mut path = None;
    let mut opts = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if switches.contains(&a.as_str()) {
            opts.push((a.as_str(), ""));
        } else if flags.contains(&a.as_str()) {
            let v = it.next().ok_or_else(|| format!("{a} needs a value"))?;
            opts.push((a.as_str(), v.as_str()));
        } else if a.starts_with("--") || path.is_some() {
            return Err(format!("unexpected argument {a}\n\n{USAGE}"));
        } else {
            path = Some(a.as_str());
        }
    }
    Ok((path.ok_or_else(|| USAGE.to_owned())?, opts))
}

fn opt<'a>(opts: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
    opts.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
}

fn parse_size(s: &str) -> Result<u64> {
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

fn parse_uuid(s: &str) -> Result<Uuid> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return Err(format!("invalid UUID {s}"));
    }
    let mut b = [0u8; 16];
    for (i, byte) in b.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16)
            .map_err(|_| format!("invalid UUID {s}"))?;
    }
    Ok(Uuid(b))
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| format!("reading /dev/urandom: {e}"))?;
    Ok(b)
}

/// A random version-4 UUID.
fn random_uuid() -> Result<Uuid> {
    let mut b: [u8; 16] = random()?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(Uuid(b))
}

fn device_size(f: &mut File) -> Result<u64> {
    f.seek(SeekFrom::End(0))
        .map_err(|e| format!("cannot determine device size: {e}"))
}

fn read_block(f: &File, offset: u64) -> io::Result<Vec<u8>> {
    let mut b = vec![0u8; BLOCK_SIZE];
    ReadAt::read_at(f, offset, &mut b)?;
    Ok(b)
}

/// Reads the superblock. Copy B is looked for at the end of the device as it is now.
fn read_superblock(f: &File, size: u64) -> Result<(Superblock, SuperblockCopy)> {
    if size < 2 * BLOCK_SIZE_U64 {
        return Err("device too small to hold a superblock".into());
    }
    let a = read_block(f, 0).map_err(|e| e.to_string())?;
    let b = read_block(f, (size & !(BLOCK_SIZE_U64 - 1)) - BLOCK_SIZE_U64)
        .map_err(|e| e.to_string())?;
    select_superblock(&a, &b).map_err(|e| format!("no valid Flaminio superblock: {e}"))
}

fn write_all(f: &File, writes: impl IntoIterator<Item = (u64, impl AsRef<[u8]>)>) -> Result<()> {
    for (offset, data) in writes {
        f.write_all_at(data.as_ref(), offset)
            .map_err(|e| format!("write at offset {offset}: {e}"))?;
    }
    f.sync_all().map_err(|e| format!("flush: {e}"))
}

fn format(args: &[String]) -> Result<()> {
    let (path, opts) = parse(
        args,
        &[
            "--size",
            "--cluster-size",
            "--journal-size",
            "--label",
            "--cluster-uuid",
        ],
        &["--force"],
    )?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(opt(&opts, "--size").is_some())
        .truncate(false)
        .open(path)
        .map_err(|e| format!("open {path}: {e}"))?;
    if let Some(size) = opt(&opts, "--size") {
        file.set_len(parse_size(size)?)
            .map_err(|e| format!("resize {path}: {e}"))?;
    }
    let size = device_size(&mut file)?;

    let existing = read_superblock(&file, size).ok();
    if let Some((sb, _)) = &existing {
        if opt(&opts, "--force").is_none() {
            return Err(format!(
                "{path} already holds Flaminio disk {} (label {:?}); pass --force to destroy it",
                sb.disk_uuid, sb.label
            ));
        }
    }

    let cluster_size = opt(&opts, "--cluster-size")
        .map(parse_size)
        .transpose()?
        .unwrap_or(DEFAULT_CLUSTER_SIZE.into());
    let options = FormatOptions {
        disk_uuid: random_uuid()?,
        cluster_uuid: opt(&opts, "--cluster-uuid")
            .map(parse_uuid)
            .transpose()?
            .unwrap_or(Uuid::NIL),
        format_nonce: u64::from_le_bytes(random()?) | 1,
        created_unix_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        layout: LayoutParams {
            cluster_size: u32::try_from(cluster_size).map_err(|_| "cluster size too large")?,
            journal_size: opt(&opts, "--journal-size").map(parse_size).transpose()?,
        },
        label: opt(&opts, "--label").unwrap_or_default().to_owned(),
    };
    let plan = mkfs::plan(size, &options).map_err(|e| e.to_string())?;

    // Phase 1: wipe the new superblock locations, and the old copy B if the device has changed size.
    let zero = [0u8; BLOCK_SIZE];
    let mut wipe: Vec<u64> = plan.wipe.clone();
    if let Some((old, _)) = &existing {
        wipe.push(old.layout.superblock_b_offset());
    }
    write_all(&file, wipe.iter().map(|&o| (o, &zero[..])))?;
    // Phase 2: metadata. Phase 3: superblocks.
    write_all(&file, plan.metadata.iter().map(|(o, p)| (*o, &p[..])))?;
    write_all(&file, plan.superblocks.iter().map(|(o, p)| (*o, &p[..])))?;

    println!("formatted {path}");
    print_superblock(&plan.superblock, SuperblockCopy::A);
    Ok(())
}

fn print_superblock(sb: &Superblock, copy: SuperblockCopy) {
    let l = &sb.layout;
    let gib = |b: u64| b as f64 / (1u64 << 30) as f64;
    println!("superblock (copy {copy:?})");
    println!("  disk uuid       {}", sb.disk_uuid);
    println!("  cluster uuid    {}", sb.cluster_uuid);
    println!("  label           {:?}", sb.label);
    println!("  format nonce    {:#018x}", sb.format_nonce);
    println!("  generation      {}", sb.sb_generation);
    println!(
        "  features        compat={:#x} ro_compat={:#x} incompat={:#x}",
        sb.features_compat, sb.features_ro_compat, sb.features_incompat
    );
    println!("layout");
    println!("  device          {:.2} GiB", gib(l.device_size));
    println!("  cluster size    {} KiB", l.cluster_size / 1024);
    println!(
        "  journal         offset {:>15}  size {:>8.2} GiB",
        l.journal_offset,
        gib(l.journal_size)
    );
    println!(
        "  metadata        offset {:>15}  size {:>8.2} GiB",
        l.meta_offset,
        gib(l.meta_size)
    );
    println!("    chunk slots   {}", l.chunk_slots);
    println!("    page pool     {} pairs", l.pool_pairs);
    println!(
        "  data            offset {:>15}  size {:>8.2} GiB ({} clusters)",
        l.data_offset,
        gib(l.data_clusters * u64::from(l.cluster_size)),
        l.data_clusters
    );
    println!("  overhead        {:.3}%", l.overhead_ratio() * 100.0);
}

fn inspect(args: &[String]) -> Result<()> {
    let (path, opts) = parse(args, &[], &["--slots"])?;
    let mut file = File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let size = device_size(&mut file)?;
    let (sb, copy) = read_superblock(&file, size)?;
    print_superblock(&sb, copy);
    if sb.requires_read_only() {
        println!("  note            unknown ro_compat features: read-only");
    }
    let l = sb.layout;
    let nonce = sb.format_nonce;
    let io = |e: io::Error| e.to_string();
    let pair = |offset: u64, page_type: PageType, max_seq: u64| -> Result<PairChoice> {
        let (a, b) = (
            read_block(&file, offset).map_err(io)?,
            read_block(&file, offset + BLOCK_SIZE_U64).map_err(io)?,
        );
        Ok(select_pair(&a, &b, page_type, nonce, offset, max_seq))
    };

    let ck = pair(l.checkpoint_offset, PageType::Checkpoint, u64::MAX)?;
    let Some((slot, _)) = ck.current else {
        return Err("no valid checkpoint header".into());
    };
    let ck_off = PairChoice::slot_offset(l.checkpoint_offset, slot);
    let (k, ckpt) =
        CheckpointHeader::decode(&read_block(&file, ck_off).map_err(io)?, nonce, ck_off, &l)
            .map_err(|e| e.to_string())?;
    println!("checkpoint {k} (slot {slot})");
    println!(
        "  journal start   offset {} seq {}",
        ckpt.journal_start_offset, ckpt.journal_start_seq
    );

    let mut used = [0u64; 2];
    for (kind, pairs, idx) in [
        (BitmapKind::Data, l.data_bitmap_pairs, 0),
        (BitmapKind::Pool, l.pool_bitmap_pairs, 1),
    ] {
        let (base, page_type) = match kind {
            BitmapKind::Data => (l.data_bitmap_offset, PageType::DataBitmap),
            BitmapKind::Pool => (l.pool_bitmap_offset, PageType::PoolBitmap),
        };
        for i in 0..pairs {
            let off = base + i * 2 * BLOCK_SIZE_U64;
            let Some((s, _)) = pair(off, page_type, k)?.current else {
                return Err(format!("{kind:?} bitmap page {i} missing"));
            };
            let slot_off = PairChoice::slot_offset(off, s);
            let (_, bm) = BitmapPage::decode(
                &read_block(&file, slot_off).map_err(io)?,
                kind,
                nonce,
                slot_off,
                i * BITS_PER_BITMAP_PAGE,
            )
            .map_err(|e| e.to_string())?;
            used[idx] += bm.count_ones();
        }
    }
    println!("  data clusters   {} used / {}", used[0], l.data_clusters);
    println!("  page pool       {} used / {}", used[1], l.pool_pairs);

    let mut active = 0;
    for s in 0..l.chunk_slots {
        let off = l.chunk_slot_offset(s);
        let Some((slot, _)) = pair(off, PageType::ChunkHeader, k)?.current else {
            continue;
        };
        let slot_off = PairChoice::slot_offset(off, slot);
        match ChunkHeader::decode(
            &read_block(&file, slot_off).map_err(io)?,
            nonce,
            slot_off,
            &l,
        ) {
            Ok((_, h)) if h.state != ChunkState::Free => {
                active += 1;
                if opt(&opts, "--slots").is_some() {
                    println!(
                        "  slot {s:>6}  {:?} volume {} chunk {} layers {}",
                        h.state,
                        h.volume_uuid,
                        h.chunk_index,
                        h.layers.len()
                    );
                }
            }
            Ok(_) => {}
            Err(e) => println!("  slot {s:>6}  INVALID: {e}"),
        }
    }
    println!("  chunk slots     {active} in use / {}", l.chunk_slots);

    let start = JournalPosition {
        offset: ckpt.journal_start_offset,
        seq: ckpt.journal_start_seq,
    };
    let mut scan = JournalScanner::new(&file, &l, nonce, start);
    let (mut batches, mut records) = (0, 0);
    for batch in scan.by_ref() {
        let batch = batch.map_err(|e| e.to_string())?;
        batches += 1;
        records += batch.header().record_count;
    }
    let end = scan.position();
    println!("journal");
    println!("  to replay       {batches} batches, {records} records");
    println!("  log end         offset {} seq {}", end.offset, end.seq);
    Ok(())
}
