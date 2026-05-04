//! In-memory loopback audio backend.
//!
//! Useful for integration tests and CI: write samples to the output and read
//! them back from the input without any real audio hardware.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;

// ── Shared sample buffer ──────────────────────────────────────────────────────

type Buf = Arc<Mutex<VecDeque<f32>>>;

// ── LoopbackBackend ───────────────────────────────────────────────────────────

/// A virtual audio backend that routes output samples directly to input.
///
/// Both the input and output streams share the same sample buffer.  Samples
/// written via [`LoopbackOutputStream::write`] can immediately be read back
/// via [`LoopbackInputStream::read`].
pub struct LoopbackBackend {
    buf: Buf,
}

impl Default for LoopbackBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl LoopbackBackend {
    /// Create a new loopback backend with an empty buffer.
    pub fn new() -> Self {
        Self {
            buf: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Create a second [`LoopbackBackend`] that shares the same underlying buffer.
    ///
    /// Both instances read from and write to the same buffer — sharing is symmetric.
    /// Samples written by either instance are immediately readable by either instance,
    /// enabling two-engine tests without real audio hardware.
    pub fn clone_shared(&self) -> Self {
        Self {
            buf: Arc::clone(&self.buf),
        }
    }

    /// Drain all samples currently sitting in the shared buffer.
    ///
    /// Used by [`ChannelSimHarness`] to intercept TX samples before the RX engine
    /// reads them, so a channel model can be applied in between.
    pub fn drain_samples(&self) -> Vec<f32> {
        let mut guard = self.buf.lock().expect("loopback buffer poisoned");
        guard.drain(..).collect()
    }

    /// Inject samples into the shared buffer, making them available to the next `read()`.
    ///
    /// Used by [`ChannelSimHarness`] to deliver channel-processed samples to the RX engine.
    pub fn fill_samples(&self, samples: &[f32]) {
        let mut guard = self.buf.lock().expect("loopback buffer poisoned");
        guard.extend(samples.iter().copied());
    }
}

impl AudioBackend for LoopbackBackend {
    fn name(&self) -> &str {
        "Loopback"
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(vec![DeviceInfo {
            name: "loopback".to_string(),
            is_input: true,
            is_output: true,
            is_default: true,
            supported_sample_rates: vec![8000, 16000, 44100, 48000],
        }])
    }

    fn open_input(
        &self,
        _device: Option<&str>,
        _config: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        Ok(Box::new(LoopbackInputStream {
            buf: Arc::clone(&self.buf),
        }))
    }

    fn open_output(
        &self,
        _device: Option<&str>,
        _config: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        Ok(Box::new(LoopbackOutputStream {
            buf: Arc::clone(&self.buf),
        }))
    }
}

// ── Loopback input stream ─────────────────────────────────────────────────────

/// Reads samples that were previously written to the loopback output.
pub struct LoopbackInputStream {
    buf: Buf,
}

impl AudioInputStream for LoopbackInputStream {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        let mut guard = self.buf.lock().expect("loopback buffer poisoned");
        Ok(guard.drain(..).collect())
    }

    fn close(self: Box<Self>) {}
}

// ── Loopback output stream ────────────────────────────────────────────────────

/// Writes samples into the shared loopback buffer.
pub struct LoopbackOutputStream {
    buf: Buf,
}

impl AudioOutputStream for LoopbackOutputStream {
    fn write(&mut self, samples: &[f32]) -> Result<(), AudioError> {
        let mut guard = self.buf.lock().expect("loopback buffer poisoned");
        guard.extend(samples.iter().copied());
        Ok(())
    }

    fn flush(&mut self) -> Result<(), AudioError> {
        Ok(())
    }

    fn close(self: Box<Self>) {}
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::audio::AudioBackend;

    #[test]
    fn loopback_write_then_read() {
        let backend = LoopbackBackend::new();
        let cfg = AudioConfig::default();

        let mut out = backend.open_output(None, &cfg).unwrap();
        let mut inp = backend.open_input(None, &cfg).unwrap();

        out.write(&[0.1, 0.2, 0.3]).unwrap();
        out.flush().unwrap();

        let samples = inp.read().unwrap();
        assert_eq!(samples, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn loopback_list_devices() {
        let backend = LoopbackBackend::new();
        let devices = backend.list_devices().unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "loopback");
        assert!(devices[0].is_input && devices[0].is_output);
    }
}
