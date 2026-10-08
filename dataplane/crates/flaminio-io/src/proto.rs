//! The replication protocol between a volume target and a remote replica (MVP step 5).
//!
//! Frames over one TCP connection: `[u32 body length][u8 type][body]`, little-endian. The
//! target sends a request and reads exactly one reply before sending the next, so requests are
//! applied in the order they are sent. Every decoder treats its input as untrusted.
//!
//! Not yet: TLS and authentication (ARCHITECTURE §15.2). The lab restricts the port by source
//! address instead.

use std::io::{self, Read, Write};

use flaminio_format::Uuid;

/// Largest frame accepted, so a peer cannot make us allocate without bound.
pub const MAX_FRAME: usize = 64 << 20;
/// Granularity of digests and resync: 64 KiB.
pub const UNIT: u64 = 64 << 10;

/// One write inside a [`Msg::Write`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteItem {
    /// Chunk index in the volume.
    pub chunk: u64,
    /// Byte offset within the chunk.
    pub offset: u64,
    /// Target write sequence number.
    pub write_seq: u64,
    /// Data, a multiple of 4 KiB.
    pub data: Vec<u8>,
}

/// Protocol messages. Requests go target → replica; each gets exactly one reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// Request: start of a session. The replica creates missing chunks and fences the volume
    /// at `epoch`. Reply: `Ok` or `Error`.
    Hello {
        volume: Uuid,
        epoch: u64,
        size: u64,
        chunk_size: u64,
    },
    /// Request: writes, committed as one group. Reply: `WriteResult`.
    Write { epoch: u64, items: Vec<WriteItem> },
    /// Request: discard a range of a chunk (same semantics as the target's store). Reply: `Ok`.
    Discard {
        epoch: u64,
        chunk: u64,
        offset: u64,
        len: u64,
    },
    /// Request: CRC-32C of each 64 KiB unit in a range of a chunk. Reply: `Digests`.
    Digest {
        epoch: u64,
        chunk: u64,
        first_unit: u64,
        count: u32,
    },
    /// Reply: success.
    Ok,
    /// Reply: per write item, 0 or a negative errno.
    WriteResult { statuses: Vec<i32> },
    /// Reply: one CRC per unit.
    Digests { crcs: Vec<u32> },
    /// Reply: the request failed.
    Error { message: String },
}

const HELLO: u8 = 1;
const WRITE: u8 = 2;
const DIGEST: u8 = 3;
const DISCARD: u8 = 4;
const OK: u8 = 0x80;
const WRITE_RESULT: u8 = 0x81;
const DIGESTS: u8 = 0x82;
const ERROR: u8 = 0xFF;

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        if n > self.0.len() {
            return Err(invalid("truncated message"));
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }
}

impl Msg {
    fn kind(&self) -> u8 {
        match self {
            Msg::Hello { .. } => HELLO,
            Msg::Write { .. } => WRITE,
            Msg::Digest { .. } => DIGEST,
            Msg::Discard { .. } => DISCARD,
            Msg::Ok => OK,
            Msg::WriteResult { .. } => WRITE_RESULT,
            Msg::Digests { .. } => DIGESTS,
            Msg::Error { .. } => ERROR,
        }
    }

    /// Encodes the whole frame.
    pub fn encode(&self) -> Vec<u8> {
        let mut b = vec![0u8; 5];
        b[4] = self.kind();
        match self {
            Msg::Hello {
                volume,
                epoch,
                size,
                chunk_size,
            } => {
                b.extend_from_slice(&volume.0);
                for v in [epoch, size, chunk_size] {
                    b.extend_from_slice(&v.to_le_bytes());
                }
            }
            Msg::Write { epoch, items } => {
                b.extend_from_slice(&epoch.to_le_bytes());
                b.extend_from_slice(&(items.len() as u32).to_le_bytes());
                for it in items {
                    for v in [it.chunk, it.offset, it.write_seq] {
                        b.extend_from_slice(&v.to_le_bytes());
                    }
                    b.extend_from_slice(&(it.data.len() as u32).to_le_bytes());
                    b.extend_from_slice(&it.data);
                }
            }
            Msg::Digest {
                epoch,
                chunk,
                first_unit,
                count,
            } => {
                for v in [epoch, chunk, first_unit] {
                    b.extend_from_slice(&v.to_le_bytes());
                }
                b.extend_from_slice(&count.to_le_bytes());
            }
            Msg::Discard {
                epoch,
                chunk,
                offset,
                len,
            } => {
                for v in [epoch, chunk, offset, len] {
                    b.extend_from_slice(&v.to_le_bytes());
                }
            }
            Msg::Ok => {}
            Msg::WriteResult { statuses } => {
                b.extend_from_slice(&(statuses.len() as u32).to_le_bytes());
                statuses
                    .iter()
                    .for_each(|s| b.extend_from_slice(&s.to_le_bytes()));
            }
            Msg::Digests { crcs } => {
                b.extend_from_slice(&(crcs.len() as u32).to_le_bytes());
                crcs.iter()
                    .for_each(|c| b.extend_from_slice(&c.to_le_bytes()));
            }
            Msg::Error { message } => b.extend_from_slice(message.as_bytes()),
        }
        let body = (b.len() - 4) as u32;
        b[..4].copy_from_slice(&body.to_le_bytes());
        b
    }

