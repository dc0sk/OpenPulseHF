use thiserror::Error;

#[derive(Debug, Error)]
pub enum PttError {
    #[error("serial port error: {0}")]
    Serial(String),
    #[error("rigctld connection error: {0}")]
    Rigctld(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Error type for full rig CAT control operations.
#[derive(Debug, Error)]
pub enum RadioError {
    #[error("rigctld I/O error: {0}")]
    RigctldIo(#[from] std::io::Error),
    #[error("rigctld protocol error: {0}")]
    RigctldProtocol(String),
    #[error("parse error: {0}")]
    Parse(String),
    /// The rig definition does not include the requested operation.
    #[error("operation not supported by this rig: {0}")]
    Unsupported(&'static str),
    /// Generic serial CAT error (I/O, protocol, or template expansion failure).
    #[error("generic CAT error: {0}")]
    GenericCat(String),
}
