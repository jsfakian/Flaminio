//! The supported volume geometry and conservative full-metadata-reply budget.

pub const BLOCK_BYTES: u64 = 4096;
pub const MIN_CHUNK_BYTES: u64 = 1 << 20;
pub const MAX_CHUNK_BYTES: u64 = 1 << 30;
pub const MAX_CHUNKS: u64 = 1 << 16;
pub const METADATA_BYTES: u64 = 4 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub chunks: u32,
    pub blocks_per_chunk: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeometryError(pub &'static str);

impl std::fmt::Display for GeometryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for GeometryError {}

/// Checks the scalar envelope carried by a replica Hello, which has no replication factor.
pub fn validate_scalar(size: u64, chunk_size: u64) -> Result<Geometry, GeometryError> {
    if !(MIN_CHUNK_BYTES..=MAX_CHUNK_BYTES).contains(&chunk_size) || !chunk_size.is_power_of_two() {
        return Err(GeometryError(
            "chunk size must be a power of two from 1 MiB through 1 GiB",
        ));
    }
    if size == 0 || !size.is_multiple_of(chunk_size) {
        return Err(GeometryError(
            "volume size must be a positive whole number of chunks",
        ));
    }
    let chunks = size / chunk_size;
    if chunks > MAX_CHUNKS {
        return Err(GeometryError("volume exceeds the 65,536 chunk limit"));
    }
    Ok(Geometry {
        chunks: u32::try_from(chunks).map_err(|_| GeometryError("chunk count overflow"))?,
        blocks_per_chunk: u32::try_from(chunk_size / BLOCK_BYTES)
            .map_err(|_| GeometryError("block count overflow"))?,
    })
}

/// Worst encoded metadata size, including temporary membership growth during a move.
pub fn metadata_bound(chunks: u64, replicas: u32) -> Result<u64, GeometryError> {
    if !(1..=5).contains(&replicas) {
        return Err(GeometryError("replication factor must be from 1 through 5"));
    }
    chunks
        .checked_mul(288 + 264 * (u64::from(replicas) + 1))
        .and_then(|bytes| bytes.checked_add(2048))
        .ok_or(GeometryError("metadata size overflow"))
}

/// Admission shared by metadata and targets; session-only validation uses `validate_scalar`.
pub fn validate(
    size: u64,
    chunk_size: u64,
    replicas: u32,
    min_isr: u32,
) -> Result<Geometry, GeometryError> {
    let geometry = validate_scalar(size, chunk_size)?;
    let bytes = metadata_bound(u64::from(geometry.chunks), replicas)?;
    if min_isr == 0 || min_isr > replicas {
        return Err(GeometryError(
            "minimum ISR must be from 1 through the replication factor",
        ));
    }
    if bytes > METADATA_BYTES {
        return Err(GeometryError(
            "volume exceeds the 4 MiB metadata reply budget",
        ));
    }
    Ok(geometry)
}
