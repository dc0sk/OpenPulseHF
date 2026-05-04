use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArdopError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("modem error: {0}")]
    Modem(#[from] openpulse_core::error::ModemError),
    #[error("background task panicked")]
    Join,
}
