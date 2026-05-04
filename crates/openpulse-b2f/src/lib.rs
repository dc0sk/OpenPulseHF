//! B2F (Binary File Transfer) protocol implementation for Winlink.
//!
//! B2F is the application-layer protocol Winlink uses for email transfer over
//! any ARQ TNC data connection (ARDOP, PACTOR, VARA, etc.).

pub mod banner;
pub mod compress;
pub mod frame;
pub mod header;
mod session;

pub use compress::{compress_gzip, compress_lzhuf, decompress_gzip, decompress_lzhuf};
pub use frame::{B2fFrame, FsAnswer, ProposalType};
pub use header::{AttachmentInfo, WlHeader};
pub use session::{B2fSession, SessionRole};

#[derive(Debug, thiserror::Error)]
pub enum B2fError {
    #[error("invalid banner: {0}")]
    InvalidBanner(String),
    #[error("invalid frame: {0}")]
    InvalidFrame(String),
    #[error("invalid header: {0}")]
    InvalidHeader(String),
    #[error("compression error: {0}")]
    Compression(String),
    #[error("invalid session state")]
    InvalidState,
}
