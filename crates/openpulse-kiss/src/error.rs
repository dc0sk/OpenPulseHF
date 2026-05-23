//! Error types for the KISS TNC server.

#[derive(Debug, thiserror::Error)]
pub enum KissTncError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("modem error: {0}")]
    Modem(#[from] openpulse_core::error::ModemError),
    #[error("task join error")]
    Join,
    #[error("KISS frame body too large: {len} bytes (max {max})")]
    FrameTooLarge { len: usize, max: usize },
}
