//! cpal-based audio backend.
//!
//! Supports ALSA and PipeWire on Linux, CoreAudio on macOS, and WASAPI on
//! Windows via the `cpal` cross-platform audio library.
//!
//! This module is only compiled when the `cpal-backend` feature is active
//! (enabled by default).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, StreamConfig};
use tracing::{debug, warn};

use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;

// ── CpalBackend ───────────────────────────────────────────────────────────────

/// Audio backend backed by `cpal`.
///
/// On Linux this will use ALSA or PipeWire (via the PipeWire ALSA plugin or
/// the native PipeWire cpal host when available).  On macOS it uses CoreAudio
/// and on Windows WASAPI.
pub struct CpalBackend {
    host: cpal::Host,
}

impl Default for CpalBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CpalBackend {
    /// Create a backend using the platform default cpal host.
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }
}

impl AudioBackend for CpalBackend {
    fn name(&self) -> &str {
        "cpal"
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        let mut infos = Vec::new();

        let default_in = self.host.default_input_device();
        let default_out = self.host.default_output_device();

        let all = self
            .host
            .devices()
            .map_err(|e| AudioError::Stream(e.to_string()))?;

        for device in all {
            let name = device.name().unwrap_or_else(|_| "unknown".to_string());

            let is_input = device.default_input_config().is_ok();
            let is_output = device.default_output_config().is_ok();

            let is_default_in = default_in
                .as_ref()
                .and_then(|d| d.name().ok())
                .map(|n| n == name)
                .unwrap_or(false);
            let is_default_out = default_out
                .as_ref()
                .and_then(|d| d.name().ok())
                .map(|n| n == name)
                .unwrap_or(false);

            // Collect supported sample rates from the device's supported
            // input configs; fall back to output configs if there are none.
            let mut rates: Vec<u32> = Vec::new();
            if let Ok(cfgs) = device.supported_input_configs() {
                for cfg in cfgs {
                    rates.push(cfg.min_sample_rate().0);
                    rates.push(cfg.max_sample_rate().0);
                }
            }
            if let Ok(cfgs) = device.supported_output_configs() {
                for cfg in cfgs {
                    rates.push(cfg.min_sample_rate().0);
                    rates.push(cfg.max_sample_rate().0);
                }
            }
            rates.sort_unstable();
            rates.dedup();

            infos.push(DeviceInfo {
                name,
                is_input,
                is_output,
                is_default: is_default_in || is_default_out,
                supported_sample_rates: rates,
            });
        }

        Ok(infos)
    }

