//! In-memory loopback audio backend.
//!
//! Useful for integration tests and CI: write samples to the output and read
//! them back from the input without any real audio hardware.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioIqOutputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;

// ── Shared sample buffer ──────────────────────────────────────────────────────

type Buf = Arc<Mutex<VecDeque<f32>>>;
type FrameQueue = Arc<Mutex<VecDeque<Vec<f32>>>>;
type IqBuf = Arc<Mutex<Vec<(f32, f32)>>>;

// ── LoopbackBackend ───────────────────────────────────────────────────────────

/// A virtual audio backend that routes output samples directly to input.
///
/// Both the input and output streams share the same sample buffer.  Samples
/// written via [`LoopbackOutputStream::write`] can immediately be read back
/// via [`LoopbackInputStream::read`].
pub struct LoopbackBackend {
    buf: Buf,
    frame_queue: FrameQueue,
    iq_buf: IqBuf,
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
            frame_queue: Arc::new(Mutex::new(VecDeque::new())),
            iq_buf: Arc::new(Mutex::new(Vec::new())),
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
            frame_queue: Arc::clone(&self.frame_queue),
            iq_buf: Arc::clone(&self.iq_buf),
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

    /// Enqueue individual sample frames for sequential per-frame reads.
    ///
    /// Each frame pushed here will be returned by one `read()` call on the input
    /// stream, in order.  Frames in the queue take priority over the flat `buf`.
    /// Useful for multi-attempt receive tests where each attempt must see exactly
    /// one frame's worth of samples.
    pub fn push_frame(&self, samples: &[f32]) {
        let mut guard = self
            .frame_queue
            .lock()
            .expect("loopback frame queue poisoned");
        guard.push_back(samples.to_vec());
    }

    /// Drain all I/Q pairs written via [`open_iq_output`](Self::open_iq_output).
    pub fn drain_iq_samples(&self) -> Vec<(f32, f32)> {
        let mut guard = self.iq_buf.lock().expect("loopback iq buffer poisoned");
        std::mem::take(&mut *guard)
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
            frame_queue: Arc::clone(&self.frame_queue),
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

    fn open_iq_output(
        &self,
        _device: Option<&str>,
        _config: &AudioConfig,
    ) -> Option<Result<Box<dyn AudioIqOutputStream>, AudioError>> {
        Some(Ok(Box::new(LoopbackIqOutputStream {
            buf: Arc::clone(&self.iq_buf),
        })))
    }
}

// ── Loopback input stream ─────────────────────────────────────────────────────

/// Reads samples that were previously written to the loopback output.
pub struct LoopbackInputStream {
    buf: Buf,
    frame_queue: FrameQueue,
}

impl AudioInputStream for LoopbackInputStream {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        // If frames were queued via `push_frame`, pop one frame per read.
        {
            let mut q = self
                .frame_queue
                .lock()
                .expect("loopback frame queue poisoned");
            if let Some(frame) = q.pop_front() {
                return Ok(frame);
            }
        }
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

// ── Loopback I/Q output stream ────────────────────────────────────────────────

/// Collects I/Q pairs written via [`AudioIqOutputStream`] into an in-memory buffer.
pub struct LoopbackIqOutputStream {
    buf: IqBuf,
}

impl AudioIqOutputStream for LoopbackIqOutputStream {
    fn write_iq(&mut self, i: &[f32], q: &[f32]) -> Result<(), AudioError> {
        let mut guard = self.buf.lock().expect("loopback iq buffer poisoned");
        guard.extend(i.iter().zip(q.iter()).map(|(&iv, &qv)| (iv, qv)));
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

    /// `push_frame` frames are returned one-per-read in order, taking priority
    /// over samples placed via `fill_samples`.
    #[test]
    fn push_frame_returns_one_frame_per_read_in_order() {
        let backend = LoopbackBackend::new();
        let cfg = AudioConfig::default();

        // Pre-fill the flat buffer — should not be returned until frame queue is drained.
        backend.fill_samples(&[9.0, 9.0]);

        backend.push_frame(&[1.0, 2.0]);
        backend.push_frame(&[3.0, 4.0]);

        let mut inp = backend.open_input(None, &cfg).unwrap();

        // First read returns first queued frame.
        assert_eq!(inp.read().unwrap(), vec![1.0, 2.0]);
        // Second read returns second queued frame.
        assert_eq!(inp.read().unwrap(), vec![3.0, 4.0]);
        // Queue drained; next read returns the flat fill_samples data.
        assert_eq!(inp.read().unwrap(), vec![9.0, 9.0]);
    }
}
