//! Journal batches and replay scanning (spec §6).

use std::io;

use crate::checksum::crc32c;
use crate::codec::{align_up, get_u16, get_u32, get_u64, put_u16, put_u32, put_u64};
use crate::consts::{BATCH_MAGIC, BLOCK_SIZE, BLOCK_SIZE_U64};
use crate::error::FormatError;
use crate::io::ReadAt;
use crate::layout::Layout;
use crate::record::{Record, RecordIter};

/// Length of the batch header.
pub const BATCH_HEADER_LEN: usize = 64;
/// Batch version written by this implementation.
pub const BATCH_VERSION: u16 = 1;
const HEADER_CRC_OFFSET: usize = 60;

/// A validated batch header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchHeader {
    /// Consecutive batch sequence number.
    pub batch_seq: u64,
    /// The batch's own offset within the journal region.
    pub offset: u64,
    /// Total length including header and padding (a multiple of 4096).
    pub total_len: u32,
    /// Length of the record payload.
    pub payload_len: u32,
    /// Number of records in the payload.
    pub record_count: u32,
    /// CRC-32C of the payload.
    pub payload_crc: u32,
}

impl BatchHeader {
    /// Decodes and checks magic, header CRC, version and nonce. Bounds against the journal are
    /// the scanner's job.
    pub fn decode(buf: &[u8], nonce: u64) -> Result<Self, FormatError> {
        if buf.len() < BATCH_HEADER_LEN {
            return Err(FormatError::Truncated {
                need: BATCH_HEADER_LEN,
                have: buf.len(),
            });
        }
        if buf[0..4] != BATCH_MAGIC {
            return Err(FormatError::BadMagic);
        }
        if crc32c(&buf[..HEADER_CRC_OFFSET]) != get_u32(buf, HEADER_CRC_OFFSET) {
            return Err(FormatError::Checksum);
        }
        let version = get_u16(buf, 4);
        if version != BATCH_VERSION {
            return Err(FormatError::UnsupportedVersion {
                major: version,
                minor: 0,
            });
        }
        if get_u64(buf, 8) != nonce {
            return Err(FormatError::NonceMismatch);
        }
        Ok(Self {
            batch_seq: get_u64(buf, 16),
            offset: get_u64(buf, 24),
            total_len: get_u32(buf, 32),
            payload_len: get_u32(buf, 36),
            record_count: get_u32(buf, 40),
            payload_crc: get_u32(buf, 44),
        })
    }
}

/// Accumulates records into one batch (a group commit).
pub struct BatchBuilder {
    buf: Vec<u8>,
    records: u32,
    max_total_len: usize,
}

impl BatchBuilder {
    /// A builder whose finished batch will be at most `max_total_len` bytes.
    ///
    /// # Panics
    ///
    /// If `max_total_len` is not a non-zero multiple of 4096 that fits in a `u32`.
    pub fn new(max_total_len: usize) -> Self {
        assert!(
            max_total_len >= BLOCK_SIZE
                && max_total_len.is_multiple_of(BLOCK_SIZE)
                && u32::try_from(max_total_len).is_ok(),
            "batch size must be a non-zero multiple of 4096"
        );
        Self {
            buf: vec![0; BATCH_HEADER_LEN],
            records: 0,
            max_total_len,
        }
    }

    /// Adds a record. Fails with [`FormatError::BatchFull`] if it does not fit, leaving the
    /// builder unchanged.
    pub fn push(&mut self, record: &Record<'_>) -> Result<(), FormatError> {
        record.validate()?;
        let after = self.buf.len() + record.encoded_len();
        if align_up(after as u64, BLOCK_SIZE_U64) > self.max_total_len as u64 {
            return Err(FormatError::BatchFull);
        }
        record.encode_into(&mut self.buf);
        self.records += 1;
        Ok(())
    }

    /// Whether `extra` more encoded bytes (see [`Record::encoded_len`]) still fit.
    pub fn would_fit(&self, extra: usize) -> bool {
        align_up((self.buf.len() + extra) as u64, BLOCK_SIZE_U64) <= self.max_total_len as u64
    }

