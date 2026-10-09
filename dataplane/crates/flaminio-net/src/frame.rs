//! Frames: `[u32 body length][u8 type][body]`, little-endian.
//!
//! A connection carries one session (see [`crate::session`]): `Hello` names the volume, the
//! epoch and the session once, and later requests carry only a request id.

use flaminio_repl::extent::Extent;
use flaminio_repl::types::Version;

use crate::BLOCK;

/// Most blocks in one write, read or copy: 1 MiB.
pub const MAX_BLOCKS: u32 = 256;
/// Largest body accepted: a full-size write and its header.
pub const MAX_BODY: usize = MAX_BLOCKS as usize * BLOCK + 128;
/// Most extents in one `Fenced` reply.
pub const MAX_UNCERTAIN: usize = 1 << 16;
/// Most chunks of one volume on one node, as listed in `Hello`.
pub const MAX_CHUNKS: usize = 1 << 16;

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
        data: Vec<u8>,
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
    Copy {
        req: u64,
        chunk: u32,
        ext: Extent,
        data: Vec<u8>,
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

mod tag {
    pub const HELLO: u8 = 1;
    pub const HEARTBEAT: u8 = 2;
    pub const WRITE: u8 = 3;
    pub const READ: u8 = 4;
    pub const BEGIN_RESYNC: u8 = 5;
    pub const COPY: u8 = 6;
    pub const FENCED: u8 = 64;
    pub const OK: u8 = 65;
    pub const DATA: u8 = 66;
    pub const STALE: u8 = 67;
    pub const REFUSED: u8 = 68;
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
                let data = r.data(ext)?;
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
            tag::COPY => {
                let req = r.u64()?;
                let chunk = r.u32()?;
                let ext = r.ext()?;
                let data = r.data(ext)?;
                Frame::Copy {
                    req,
                    chunk,
                    ext,
                    data,
                }
            }
            tag::FENCED => {
                let req = r.u64()?;
                let n = r.u32()? as usize;
                let mut uncertain = Vec::new();
                let mut total = 0usize;
                for _ in 0..n {
                    let chunk = r.u32()?;
                    let m = r.u32()? as usize;
                    total += m;
                    if total > MAX_UNCERTAIN {
                        return Err(DecodeError::Invalid("too many uncertain extents"));
                    }
                    let mut exts = Vec::with_capacity(m.min(r.0.len() / 8));
                    for _ in 0..m {
                        exts.push(r.any_ext()?);
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
                data: (0..2 * BLOCK).map(|i| i as u8).collect(),
            },
            Frame::Write {
                req: 4,
                chunk: 5,
                ext: Extent { start: 0, len: 1 },
                version: Version { epoch: 7, seq: 41 },
                prune: None,
                data: vec![1; BLOCK],
            },
            Frame::Read {
                req: 5,
                chunk: 1,
                ext,
            },
            Frame::BeginResync { req: 6, chunk: 4 },
            Frame::Copy {
                req: 7,
                chunk: 4,
                ext,
                data: vec![2; 2 * BLOCK],
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
            let b = f.encode();
            let n = Frame::body_len(b[..4].try_into().unwrap()).unwrap();
            assert_eq!(n, b.len() - 4);
            assert_eq!(Frame::decode(&b[4..]).unwrap(), f);
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
