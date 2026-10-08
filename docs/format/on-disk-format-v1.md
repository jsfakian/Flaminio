# Flaminio On-Disk Format, Version 1.0

> **Status:** Draft, phase 0. The reference implementation is the
> `flaminio-format` crate (`dataplane/crates/flaminio-format`). If this document and the code
> disagree, that is a bug: fix one so they match.
>
> Background and rationale: [ARCHITECTURE.md §6](../ARCHITECTURE.md#6-on-disk-format).

## 1. Conventions

- All integers are **little-endian**. Offsets and sizes are in **bytes** unless stated otherwise.
- **Block** = 4096 bytes. Every metadata page, every journal batch and every superblock is one or
  more whole blocks, and is aligned to 4096. The device's logical block size must divide 4096.
- **Cluster** = the data allocation unit, chosen at format time: a power of two from 16 KiB to
  256 KiB (default 64 KiB).
- **Checksum** = CRC-32C (Castagnoli). Data checksums cover each 4 KiB block of data separately.
- **UUID** = 16 raw bytes.
- `reserved` fields are written as zero and ignored on read.
- **Format nonce:** a random 64-bit value chosen at format time and stored in the superblock.
  Every metadata page and journal batch carries it. Anything left over from an earlier format of
  the same device has a different nonce and is rejected, even if its checksum is valid. That is
  why formatting never needs to zero the device.

## 2. Device layout

```
offset 0         1 MiB                                                          device_size
│ SB-A │ reserved │ Journal │ Metadata area                      │ Data area        │ … │ SB-B │
│ 4K   │          │         │ ckpt │ data bmp │ pool bmp │ chunk slots │ page pool │ clusters │   │ 4K  │
```

| Region | Offset | Size |
|---|---|---|
| Superblock A | 0 | 4096 |
| Reserved | 4096 | up to 1 MiB |
| Journal | 1 MiB | `journal_size`, a multiple of 1 MiB. Default `clamp(device_size / 512, 16 MiB, 1 GiB)` |
| Metadata area | `meta_offset` = 1 MiB + `journal_size` | `meta_size`, a multiple of 4096 |
| Data area | `data_offset`, aligned to `max(cluster_size, 1 MiB)` | `data_clusters × cluster_size` |
| Superblock B | `device_size − 4096` | 4096 |

`device_size` is the device size at format time, rounded down to 4096. The data area ends at or
before superblock B. If the device grows later, the format does not change. Growing into the new
space is a future feature.

### 2.1 Metadata area layout

Sub-regions, in order and contiguous starting at `meta_offset`:

| Sub-region | Size (pages) | Content |
|---|---|---|
| Checkpoint header | 2 (one pair) | §5.1 |
| Data bitmap | `2 × data_bitmap_pairs` | One allocation bit per data cluster (§5.2) |
| Pool bitmap | `2 × pool_bitmap_pairs` | One allocation bit per page-pool pair (§5.2) |
| Chunk slots | `2 × chunk_slots` | One page pair per chunk-replica slot (§5.3) |
| Page pool | `2 × pool_pairs` | Dynamically allocated pairs: page tables (§5.4) and map pages (§5.5) |

Sizing at format time (`N` = data clusters, `E` = map entries per page, §5.5):

| Quantity | Formula |
|---|---|
| `chunk_slots` | `max(64, ceil(N × cluster_size / 64 MiB) × 2)`: enough for 64 MiB chunks at 2× thin overcommit |
| `pool_pairs` | `ceil(N / E) × 2 + chunk_slots × 2`: dense maps × 2 for snapshot sparsity, plus page tables |
| `data_bitmap_pairs` | `ceil(N / 32416)` |
| `pool_bitmap_pairs` | `ceil(pool_pairs / 32416)` |

The formatter solves for the largest `N` that fits. With default parameters, metadata takes about
0.6% of the device.

All of these values are **stored in the superblock**. Readers never recompute the layout.

## 3. Superblock

There are two identical copies, A and B. The superblock is written at format time and changes
only when features change, at which point `sb_generation` is incremented. A reader takes every
copy that is valid (magic, CRC, version supported). If both are valid it uses the one with the
higher `sb_generation`, and if the generations are equal it uses A.

| Offset | Size | Field | Notes |
|---|---|---|---|
| 0 | 8 | `magic` | ASCII `FLAMINIO` |
| 8 | 2 | `version_major` | 1. A reader rejects a major version it does not know |
| 10 | 2 | `version_minor` | 0. Minor versions are backward compatible |
| 12 | 4 | `sb_size` | 4096 |
| 16 | 8 | `features_compat` | Unknown bits: ignore |
| 24 | 8 | `features_ro_compat` | Unknown bits: open read-only |
| 32 | 8 | `features_incompat` | Unknown bits: refuse to open |
| 40 | 16 | `disk_uuid` | |
| 56 | 16 | `cluster_uuid` | The Flaminio cluster this disk belongs to |
| 72 | 8 | `format_nonce` | §1 |
| 80 | 8 | `created_unix_secs` | |
| 88 | 8 | `device_size` | |
| 96 | 4 | `block_size` | 4096 |
| 100 | 4 | `cluster_size` | |
| 104 | 8 | `journal_offset` | |
| 112 | 8 | `journal_size` | |
| 120 | 8 | `meta_offset` | |
| 128 | 8 | `meta_size` | |
| 136 | 8 | `data_offset` | |
| 144 | 8 | `data_clusters` | |
| 152 | 8 | `checkpoint_offset` | |
| 160 | 8 | `data_bitmap_offset` | |
| 168 | 8 | `data_bitmap_pairs` | |
| 176 | 8 | `pool_bitmap_offset` | |
| 184 | 8 | `pool_bitmap_pairs` | |
| 192 | 8 | `chunk_slots_offset` | |
| 200 | 8 | `chunk_slots` | |
| 208 | 8 | `pool_offset` | |
| 216 | 8 | `pool_pairs` | |
| 224 | 4 | `max_layers` | 64 |
| 228 | 4 | reserved | |
| 232 | 8 | `sb_generation` | Starts at 1 |
| 240 | 64 | `label` | UTF-8, zero-padded, optional |
| 304 | 3788 | reserved | |
| 4092 | 4 | `crc` | CRC-32C of bytes 0..4092 |

**Feature flags in 1.0:** none are defined. Reserved for later: incompat bit 0 = external journal
device, incompat bit 1 = authenticated (MAC) checksums.

A reader must also verify that every region is in bounds, aligned and non-overlapping before it
trusts the superblock.

## 4. Pages and page pairs

All metadata is stored in 4096-byte **pages** with this header:

| Offset | Size | Field | Notes |
|---|---|---|---|
| 0 | 4 | `magic` | ASCII `FLPG` |
| 4 | 2 | `page_type` | 1 checkpoint · 2 data bitmap · 3 pool bitmap · 4 chunk header · 5 page table · 6 map · 7 volume descriptor (reserved) |
| 6 | 2 | `page_version` | 1 |
| 8 | 8 | `format_nonce` | Must equal the superblock's |
| 16 | 8 | `seq` | The checkpoint sequence number that wrote this page |
| 24 | 8 | `location` | This page's own byte offset ÷ 4096. Catches misdirected writes |
| 32 | 4060 | payload | Type-specific |
| 4092 | 4 | `crc` | CRC-32C of bytes 0..4092 |

Every logical metadata page is stored as a **pair** of physical pages (slot 0 at offset `o`, slot
1 at `o + 4096`). Updates are copy-on-write between the two slots, so a torn page write can never
destroy the last good version:

- **Read:** a slot is *valid* if magic, CRC, type, nonce and location all match. When recovering
  checkpoint `K`, use the valid slot with the **highest `seq` that is ≤ K**. A slot with
  `seq > K` was written by a checkpoint that never completed, so ignore it. If neither slot
  qualifies, the logical page is *absent*.
- **Write:** always write the slot that is *not* the current one.

Why this is correct: suppose a page was last completely written at checkpoint `j ≤ K`. Any change
after `K` is still in the journal, because the journal start only moves after a checkpoint
completes. Any change between `j` and `K` would have been written at a checkpoint ≤ K, which
contradicts `j` being the last one. So the chosen slot is exactly the state at `K`.

## 5. Metadata page types

All page payloads start at page offset 32. The offsets below are relative to the start of the
payload.

### 5.1 Checkpoint header (type 1)

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | `journal_start_offset`: where replay starts, relative to the journal region |
| 8 | 8 | `journal_start_seq`: the batch sequence number expected at that offset |
| 16 | 8 | `data_free_clusters` (informational) |
| 24 | 8 | `pool_free_pairs` (informational) |
| 32 | 8 | `created_unix_secs` |

The page header's `seq` **is** the checkpoint sequence number. Recovery picks the valid
checkpoint slot with the highest `seq`. A freshly formatted disk has checkpoint 1, with the
journal starting at offset 0 and sequence 1.

**Checkpoint protocol.** (1) Write every dirty page to its non-current slot with `seq = K+1`.
(2) Flush. (3) Write the checkpoint header with `seq = K+1` and the new journal start. (4) Flush.
Only now may journal space before the new start be reused.

### 5.2 Bitmap pages (types 2 and 3)

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | `first_index`: the index of the first bit on this page |
| 8 | 4052 | Bits. LSB-first within each byte. 1 = allocated. 32 416 bits per page |

Bits past the end of the area they describe are ignored.

### 5.3 Chunk header (type 4)

One pair per slot, at `chunk_slots_offset + slot × 8192`. A slot whose pair is absent, or whose
`state` is 0, is free.

| Offset | Size | Field | Notes |
|---|---|---|---|
| 0 | 1 | `state` | 0 free · 1 active · 2 rebuilding · 3 deleting |
| 1 | 7 | reserved | |
| 8 | 16 | `volume_uuid` | |
| 24 | 8 | `chunk_index` | The chunk's index within the volume |
| 32 | 8 | `chunk_size` | Must be a multiple of `cluster_size`, and the map must fit a page table (§5.4) |
| 40 | 8 | `volume_size` | For self-description and metadata recovery |
| 48 | 8 | `fenced_epoch` | Requests with a lower epoch are rejected |
| 56 | 8 | `last_write_seq` | Highest applied target write sequence number |
| 64 | 4 | `layer_count` | 1..`max_layers`. The last layer is the writable top layer |
| 68 | 4 | reserved | |
| 72 | 16 × 64 | `layers[64]` | Each: `snapshot_gen u64`, `page_table_ref u64` (pool pair index + 1, 0 = none yet) |

### 5.4 Page table (type 5)

One per (chunk slot, layer), allocated from the page pool. It maps map-page index → pool pair.

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | `slot` (owner check) |
| 4 | 4 | `layer_index` (owner check) |
| 8 | 8 × 506 | `map_page_ref[506]`: pool pair index + 1, 0 = no map page (every entry unmapped) |

So a chunk can have at most `506 × E` clusters. With 64 KiB clusters that is about 1.7 GiB,
which covers the 1 GiB maximum chunk size.

### 5.5 Map page (type 6)

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | `slot` (owner check) |
| 4 | 4 | `first_cluster`: the chunk-relative index of entry 0 |
| 8 | `E × entry_size` | Entries |

Each entry is `entry_size = 8 + 4 × (cluster_size / 4096)` bytes:

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | `phys`: 0 = **unmapped** (reads fall through to the layer below, or zeros at the bottom) · `u64::MAX` = **zero** (discarded, does not fall through) · otherwise data-cluster index + 1 |
| 8 | 4 × k | CRC-32C of each 4 KiB block in the cluster |

`E = floor(4052 / entry_size)`:

| Cluster size | Entry size | E | Max chunk |
|---|---|---|---|
| 16 KiB | 24 | 168 | 1.3 GiB |
| 64 KiB | 72 | 56 | 1.7 GiB |
| 256 KiB | 264 | 15 | 1.8 GiB |

## 6. Journal

The journal is a circular redo log of **batches** (group commits). Each batch is a whole number
of blocks and **never wraps** past the end of the region. If a batch does not fit in the space
left before the end, the writer places it at offset 0.

### 6.1 Batch header (the first 64 bytes of the batch)

| Offset | Size | Field | Notes |
|---|---|---|---|
| 0 | 4 | `magic` | ASCII `FLJB` |
| 4 | 2 | `version` | 1 |
| 6 | 2 | `flags` | 0 |
| 8 | 8 | `format_nonce` | |
| 16 | 8 | `batch_seq` | Consecutive: each batch is exactly the previous plus 1 |
| 24 | 8 | `offset` | This batch's own offset within the journal |
| 32 | 4 | `total_len` | Multiple of 4096, including the header and padding |
| 36 | 4 | `payload_len` | |
| 40 | 4 | `record_count` | |
| 44 | 4 | `payload_crc` | CRC-32C of the payload |
| 48 | 12 | reserved | |
| 60 | 4 | `header_crc` | CRC-32C of bytes 0..60 |

The payload starts at byte 64 and is followed by zero padding up to `total_len`.

### 6.2 Replay

Start at the checkpoint's (`journal_start_offset`, `journal_start_seq`). Then repeat:

1. Try the header at the expected offset. If the batch it describes would not fit there, or the
   header is not valid with `batch_seq == expected` and `offset == position`, try offset 0.
2. A batch is accepted only if its header CRC, nonce, sequence number, offset, length bounds,
   payload CRC and record count are all valid.
3. The first offset where no batch is accepted is the **end of the log**. A torn batch (a crash
   mid-write) ends the log, and that is correct, because no write in it was acknowledged.

Sequence numbers are unique and consecutive, so a valid-looking batch from an earlier lap of the
circular log can never be mistaken for the next one.

**Write ordering (one flush per batch).** A batch may be written together with the data-area
writes it depends on (the redirect-on-write clusters of its `MapUpdate` records), followed by a
single flush. The batch is committed when that flush completes. The next batch may not be
written before then.

That order makes recovery simple. Only the **last** batch of the log can be missing data,
because every later batch was written after the previous flush had completed. Replay therefore:

1. Applies every batch except the last one, unconditionally.
2. Reads the clusters named by the last batch's `MapUpdate` records and checks them against the
   checksums the records carry.
3. If they all match, applies it. If any block mismatches, rejects it: its flush never
   completed, so nothing in it was acknowledged. The log then ends at that batch.
4. **Zeroes the rejected batch's first block, and flushes, before anything else is written.** A
   rejected batch still has a valid CRC. If the next batch, which reuses its sequence number,
   had to wrap to offset 0, a later replay would find the stale batch first and lose the new
   one. (`tests/recovery_edge_cases.rs` reproduces exactly that.)

A cluster that happened to contain the right bytes already passes the check, and that is
correct, because the contents are what the record describes.

**Reuse of freed space.** A data cluster or page-pool pair freed by a record must not be reused
until a checkpoint that includes the record has **completed**. Being durable in the journal is
not enough. Replay redoes `DeferredWrite` records in place, so if a freed cluster were reused
right away (say, for a redirect-on-write), replaying an older deferred write to it would
overwrite the new data. A freed page pair may still be the current version of a page that the
last checkpoint references. Once a checkpoint completes, the journal start has moved past every
record that could write to the space. The crash-consistency tests in `flaminio-store` fail if
this rule is broken.

### 6.3 Records

Each record has an 8-byte header followed by a body. The whole record is padded with zeros to a
multiple of 8 bytes.

| Offset | Size | Field |
|---|---|---|
| 0 | 2 | `type` |
| 2 | 2 | `flags`: bit 0 = *optional* (a reader that does not know the type may skip it) |
| 4 | 4 | `body_len` (excluding padding) |

An unknown record type without the optional flag is a fatal error (the disk is too new for this
reader).

| Type | Record | Body |
|---|---|---|
| 1 | **ChunkCreate** | `slot u32`, reserved u32, `volume_uuid [16]`, `chunk_index u64`, `chunk_size u64`, `volume_size u64`, `fenced_epoch u64` (56 B). Creates a chunk with one empty layer (`snapshot_gen` 0) |
| 2 | **ChunkDelete** | `slot u32`, reserved u32. Frees the chunk's clusters and pages |
| 3 | **Fence** | `volume_uuid [16]`, `epoch u64`. Raises `fenced_epoch` on every slot of the volume |
| 4 | **LayerCreate** | `slot u32`, reserved u32, `snapshot_gen u64`. Pushes a new empty top layer |
| 5 | **MapUpdate** | `slot u32`, `first_cluster u32`, `count u32`, `csums_per_cluster u32`, `phys_first u64` (data-cluster index), `write_seq u64`, `csums[count × csums_per_cluster] u32`. Maps `count` consecutive clusters of the top layer to consecutive physical clusters (redirect-on-write). Any physical clusters those entries *in the top layer* pointed to before are freed |
| 6 | **DeferredWrite** | `slot u32`, `offset u32` (bytes in the chunk, 4 KiB aligned), `len u32` (multiple of 4096, within one cluster), reserved u32, `write_seq u64`, `csums[len / 4096] u32`, `data[len]`. Small write to a cluster already mapped in the top layer. The data is applied in place after commit, and again on replay |
| 7 | **Discard** | `slot u32`, `first_cluster u32`, `count u32`, reserved u32. Top-layer entries become *zero* (if lower layers exist) or *unmapped* (single layer), and their physical clusters are freed |
| 8 | *LayerMerge* | Reserved (snapshot deletion, phase 3) |

A physical cluster is referenced by **exactly one** map entry. Snapshots never share entries,
because lower layers are read through, not copied. That is why the freeing rules above need no
reference counts. Linked clones (phase 3) will add a cross-slot reference type.

## 7. Recovery summary

1. Read SB-A and SB-B, and select one (§3). Check the feature flags.
2. Read the checkpoint pair, and take the highest valid `seq` = `K`.
3. Load the bitmaps and the chunk headers (and, lazily, the page tables and map pages), always
   choosing slots with `seq ≤ K`.
4. Replay the journal from the checkpoint's start until the end of the log (§6.2). Apply each
   record to the in-memory state, including redoing `DeferredWrite` data in place.
5. Write a new checkpoint `K+1`.

## 8. Known costs and limits (v1)

- A single small write with nothing to batch it with costs one 8 KiB journal batch: a 64-byte
  header plus the record, padded to whole blocks. Group commit amortizes this under load.
- After a snapshot, the first small write to a cluster pays a full-cluster copy-on-write, because
  `DeferredWrite` requires a cluster that is mapped in the top layer.
- Metadata space is pre-sized (§2.1). Heavy snapshot use with sparse writes can exhaust the page
  pool before data space runs out, in which case new layers are refused. This is reported as a
  capacity condition.
- At most 64 layers (snapshots + 1) per chunk replica.
