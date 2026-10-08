//! Measures CRC-32C throughput: one checksum over 64 MiB, and per-4 KiB block checksums (what
//! the store computes on every read and write).

use std::time::Instant;

use flaminio_format::checksum::{block_checksums, crc32c};

fn main() {
    let data: Vec<u8> = (0..64u32 << 20)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let mib = data.len() as f64 / (1 << 20) as f64;
    for (name, f) in [
        (
            "whole buffer",
            &(|d: &[u8]| u64::from(crc32c(d))) as &dyn Fn(&[u8]) -> u64,
        ),
        ("per 4 KiB block", &|d: &[u8]| {
            block_checksums(d).iter().map(|&c| u64::from(c)).sum()
        }),
    ] {
        let mut sink = 0u64;
        let t = Instant::now();
        let rounds = 20;
        for _ in 0..rounds {
            sink = sink.wrapping_add(f(&data));
        }
        let secs = t.elapsed().as_secs_f64();
        println!(
            "{name:<16} {:>8.0} MiB/s  ({:.1} us per MiB)  [{sink}]",
            mib * rounds as f64 / secs,
            secs * 1e6 / (mib * rounds as f64)
        );
    }
}
