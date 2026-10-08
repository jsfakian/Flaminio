use crate::page::PageType;

/// Everything that can be wrong with on-disk bytes or format parameters.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    /// The input is shorter than the structure being decoded.
    #[error("truncated input: need {need} bytes, have {have}")]
    Truncated {
        /// Bytes required.
        need: usize,
        /// Bytes available.
        have: usize,
    },
    /// The magic number does not match.
    #[error("bad magic")]
    BadMagic,
    /// A CRC does not match the content.
    #[error("checksum mismatch")]
    Checksum,
    /// The structure was written by an unsupported format version.
    #[error("unsupported format version {major}.{minor}")]
    UnsupportedVersion {
        /// Major version found.
        major: u16,
        /// Minor version found.
        minor: u16,
    },
    /// The disk uses incompatible features this implementation does not know.
    #[error("unsupported incompatible features {0:#x}")]
    IncompatFeatures(u64),
    /// The structure belongs to a different format of this device.
    #[error("format nonce mismatch")]
    NonceMismatch,
    /// A page was found at a different location than the one it was written for.
    #[error("misdirected page: expected page {expected}, found page {found}")]
    Misdirected {
        /// Page number that was read.
        expected: u64,
        /// Page number recorded inside the page.
        found: u64,
    },
    /// The page has a different type than expected.
    #[error("unexpected page type: expected {expected:?}, found {found}")]
    WrongPageType {
        /// Type the caller asked for.
        expected: PageType,
        /// Raw type found on disk.
        found: u16,
    },
    /// A field has a value that is out of range or inconsistent.
    #[error("invalid {field}: {reason}")]
    Invalid {
        /// Name of the offending field.
        field: &'static str,
        /// Human-readable explanation.
        reason: String,
    },
    /// A journal record type is unknown and not marked optional.
    #[error("unknown mandatory journal record type {0}")]
    UnknownRecord(u16),
    /// The device is too small to format.
    #[error("device too small: {size} bytes, need at least {min}")]
    DeviceTooSmall {
        /// Device size.
        size: u64,
        /// Minimum required.
        min: u64,
    },
    /// A journal batch has no room for another record.
    #[error("journal batch is full")]
    BatchFull,
}

impl FormatError {
    pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }
}
