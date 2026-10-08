#![allow(dead_code)]

use flaminio_format::Uuid;
use flaminio_format::consts::MIB;
use flaminio_format::layout::LayoutParams;
use flaminio_format::mkfs::FormatOptions;
use flaminio_store::{ChunkKey, MemDevice, Store, StoreConfig};

pub const DEVICE: u64 = 256 * MIB;
pub const CHUNK: u64 = 4 * MIB;
pub const BLOCK: usize = 4096;

pub fn key(i: u64) -> ChunkKey {
    ChunkKey {
        volume: Uuid([0xAA; 16]),
        index: i,
    }
}

pub fn options(journal_mib: u64) -> FormatOptions {
    FormatOptions {
        disk_uuid: Uuid([1; 16]),
        cluster_uuid: Uuid([2; 16]),
        format_nonce: 0x5EED,
        created_unix_secs: 0,
        layout: LayoutParams {
            journal_size: Some(journal_mib * MIB),
            ..Default::default()
        },
        label: String::new(),
    }
}

/// A freshly formatted, opened store on an in-memory device.
pub fn new_store(journal_mib: u64) -> Store<MemDevice> {
    let dev = MemDevice::new(DEVICE);
    Store::format(&dev, &options(journal_mib)).unwrap();
    Store::open(dev, StoreConfig::default()).unwrap()
}

/// Reopens a store from its device (clean shutdown, no checkpoint).
pub fn reopen(store: Store<MemDevice>) -> Store<MemDevice> {
    Store::open(store.into_device(), StoreConfig::default()).unwrap()
}

/// Recognizable block contents: write id, absolute block number, then a fill byte.
pub fn block(write_id: u32, block_no: u64) -> Vec<u8> {
    let mut b = vec![write_id as u8; BLOCK];
    b[0..4].copy_from_slice(&write_id.to_le_bytes());
    b[4..12].copy_from_slice(&block_no.to_le_bytes());
    b
}

pub fn blocks(write_id: u32, first_block: u64, n: u64) -> Vec<u8> {
    (first_block..first_block + n)
        .flat_map(|b| block(write_id, b))
        .collect()
}