    fn open_input(
        &self,
        device: Option<&str>,
        config: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        let dev = match device {
            None => self
                .host
                .default_input_device()
                .ok_or_else(|| AudioError::DeviceNotFound("no default input device".into()))?,
            Some(name) => {
                let mut all = self
                    .host
                    .input_devices()
                    .map_err(|e| AudioError::Stream(e.to_string()))?;
                all.find(|d| d.name().ok().as_deref() == Some(name))
                    .ok_or_else(|| AudioError::DeviceNotFound(name.to_string()))?
            }
        };

        let stream_config = StreamConfig {
            channels: config.channels,
            sample_rate: cpal::SampleRate(config.sample_rate),
            buffer_size: match config.buffer_size {
                Some(n) => cpal::BufferSize::Fixed(n),
                None => cpal::BufferSize::Default,
            },
        };

        let buf: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
        let buf_write = Arc::clone(&buf);

        let stream = dev
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    // Recover from a poisoned lock: another thread panicked while
                    // holding it, but the VecDeque is still in a usable state.
                    let mut guard = buf_write.lock().unwrap_or_else(|p| p.into_inner());
                    guard.extend(data.iter().copied());
                },
                |err| warn!("cpal input error: {err}"),
                None,
            )
            .map_err(|e| AudioError::Stream(e.to_string()))?;

        stream
            .play()
            .map_err(|e| AudioError::Stream(e.to_string()))?;
        debug!(
            "opened cpal input stream on '{}'",
            dev.name().unwrap_or_default()
        );

        Ok(Box::new(CpalInputStream {
            _stream: stream,
            buf,
        }))
    }

    fn open_output(
        &self,
        device: Option<&str>,
        config: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        let dev = match device {
            None => self
                .host
                .default_output_device()
                .ok_or_else(|| AudioError::DeviceNotFound("no default output device".into()))?,
            Some(name) => {
                let mut all = self
                    .host
                    .output_devices()
                    .map_err(|e| AudioError::Stream(e.to_string()))?;
                all.find(|d| d.name().ok().as_deref() == Some(name))
                    .ok_or_else(|| AudioError::DeviceNotFound(name.to_string()))?
            }
        };

        let stream_config = StreamConfig {
            channels: config.channels,
            sample_rate: cpal::SampleRate(config.sample_rate),
            buffer_size: match config.buffer_size {
                Some(n) => cpal::BufferSize::Fixed(n),
                None => cpal::BufferSize::Default,
            },
        };

        let buf: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
        let buf_read = Arc::clone(&buf);

        let stream = dev
            .build_output_stream(
                &stream_config,
                move |output: &mut [f32], _| {
                    let mut guard = buf_read.lock().unwrap_or_else(|p| p.into_inner());
                    for sample in output.iter_mut() {
                        *sample = guard.pop_front().unwrap_or(0.0);
                    }
                },
                |err| warn!("cpal output error: {err}"),
                None,
            )
            .map_err(|e| AudioError::Stream(e.to_string()))?;

        stream
            .play()
            .map_err(|e| AudioError::Stream(e.to_string()))?;
        debug!(
            "opened cpal output stream on '{}'",
            dev.name().unwrap_or_default()
        );

        Ok(Box::new(CpalOutputStream {
            _stream: stream,
            buf,
            sample_rate_hz: config.sample_rate,
            channels: config.channels,
        }))
    }
}

// ── CpalInputStream ───────────────────────────────────────────────────────────

/// Reads from a live cpal capture stream.
pub struct CpalInputStream {
    _stream: Stream,
    buf: Arc<Mutex<VecDeque<f32>>>,
}

impl AudioInputStream for CpalInputStream {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        // Poll the buffer; only sleep when it is empty to avoid unnecessary latency.
        let mut guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
        if !guard.is_empty() {
            return Ok(guard.drain(..).collect());
        }
        drop(guard);
        // Buffer is empty – give the driver a moment to fill it.
        std::thread::sleep(Duration::from_millis(10));
        let mut guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
        Ok(guard.drain(..).collect())
    }

    fn close(self: Box<Self>) {
        // Stream is stopped when `_stream` is dropped.
    }
}

// ── CpalOutputStream ──────────────────────────────────────────────────────────

/// Writes to a live cpal playback stream.
pub struct CpalOutputStream {
    _stream: Stream,
    buf: Arc<Mutex<VecDeque<f32>>>,
    sample_rate_hz: u32,
    channels: u16,
}

impl AudioOutputStream for CpalOutputStream {
    fn write(&mut self, samples: &[f32]) -> Result<(), AudioError> {
        let mut guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
        guard.extend(samples.iter().copied());
        Ok(())
    }

    fn flush(&mut self) -> Result<(), AudioError> {
        // Wait until the driver has consumed all buffered samples.
        // Timeout adapts to queued audio length so slow modes can fully drain.
        let queued_samples = {
            let guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
            guard.len()
        };
        let samples_per_second = (self.sample_rate_hz as f64) * (self.channels as f64);
        let queued_seconds = if samples_per_second > 0.0 {
            (queued_samples as f64) / samples_per_second
        } else {
            0.0
        };
        let timeout_seconds = (queued_seconds + 3.0).max(5.0).min(60.0);
        let deadline = std::time::Instant::now() + Duration::from_secs_f64(timeout_seconds);
        loop {
            std::thread::sleep(Duration::from_millis(10));
            let guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
            if guard.is_empty() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(AudioError::Stream(format!(
                    "flush timeout: output buffer did not drain within {:.1} s",
                    timeout_seconds
                )));
            }
        }
    }

    fn close(self: Box<Self>) {}
}
