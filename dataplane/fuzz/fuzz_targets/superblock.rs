//! Superblock decoding. The fuzzer input is used as a superblock with a repaired magic and CRC,
//! so mutations reach the field validation instead of dying at the checksum.
#![no_main]

use flaminio_format::checksum::crc32c;
use flaminio_format::consts::{BLOCK_SIZE, SUPERBLOCK_MAGIC};
use flaminio_format::superblock::Superblock;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut b = vec![0u8; BLOCK_SIZE];
    let n = data.len().min(BLOCK_SIZE);
    b[..n].copy_from_slice(&data[..n]);
    let _ = Superblock::decode(&b);
    b[0..8].copy_from_slice(&SUPERBLOCK_MAGIC);
    let crc = crc32c(&b[..BLOCK_SIZE - 4]);
    b[BLOCK_SIZE - 4..].copy_from_slice(&crc.to_le_bytes());
    let _ = Superblock::decode(&b);
});
