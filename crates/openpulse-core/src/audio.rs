use crate::error::AudioError;

// ── Device info ───────────────────────────────────────────────────────────────

/// Describes a physical or virtual audio device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// System name of the device.
    pub name: String,
    /// `true` when the device can capture audio.
    pub is_input: bool,
    /// `true` when the device can play back audio.
    pub is_output: bool,
    /// `true` when this is the system default device for its direction.
    pub is_default: bool,
    /// Non-exhaustive list of sample rates the device accepts.
    pub supported_sample_rates: Vec<u32>,
}

// ── Stream configuration ──────────────────────────────────────────────────────

/// Parameters used when opening an audio stream.
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Desired sample rate in Hz.
    pub sample_rate: u32,
    /// Number of channels (1 = mono is sufficient for radio work).
    pub channels: u16,
    /// Optional driver buffer size hint in frames.
    pub buffer_size: Option<u32>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 8000,
            channels: 1,
            buffer_size: None,
        }
    }
}

// ── Stream traits ─────────────────────────────────────────────────────────────

/// An open audio capture stream.
pub trait AudioInputStream {
    /// Block until at least one sample is available, then return all buffered
    /// samples normalised to `−1.0 … +1.0`.
    fn read(&mut self) -> Result<Vec<f32>, AudioError>;

    /// Release underlying resources.
    fn close(self: Box<Self>);
}

/// An open audio playback stream.
pub trait AudioOutputStream {
    /// Write `samples` (normalised `−1.0 … +1.0`) to the device.
    fn write(&mut self, samples: &[f32]) -> Result<(), AudioError>;

    /// Ensure all buffered samples have been submitted to the driver.
    fn flush(&mut self) -> Result<(), AudioError>;

    /// Release underlying resources.
    fn close(self: Box<Self>);
}

// ── Backend trait ─────────────────────────────────────────────────────────────

/// An audio subsystem backend (ALSA, PipeWire, CoreAudio, WASAPI, Loopback …).
pub trait AudioBackend: Send + Sync {
    /// Human-readable backend name.
    fn name(&self) -> &str;

    /// Enumerate all available devices.
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError>;

    /// Open a capture stream.  Pass `None` for `device` to use the default.
    fn open_input(
        &self,
        device: Option<&str>,
        config: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError>;

    /// Open a playback stream.  Pass `None` for `device` to use the default.
    fn open_output(
        &self,
        device: Option<&str>,
        config: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError>;
}
