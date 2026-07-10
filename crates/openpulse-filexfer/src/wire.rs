//! `OPFX` wire frames: binary encode/decode. All integers big-endian; strings are `len(u8) | UTF-8`.

use crate::error::FxError;
use crate::offer::FileOffer;

/// 4-byte frame magic — "OpenPulse File Xfer". `compression::unpack()` passes it through untouched.
pub const FILEXFER_MAGIC: [u8; 4] = *b"OPFX";
/// Protocol version carried in every frame header.
pub const FILEXFER_VERSION: u8 = 0x01;

/// Shared reason codes for `FileReject` / `FileCancel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Reason {
    OperatorDeclined = 0,
    FeatureDisabled = 1,
    TooLarge = 2,
    QuotaExceeded = 3,
    Busy = 4,
    UntrustedPeer = 5,
    Timeout = 6,
    UnsupportedVersion = 7,
    OperatorCancel = 8,
    Stall = 9,
}

impl Reason {
    fn from_u8(b: u8) -> Result<Self, FxError> {
        Ok(match b {
            0 => Self::OperatorDeclined,
            1 => Self::FeatureDisabled,
            2 => Self::TooLarge,
            3 => Self::QuotaExceeded,
            4 => Self::Busy,
            5 => Self::UntrustedPeer,
            6 => Self::Timeout,
            7 => Self::UnsupportedVersion,
            8 => Self::OperatorCancel,
            9 => Self::Stall,
            _ => {
                return Err(FxError::InvalidEnum {
                    field: "reason",
                    byte: b,
                })
            }
        })
    }
}

/// Verification outcome carried in `FileComplete`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompleteStatus {
    VerifiedOk = 0,
    HashMismatch = 1,
    SignatureInvalid = 2,
    SizeMismatch = 3,
}

impl CompleteStatus {
    /// True only for a fully verified transfer (hash + signature).
    pub fn is_ok(self) -> bool {
        matches!(self, Self::VerifiedOk)
    }

    fn from_u8(b: u8) -> Result<Self, FxError> {
        Ok(match b {
            0 => Self::VerifiedOk,
            1 => Self::HashMismatch,
            2 => Self::SignatureInvalid,
            3 => Self::SizeMismatch,
            _ => {
                return Err(FxError::InvalidEnum {
                    field: "status",
                    byte: b,
                })
            }
        })
    }
}

/// A decoded file-transfer protocol frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FxFrame {
    /// Sender → receiver: metadata + embedded signed manifest fields.
    FileOffer(FileOffer),
    /// Receiver → sender: accept; `have_bitmap` marks blocks already held (empty in v1, resume in E).
    FileAccept {
        transfer_id: u32,
        have_bitmap: Vec<u8>,
    },
    /// Receiver → sender: decline the offer.
    FileReject { transfer_id: u32, reason: Reason },
    /// Sender → receiver: one packed data block (this whole frame is one SAR segment).
    FileData {
        transfer_id: u32,
        block_index: u16,
        packed: Vec<u8>,
    },
    /// Receiver → sender: per-block delivery status + missing-fragment bitmap.
    BlockAck {
        transfer_id: u32,
        block_index: u16,
        complete: bool,
        missing_frag_bitmap: Vec<u8>,
    },
    /// Receiver → sender: terminal verification result (+ countersignature when verified).
    FileComplete {
        transfer_id: u32,
        status: CompleteStatus,
        countersignature: [u8; 64],
    },
    /// Either side: abort an in-flight transfer.
    FileCancel { transfer_id: u32, reason: Reason },
}

impl FxFrame {
    /// The frame's `transfer_id` (present on every frame).
    pub fn transfer_id(&self) -> u32 {
        match self {
            Self::FileOffer(o) => o.transfer_id,
            Self::FileAccept { transfer_id, .. }
            | Self::FileReject { transfer_id, .. }
            | Self::FileData { transfer_id, .. }
            | Self::BlockAck { transfer_id, .. }
            | Self::FileComplete { transfer_id, .. }
            | Self::FileCancel { transfer_id, .. } => *transfer_id,
        }
    }

    /// The 1-byte frame-type discriminant.
    pub fn frame_type(&self) -> u8 {
        match self {
            Self::FileOffer(_) => 0x01,
            Self::FileAccept { .. } => 0x02,
            Self::FileReject { .. } => 0x03,
            Self::FileData { .. } => 0x04,
            Self::BlockAck { .. } => 0x05,
            Self::FileComplete { .. } => 0x06,
            Self::FileCancel { .. } => 0x07,
        }
    }

