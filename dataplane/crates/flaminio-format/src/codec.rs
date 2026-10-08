//! Little-endian field access. Fixed-offset helpers are for buffers whose length the caller has
//! already checked; [`Reader`] is for variable-length, untrusted input.

use crate::error::FormatError;

pub(crate) fn get_u16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(b[off..off + 2].try_into().expect("2 bytes"))
}

pub(crate) fn get_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().expect("4 bytes"))
}

pub(crate) fn get_u64(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().expect("8 bytes"))
}

pub(crate) fn get_16(b: &[u8], off: usize) -> [u8; 16] {
    b[off..off + 16].try_into().expect("16 bytes")
}

pub(crate) fn put_u16(b: &mut [u8], off: usize, v: u16) {
    b[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_u64(b: &mut [u8], off: usize, v: u64) {
    b[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

/// Bounds-checked sequential reader over untrusted bytes.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&end| end <= self.buf.len())
            .ok_or(FormatError::Truncated {
                need: self.pos.saturating_add(n),
                have: self.buf.len(),
            })?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, FormatError> {
        Ok(get_u32(self.take(4)?, 0))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, FormatError> {
        Ok(get_u64(self.take(8)?, 0))
    }

    pub(crate) fn bytes16(&mut self) -> Result<[u8; 16], FormatError> {
        Ok(get_16(self.take(16)?, 0))
    }

    pub(crate) fn u32_vec(&mut self, n: usize) -> Result<Vec<u32>, FormatError> {
        let bytes = self.take(
            n.checked_mul(4)
                .ok_or_else(|| FormatError::invalid("length", "checksum count overflows"))?,
        )?;
        Ok(bytes.chunks_exact(4).map(|c| get_u32(c, 0)).collect())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pos == self.buf.len()
    }
}

/// Appending little-endian writer.
pub(crate) struct Writer<'a> {
    out: &'a mut Vec<u8>,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(out: &'a mut Vec<u8>) -> Self {
        Self { out }
    }

    pub(crate) fn u16(&mut self, v: u16) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn bytes(&mut self, b: &[u8]) {
        self.out.extend_from_slice(b);
    }

    pub(crate) fn u32s(&mut self, vs: &[u32]) {
        for v in vs {
            self.u32(*v);
        }
    }
}

pub(crate) const fn align_up(v: u64, align: u64) -> u64 {
    v.div_ceil(align) * align
}