    /// Records added so far.
    pub fn record_count(&self) -> u32 {
        self.records
    }

    /// Length of the finished batch.
    pub fn total_len(&self) -> u64 {
        align_up(self.buf.len() as u64, BLOCK_SIZE_U64)
    }

    /// Seals the batch for position `offset` with sequence number `batch_seq`.
    pub fn finish(mut self, nonce: u64, batch_seq: u64, offset: u64) -> Vec<u8> {
        let payload_len = self.buf.len() - BATCH_HEADER_LEN;
        let total_len = self.total_len() as usize;
        let payload_crc = crc32c(&self.buf[BATCH_HEADER_LEN..]);
        self.buf.resize(total_len, 0);
        let h = &mut self.buf[..BATCH_HEADER_LEN];
        h[0..4].copy_from_slice(&BATCH_MAGIC);
        put_u16(h, 4, BATCH_VERSION);
        put_u16(h, 6, 0);
        put_u64(h, 8, nonce);
        put_u64(h, 16, batch_seq);
        put_u64(h, 24, offset);
        put_u32(h, 32, total_len as u32);
        put_u32(h, 36, payload_len as u32);
        put_u32(h, 40, self.records);
        put_u32(h, 44, payload_crc);
        let crc = crc32c(&h[..HEADER_CRC_OFFSET]);
        put_u32(h, HEADER_CRC_OFFSET, crc);
        self.buf
    }
}

/// Where the writer must place a batch of `total_len` bytes when the log ends at `next_offset`
/// (spec §6: batches never wrap). `None` if the batch is larger than the whole journal.
///
/// This only decides placement. Not overwriting batches that are still needed for replay is the
/// writer's job.
pub fn place_batch(journal_size: u64, next_offset: u64, total_len: u64) -> Option<u64> {
    if total_len > journal_size {
        None
    } else if next_offset
        .checked_add(total_len)
        .is_some_and(|end| end <= journal_size)
    {
        Some(next_offset)
    } else {
        Some(0)
    }
}

/// A batch read back from the journal.
#[derive(Debug)]
pub struct Batch {
    header: BatchHeader,
    buf: Vec<u8>,
}

impl Batch {
    /// A batch built by [`BatchBuilder::finish`], taken back from memory: for a writer that
    /// applies a batch once it is durable, from the same bytes replay would apply.
    pub fn from_bytes(buf: Vec<u8>, nonce: u64) -> Result<Self, FormatError> {
        let header = BatchHeader::decode(&buf, nonce)?;
        if header.total_len as usize != buf.len()
            || header.payload_len as usize > buf.len() - BATCH_HEADER_LEN
        {
            return Err(FormatError::Truncated {
                need: header.total_len as usize,
                have: buf.len(),
            });
        }
        let batch = Self { header, buf };
        if crc32c(batch.payload()) != header.payload_crc {
            return Err(FormatError::Checksum);
        }
        Ok(batch)
    }

    /// The batch header.
    pub fn header(&self) -> &BatchHeader {
        &self.header
    }

    /// The record payload.
    pub fn payload(&self) -> &[u8] {
        &self.buf[BATCH_HEADER_LEN..BATCH_HEADER_LEN + self.header.payload_len as usize]
    }

    /// The records in the batch.
    pub fn records(&self) -> RecordIter<'_> {
        RecordIter::new(self.payload(), self.header.record_count)
    }
}

/// A position in the log: where the next batch is expected, and its sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalPosition {
    /// Offset within the journal region.
    pub offset: u64,
    /// Expected batch sequence number.
    pub seq: u64,
}

/// Errors that stop a journal scan. Reaching the end of the log is not an error.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// The device could not be read.
    #[error("I/O error reading the journal: {0}")]
    Io(#[from] io::Error),
    /// A batch passed its CRC but its records do not decode. That is not a torn write: it is a
    /// bug or a newer format, and replay must stop rather than skip it.
    #[error("corrupt journal batch {seq} at offset {offset}: {source}")]
    Corrupt {
        /// Batch sequence number.
        seq: u64,
        /// Batch offset.
        offset: u64,
        /// What is wrong with it.
        source: FormatError,
    },
}

