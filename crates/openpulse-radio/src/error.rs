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
