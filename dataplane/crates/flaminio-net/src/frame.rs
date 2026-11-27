//! Frames: `[u32 body length][u8 type][body]`, little-endian.
//!
//! A connection carries one session (see [`crate::session`]): `Hello` names the volume, the
//! epoch and the session once, and later requests carry only a request id.

use bytes::Bytes;
use flaminio_repl::extent::Extent;
use flaminio_repl::msg::{MAX_DIGEST_BLOCKS, MAX_MAPPED_BLOCKS};
use flaminio_repl::replica::UNCERTAINTY_BUDGET;
use flaminio_repl::types::Version;

use crate::BLOCK;

/// Most blocks in one write, read or copy: 1 MiB.
pub const MAX_BLOCKS: u32 = 256;
/// Largest body accepted: a full-size write and its header.
pub const MAX_BODY: usize = MAX_BLOCKS as usize * BLOCK + 128;
/// Most extents in one `Fenced` reply.
pub const MAX_UNCERTAIN: usize = 1 << 16;
/// Most chunks of one volume on one node, as listed in `Hello` or `Fenced`.
pub const MAX_CHUNKS: usize = 1 << 16;

const _: () = assert!(MAX_UNCERTAIN == UNCERTAINTY_BUDGET as usize);
const _: () = assert!(MAX_CHUNKS == UNCERTAINTY_BUDGET as usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    // Requests, target → replica.
    /// Opens the session. Besides the token, it carries the volume's geometry and the chunks
    /// the metadata service placed on this node, so the replica can create them (and make the
    /// fence durable) before any I/O. The token's holder controls the volume, so trusting its
    /// geometry grants it nothing more.
    Hello {
        req: u64,
        epoch: u64,
        session: u64,
        volume: String,
        token: Vec<u8>,
        size: u64,
        chunk_size: u64,
        chunks: Vec<u32>,
    },
    Heartbeat {
        req: u64,
    },
    Write {
        req: u64,
        chunk: u32,
        ext: Extent,
        version: Version,
        prune: Option<Version>,
        /// Shared: the same bytes go to every replica, and to the local store, without a copy.
        data: Bytes,
    },
    Read {
        req: u64,
        chunk: u32,
        ext: Extent,
    },
    BeginResync {
        req: u64,
        chunk: u32,
    },
    Settle {
        req: u64,
        chunk: u32,
        below: Version,
    },
    Copy {
        req: u64,
        chunk: u32,
        ext: Extent,
        data: Bytes,
    },
    Digest {
        req: u64,
        chunk: u32,
        ext: Extent,
    },
    Mapped {
        req: u64,
        chunk: u32,
        ext: Extent,
    },
    Discard {
        req: u64,
        chunk: u32,
        ext: Extent,
    },
    // Replies, replica → target.
    Fenced {
        req: u64,
        uncertain: Vec<(u32, Vec<Extent>)>,
    },
    Ok {
        req: u64,
    },
    Data {
        req: u64,
        data: Vec<u8>,
    },
    /// One optional CRC-32C per block of the digest request's extent.
    Digests {
        req: u64,
        sums: Vec<Option<u32>>,
    },
    /// Sub-extents of a `Mapped` request that hold data.
    Mappings {
        req: u64,
        exts: Vec<Extent>,
    },
    Stale {
        req: u64,
        fenced: u64,
    },
    /// The connection is refused (authentication or protocol error) and will be closed.
    Refused {
        reason: String,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("frame truncated")]
    Truncated,
    #[error("frame of {0} bytes exceeds the limit")]
    TooLarge(usize),
    #[error("unknown frame type {0}")]
    UnknownType(u8),
    #[error("invalid frame: {0}")]
    Invalid(&'static str),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EncodeError {
    #[error("invalid frame: {0}")]
    Invalid(&'static str),
    #[error("frame of {0} bytes exceeds the limit")]
    TooLarge(usize),
}

/// The Fenced body length, excluding its prefix. Count limits are checked by the caller.
fn fenced_body_len(chunks: usize, extents: usize) -> Result<usize, EncodeError> {
    let body = chunks
        .checked_mul(8)
        .and_then(|rows| {
            extents
                .checked_mul(8)
                .and_then(|exts| rows.checked_add(exts))
        })
        .and_then(|bytes| bytes.checked_add(13))
        .ok_or(EncodeError::Invalid("Fenced body length overflow"))?;
    if body > MAX_BODY {
        return Err(EncodeError::TooLarge(body));
    }
    Ok(body)
}

/// Count loop visits and requested capacity, independently of elapsed time or allocator size.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FencedDecodeWork {
    rows: usize,
    extents: usize,
    row_slots: usize,
    extent_slots: usize,
    reserves: usize,
}

#[cfg(test)]
std::thread_local! {
    static FENCED_DECODE_WORK: std::cell::Cell<FencedDecodeWork> = const {
        std::cell::Cell::new(FencedDecodeWork {
            rows: 0, extents: 0, row_slots: 0, extent_slots: 0, reserves: 0,
        })
    };
}

#[cfg(test)]
fn count_fenced_decode(count: impl FnOnce(&mut FencedDecodeWork)) {
    FENCED_DECODE_WORK.with(|cell| {
        let mut work = cell.get();
        count(&mut work);
        cell.set(work);
    });
}

mod tag {
    pub const HELLO: u8 = 1;
    pub const HEARTBEAT: u8 = 2;
    pub const WRITE: u8 = 3;
    pub const READ: u8 = 4;
    pub const BEGIN_RESYNC: u8 = 5;
    pub const COPY: u8 = 6;
    pub const SETTLE: u8 = 7;
    pub const DIGEST: u8 = 8;
    pub const MAPPED: u8 = 9;
    pub const DISCARD: u8 = 10;
    pub const FENCED: u8 = 64;
    pub const OK: u8 = 65;
    pub const DATA: u8 = 66;
    pub const STALE: u8 = 67;
    pub const REFUSED: u8 = 68;
    pub const DIGESTS: u8 = 69;
    pub const MAPPINGS: u8 = 70;
}

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn ext(&mut self, x: &Extent) {
        self.u32(x.start);
        self.u32(x.len);
    }
    fn version(&mut self, v: &Version) {
        self.u64(v.epoch);
        self.u64(v.seq);
    }
    fn short_bytes(&mut self, b: &[u8]) {
        self.u16(b.len() as u16);
        self.0.extend_from_slice(b);
    }
}

