//! Journal records (spec §6.3).

use std::borrow::Cow;

use crate::codec::{Reader, Writer, get_u16, get_u32};
use crate::consts::{BLOCK_SIZE, MAX_CLUSTER_SIZE};
use crate::error::FormatError;
use crate::uuid::Uuid;

/// Length of the record header (`type`, `flags`, `body_len`).
pub const RECORD_HEADER_LEN: usize = 8;
/// Record flag: a reader that does not know this record type may skip it.
pub const FLAG_OPTIONAL: u16 = 1;

/// Record type numbers.
pub mod record_type {
    /// [`Record::ChunkCreate`](super::Record::ChunkCreate).
    pub const CHUNK_CREATE: u16 = 1;
    /// [`Record::ChunkDelete`](super::Record::ChunkDelete).
    pub const CHUNK_DELETE: u16 = 2;
    /// [`Record::Fence`](super::Record::Fence).
    pub const FENCE: u16 = 3;
    /// [`Record::LayerCreate`](super::Record::LayerCreate).
    pub const LAYER_CREATE: u16 = 4;
    /// [`Record::MapUpdate`](super::Record::MapUpdate).
    pub const MAP_UPDATE: u16 = 5;
    /// [`Record::DeferredWrite`](super::Record::DeferredWrite).
    pub const DEFERRED_WRITE: u16 = 6;
    /// [`Record::Discard`](super::Record::Discard).
    pub const DISCARD: u16 = 7;
    /// Reserved for snapshot deletion (phase 3).
    pub const LAYER_MERGE: u16 = 8;
}

/// One journal record. Variable-length parts borrow from the batch buffer when decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record<'a> {
    /// Creates a chunk replica in `slot` with one empty layer.
    ChunkCreate {
        /// Chunk slot.
        slot: u32,
        /// Owning volume.
        volume_uuid: Uuid,
        /// Index of the chunk within the volume.
        chunk_index: u64,
        /// Chunk size in bytes.
        chunk_size: u64,
        /// Volume size in bytes.
        volume_size: u64,
        /// Initial fenced epoch.
        fenced_epoch: u64,
    },
    /// Deletes the chunk replica in `slot` and frees its clusters and pages.
    ChunkDelete {
        /// Chunk slot.
        slot: u32,
    },
    /// Raises the fenced epoch of every slot belonging to the volume.
    Fence {
        /// Volume to fence.
        volume_uuid: Uuid,
        /// New minimum epoch.
        epoch: u64,
    },
    /// Pushes a new, empty top layer (a snapshot of everything below).
    LayerCreate {
        /// Chunk slot.
        slot: u32,
        /// Snapshot generation of the new layer.
        snapshot_gen: u64,
    },
    /// Maps `count` clusters of the top layer to consecutive physical clusters (redirect-on-write).
    MapUpdate {
        /// Chunk slot.
        slot: u32,
        /// First chunk-relative cluster.
        first_cluster: u32,
        /// Number of clusters.
        count: u32,
        /// Checksums per cluster (`cluster_size / 4096`).
        csums_per_cluster: u32,
        /// First physical data cluster.
        phys_first: u64,
        /// Target write sequence number.
        write_seq: u64,
        /// `count × csums_per_cluster` checksums.
        csums: Cow<'a, [u32]>,
    },
    /// A small write to a cluster already mapped in the top layer, applied in place after commit.
    DeferredWrite {
        /// Chunk slot.
        slot: u32,
        /// Byte offset within the chunk (4 KiB aligned).
        offset: u32,
        /// Target write sequence number.
        write_seq: u64,
        /// One checksum per 4 KiB of `data`.
        csums: Cow<'a, [u32]>,
        /// The data, a multiple of 4 KiB within one cluster.
        data: &'a [u8],
    },
    /// Discards `count` clusters of the top layer.
    Discard {
        /// Chunk slot.
        slot: u32,
        /// First chunk-relative cluster.
        first_cluster: u32,
        /// Number of clusters.
        count: u32,
    },
    /// A record marked optional whose type this reader does not know.
    Unknown {
        /// Raw record type.
        record_type: u16,
        /// Raw body.
        body: &'a [u8],
    },
}

