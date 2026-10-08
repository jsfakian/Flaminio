//! Format-wide constants. See the spec §1–§3.

/// Size of a block: the unit of alignment for superblocks, metadata pages and journal batches.
pub const BLOCK_SIZE: usize = 4096;
/// [`BLOCK_SIZE`] as a `u64`, for offset arithmetic.
pub const BLOCK_SIZE_U64: u64 = BLOCK_SIZE as u64;

/// One mebibyte.
pub const MIB: u64 = 1 << 20;
/// One gibibyte.
pub const GIB: u64 = 1 << 30;

/// Major format version written by this implementation. A different major is not readable.
pub const VERSION_MAJOR: u16 = 1;
/// Minor format version written by this implementation. Minor versions are backward compatible.
pub const VERSION_MINOR: u16 = 0;

/// Superblock magic.
pub const SUPERBLOCK_MAGIC: [u8; 8] = *b"FLAMINIO";
/// Metadata page magic.
pub const PAGE_MAGIC: [u8; 4] = *b"FLPG";
/// Journal batch magic.
pub const BATCH_MAGIC: [u8; 4] = *b"FLJB";

/// Smallest allowed cluster size.
pub const MIN_CLUSTER_SIZE: u32 = 16 * 1024;
/// Largest allowed cluster size.
pub const MAX_CLUSTER_SIZE: u32 = 256 * 1024;
/// Cluster size used when none is specified.
pub const DEFAULT_CLUSTER_SIZE: u32 = 64 * 1024;

/// Maximum number of layers (snapshots + 1) per chunk replica.
pub const MAX_LAYERS: u32 = 64;

/// Smallest device that can be formatted.
pub const MIN_DEVICE_SIZE: u64 = 256 * MIB;
/// Byte offset of the journal region.
pub const JOURNAL_OFFSET: u64 = MIB;
/// Smallest journal accepted when the size is given explicitly.
pub const MIN_JOURNAL_SIZE: u64 = 4 * MIB;
/// Lower clamp of the default journal size.
pub const DEFAULT_JOURNAL_MIN: u64 = 16 * MIB;
/// Upper clamp of the default journal size.
pub const DEFAULT_JOURNAL_MAX: u64 = GIB;

/// Smallest chunk size a volume may use. Drives chunk-slot sizing.
pub const MIN_CHUNK_SIZE: u64 = 64 * MIB;
/// Largest chunk size a volume may use.
pub const MAX_CHUNK_SIZE: u64 = GIB;
/// Chunk-slot sizing factor: allows 2× thin overcommit of minimum-size chunks.
pub const CHUNK_SLOT_OVERCOMMIT: u64 = 2;
/// Minimum number of chunk slots on any disk.
pub const MIN_CHUNK_SLOTS: u64 = 64;
/// Page-pool sizing factor for dense maps, leaving room for sparse snapshot layers.
pub const MAP_SPARSITY_FACTOR: u64 = 2;
/// Page tables budgeted per chunk slot in the page pool.
pub const PAGE_TABLES_PER_SLOT: u64 = 2;

/// Bits stored in one bitmap page.
pub const BITS_PER_BITMAP_PAGE: u64 = 32_416;
/// Map-page references stored in one page table.
pub const PAGE_TABLE_ENTRIES: usize = 506;

/// `features_compat` bits this implementation understands.
pub const KNOWN_FEATURES_COMPAT: u64 = 0;
/// `features_ro_compat` bits this implementation understands.
pub const KNOWN_FEATURES_RO_COMPAT: u64 = 0;
/// `features_incompat` bits this implementation understands.
pub const KNOWN_FEATURES_INCOMPAT: u64 = 0;
