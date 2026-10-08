//! CRC-32C helpers. All checksums in the format are CRC-32C (Castagnoli).

use crate::consts::BLOCK_SIZE;

/// CRC-32C of `data`. Hardware accelerated where the CPU supports it.
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c::crc32c(data)
}

/// One CRC-32C per 4 KiB block of `data`, as stored in map entries and write records.
///
/// # Panics
///
/// If `data.len()` is not a multiple of 4096.
pub fn block_checksums(data: &[u8]) -> Vec<u32> {
    assert!(
        data.len() % BLOCK_SIZE == 0,
        "data length must be a multiple of the block size"
    );
    data.chunks_exact(BLOCK_SIZE).map(crc32c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