const fn align8(v: usize) -> usize {
    v.div_ceil(8) * 8
}

impl Record<'_> {
    /// The record's type number.
    pub fn record_type(&self) -> u16 {
        use record_type::*;
        match self {
            Record::ChunkCreate { .. } => CHUNK_CREATE,
            Record::ChunkDelete { .. } => CHUNK_DELETE,
            Record::Fence { .. } => FENCE,
            Record::LayerCreate { .. } => LAYER_CREATE,
            Record::MapUpdate { .. } => MAP_UPDATE,
            Record::DeferredWrite { .. } => DEFERRED_WRITE,
            Record::Discard { .. } => DISCARD,
            Record::Unknown { record_type, .. } => *record_type,
        }
    }

    fn flags(&self) -> u16 {
        match self {
            Record::Unknown { .. } => FLAG_OPTIONAL,
            _ => 0,
        }
    }

    fn body_len(&self) -> usize {
        match self {
            Record::ChunkCreate { .. } => 56,
            Record::ChunkDelete { .. } => 8,
            Record::Fence { .. } => 24,
            Record::LayerCreate { .. } => 16,
            Record::MapUpdate { csums, .. } => 32 + 4 * csums.len(),
            Record::DeferredWrite { csums, data, .. } => 24 + 4 * csums.len() + data.len(),
            Record::Discard { .. } => 16,
            Record::Unknown { body, .. } => body.len(),
        }
    }

    /// Bytes this record occupies in a batch payload, including header and padding.
    pub fn encoded_len(&self) -> usize {
        align8(RECORD_HEADER_LEN + self.body_len())
    }

    /// Structural checks that need no layout. Semantic checks (slot exists, cluster in range)
    /// belong to replay.
    pub fn validate(&self) -> Result<(), FormatError> {
        let inv = FormatError::invalid;
        match self {
            Record::ChunkCreate { chunk_size, .. } if *chunk_size == 0 => {
                Err(inv("chunk_size", "must be non-zero"))
            }
            Record::MapUpdate {
                first_cluster,
                count,
                csums_per_cluster: cpc,
                phys_first,
                csums,
                ..
            } => {
                let max_cpc = MAX_CLUSTER_SIZE / BLOCK_SIZE as u32;
                if *count == 0 {
                    Err(inv("count", "must be non-zero"))
                } else if !cpc.is_power_of_two() || !(4..=max_cpc).contains(cpc) {
                    Err(inv(
                        "csums_per_cluster",
                        "does not match a valid cluster size",
                    ))
                } else if csums.len() as u64 != u64::from(*count) * u64::from(*cpc) {
                    Err(inv("csums", "length must be count × csums_per_cluster"))
                } else if first_cluster.checked_add(*count).is_none()
                    || phys_first.checked_add(u64::from(*count)).is_none()
                {
                    Err(inv("count", "range overflows"))
                } else {
                    Ok(())
                }
            }
            Record::DeferredWrite {
                offset,
                csums,
                data,
                ..
            } => {
                if data.is_empty()
                    || data.len() % BLOCK_SIZE != 0
                    || data.len() > MAX_CLUSTER_SIZE as usize
                {
                    Err(inv(
                        "len",
                        "must be a non-zero multiple of 4096 no larger than a cluster",
                    ))
                } else if *offset as usize % BLOCK_SIZE != 0 {
                    Err(inv("offset", "must be 4 KiB aligned"))
                } else if csums.len() != data.len() / BLOCK_SIZE {
                    Err(inv("csums", "need one checksum per 4 KiB block"))
                } else if offset.checked_add(data.len() as u32).is_none() {
                    Err(inv("offset", "range overflows"))
                } else {
                    Ok(())
                }
            }
            Record::Discard {
                first_cluster,
                count,
                ..
            } => {
                if *count == 0 || first_cluster.checked_add(*count).is_none() {
                    Err(inv("count", "must be non-zero and not overflow"))
                } else {
                    Ok(())
                }
            }
            Record::Unknown { record_type, body } => {
                if (record_type::CHUNK_CREATE..=record_type::DISCARD).contains(record_type) {
                    Err(inv("record_type", "known type encoded as unknown"))
                } else if u32::try_from(body.len()).is_err() {
                    Err(inv("body", "too long"))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    /// Appends the encoded record. The caller must have validated it.
    pub(crate) fn encode_into(&self, out: &mut Vec<u8>) {
        let start = out.len();
        let mut w = Writer::new(out);
        w.u16(self.record_type());
        w.u16(self.flags());
        w.u32(self.body_len() as u32);
        match self {
            Record::ChunkCreate {
                slot,
                volume_uuid,
                chunk_index,
                chunk_size,
                volume_size,
                fenced_epoch,
            } => {
                w.u32(*slot);
                w.u32(0);
                w.bytes(&volume_uuid.0);
                w.u64(*chunk_index);
                w.u64(*chunk_size);
                w.u64(*volume_size);
                w.u64(*fenced_epoch);
            }
            Record::ChunkDelete { slot } => {
                w.u32(*slot);
                w.u32(0);
            }
            Record::Fence { volume_uuid, epoch } => {
                w.bytes(&volume_uuid.0);
                w.u64(*epoch);
            }
            Record::LayerCreate { slot, snapshot_gen } => {
                w.u32(*slot);
                w.u32(0);
                w.u64(*snapshot_gen);
            }
            Record::MapUpdate {
                slot,
                first_cluster,
                count,
                csums_per_cluster,
                phys_first,
                write_seq,
                csums,
            } => {
                w.u32(*slot);
                w.u32(*first_cluster);
                w.u32(*count);
                w.u32(*csums_per_cluster);
                w.u64(*phys_first);
                w.u64(*write_seq);
                w.u32s(csums);
            }
            Record::DeferredWrite {
                slot,
                offset,
                write_seq,
                csums,
                data,
            } => {
                w.u32(*slot);
                w.u32(*offset);
                w.u32(data.len() as u32);
                w.u32(0);
                w.u64(*write_seq);
                w.u32s(csums);
                w.bytes(data);
            }
            Record::Discard {
                slot,
                first_cluster,
                count,
            } => {
                w.u32(*slot);
                w.u32(*first_cluster);
                w.u32(*count);
                w.u32(0);
            }
            Record::Unknown { body, .. } => w.bytes(body),
        }
        out.resize(start + self.encoded_len(), 0);
    }
}

impl<'a> Record<'a> {
    /// Decodes one record body.
    pub fn decode(record_type: u16, flags: u16, body: &'a [u8]) -> Result<Self, FormatError> {
        use record_type::*;
        let mut r = Reader::new(body);
        let rec = match record_type {
            CHUNK_CREATE => {
                let slot = r.u32()?;
                r.u32()?;
                Record::ChunkCreate {
                    slot,
                    volume_uuid: Uuid(r.bytes16()?),
                    chunk_index: r.u64()?,
                    chunk_size: r.u64()?,
                    volume_size: r.u64()?,
                    fenced_epoch: r.u64()?,
                }
            }
            CHUNK_DELETE => {
                let slot = r.u32()?;
                r.u32()?;
                Record::ChunkDelete { slot }
            }
            FENCE => Record::Fence {
                volume_uuid: Uuid(r.bytes16()?),
                epoch: r.u64()?,
            },
            LAYER_CREATE => {
                let slot = r.u32()?;
                r.u32()?;
                Record::LayerCreate {
                    slot,
                    snapshot_gen: r.u64()?,
                }
            }
            MAP_UPDATE => {
                let (slot, first_cluster, count, csums_per_cluster) =
                    (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
                let (phys_first, write_seq) = (r.u64()?, r.u64()?);
                let n = usize::try_from(u64::from(count) * u64::from(csums_per_cluster))
                    .map_err(|_| FormatError::invalid("count", "too large"))?;
                Record::MapUpdate {
                    slot,
                    first_cluster,
                    count,
                    csums_per_cluster,
                    phys_first,
                    write_seq,
                    csums: Cow::Owned(r.u32_vec(n)?),
                }
            }
            DEFERRED_WRITE => {
                let (slot, offset, len) = (r.u32()?, r.u32()?, r.u32()? as usize);
                r.u32()?;
                let write_seq = r.u64()?;
                let csums = r.u32_vec(len / BLOCK_SIZE)?;
                Record::DeferredWrite {
                    slot,
                    offset,
                    write_seq,
                    csums: Cow::Owned(csums),
                    data: r.take(len)?,
                }
            }
            DISCARD => {
                let (slot, first_cluster, count) = (r.u32()?, r.u32()?, r.u32()?);
                r.u32()?;
                Record::Discard {
                    slot,
                    first_cluster,
                    count,
                }
            }
            _ if flags & FLAG_OPTIONAL != 0 => return Ok(Record::Unknown { record_type, body }),
            _ => return Err(FormatError::UnknownRecord(record_type)),
        };
        if !r.is_empty() {
            return Err(FormatError::invalid(
                "body_len",
                "trailing bytes in record body",
            ));
        }
        rec.validate()?;
        Ok(rec)
    }
}

/// Iterates over the records of a batch payload.
pub struct RecordIter<'a> {
    buf: &'a [u8],
    pos: usize,
    remaining: u32,
    done: bool,
}

impl<'a> RecordIter<'a> {
    /// Iterates over `record_count` records in `payload`, then checks nothing is left over.
    pub fn new(payload: &'a [u8], record_count: u32) -> Self {
        Self {
            buf: payload,
            pos: 0,
            remaining: record_count,
            done: false,
        }
    }

    fn next_record(&mut self) -> Result<Record<'a>, FormatError> {
        let rest = &self.buf[self.pos..];
        if rest.len() < RECORD_HEADER_LEN {
            return Err(FormatError::Truncated {
                need: RECORD_HEADER_LEN,
                have: rest.len(),
            });
        }
        let (ty, flags) = (get_u16(rest, 0), get_u16(rest, 2));
        let body_len = get_u32(rest, 4) as usize;
        let total = RECORD_HEADER_LEN
            .checked_add(body_len)
            .map(align8)
            .filter(|&t| t <= rest.len())
            .ok_or(FormatError::Truncated {
                need: RECORD_HEADER_LEN.saturating_add(body_len),
                have: rest.len(),
            })?;
        let end = RECORD_HEADER_LEN + body_len;
        if rest[end..total].iter().any(|&b| b != 0) {
            return Err(FormatError::invalid(
                "padding",
                "record padding must be zero",
            ));
        }
        self.pos += total;
        Record::decode(ty, flags, &rest[RECORD_HEADER_LEN..end])
    }
}

impl<'a> Iterator for RecordIter<'a> {
    type Item = Result<Record<'a>, FormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if self.remaining == 0 {
            self.done = true;
            return (self.pos != self.buf.len()).then(|| {
                Err(FormatError::invalid(
                    "payload",
                    "bytes left after the last record",
                ))
            });
        }
        let result = self.next_record();
        match result {
            Ok(_) => self.remaining -= 1,
            Err(_) => self.done = true,
        }
        Some(result)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn samples(data: &[u8]) -> Vec<Record<'_>> {
        vec![
            Record::ChunkCreate {
                slot: 3,
                volume_uuid: Uuid([7; 16]),
                chunk_index: 11,
                chunk_size: 256 << 20,
                volume_size: 1 << 40,
                fenced_epoch: 2,
            },
            Record::ChunkDelete { slot: 9 },
            Record::Fence {
                volume_uuid: Uuid([1; 16]),
                epoch: 5,
            },
            Record::LayerCreate {
                slot: 3,
                snapshot_gen: 1,
            },
            Record::MapUpdate {
                slot: 3,
                first_cluster: 10,
                count: 2,
                csums_per_cluster: 16,
                phys_first: 1234,
                write_seq: 77,
                csums: Cow::Owned((0..32).collect()),
            },
            Record::DeferredWrite {
                slot: 3,
                offset: 8192,
                write_seq: 78,
                csums: Cow::Owned(crate::checksum::block_checksums(data)),
                data,
            },
            Record::Discard {
                slot: 3,
                first_cluster: 0,
                count: 4,
            },
            Record::Unknown {
                record_type: 900,
                body: &[1, 2, 3],
            },
        ]
    }

    #[test]
    fn every_record_roundtrips() {
        let data = vec![0x5A; 2 * BLOCK_SIZE];
        let recs = samples(&data);
        let mut buf = Vec::new();
        for r in &recs {
            r.validate().unwrap();
            r.encode_into(&mut buf);
        }
        assert_eq!(
            buf.len(),
            recs.iter().map(Record::encoded_len).sum::<usize>()
        );
        assert_eq!(buf.len() % 8, 0);
        let back: Vec<_> = RecordIter::new(&buf, recs.len() as u32)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(back, recs);
    }

    #[test]
    fn unknown_mandatory_record_is_fatal() {
        let mut buf = Vec::new();
        Record::Unknown {
            record_type: 900,
            body: &[],
        }
        .encode_into(&mut buf);
        buf[2] = 0; // clear the optional flag
        let err = RecordIter::new(&buf, 1).next().unwrap().unwrap_err();
        assert_eq!(err, FormatError::UnknownRecord(900));
    }

    #[test]
    fn count_mismatch_and_trailing_bytes_are_errors() {
        let mut buf = Vec::new();
        Record::ChunkDelete { slot: 1 }.encode_into(&mut buf);
        assert!(RecordIter::new(&buf, 2).any(|r| r.is_err()));
        buf.extend_from_slice(&[0; 8]);
        assert!(RecordIter::new(&buf, 1).any(|r| r.is_err()));
    }

    #[test]
    fn invalid_records_are_rejected() {
        let data = [0u8; 100];
        let bad = [
            Record::MapUpdate {
                slot: 0,
                first_cluster: 0,
                count: 1,
                csums_per_cluster: 16,
                phys_first: 0,
                write_seq: 0,
                csums: Cow::Owned(vec![0; 15]),
            },
            Record::MapUpdate {
                slot: 0,
                first_cluster: u32::MAX,
                count: 2,
                csums_per_cluster: 4,
                phys_first: 0,
                write_seq: 0,
                csums: Cow::Owned(vec![0; 8]),
            },
            Record::MapUpdate {
                slot: 0,
                first_cluster: 0,
                count: 1,
                csums_per_cluster: 3,
                phys_first: 0,
                write_seq: 0,
                csums: Cow::Owned(vec![0; 3]),
            },
            Record::DeferredWrite {
                slot: 0,
                offset: 0,
                write_seq: 0,
                csums: Cow::Owned(vec![0]),
                data: &data,
            },
            Record::DeferredWrite {
                slot: 0,
                offset: 100,
                write_seq: 0,
                csums: Cow::Owned(vec![]),
                data: &[],
            },
            Record::Discard {
                slot: 0,
                first_cluster: 0,
                count: 0,
            },
            Record::ChunkCreate {
                slot: 0,
                volume_uuid: Uuid::NIL,
                chunk_index: 0,
                chunk_size: 0,
                volume_size: 0,
                fenced_epoch: 0,
            },
            Record::Unknown {
                record_type: record_type::FENCE,
                body: &[],
            },
        ];
        for r in bad {
            assert!(r.validate().is_err(), "{r:?}");
        }
    }
}
