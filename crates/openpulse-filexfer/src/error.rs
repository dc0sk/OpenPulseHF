//! Error type for the file-transfer protocol.

use thiserror::Error;

/// Errors from `FxFrame` codec and session state machines.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FxError {
    /// Frame ended before a required field was read.
    #[error("truncated frame: needed {needed} bytes, had {had}")]
    Truncated { needed: usize, had: usize },
    /// The 4-byte `OPFX` magic was absent.
    #[error("bad magic (not an OPFX frame)")]
    BadMagic,
    /// Protocol version byte the codec does not understand.
    #[error("unsupported protocol version {0:#04x}")]
    UnsupportedVersion(u8),
    /// Frame-type byte the codec does not understand.
    #[error("unknown frame type {0:#04x}")]
    UnknownFrameType(u8),
    /// A length-prefixed string exceeded its field's maximum.
    #[error("field '{field}' too long: {len} > {max}")]
    FieldTooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },
    /// A string field was not valid UTF-8.
    #[error("field '{0}' is not valid UTF-8")]
    InvalidUtf8(&'static str),
    /// A `reason`/`status` enum byte was out of range.
    #[error("invalid enum byte {byte:#04x} for {field}")]
    InvalidEnum { field: &'static str, byte: u8 },
    /// `block_size` outside the protocol-legal window.
    #[error("block_size {0} outside {min}..={max}", min = crate::MIN_BLOCK_SIZE, max = crate::MAX_BLOCK_SIZE)]
    BlockSizeOutOfRange(u32),
}