/// Replays the log from a starting position (spec §6.2), yielding each committed batch.
///
/// When the iterator ends, [`position`](Self::position) is where the next batch goes.
pub struct JournalScanner<'d, D: ReadAt + ?Sized> {
    dev: &'d D,
    journal_offset: u64,
    journal_size: u64,
    nonce: u64,
    next: JournalPosition,
    done: bool,
}

impl<'d, D: ReadAt + ?Sized> JournalScanner<'d, D> {
    /// Scans the journal of `layout` on `dev`, starting at `start`.
    pub fn new(dev: &'d D, layout: &Layout, nonce: u64, start: JournalPosition) -> Self {
        Self {
            dev,
            journal_offset: layout.journal_offset,
            journal_size: layout.journal_size,
            nonce,
            next: start,
            done: false,
        }
    }

    /// The position after the last batch yielded so far.
    pub fn position(&self) -> JournalPosition {
        self.next
    }

    fn try_at(&self, pos: u64) -> Result<Option<Batch>, ScanError> {
        let js = self.journal_size;
        if !pos.is_multiple_of(BLOCK_SIZE_U64)
            || pos.checked_add(BLOCK_SIZE_U64).is_none_or(|end| end > js)
        {
            return Ok(None);
        }
        let mut buf = vec![0u8; BLOCK_SIZE];
        self.dev.read_at(self.journal_offset + pos, &mut buf)?;
        let Ok(h) = BatchHeader::decode(&buf, self.nonce) else {
            return Ok(None);
        };
        let total = u64::from(h.total_len);
        if h.batch_seq != self.next.seq
            || h.offset != pos
            || total < BLOCK_SIZE_U64
            || total % BLOCK_SIZE_U64 != 0
            || pos + total > js
            || u64::from(h.payload_len) > total - BATCH_HEADER_LEN as u64
        {
            return Ok(None);
        }
        buf.resize(total as usize, 0);
        if total > BLOCK_SIZE_U64 {
            self.dev.read_at(
                self.journal_offset + pos + BLOCK_SIZE_U64,
                &mut buf[BLOCK_SIZE..],
            )?;
        }
        let batch = Batch { header: h, buf };
        if crc32c(batch.payload()) != h.payload_crc {
            return Ok(None);
        }
        for r in batch.records() {
            r.map_err(|source| ScanError::Corrupt {
                seq: h.batch_seq,
                offset: pos,
                source,
            })?;
        }
        Ok(Some(batch))
    }
}

