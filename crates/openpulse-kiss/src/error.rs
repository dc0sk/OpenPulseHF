//! Error types for the KISS TNC server.

#[derive(Debug, thiserror::Error)]
pub enum KissTncError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("task join error")]
    Join,
}
