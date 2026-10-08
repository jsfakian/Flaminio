//! Commands that need no ublk, so they run on any platform: the replica service and offline
//! digests. Also the helpers the ublk frontend shares.

use std::fs::File;
use std::io::Read;
use std::net::TcpListener;
use std::time::{SystemTime, UNIX_EPOCH};

use flaminio_format::Uuid;
use flaminio_format::consts::{BLOCK_SIZE, BLOCK_SIZE_U64, GIB};
use flaminio_format::layout::LayoutParams;
use flaminio_format::mkfs::FormatOptions;
use flaminio_format::superblock::select_superblock;
use flaminio_store::{BlockDevice, Store, StoreConfig};
use flaminio_uring::AlignedBuf;

use crate::volume::{Volume, VolumeLayout};

const USAGE: &str = "\
usage:
  flaminio-io replica --device PATH --listen ADDR [--format [--force]] [--buffered]
      serve this disk as the remote replica of a volume (one target at a time)
  flaminio-io digest --device PATH --size SIZE [--chunk-size SIZE] [--volume NAME] [--epoch N]
      print a checksum of every chunk of a volume, to compare replicas offline";

pub fn parse_size(s: &str) -> Result<u64, String> {
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
pub fn volume_uuid(name: &str) -> Result<Uuid, String> {
    if name.is_empty() || name.len() > 16 {
        return Err("volume name must be 1 to 16 bytes".into());
    }
    let mut b = [0u8; 16];
    b[..name.len()].copy_from_slice(name.as_bytes());
    Ok(Uuid(b))
}

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| e.to_string())?;
    Ok(b)
}

/// Formats `dev` if asked; refuses to destroy an existing Flaminio disk without `force`.
pub fn format_if_asked<D: BlockDevice>(
    dev: &D,
    path: &str,
    format: bool,
    force: bool,
) -> Result<(), String> {
    if !format {
        return Ok(());
    }
    let size = dev.size();
    let (mut first, mut last) = (
        AlignedBuf::zeroed(BLOCK_SIZE),
        AlignedBuf::zeroed(BLOCK_SIZE),
    );
    dev.read_at(0, &mut first).map_err(|e| e.to_string())?;
    dev.read_at((size & !(BLOCK_SIZE_U64 - 1)) - BLOCK_SIZE_U64, &mut last)
        .map_err(|e| e.to_string())?;
    if select_superblock(&first, &last).is_ok() && !force {
        return Err(format!(
            "{path} already holds a Flaminio disk; pass --force to destroy it"
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
    Store::format(dev, &options).map_err(|e| format!("format: {e}"))?;
    eprintln!("formatted {path}");
    Ok(())
}

fn open_device(path: &str, buffered: bool) -> Result<Box<dyn BlockDevice>, String> {
    #[cfg(target_os = "linux")]
    let dev: Box<dyn BlockDevice> = Box::new(
        flaminio_uring::UringDevice::open(path, !buffered)
            .map_err(|e| format!("open {path}: {e}"))?,
    );
    #[cfg(not(target_os = "linux"))]
    let dev: Box<dyn BlockDevice> = {
        let _ = buffered;
        Box::new(flaminio_store::FileDevice::open(path).map_err(|e| format!("open {path}: {e}"))?)
    };
    Ok(dev)
}

pub fn main(cmd: &str, args: &[String]) -> Result<(), String> {
    let (mut device, mut listen, mut size, mut chunk_size) = (String::new(), String::new(), 0, GIB);
    let (mut volume, mut epoch) = (volume_uuid("vol0")?, 1);
    let (mut format, mut force, mut buffered) = (false, false, false);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--device" => device = value()?,
            "--listen" => listen = value()?,
            "--size" => size = parse_size(&value()?)?,
            "--chunk-size" => chunk_size = parse_size(&value()?)?,
            "--volume" => volume = volume_uuid(&value()?)?,
            "--epoch" => epoch = value()?.parse().map_err(|_| "invalid --epoch")?,
            "--format" => format = true,
            "--force" => force = true,
            "--buffered" => buffered = true,
            _ => return Err(format!("unknown argument {flag}\n\n{USAGE}")),
        }
    }
    if device.is_empty() {
        return Err(USAGE.into());
    }
    let dev = open_device(&device, buffered)?;
    format_if_asked(&dev, &device, format, force)?;
    let store = Store::open(dev, StoreConfig::default()).map_err(|e| format!("open store: {e}"))?;
    match cmd {
        "replica" => {
            if listen.is_empty() {
                return Err(USAGE.into());
            }
            let listener =
                TcpListener::bind(&listen).map_err(|e| format!("listen on {listen}: {e}"))?;
            crate::replica::serve(store, listener)
        }
        "digest" => {
            if size == 0 {
                return Err(USAGE.into());
            }
            let layout = VolumeLayout {
                volume,
                size,
                chunk_size,
            };
            let v = Volume::open(store, layout, epoch).map_err(|e| format!("open volume: {e}"))?;
            let digests = v.digests().map_err(|e| format!("read: {e}"))?;
            let mut all = Vec::new();
            for (i, units) in digests.iter().enumerate() {
                let bytes: Vec<u8> = units.iter().flat_map(|c| c.to_le_bytes()).collect();
                let crc = flaminio_format::checksum::crc32c(&bytes);
                all.extend_from_slice(&crc.to_le_bytes());
                println!("chunk {i:>4} {crc:08x}");
            }
            println!("volume {:08x}", flaminio_format::checksum::crc32c(&all));
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}