impl<D: ReadAt + ?Sized> Iterator for JournalScanner<'_, D> {
    type Item = Result<Batch, ScanError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let pos = self.next.offset;
        let found = match self.try_at(pos) {
            Ok(None) if pos != 0 => self.try_at(0),
            other => other,
        };
        match found {
            Ok(Some(batch)) => {
                let h = batch.header;
                self.next = JournalPosition {
                    offset: h.offset + u64::from(h.total_len),
                    seq: h.batch_seq + 1,
                };
                Some(Ok(batch))
            }
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::{GIB, MIB};
    use crate::layout::LayoutParams;
    use crate::record::tests::samples;

    const NONCE: u64 = 0xBEEF;

    /// A device holding just enough bytes for the journal of `layout`.
    struct Journal {
        layout: Layout,
        dev: Vec<u8>,
        next: JournalPosition,
    }

    impl Journal {
        fn new(journal_size: u64) -> Self {
            let layout = Layout::compute(
                GIB,
                LayoutParams {
                    journal_size: Some(journal_size),
                    ..Default::default()
                },
            )
            .unwrap();
            Self {
                dev: vec![0; (layout.journal_offset + layout.journal_size) as usize],
                layout,
                next: JournalPosition { offset: 0, seq: 1 },
            }
        }

        /// Appends a batch and returns where it went.
        fn append(&mut self, records: &[Record<'_>], pad_to: usize) -> u64 {
            let mut b = BatchBuilder::new(4 * MIB as usize);
            for r in records {
                b.push(r).unwrap();
            }
            let mut total = b.total_len();
            if pad_to > 0 {
                // Grow the batch with an optional record to reach a target size.
                let filler = vec![0u8; pad_to];
                b.push(&Record::Unknown {
                    record_type: 999,
                    body: &filler,
                })
                .unwrap();
                total = b.total_len();
            }
            let at = place_batch(self.layout.journal_size, self.next.offset, total).unwrap();
            let bytes = b.finish(NONCE, self.next.seq, at);
            let start = (self.layout.journal_offset + at) as usize;
            self.dev[start..start + bytes.len()].copy_from_slice(&bytes);
            self.next = JournalPosition {
                offset: at + bytes.len() as u64,
                seq: self.next.seq + 1,
            };
            at
        }

        fn scan(&self, from: JournalPosition) -> (Vec<u64>, JournalPosition) {
            let mut s = JournalScanner::new(&self.dev, &self.layout, NONCE, from);
            let seqs = s.by_ref().map(|b| b.unwrap().header().batch_seq).collect();
            (seqs, s.position())
        }

        fn corrupt(&mut self, journal_pos: u64) {
            self.dev[(self.layout.journal_offset + journal_pos) as usize] ^= 0xFF;
        }
    }

    const START: JournalPosition = JournalPosition { offset: 0, seq: 1 };

    #[test]
    fn replays_every_committed_batch_and_reports_the_end() {
        let data = vec![0x11; BLOCK_SIZE];
        let mut j = Journal::new(16 * MIB);
        for _ in 0..5 {
            j.append(&samples(&data), 0);
        }
        let (seqs, end) = j.scan(START);
        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
        assert_eq!(end, j.next);

        let s = JournalScanner::new(&j.dev, &j.layout, NONCE, START);
        for batch in s {
            let batch = batch.unwrap();
            let recs: Vec<_> = batch.records().collect::<Result<_, _>>().unwrap();
            assert_eq!(recs, samples(&data));
        }
    }

    #[test]
    fn torn_batch_ends_the_log() {
        let mut j = Journal::new(16 * MIB);
        j.append(&[Record::ChunkDelete { slot: 1 }], 0);
        let second = j.append(
            &[Record::ChunkDelete { slot: 2 }],
            sized(4 * BLOCK_SIZE_U64),
        );
        j.append(&[Record::ChunkDelete { slot: 3 }], 0);
        // Damage the last block of batch 2 (payload), as a torn multi-block write would.
        j.corrupt(second + 3 * BLOCK_SIZE_U64 + 10);
        let (seqs, end) = j.scan(START);
        assert_eq!(seqs, vec![1]);
        assert_eq!(
            end,
            JournalPosition {
                offset: second,
                seq: 2
            }
        );
    }

    /// Filler that makes a batch with no other records exactly `total` bytes long
    /// (64-byte header + 8-byte record header + body, padded to 4096).
    fn sized(total: u64) -> usize {
        total as usize - 100
    }

    #[test]
    fn wraps_around_and_ignores_the_previous_lap() {
        let mut j = Journal::new(4 * MIB);
        let mut placed = Vec::new();
        for _ in 0..6 {
            placed.push(j.append(&[], sized(MIB)));
        }
        // Six 1 MiB batches in a 4 MiB journal: the last two overwrote the first two.
        assert_eq!(placed, vec![0, MIB, 2 * MIB, 3 * MIB, 0, MIB]);
        // Batch 3 from the previous lap sits at the end position, but its sequence number is wrong.
        let (seqs, end) = j.scan(JournalPosition {
            offset: 2 * MIB,
            seq: 3,
        });
        assert_eq!(seqs, vec![3, 4, 5, 6]);
        assert_eq!(
            end,
            JournalPosition {
                offset: 2 * MIB,
                seq: 7
            }
        );
    }

    #[test]
    fn batch_that_does_not_fit_goes_to_offset_zero() {
        let mut j = Journal::new(4 * MIB);
        assert_eq!(j.append(&[], sized(3 * MIB)), 0);
        assert_eq!(j.append(&[], sized(MIB / 2)), 3 * MIB);
        assert_eq!(
            j.append(&[], sized(MIB / 2 + BLOCK_SIZE_U64)),
            0,
            "would cross the end"
        );
        let (seqs, _) = j.scan(JournalPosition {
            offset: 3 * MIB,
            seq: 2,
        });
        assert_eq!(seqs, vec![2, 3]);
    }

    #[test]
    fn rewritten_batch_after_crash_is_found_at_either_position() {
        // A crash tore batch 3 at offset 3 MiB. After recovery the writer rewrote batch 3, but
        // it was too large for the space left, so it went to offset 0. Replay must find it.
        // Batch 1 (2 MiB at 0) is already checkpointed, so its space may be reused.
        let mut j = Journal::new(4 * MIB);
        j.append(&[], sized(2 * MIB));
        j.append(&[], sized(MIB));
        let torn_at = j.append(&[Record::ChunkDelete { slot: 7 }], 0);
        assert_eq!(torn_at, 3 * MIB);
        j.corrupt(torn_at + 100);
        j.next = JournalPosition {
            offset: torn_at,
            seq: 3,
        };
        assert_eq!(j.append(&[], sized(MIB + BLOCK_SIZE_U64)), 0);
        let (seqs, _) = j.scan(JournalPosition {
            offset: 2 * MIB,
            seq: 2,
        });
        assert_eq!(seqs, vec![2, 3]);
    }

    #[test]
    fn stale_format_is_ignored() {
        let mut j = Journal::new(16 * MIB);
        j.append(&[Record::ChunkDelete { slot: 1 }], 0);
        let s = JournalScanner::new(&j.dev, &j.layout, NONCE + 1, START);
        assert_eq!(s.count(), 0);
    }

    #[test]
    fn valid_crc_with_bad_records_is_corruption_not_end_of_log() {
        let mut b = BatchBuilder::new(BLOCK_SIZE);
        b.push(&Record::ChunkDelete { slot: 1 }).unwrap();
        let mut bytes = b.finish(NONCE, 1, 0);
        // Claim two records while only one is present; re-seal the header so CRCs pass.
        put_u32(&mut bytes, 40, 2);
        let crc = crc32c(&bytes[..HEADER_CRC_OFFSET]);
        put_u32(&mut bytes, HEADER_CRC_OFFSET, crc);
        let mut j = Journal::new(16 * MIB);
        let start = j.layout.journal_offset as usize;
        j.dev[start..start + bytes.len()].copy_from_slice(&bytes);
        let mut s = JournalScanner::new(&j.dev, &j.layout, NONCE, START);
        assert!(matches!(
            s.next(),
            Some(Err(ScanError::Corrupt { seq: 1, .. }))
        ));
        assert!(s.next().is_none());
    }

    #[test]
    fn builder_reports_full() {
        let mut b = BatchBuilder::new(BLOCK_SIZE);
        let data = vec![0u8; BLOCK_SIZE];
        let rec = Record::DeferredWrite {
            slot: 0,
            offset: 0,
            write_seq: 1,
            csums: std::borrow::Cow::Owned(vec![0]),
            data: &data,
        };
        assert_eq!(b.push(&rec), Err(FormatError::BatchFull));
        assert_eq!(b.record_count(), 0);
    }

    #[test]
    fn placement_rules() {
        assert_eq!(place_batch(100, 0, 101), None);
        assert_eq!(place_batch(100, 40, 60), Some(40));
        assert_eq!(place_batch(100, 41, 60), Some(0));
        assert_eq!(place_batch(100, 100, 4), Some(0));
    }
}
