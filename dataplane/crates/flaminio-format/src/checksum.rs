//! CRC-32C helpers. All checksums in the format are CRC-32C (Castagnoli).

use crate::consts::BLOCK_SIZE;

/// CRC-32C of `data`. Carry-less multiplication folding, with the widest vectors the CPU has
/// (chosen at run time: AVX-512 or AVX2 `vpclmulqdq`, `pclmulqdq`, or ARM `pmull`). It is
/// 4–9 times faster per 4 KiB block than the `crc32` instruction alone
/// (`examples/crc_bench.rs`), and verifying reads is where it counts.
pub fn crc32c(data: &[u8]) -> u32 {
    crc_fast::checksum(crc_fast::CrcAlgorithm::Crc32Iscsi, data) as u32
}

/// One CRC-32C per 4 KiB block of `data`, as stored in map entries and write records.
///
/// # Panics
///
/// If `data.len()` is not a multiple of 4096.
pub fn block_checksums(data: &[u8]) -> Vec<u32> {
    assert!(
        data.len().is_multiple_of(BLOCK_SIZE),
        "data length must be a multiple of the block size"
    );
    data.chunks_exact(BLOCK_SIZE).map(crc32c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same values as the reference implementation, for every length and alignment that
    /// matters to the folding code's edges.
    #[test]
    fn matches_the_reference() {
        let data: Vec<u8> = (0..70_000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8)
            .collect();
        for start in [0, 1, 7, 15, 64] {
            for len in (0..300).chain([1023, 1024, 4095, 4096, 4097, 65_536]) {
                let d = &data[start..start + len];
                assert_eq!(crc32c(d), crc32c::crc32c(d), "start {start} len {len}");
            }
        }
    }

    #[test]
    fn known_vector() {
        // RFC 3720 (iSCSI) test vector: 32 bytes of zeros.
        assert_eq!(crc32c(&[0u8; 32]), 0x8A91_36AA);
    }

    #[test]
    fn per_block() {
        let mut data = vec![0u8; 3 * BLOCK_SIZE];
        data[BLOCK_SIZE] = 1;
        let sums = block_checksums(&data);
        assert_eq!(sums.len(), 3);
        assert_eq!(sums[0], sums[2]);
        assert_ne!(sums[0], sums[1]);
    }
}