impl Frame {
    /// Encodes the frame, checking Fenced reports before allocation or sending any bytes.
    /// Other frame types use the existing trusted encoder until their preflight is added.
    pub fn encode_checked(&self) -> Result<Vec<u8>, EncodeError> {
        let Frame::Fenced { req, uncertain } = self else {
            return Ok(self.encode());
        };
        if uncertain.len() > MAX_CHUNKS {
            return Err(EncodeError::Invalid("too many uncertain chunks"));
        }
        let mut total = 0usize;
        let mut previous_chunk = None;
        for (chunk, exts) in uncertain {
            if previous_chunk.is_some_and(|previous| previous >= *chunk) {
                return Err(EncodeError::Invalid("uncertain chunks are not ordered"));
            }
            previous_chunk = Some(*chunk);
            total = total
                .checked_add(exts.len())
                .ok_or(EncodeError::Invalid("uncertain extent count overflow"))?;
            if total > MAX_UNCERTAIN {
                return Err(EncodeError::Invalid("too many uncertain extents"));
            }
            let mut previous_end = None;
            for ext in exts {
                let end = ext
                    .start
                    .checked_add(ext.len)
                    .filter(|_| ext.len != 0)
                    .ok_or(EncodeError::Invalid("uncertain extent"))?;
                if previous_end.is_some_and(|previous| previous > ext.start) {
                    return Err(EncodeError::Invalid(
                        "uncertain extents overlap or are unordered",
                    ));
                }
                previous_end = Some(end);
            }
        }
        let body = fenced_body_len(uncertain.len(), total)?;
        let len = body
            .checked_add(4)
            .ok_or(EncodeError::Invalid("Fenced frame length overflow"))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(len)
            .map_err(|_| EncodeError::Invalid("Fenced allocation failed"))?;
        bytes.extend_from_slice(&(body as u32).to_le_bytes());
        let mut w = W(bytes);
        w.u8(tag::FENCED);
        w.u64(*req);
        w.u32(uncertain.len() as u32);
        for (chunk, exts) in uncertain {
            w.u32(*chunk);
            w.u32(exts.len() as u32);
            for ext in exts {
                w.ext(ext);
            }
        }
        Ok(w.0)
    }