    /// Encode to `MAGIC | ver | type | body`.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16);
        out.extend_from_slice(&FILEXFER_MAGIC);
        out.push(FILEXFER_VERSION);
        out.push(self.frame_type());
        match self {
            Self::FileOffer(o) => o.encode_body(&mut out),
            Self::FileAccept {
                transfer_id,
                have_bitmap,
            } => {
                out.extend_from_slice(&transfer_id.to_be_bytes());
                out.extend_from_slice(&(have_bitmap.len() as u16).to_be_bytes());
                out.extend_from_slice(have_bitmap);
            }
            Self::FileReject {
                transfer_id,
                reason,
            } => {
                out.extend_from_slice(&transfer_id.to_be_bytes());
                out.push(*reason as u8);
            }
            Self::FileData {
                transfer_id,
                block_index,
                packed,
            } => {
                out.extend_from_slice(&transfer_id.to_be_bytes());
                out.extend_from_slice(&block_index.to_be_bytes());
                out.extend_from_slice(packed);
            }
            Self::BlockAck {
                transfer_id,
                block_index,
                complete,
                missing_frag_bitmap,
            } => {
                out.extend_from_slice(&transfer_id.to_be_bytes());
                out.extend_from_slice(&block_index.to_be_bytes());
                out.push(*complete as u8);
                out.push(missing_frag_bitmap.len() as u8);
                out.extend_from_slice(missing_frag_bitmap);
            }
            Self::FileComplete {
                transfer_id,
                status,
                countersignature,
            } => {
                out.extend_from_slice(&transfer_id.to_be_bytes());
                out.push(*status as u8);
                out.extend_from_slice(countersignature);
            }
            Self::FileCancel {
                transfer_id,
                reason,
            } => {
                out.extend_from_slice(&transfer_id.to_be_bytes());
                out.push(*reason as u8);
            }
        }
        out
    }

    /// Decode a `MAGIC | ver | type | body` frame.
    pub fn decode(bytes: &[u8]) -> Result<Self, FxError> {
        let mut r = Reader::new(bytes);
        if r.take(4)? != FILEXFER_MAGIC {
            return Err(FxError::BadMagic);
        }
        let ver = r.u8()?;
        if ver != FILEXFER_VERSION {
            return Err(FxError::UnsupportedVersion(ver));
        }
        let ty = r.u8()?;
        Ok(match ty {
            0x01 => Self::FileOffer(FileOffer::decode_body(&mut r)?),
            0x02 => {
                let transfer_id = r.u32()?;
                let have_len = r.u16()? as usize;
                let have_bitmap = r.take(have_len)?.to_vec();
                Self::FileAccept {
                    transfer_id,
                    have_bitmap,
                }
            }
            0x03 => Self::FileReject {
                transfer_id: r.u32()?,
                reason: Reason::from_u8(r.u8()?)?,
            },
            0x04 => {
                let transfer_id = r.u32()?;
                let block_index = r.u16()?;
                Self::FileData {
                    transfer_id,
                    block_index,
                    packed: r.rest().to_vec(),
                }
            }
            0x05 => {
                let transfer_id = r.u32()?;
                let block_index = r.u16()?;
                let complete = r.u8()? != 0;
                let missing_len = r.u8()? as usize;
                let missing_frag_bitmap = r.take(missing_len)?.to_vec();
                Self::BlockAck {
                    transfer_id,
                    block_index,
                    complete,
                    missing_frag_bitmap,
                }
            }
            0x06 => {
                let transfer_id = r.u32()?;
                let status = CompleteStatus::from_u8(r.u8()?)?;
                let countersignature = r.array::<64>()?;
                Self::FileComplete {
                    transfer_id,
                    status,
                    countersignature,
                }
            }
            0x07 => Self::FileCancel {
                transfer_id: r.u32()?,
                reason: Reason::from_u8(r.u8()?)?,
            },
            other => return Err(FxError::UnknownFrameType(other)),
        })
    }
}

/// True if `bytes` begins with the OPFX magic (cheap dispatch at the receive seam).
pub fn is_filexfer_frame(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == FILEXFER_MAGIC
}

/// Bounds-checked, panic-free reader over a frame body.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], FxError> {
        let end = self.pos.checked_add(n).ok_or(FxError::Truncated {
            needed: n,
            had: self.buf.len().saturating_sub(self.pos),
        })?;
        if end > self.buf.len() {
            return Err(FxError::Truncated {
                needed: n,
                had: self.buf.len().saturating_sub(self.pos),
            });
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, FxError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, FxError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, FxError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, FxError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_be_bytes(a))
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], FxError> {
        let b = self.take(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(b);
        Ok(a)
    }

    /// `len(u8) | UTF-8 bytes`, bounded by `max`.
    pub(crate) fn string(&mut self, field: &'static str, max: usize) -> Result<String, FxError> {
        let len = self.u8()? as usize;
        if len > max {
            return Err(FxError::FieldTooLong { field, len, max });
        }
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| FxError::InvalidUtf8(field))
    }

    pub(crate) fn rest(&mut self) -> &'a [u8] {
        let slice = &self.buf[self.pos..];
        self.pos = self.buf.len();
        slice
    }
}

/// `len(u8) | UTF-8 bytes` writer, truncating over-long strings at the byte boundary `max`.
pub(crate) fn write_string(out: &mut Vec<u8>, s: &str, max: usize) {
    let mut bytes = s.as_bytes();
    if bytes.len() > max {
        // Truncate on a char boundary so the field always decodes as valid UTF-8.
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        bytes = &s.as_bytes()[..end];
    }
    out.push(bytes.len() as u8);
    out.extend_from_slice(bytes);
}
