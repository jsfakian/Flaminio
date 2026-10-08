use flaminio_format::FormatError;
use flaminio_format::journal::ScanError;

use crate::state::ChunkKey;

/// Errors returned by the replica store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The device failed. The store is poisoned afterwards.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// On-disk bytes failed to decode.
    #[error("on-disk format error: {0}")]
    Format(#[from] FormatError),
    /// Journal replay hit a corrupt batch.
    #[error("journal replay failed: {0}")]
    Scan(#[from] ScanError),
    /// Metadata contradicts itself. Opening the disk is not safe.
    #[error("metadata is inconsistent: {0}")]
    Corrupt(String),
    /// An earlier failure left in-memory state unknown. Reopen the store to recover.
    #[error("store is unusable after an earlier failure; reopen it")]
    Poisoned,
    /// The disk uses read-only-compatible features this version does not know.
    #[error("disk uses features this version can only read")]
    ReadOnly,
    /// The chunk does not exist on this disk.
    #[error("no such chunk {0}")]
    NoSuchChunk(ChunkKey),
    /// The chunk already exists on this disk.
    #[error("chunk {0} already exists")]
    ChunkExists(ChunkKey),
    /// The request carries an epoch older than the chunk's fenced epoch.
    #[error("stale epoch {got}: fenced at {fenced}")]
    StaleEpoch {
        /// Current fenced epoch.
        fenced: u64,
        /// Epoch in the request.
        got: u64,
    },
    /// No room left for data, metadata pages, chunk slots or layers.
    #[error("out of space: {0}")]
    OutOfSpace(&'static str),
    /// The request is malformed (alignment, bounds, size).
    #[error("invalid request: {0}")]
    Invalid(String),
    /// Stored data does not match its checksum. The caller should read another replica.
    #[error("checksum mismatch in chunk {key} at offset {offset}")]
    Checksum {
        /// Chunk.
        key: ChunkKey,
        /// Byte offset of the bad 4 KiB block within the chunk.
        offset: u64,
    },
}

impl StoreError {
    pub(crate) fn corrupt(msg: impl Into<String>) -> Self {
        Self::Corrupt(msg.into())
    }

    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }
}