    /// Encodes the frame, with its length prefix.
    ///
    /// Panics if the frame breaks a limit that `decode` enforces, which is a bug in the caller.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = W(vec![0; 4]);
        match self {
            Frame::Hello {
                req,
                epoch,
                session,
                volume,
                token,
                size,
                chunk_size,
                chunks,
            } => {
                assert!(chunks.len() <= MAX_CHUNKS, "too many chunks in Hello");
                w.u8(tag::HELLO);
                w.u64(*req);
                w.u64(*epoch);
                w.u64(*session);
                w.short_bytes(volume.as_bytes());
                w.short_bytes(token);
                w.u64(*size);
                w.u64(*chunk_size);
                w.u32(chunks.len() as u32);
                for c in chunks {
                    w.u32(*c);
                }
            }
            Frame::Heartbeat { req } => {
                w.u8(tag::HEARTBEAT);
                w.u64(*req);
            }
            Frame::Write {
                req,
                chunk,
                ext,
                version,
                prune,
                data,
            } => {
                assert_eq!(data.len(), ext.len as usize * BLOCK, "write payload size");
                w.u8(tag::WRITE);
                w.u64(*req);
                w.u32(*chunk);
                w.ext(ext);
                w.version(version);
                match prune {
                    Some(p) => {
                        w.u8(1);
                        w.version(p);
                    }
                    None => w.u8(0),
                }
                w.0.extend_from_slice(data);
            }
            Frame::Read { req, chunk, ext } => {
                w.u8(tag::READ);
                w.u64(*req);
                w.u32(*chunk);
                w.ext(ext);
            }
            Frame::BeginResync { req, chunk } => {
                w.u8(tag::BEGIN_RESYNC);
                w.u64(*req);
                w.u32(*chunk);
            }
            Frame::Settle { req, chunk, below } => {
                w.u8(tag::SETTLE);
                w.u64(*req);
                w.u32(*chunk);
                w.version(below);
            }
            Frame::Copy {
                req,
                chunk,
                ext,
                data,
            } => {
                assert_eq!(data.len(), ext.len as usize * BLOCK, "copy payload size");
                w.u8(tag::COPY);
                w.u64(*req);
                w.u32(*chunk);
                w.ext(ext);
                w.0.extend_from_slice(data);
            }
            Frame::Digest { req, chunk, ext } => {
                w.u8(tag::DIGEST);
                w.u64(*req);
                w.u32(*chunk);
                w.ext(ext);
            }
            Frame::Mapped { req, chunk, ext } => {
                w.u8(tag::MAPPED);
                w.u64(*req);
                w.u32(*chunk);
                w.ext(ext);
            }
            Frame::Discard { req, chunk, ext } => {
                w.u8(tag::DISCARD);
                w.u64(*req);
                w.u32(*chunk);
                w.ext(ext);
            }
            Frame::Fenced { req, uncertain } => {
                w.u8(tag::FENCED);
                w.u64(*req);
                w.u32(uncertain.len() as u32);
                for (chunk, exts) in uncertain {
                    w.u32(*chunk);
                    w.u32(exts.len() as u32);
                    for x in exts {
                        w.ext(x);
                    }
                }
            }
            Frame::Ok { req } => {
                w.u8(tag::OK);
                w.u64(*req);
            }
            Frame::Data { req, data } => {
                w.u8(tag::DATA);
                w.u64(*req);
                w.0.extend_from_slice(data);
            }
            Frame::Digests { req, sums } => {
                assert!(sums.len() <= MAX_DIGEST_BLOCKS as usize, "too many digests");
                w.u8(tag::DIGESTS);
                w.u64(*req);
                w.u32(sums.len() as u32);
                for s in sums {
                    match s {
                        None => w.u8(0),
                        Some(c) => {
                            w.u8(1);
                            w.u32(*c);
                        }
                    }
                }
            }
            Frame::Mappings { req, exts } => {
                assert!(exts.len() <= MAX_UNCERTAIN, "too many mapped extents");
                w.u8(tag::MAPPINGS);
                w.u64(*req);
                w.u32(exts.len() as u32);
                for x in exts {
                    w.ext(x);
                }
            }
            Frame::Stale { req, fenced } => {
                w.u8(tag::STALE);
                w.u64(*req);
                w.u64(*fenced);
            }
            Frame::Refused { reason } => {
                w.u8(tag::REFUSED);
                let r = &reason.as_bytes()[..reason.len().min(1024)];
                w.short_bytes(r);
            }
        }
        let body = w.0.len() - 4;
        assert!(body <= MAX_BODY, "frame body of {body} bytes");
        w.0[..4].copy_from_slice(&(body as u32).to_le_bytes());
        w.0
    }

    /// Reads the body length from a 4-byte prefix, refusing oversized frames before any
    /// allocation.
    pub fn body_len(prefix: [u8; 4]) -> Result<usize, DecodeError> {
        let n = u32::from_le_bytes(prefix) as usize;
        if n > MAX_BODY {
            return Err(DecodeError::TooLarge(n));
        }
        if n == 0 {
            return Err(DecodeError::Truncated);
        }
        Ok(n)
    }

    /// Decodes a frame body (without the length prefix).
    pub fn decode(body: &[u8]) -> Result<Frame, DecodeError> {
        if body.len() > MAX_BODY {
            return Err(DecodeError::TooLarge(body.len()));
        }
        let mut r = R(body);
        let t = r.u8()?;
        let f = match t {
            tag::HELLO => {
                let req = r.u64()?;
                let epoch = r.u64()?;
                let session = r.u64()?;
                let volume = String::from_utf8(r.short_bytes()?.to_vec())
                    .map_err(|_| DecodeError::Invalid("volume id is not UTF-8"))?;
                let token = r.short_bytes()?.to_vec();
                let size = r.u64()?;
                let chunk_size = r.u64()?;
                let n = r.u32()? as usize;
                if n > MAX_CHUNKS {
                    return Err(DecodeError::Invalid("too many chunks"));
                }
                let mut chunks = Vec::with_capacity(n.min(r.0.len() / 4));
                for _ in 0..n {
                    chunks.push(r.u32()?);
                }
                Frame::Hello {
                    req,
                    epoch,
                    session,
                    volume,
                    token,
                    size,
                    chunk_size,
                    chunks,
                }
            }
            tag::HEARTBEAT => Frame::Heartbeat { req: r.u64()? },
            tag::WRITE => {
                let req = r.u64()?;
                let chunk = r.u32()?;
                let ext = r.ext()?;
                let version = r.version()?;
                let prune = match r.u8()? {
                    0 => None,
                    1 => Some(r.version()?),
                    _ => return Err(DecodeError::Invalid("prune flag")),
                };
                let data = Bytes::from(r.data(ext)?);
                Frame::Write {
                    req,
                    chunk,
                    ext,
                    version,
                    prune,
                    data,
                }
            }
            tag::READ => Frame::Read {
                req: r.u64()?,
                chunk: r.u32()?,
                ext: r.ext()?,
            },
            tag::BEGIN_RESYNC => Frame::BeginResync {
                req: r.u64()?,
                chunk: r.u32()?,
            },
            tag::SETTLE => Frame::Settle {
                req: r.u64()?,
                chunk: r.u32()?,
                below: r.version()?,
            },
            tag::COPY => {
                let req = r.u64()?;
                let chunk = r.u32()?;
                let ext = r.ext()?;
                let data = Bytes::from(r.data(ext)?);
                Frame::Copy {
                    req,
                    chunk,
                    ext,
                    data,
                }
            }
            tag::DIGEST => {
                let (req, chunk) = (r.u64()?, r.u32()?);
                let ext = r.any_ext()?;
                if ext.len > MAX_DIGEST_BLOCKS {
                    return Err(DecodeError::Invalid("digest extent too long"));
                }
                Frame::Digest { req, chunk, ext }
            }
            tag::MAPPED => {
                let (req, chunk) = (r.u64()?, r.u32()?);
                // A probe carries no data: it may cover a whole chunk, up to its own limit.
                let ext = r.any_ext()?;
                if ext.len > MAX_MAPPED_BLOCKS {
                    return Err(DecodeError::Invalid("mapped extent too long"));
                }
                Frame::Mapped { req, chunk, ext }
            }
            tag::DISCARD => Frame::Discard {
                req: r.u64()?,
                chunk: r.u32()?,
                ext: r.ext()?,
            },
            tag::FENCED => {
                let req = r.u64()?;
                let n = r.u32()? as usize;
                if n > MAX_CHUNKS {
                    return Err(DecodeError::Invalid("too many uncertain chunks"));
                }
                // Every row needs its chunk id and extent count, even if it is empty.
                if n > r.0.len() / 8 {
                    return Err(DecodeError::Truncated);
                }
                let mut uncertain = Vec::new();
                #[cfg(test)]
                count_fenced_decode(|work| {
                    work.row_slots += n;
                    work.reserves += 1;
                });
                uncertain
                    .try_reserve_exact(n)
                    .map_err(|_| DecodeError::Invalid("Fenced allocation failed"))?;
                let mut total = 0usize;
                let mut previous_chunk = None;
                for _ in 0..n {
                    #[cfg(test)]
                    count_fenced_decode(|work| work.rows += 1);
                    let chunk = r.u32()?;
                    if previous_chunk.is_some_and(|previous| previous >= chunk) {
                        return Err(DecodeError::Invalid("uncertain chunks are not ordered"));
                    }
                    previous_chunk = Some(chunk);
                    let m = r.u32()? as usize;
                    total = total
                        .checked_add(m)
                        .ok_or(DecodeError::Invalid("uncertain extent count overflow"))?;
                    if total > MAX_UNCERTAIN {
                        return Err(DecodeError::Invalid("too many uncertain extents"));
                    }
                    if m > r.0.len() / 8 {
                        return Err(DecodeError::Truncated);
                    }
                    let mut exts = Vec::new();
                    #[cfg(test)]
                    count_fenced_decode(|work| {
                        work.extent_slots += m;
                        work.reserves += 1;
                    });
                    exts.try_reserve_exact(m)
                        .map_err(|_| DecodeError::Invalid("Fenced allocation failed"))?;
                    let mut previous_end = None;
                    for _ in 0..m {
                        #[cfg(test)]
                        count_fenced_decode(|work| work.extents += 1);
                        let ext = r.any_ext()?;
                        if previous_end.is_some_and(|previous| previous > ext.start) {
                            return Err(DecodeError::Invalid(
                                "uncertain extents overlap or are unordered",
                            ));
                        }
                        previous_end = Some(ext.start + ext.len);
                        exts.push(ext);
                    }
                    uncertain.push((chunk, exts));
                }
                Frame::Fenced { req, uncertain }
            }
            tag::OK => Frame::Ok { req: r.u64()? },
            tag::DATA => {
                let req = r.u64()?;
                let data = r.rest();
                // Whole blocks, at least one: a read is never empty. The target also checks the
                // size against the extent it asked for.
                if data.is_empty()
                    || data.len() % BLOCK != 0
                    || data.len() > MAX_BLOCKS as usize * BLOCK
                {
                    return Err(DecodeError::Invalid("data size"));
                }
                Frame::Data {
                    req,
                    data: data.to_vec(),
                }
            }
            tag::DIGESTS => {
                let req = r.u64()?;
                let n = r.u32()? as usize;
                if n == 0 || n > MAX_DIGEST_BLOCKS as usize {
                    return Err(DecodeError::Invalid("digest count"));
                }
                let mut sums = Vec::with_capacity(n);
                for _ in 0..n {
                    match r.u8()? {
                        0 => sums.push(None),
                        1 => sums.push(Some(r.u32()?)),
                        _ => return Err(DecodeError::Invalid("digest flag")),
                    }
                }
                Frame::Digests { req, sums }
            }
            tag::MAPPINGS => {
                let req = r.u64()?;
                let n = r.u32()? as usize;
                if n > MAX_UNCERTAIN {
                    return Err(DecodeError::Invalid("too many mapped extents"));
                }
                let mut exts = Vec::with_capacity(n.min(r.0.len() / 8));
                for _ in 0..n {
                    exts.push(r.any_ext()?);
                }
                Frame::Mappings { req, exts }
            }
            tag::STALE => Frame::Stale {
                req: r.u64()?,
                fenced: r.u64()?,
            },
            tag::REFUSED => Frame::Refused {
                reason: String::from_utf8_lossy(r.short_bytes()?).into_owned(),
            },
            other => return Err(DecodeError::UnknownType(other)),
        };
        if !r.0.is_empty() {
            return Err(DecodeError::Invalid("trailing bytes"));
        }
        Ok(f)
    }
}

