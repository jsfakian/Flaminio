//! CRC-32C throughput on this CPU: the `crc32c` crate (SSE4.2 `crc32`, 3 streams) against
//! `crc-fast` (carry-less multiplication folding, chosen at run time), per 4 KiB block as
//! read verification computes them, and over a whole 512 KiB buffer.
//!
//! `cargo run --release -p flaminio-format --example crc_bench`

use std::hint::black_box;
use std::time::Instant;

use crc_fast::{CrcAlgorithm, checksum};

fn main() {
    let data: Vec<u8> = (0..512 << 10)
        .map(|i: u32| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let rounds = 4000;
    let gb = |t: f64| (data.len() * rounds) as f64 / t / 1e9;
    for (name, f) in [
        (
            "crc32c, per 4 KiB  ",
            &(|b: &[u8]| crc32c::crc32c(b) as u64) as &dyn Fn(&[u8]) -> u64,
        ),
        (
            "crc-fast, per 4 KiB",
            &(|b: &[u8]| checksum(CrcAlgorithm::Crc32Iscsi, b)),
        ),
    ] {
        let t = Instant::now();
        let mut x = 0;
        for _ in 0..rounds {
            for block in data.chunks_exact(4096) {
                x ^= f(black_box(block));
            }
        }
        black_box(x);
        println!("{name}: {:5.1} GB/s", gb(t.elapsed().as_secs_f64()));
    }
    for (name, f) in [
        (
            "crc32c, 512 KiB    ",
            &(|b: &[u8]| crc32c::crc32c(b) as u64) as &dyn Fn(&[u8]) -> u64,
        ),
        (
            "crc-fast, 512 KiB  ",
            &(|b: &[u8]| checksum(CrcAlgorithm::Crc32Iscsi, b)),
        ),
    ] {
        let t = Instant::now();
        let mut x = 0;
        for _ in 0..rounds {
            x ^= f(black_box(&data));
        }
        black_box(x);
        println!("{name}: {:5.1} GB/s", gb(t.elapsed().as_secs_f64()));
    }
    // Same values.
    for block in data.chunks_exact(4096) {
        assert_eq!(
            crc32c::crc32c(block) as u64,
            checksum(CrcAlgorithm::Crc32Iscsi, block)
        );
    }
    println!("same checksums: yes");
}