    /// Decodes a frame body (type byte included).
    pub fn decode(frame: &[u8]) -> io::Result<Msg> {
        let (&kind, body) = frame.split_first().ok_or_else(|| invalid("empty frame"))?;
        let mut c = Cursor(body);
        let msg = match kind {
            HELLO => Msg::Hello {
                volume: Uuid(c.take(16)?.try_into().expect("16 bytes")),
                epoch: c.u64()?,
                size: c.u64()?,
                chunk_size: c.u64()?,
            },
            WRITE => {
                let epoch = c.u64()?;
                let n = c.u32()? as usize;
                let mut items = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    let (chunk, offset, write_seq) = (c.u64()?, c.u64()?, c.u64()?);
                    let len = c.u32()? as usize;
                    items.push(WriteItem {
                        chunk,
                        offset,
                        write_seq,
                        data: c.take(len)?.to_vec(),
                    });
                }
                Msg::Write { epoch, items }
            }
            DIGEST => Msg::Digest {
                epoch: c.u64()?,
                chunk: c.u64()?,
                first_unit: c.u64()?,
                count: c.u32()?,
            },
            DISCARD => Msg::Discard {
                epoch: c.u64()?,
                chunk: c.u64()?,
                offset: c.u64()?,
                len: c.u64()?,
            },
            OK => Msg::Ok,
            WRITE_RESULT => {
                let n = c.u32()? as usize;
                let raw = c.take(n.checked_mul(4).ok_or_else(|| invalid("count overflows"))?)?;
                Msg::WriteResult {
                    statuses: raw
                        .chunks_exact(4)
                        .map(|s| i32::from_le_bytes(s.try_into().expect("4")))
                        .collect(),
                }
            }
            DIGESTS => {
                let n = c.u32()? as usize;
                let raw = c.take(n.checked_mul(4).ok_or_else(|| invalid("count overflows"))?)?;
                Msg::Digests {
                    crcs: raw
                        .chunks_exact(4)
                        .map(|s| u32::from_le_bytes(s.try_into().expect("4")))
                        .collect(),
                }
            }
            ERROR => Msg::Error {
                message: String::from_utf8_lossy(c.take(c.0.len())?).into_owned(),
            },
            k => return Err(invalid(&format!("unknown message type {k:#x}"))),
        };
        if !c.0.is_empty() {
            return Err(invalid("trailing bytes in message"));
        }
        Ok(msg)
    }
}

/// Reads one message.
pub fn read_msg(r: &mut impl Read) -> io::Result<Msg> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(invalid("frame length out of range"));
    }
    let mut frame = vec![0u8; len];
    r.read_exact(&mut frame)?;
    Msg::decode(&frame)
}

/// Writes one message.
pub fn write_msg(w: &mut impl Write, msg: &Msg) -> io::Result<()> {
    let frame = msg.encode();
    if frame.len() - 4 > MAX_FRAME {
        return Err(invalid("message too large"));
    }
    w.write_all(&frame)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(m: Msg) {
        let mut buf = Vec::new();
        write_msg(&mut buf, &m).unwrap();
        assert_eq!(read_msg(&mut &buf[..]).unwrap(), m);
    }

    #[test]
    fn every_message_roundtrips() {
        roundtrip(Msg::Hello {
            volume: Uuid([3; 16]),
            epoch: 2,
            size: 1 << 36,
            chunk_size: 1 << 30,
        });
        roundtrip(Msg::Write {
            epoch: 2,
            items: vec![
                WriteItem {
                    chunk: 1,
                    offset: 4096,
                    write_seq: 9,
                    data: vec![7; 8192],
                },
                WriteItem {
                    chunk: 5,
                    offset: 0,
                    write_seq: 10,
                    data: vec![],
                },
            ],
        });
        roundtrip(Msg::Digest {
            epoch: 2,
            chunk: 3,
            first_unit: 100,
            count: 256,
        });
        roundtrip(Msg::Discard {
            epoch: 2,
            chunk: 1,
            offset: 65536,
            len: 1 << 20,
        });
        roundtrip(Msg::Ok);
        roundtrip(Msg::WriteResult {
            statuses: vec![0, -5, 0],
        });
        roundtrip(Msg::Digests {
            crcs: vec![1, 2, 3],
        });
        roundtrip(Msg::Error {
            message: "stale epoch".into(),
        });
    }

    #[test]
    fn rejects_malformed_frames() {
        let good = Msg::Write {
            epoch: 1,
            items: vec![WriteItem {
                chunk: 0,
                offset: 0,
                write_seq: 1,
                data: vec![1; 16],
            }],
        }
        .encode();
        // Every truncation fails cleanly.
        for cut in 5..good.len() {
            let mut f = good[..cut].to_vec();
            let body = (cut - 4) as u32;
            f[..4].copy_from_slice(&body.to_le_bytes());
            assert!(read_msg(&mut &f[..]).is_err(), "cut at {cut}");
        }
        // Oversized and zero lengths, unknown types, trailing bytes.
        assert!(read_msg(&mut &((MAX_FRAME as u32 + 1).to_le_bytes())[..]).is_err());
        assert!(read_msg(&mut &[0u8, 0, 0, 0][..]).is_err());
        assert!(Msg::decode(&[0x42]).is_err());
        assert!(Msg::decode(&[OK, 1]).is_err());
        // A huge item count with no data must not allocate or panic.
        let mut f = vec![WRITE];
        f.extend_from_slice(&1u64.to_le_bytes());
        f.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(Msg::decode(&f).is_err());
    }
}