struct R<'a>(&'a [u8]);

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.0.len() < n {
            return Err(DecodeError::Truncated);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("2")))
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4")))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8")))
    }
    /// An extent used for I/O: 1 to MAX_BLOCKS blocks, not wrapping.
    fn ext(&mut self) -> Result<Extent, DecodeError> {
        let x = self.any_ext()?;
        if x.len > MAX_BLOCKS {
            return Err(DecodeError::Invalid("extent too long"));
        }
        Ok(x)
    }
    /// An extent of any non-zero length, not wrapping.
    fn any_ext(&mut self) -> Result<Extent, DecodeError> {
        let start = self.u32()?;
        let len = self.u32()?;
        if len == 0 || start.checked_add(len).is_none() {
            return Err(DecodeError::Invalid("extent"));
        }
        Ok(Extent { start, len })
    }
    fn version(&mut self) -> Result<Version, DecodeError> {
        Ok(Version {
            epoch: self.u64()?,
            seq: self.u64()?,
        })
    }
    fn short_bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let n = self.u16()? as usize;
        self.take(n)
    }
    /// Exactly the extent's payload, and nothing after it.
    fn data(&mut self, ext: Extent) -> Result<Vec<u8>, DecodeError> {
        let n = ext.len as usize * BLOCK;
        if self.0.len() != n {
            return Err(DecodeError::Invalid(
                "payload size does not match the extent",
            ));
        }
        Ok(self.take(n)?.to_vec())
    }
    fn rest(&mut self) -> &'a [u8] {
        std::mem::take(&mut self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<Frame> {
        let ext = Extent { start: 3, len: 2 };
        vec![
            Frame::Hello {
                req: 1,
                epoch: 7,
                session: 2,
                volume: "pvc-0b1c".into(),
                token: vec![9; 90],
                size: 64 << 30,
                chunk_size: 256 << 20,
                chunks: vec![0, 3, 255],
            },
            Frame::Heartbeat { req: 2 },
            Frame::Write {
                req: 3,
                chunk: 5,
                ext,
                version: Version { epoch: 7, seq: 40 },
                prune: Some(Version { epoch: 7, seq: 38 }),
                data: (0..2 * BLOCK).map(|i| i as u8).collect::<Vec<u8>>().into(),
            },
            Frame::Write {
                req: 4,
                chunk: 5,
                ext: Extent { start: 0, len: 1 },
                version: Version { epoch: 7, seq: 41 },
                prune: None,
                data: vec![1; BLOCK].into(),
            },
            Frame::Read {
                req: 5,
                chunk: 1,
                ext,
            },
            Frame::BeginResync { req: 6, chunk: 4 },
            Frame::Settle {
                req: 12,
                chunk: 4,
                below: Version { epoch: 7, seq: 90 },
            },
            Frame::Copy {
                req: 7,
                chunk: 4,
                ext,
                data: vec![2; 2 * BLOCK].into(),
            },
            Frame::Digest {
                req: 13,
                chunk: 1,
                ext: Extent {
                    start: 0,
                    len: MAX_DIGEST_BLOCKS,
                },
            },
            // A whole 256 MiB chunk: longer than an I/O, as resync probes are.
            Frame::Mapped {
                req: 15,
                chunk: 1,
                ext: Extent {
                    start: 0,
                    len: MAX_MAPPED_BLOCKS,
                },
            },
            Frame::Discard {
                req: 16,
                chunk: 1,
                ext,
            },
            Frame::Digests {
                req: 14,
                sums: vec![None, Some(0xdead_beef)],
            },
            Frame::Mappings {
                req: 17,
                exts: vec![ext, Extent { start: 4, len: 2 }],
            },
            Frame::Fenced {
                req: 8,
                uncertain: vec![
                    (0, vec![]),
                    (
                        3,
                        vec![
                            ext,
                            Extent {
                                start: 100,
                                len: 65_536,
                            },
                        ],
                    ),
                ],
            },
            Frame::Ok { req: 9 },
            Frame::Data {
                req: 10,
                data: vec![3; BLOCK],
            },
            Frame::Stale {
                req: 11,
                fenced: 12,
            },
            Frame::Refused {
                reason: "token names another node".into(),
            },
        ]
    }

    #[test]
    fn round_trip() {
        for f in samples() {
            let b = f.encode_checked().unwrap();
            assert_eq!(b, f.encode(), "existing wire encoding changed");
            let n = Frame::body_len(b[..4].try_into().unwrap()).unwrap();
            assert_eq!(n, b.len() - 4);
            assert_eq!(Frame::decode(&b[4..]).unwrap(), f);
        }
    }

    fn fenced_header(rows: u32) -> Vec<u8> {
        let mut body = vec![tag::FENCED];
        body.extend_from_slice(&7u64.to_le_bytes());
        body.extend_from_slice(&rows.to_le_bytes());
        body
    }

    /// Construct untrusted wire bytes without invoking either encoder's validation.
    fn raw_fenced(rows: &[(u32, Vec<Extent>)]) -> Vec<u8> {
        let mut body = fenced_header(u32::try_from(rows.len()).unwrap());
        for (chunk, exts) in rows {
            body.extend_from_slice(&chunk.to_le_bytes());
            body.extend_from_slice(&u32::try_from(exts.len()).unwrap().to_le_bytes());
            for ext in exts {
                body.extend_from_slice(&ext.start.to_le_bytes());
                body.extend_from_slice(&ext.len.to_le_bytes());
            }
        }
        body
    }

    fn decode_work(body: &[u8]) -> (Result<Frame, DecodeError>, FencedDecodeWork) {
        FENCED_DECODE_WORK.with(|work| work.set(FencedDecodeWork::default()));
        let decoded = Frame::decode(body);
        (decoded, FENCED_DECODE_WORK.with(std::cell::Cell::get))
    }

    #[test]
    fn fenced_body_length_and_arithmetic_boundaries() {
        assert_eq!(fenced_body_len(0, 0), Ok(13));
        assert_eq!(fenced_body_len(MAX_CHUNKS, MAX_UNCERTAIN), Ok(1_048_589));
        assert_eq!(MAX_BODY - 1_048_589, 115);
        let words = (MAX_BODY - 13) / 8;
        assert_eq!(fenced_body_len(words, 0), Ok(13 + 8 * words));
        assert_eq!(
            fenced_body_len(words + 1, 0),
            Err(EncodeError::TooLarge(13 + 8 * (words + 1)))
        );
        for (rows, extents) in [
            (usize::MAX, 0),
            (0, usize::MAX),
            (usize::MAX / 8, usize::MAX / 8),
            (usize::MAX / 8, 0),
        ] {
            assert_eq!(
                fenced_body_len(rows, extents),
                Err(EncodeError::Invalid("Fenced body length overflow"))
            );
        }
    }

    #[test]
    fn fenced_maximum_shapes_roundtrip_with_linear_decode_work() {
        // Independently saturate rows and extents, then saturate both together. Empty held
        // rows count too; the last case is the exact N=T=B body-size proof.
        for (rows, extents) in [
            (MAX_CHUNKS, 0),
            (1, MAX_UNCERTAIN),
            (MAX_CHUNKS, MAX_UNCERTAIN),
        ] {
            let uncertain = if rows == 1 {
                vec![(
                    0,
                    (0..extents as u32).map(|i| Extent::new(i * 2, 1)).collect(),
                )]
            } else {
                (0..rows as u32)
                    .map(|chunk| {
                        (
                            chunk,
                            if extents == 0 {
                                vec![]
                            } else {
                                vec![Extent::new(0, 1)]
                            },
                        )
                    })
                    .collect()
            };
            let frame = Frame::Fenced { req: 7, uncertain };
            let encoded = frame.encode_checked().unwrap();
            assert_eq!(encoded.len() - 4, 13 + 8 * rows + 8 * extents);
            assert_eq!(
                Frame::body_len(encoded[..4].try_into().unwrap()),
                Ok(encoded.len() - 4)
            );
            let (decoded, work) = decode_work(&encoded[4..]);
            let decoded = decoded.unwrap();
            assert_eq!(decoded, frame);
            assert_eq!(decoded.encode_checked().unwrap(), encoded);
            assert_eq!(
                work,
                FencedDecodeWork {
                    rows,
                    extents,
                    row_slots: rows,
                    extent_slots: extents,
                    reserves: rows + 1,
                }
            );
        }
    }

    #[test]
    fn fenced_shape_is_checked_in_both_directions() {
        let invalid = [
            (
                "uncertain chunks are not ordered",
                vec![(4, vec![]), (4, vec![])],
            ),
            (
                "uncertain chunks are not ordered",
                vec![(4, vec![]), (3, vec![])],
            ),
            (
                "uncertain extent",
                vec![(0, vec![Extent { start: 0, len: 0 }])],
            ),
            (
                "uncertain extent",
                vec![(
                    0,
                    vec![Extent {
                        start: u32::MAX,
                        len: 1,
                    }],
                )],
            ),
            (
                "uncertain extent",
                vec![(
                    0,
                    vec![Extent {
                        start: u32::MAX - 1,
                        len: 2,
                    }],
                )],
            ),
            (
                "uncertain extents overlap or are unordered",
                vec![(0, vec![Extent::new(2, 4), Extent::new(4, 1)])],
            ),
            (
                "uncertain extents overlap or are unordered",
                vec![(0, vec![Extent::new(2, 1), Extent::new(2, 1)])],
            ),
            (
                "uncertain extents overlap or are unordered",
                vec![(0, vec![Extent::new(4, 1), Extent::new(2, 1)])],
            ),
        ];
        for (reason, uncertain) in invalid {
            let body = raw_fenced(&uncertain);
            let frame = Frame::Fenced { req: 7, uncertain };
            assert_eq!(frame.encode_checked(), Err(EncodeError::Invalid(reason)));
            let decode_reason = if reason == "uncertain extent" {
                "extent"
            } else {
                reason
            };
            assert_eq!(
                Frame::decode(&body),
                Err(DecodeError::Invalid(decode_reason))
            );
        }
    }

    #[test]
    fn fenced_whole_chunk_and_adjacent_extents_are_legal() {
        // Fenced ranges are independent of the 256-block I/O limit and chunk geometry.
        let frame = Frame::Fenced {
            req: 7,
            uncertain: vec![
                (0, vec![]),
                (1, vec![Extent::new(0, 262_144)]),
                (
                    2,
                    vec![Extent::new(2, 2), Extent::new(4, 1), Extent::new(100, 3)],
                ),
                (u32::MAX, vec![Extent::new(u32::MAX - 1, 1)]),
            ],
        };
        let encoded = frame.encode_checked().unwrap();
        assert_eq!(Frame::decode(&encoded[4..]), Ok(frame));
    }

    #[test]
    fn fenced_count_one_over_is_refused_in_both_directions() {
        for (reason, uncertain) in [
            (
                "too many uncertain chunks",
                (0..=MAX_CHUNKS as u32)
                    .map(|chunk| (chunk, vec![]))
                    .collect(),
            ),
            (
                "too many uncertain extents",
                vec![(
                    0,
                    (0..=MAX_UNCERTAIN as u32)
                        .map(|i| Extent::new(i * 2, 1))
                        .collect(),
                )],
            ),
            (
                "too many uncertain extents",
                vec![
                    (
                        0,
                        (0..MAX_UNCERTAIN as u32)
                            .map(|i| Extent::new(i * 2, 1))
                            .collect(),
                    ),
                    (1, vec![Extent::new(0, 1)]),
                ],
            ),
        ] {
            let body = raw_fenced(&uncertain);
            let frame = Frame::Fenced { req: 7, uncertain };
            assert_eq!(frame.encode_checked(), Err(EncodeError::Invalid(reason)));
            assert_eq!(Frame::decode(&body), Err(DecodeError::Invalid(reason)));
        }
    }

    #[test]
    fn fenced_decoder_bounds_counts_and_wire_before_reserving() {
        for rows in [MAX_CHUNKS as u32 + 1, u32::MAX] {
            let (decoded, work) = decode_work(&fenced_header(rows));
            assert_eq!(
                decoded,
                Err(DecodeError::Invalid("too many uncertain chunks"))
            );
            assert_eq!(work, FencedDecodeWork::default());
        }
        for rows in [1, MAX_CHUNKS as u32] {
            let (decoded, work) = decode_work(&fenced_header(rows));
            assert_eq!(decoded, Err(DecodeError::Truncated));
            assert_eq!(work, FencedDecodeWork::default());
        }
        for extents in [MAX_UNCERTAIN as u32, MAX_UNCERTAIN as u32 + 1, u32::MAX] {
            let mut body = fenced_header(1);
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend_from_slice(&extents.to_le_bytes());
            let (decoded, work) = decode_work(&body);
            let expected = if extents == MAX_UNCERTAIN as u32 {
                DecodeError::Truncated
            } else {
                DecodeError::Invalid("too many uncertain extents")
            };
            assert_eq!(decoded, Err(expected));
            assert_eq!(
                work,
                FencedDecodeWork {
                    rows: 1,
                    row_slots: 1,
                    reserves: 1,
                    ..Default::default()
                }
            );
        }
        // Aggregate one-over must stop before reserving the second row's extents, even
        // though the first row used the entire permitted extent budget.
        let mut body = raw_fenced(&[
            (
                0,
                (0..MAX_UNCERTAIN as u32)
                    .map(|i| Extent::new(i * 2, 1))
                    .collect(),
            ),
            (1, vec![Extent::new(0, 1)]),
        ]);
        body.truncate(body.len() - 8);
        let (decoded, work) = decode_work(&body);
        assert_eq!(
            decoded,
            Err(DecodeError::Invalid("too many uncertain extents"))
        );
        assert_eq!(
            work,
            FencedDecodeWork {
                rows: 2,
                extents: MAX_UNCERTAIN,
                row_slots: 2,
                extent_slots: MAX_UNCERTAIN,
                reserves: 2,
            }
        );
    }

    #[test]
    fn fenced_body_overflow_trailing_bytes_and_truncations_are_refused() {
        let (decoded, work) = decode_work(&vec![0; MAX_BODY + 1]);
        assert_eq!(decoded, Err(DecodeError::TooLarge(MAX_BODY + 1)));
        assert_eq!(work, FencedDecodeWork::default());
        assert_eq!(
            Frame::body_len(u32::MAX.to_le_bytes()),
            Err(DecodeError::TooLarge(u32::MAX as usize))
        );
        let mut body = raw_fenced(&[(0, vec![]), (3, vec![Extent::new(0, 262_144)])]);
        for cut in 0..body.len() {
            assert!(
                Frame::decode(&body[..cut]).is_err(),
                "accepted cut at {cut}"
            );
        }
        body.push(0);
        assert_eq!(
            Frame::decode(&body),
            Err(DecodeError::Invalid("trailing bytes"))
        );
    }

    #[test]
    fn retained_f09_real_core_workloads_roundtrip_conservatively() {
        use flaminio_repl::msg::{Request, Response, ToReplica};
        use flaminio_repl::replica::{Action, ReplicaCore, UNCERTAINTY_BUDGET};

        // Preserve the exact safe in-memory audit probes, including same-epoch reconnect:
        // docs/audits/2026-11-12/repro/fenced.rs.txt. No disk or large payload is needed.
        for (chunks, writes_per_chunk) in [(1, 65_537), (2, 65_543)] {
            let mut core = ReplicaCore::default();
            for chunk in 0..chunks {
                core.add_chunk(1, chunk, 262_144).unwrap();
            }
            let mut message = ToReplica {
                vol: 1,
                epoch: 1,
                session: 1,
                req: 1,
                body: Request::<()>::Hello {
                    chunks: (0..chunks).collect(),
                },
            };
            assert!(matches!(core.handle(&message), Action::Fence { .. }));
            for chunk in 0..chunks {
                for i in 0..writes_per_chunk {
                    message.req += 1;
                    message.body = Request::Write {
                        chunk,
                        ext: Extent::new(i * 2, 1),
                        data: (),
                        version: Version {
                            epoch: 1,
                            seq: message.req,
                        },
                        prune: None,
                    };
                    assert!(matches!(core.handle(&message), Action::Write { .. }));
                }
            }
            let stats = core.uncertainty_stats(1);
            assert!(stats.chunks + stats.records <= UNCERTAINTY_BUDGET as usize);
            message.session = 2;
            message.req += 1;
            message.body = Request::Hello {
                chunks: (0..chunks).collect(),
            };
            let Action::Fence {
                reply: Response::Fenced { uncertain },
                ..
            } = core.handle(&message)
            else {
                panic!("same-epoch reconnect did not return Fenced");
            };
            let frame = Frame::Fenced {
                req: message.req,
                uncertain,
            };
            let encoded = frame.encode_checked().unwrap();
            assert!(encoded.len() - 4 <= MAX_BODY);
            let decoded = Frame::decode(&encoded[4..]).unwrap();
            assert_eq!(decoded, frame);
            let Frame::Fenced { uncertain, .. } = decoded else {
                unreachable!()
            };
            assert_eq!(uncertain.len(), chunks as usize);
            assert!(uncertain.iter().map(|(_, exts)| exts.len()).sum::<usize>() <= MAX_UNCERTAIN);
            for (chunk, (reported_chunk, exts)) in (0..chunks).zip(&uncertain) {
                assert_eq!(*reported_chunk, chunk);
                assert_eq!(exts, &[Extent::new(0, 262_144)]);
                // Independent exact write shadow: every originally affected block remains
                // covered, including those retained after the first coarsening.
                for i in 0..writes_per_chunk {
                    let block = i * 2;
                    assert!(
                        exts.iter()
                            .any(|ext| ext.start <= block && block < ext.start + ext.len)
                    );
                }
            }
        }
    }

    /// Every truncation of every frame is refused without a panic.
    #[test]
    fn truncations_are_refused() {
        for f in samples() {
            let b = f.encode();
            for cut in 4..b.len() {
                assert!(
                    Frame::decode(&b[4..cut]).is_err(),
                    "frame {:?}... accepted when cut at {cut}",
                    &b[4..b.len().min(24)]
                );
            }
        }
    }

    #[test]
    fn malformed_frames_are_refused() {
        let bad: Vec<Vec<u8>> = vec![
            vec![200],                                                      // unknown type
            [vec![tag::OK], 1u64.to_le_bytes().to_vec(), vec![0]].concat(), // trailing byte
            // A write whose payload is one byte short of its extent.
            {
                let mut b = samples()[2].encode()[4..].to_vec();
                b.pop();
                b
            },
            // An empty extent, and one that wraps.
            [
                vec![tag::READ],
                1u64.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
            ]
            .concat(),
            [
                vec![tag::READ],
                1u64.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                u32::MAX.to_le_bytes().to_vec(),
                2u32.to_le_bytes().to_vec(),
            ]
            .concat(),
            // An extent longer than an I/O may be.
            [
                vec![tag::READ],
                1u64.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                (MAX_BLOCKS + 1).to_le_bytes().to_vec(),
            ]
            .concat(),
            // A probe and a digest request longer than their limits.
            [
                vec![tag::MAPPED],
                1u64.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                (MAX_MAPPED_BLOCKS + 1).to_le_bytes().to_vec(),
            ]
            .concat(),
            [
                vec![tag::DIGEST],
                1u64.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                (MAX_DIGEST_BLOCKS + 1).to_le_bytes().to_vec(),
            ]
            .concat(),
            // A bad prune flag.
            {
                let mut b = samples()[3].encode()[4..].to_vec();
                b[1 + 8 + 4 + 8 + 16] = 7;
                b
            },
            // Data that is not whole blocks.
            [vec![tag::DATA], 1u64.to_le_bytes().to_vec(), vec![0; 100]].concat(),
            // A Fenced reply claiming more extents than allowed.
            [
                vec![tag::FENCED],
                1u64.to_le_bytes().to_vec(),
                1u32.to_le_bytes().to_vec(),
                0u32.to_le_bytes().to_vec(),
                ((MAX_UNCERTAIN + 1) as u32).to_le_bytes().to_vec(),
            ]
            .concat(),
        ];
        for b in bad {
            assert!(
                Frame::decode(&b).is_err(),
                "accepted {:?}",
                &b[..b.len().min(24)]
            );
        }
        assert_eq!(
            Frame::body_len(((MAX_BODY + 1) as u32).to_le_bytes()),
            Err(DecodeError::TooLarge(MAX_BODY + 1))
        );
        assert_eq!(Frame::body_len([0; 4]), Err(DecodeError::Truncated));
    }
}
