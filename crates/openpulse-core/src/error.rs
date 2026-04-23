use thiserror::Error;

/// Errors produced by the modem layer.
#[derive(Debug, Error)]
pub enum ModemError {
    #[error("modulation failed: {0}")]
    Modulation(String),

    #[error("demodulation failed: {0}")]
    Demodulation(String),

    #[error("frame encoding/decoding error: {0}")]
    Frame(String),

    #[error("plugin not found for mode '{0}'")]
    PluginNotFound(String),

    #[error("configuration error: {0}")]
    Configuration(String),

    #[error("audio error: {0}")]
    Audio(String),
}

/// Errors produced by the audio layer.
#[derive(Debug, Error)]
pub enum AudioError {
    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("stream error: {0}")]
    Stream(String),

    #[error("configuration error: {0}")]
    Configuration(String),

    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),
}
