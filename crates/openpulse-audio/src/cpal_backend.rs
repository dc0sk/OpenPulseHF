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
    resolve_device, AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
    DeviceResolution,
};
use openpulse_core::error::AudioError;

use crate::fault::StreamFault;

/// Pick a cpal device matching `selector` from `devices` using the hotplug-safe resolver
/// ([`resolve_device`]): exact name → ALSA `CARD=` token → case-insensitive substring, refusing to
/// guess on ambiguity. Survives a device reorder/rename that changes the exact system name.
fn select_cpal_device<I: Iterator<Item = cpal::Device>>(
    devices: I,
    selector: &str,
) -> Result<cpal::Device, AudioError> {
    let named: Vec<(String, cpal::Device)> = devices
        .filter_map(|d| d.name().ok().map(|n| (n, d)))
        .collect();
    let names: Vec<String> = named.iter().map(|(n, _)| n.clone()).collect();
    match resolve_device(selector, &names) {
        DeviceResolution::Resolved(name) => {
            if name != selector {
                debug!("audio device '{selector}' resolved to '{name}' (hotplug-safe match)");
            }
            named
                .into_iter()
                .find(|(n, _)| *n == name)
                .map(|(_, d)| d)
                .ok_or(AudioError::DeviceNotFound(name))
        }
        DeviceResolution::Ambiguous(hits) => Err(AudioError::DeviceNotFound(format!(
            "'{selector}' matches multiple devices ({}); set an exact name",
            hits.join(", ")
        ))),
        DeviceResolution::NotFound => Err(AudioError::DeviceNotFound(selector.to_string())),
    }
}

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
                let all = self
                    .host
                    .input_devices()
                    .map_err(|e| AudioError::Stream(e.to_string()))?;
                select_cpal_device(all, name)?
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
        let fault = StreamFault::new();
        let fault_write = fault.clone();

        let stream = dev
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    // Recover from a poisoned lock: another thread panicked while
                    // holding it, but the VecDeque is still in a usable state.
                    let mut guard = buf_write.lock().unwrap_or_else(|p| p.into_inner());
                    guard.extend(data.iter().copied());
                },
                {
                    let fault = fault_write.clone();
                    move |err| {
                        // Latch before logging: the reader needs this, and a log line alone is what
                        // let a dead capture device look like a quiet band.
                        fault.record(&err);
                        warn!("cpal input error: {err}");
                    }
                },
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
            fault,
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
                let all = self
                    .host
                    .output_devices()
                    .map_err(|e| AudioError::Stream(e.to_string()))?;
                select_cpal_device(all, name)?
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

        // Do NOT play() yet. Starting the stream before any samples are buffered
        // makes the output callback fire against an empty queue and underrun
        // (audible at the frame start, and flaky for slow/bursty modes). Playback
        // is started lazily on the first write(), once the frame is buffered.
        debug!(
            "opened cpal output stream on '{}' (playback deferred to first write)",
            dev.name().unwrap_or_default()
        );

        Ok(Box::new(CpalOutputStream {
            stream,
            started: false,
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
    /// Latched device-loss error. Without this, a dead stream is indistinguishable from a quiet
    /// band: the callback stops firing and `read` returns `Ok(vec![])` forever.
    fault: StreamFault,
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
        let pending: Vec<f32> = guard.drain(..).collect();
        drop(guard);
        // Deliver whatever the driver managed to buffer before it failed, then report the fault.
        // Checking only when the buffer is empty keeps the fault off the hot path entirely.
        if pending.is_empty() {
            self.fault.check()?;
        }
        Ok(pending)
    }

    fn close(self: Box<Self>) {
        // Stream is stopped when `_stream` is dropped.
    }
}

// ── CpalOutputStream ──────────────────────────────────────────────────────────

/// Writes to a live cpal playback stream.
pub struct CpalOutputStream {
    stream: Stream,
    /// Playback is started lazily on the first write so the queue is non-empty
    /// when the output callback first fires (prevents the startup underrun).
    started: bool,
    buf: Arc<Mutex<VecDeque<f32>>>,
    sample_rate_hz: u32,
    channels: u16,
}

impl AudioOutputStream for CpalOutputStream {
    fn write(&mut self, samples: &[f32]) -> Result<(), AudioError> {
        {
            let mut guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
            guard.extend(samples.iter().copied());
        }
        // Start playback only once the queue holds data (drop the buffer lock
        // first — the output callback also takes it).
        if !self.started {
            self.stream
                .play()
                .map_err(|e| AudioError::Stream(e.to_string()))?;
            self.started = true;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), AudioError> {
        // Append a short trailing-silence pad so the last data samples are never at
        // the buffer boundary: the frame tail plays fully, and the unavoidable
        // pull-based end-of-stream underrun lands in silence rather than clipping the
        // final symbols. ~32 ms is plenty and adds negligible TX time.
        {
            let pad = ((self.sample_rate_hz as usize) * (self.channels.max(1) as usize)) / 32;
            let mut guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
            if !guard.is_empty() {
                guard.extend(std::iter::repeat_n(0.0_f32, pad));
            }
        }
        // Wait until the driver has consumed all buffered samples.
        // Timeout adapts to queued audio length so slow modes can fully drain; see
        // `flush::flush_timeout_seconds` for why the old 60 s cap made that adaptation inert.
        let queued_samples = {
            let guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
            guard.len()
        };
        let timeout_seconds =
            crate::flush::flush_timeout_seconds(queued_samples, self.sample_rate_hz, self.channels);
        let deadline = std::time::Instant::now() + Duration::from_secs_f64(timeout_seconds);
        loop {
            std::thread::sleep(Duration::from_millis(10));
            let guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
            if guard.is_empty() {
                // The software queue is empty, but the soundcard's hardware
                // output buffer may still hold up to ~2688 samples at 48 kHz
                // (≈ 6 BPSK symbols at 8 kHz).  Sleep for 200 ms to let the
                // hardware buffer drain before the caller closes the stream.
                std::thread::sleep(Duration::from_millis(200));
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
