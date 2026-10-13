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

use crate::device_cache::{resolve_name_from, Direction, NameCache, Scan};
use crate::fault::StreamFault;

/// Enumerate up to the device named `target`, naming and dropping every device before it; `None`
/// names them all. Selection by name is [`NameCache::select`]: the hotplug-safe resolver
/// ([`resolve_device`](openpulse_core::audio::resolve_device)) over the names, then a second scan
/// that keeps just the match.
///
/// **Never retain a device while enumerating.** cpal's ALSA enumeration is stateful: holding
/// `cpal::Device` values alive while iterating silently truncates the list. On this host, naming each
/// device and dropping it yields 39 devices; naming it and *retaining* it yields 18; collecting the
/// devices first and naming them afterwards yields 4. The truncation is silent — the missing entries
/// simply never arrive — so `openpulse devices` (which drops as it goes) listed devices that
/// `--device` could not open, and `aloop_tx` was unreachable while `hwloop_tx` happened to survive.
fn scan_devices<I>(devices: I, target: Option<&str>) -> Scan<cpal::Device>
where
    I: Iterator<Item = cpal::Device>,
{
    let mut names = Vec::new();
    for device in devices {
        let Ok(name) = device.name() else { continue };
        if Some(name.as_str()) == target {
            return Scan {
                found: Some(device),
                names,
            };
        }
        names.push(name);
    }
    Scan { found: None, names }
}

// ── CpalBackend ───────────────────────────────────────────────────────────────

/// Audio backend backed by `cpal`.
///
/// On Linux this will use ALSA or PipeWire (via the PipeWire ALSA plugin or
/// the native PipeWire cpal host when available).  On macOS it uses CoreAudio
/// and on Windows WASAPI.
pub struct CpalBackend {
    host: cpal::Host,
    names: NameCache,
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
            names: NameCache::default(),
        }
    }
}

impl CpalBackend {
    /// `open_output` with the concrete stream, so the station probe can read its first-sample time.
    fn open_output_stream(
        &self,
        device: Option<&str>,
        config: &AudioConfig,
    ) -> Result<CpalOutputStream, AudioError> {
        let dev = match device {
            None => self
                .host
                .default_output_device()
                .ok_or_else(|| AudioError::DeviceNotFound("no default output device".into()))?,
            Some(name) => self.names.select(Direction::Output, name, |target| {
                let devices = self
                    .host
                    .output_devices()
                    .map_err(|e| AudioError::Stream(e.to_string()))?;
                Ok(scan_devices(devices, target))
            })?,
        };

        let stream_config = StreamConfig {
            channels: config.channels,
            sample_rate: cpal::SampleRate(config.sample_rate),
            buffer_size: match config.buffer_size {
                Some(n) => cpal::BufferSize::Fixed(n),
                None => cpal::BufferSize::Default,
            },
        };

        let buf: Arc<Mutex<OutQueue>> = Arc::new(Mutex::new(OutQueue::default()));
        let buf_read = Arc::clone(&buf);
        let (rate, channels) = (config.sample_rate, config.channels.max(1) as usize);

        let stream = dev
            .build_output_stream(
                &stream_config,
                move |output: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    let mut guard = buf_read.lock().unwrap_or_else(|p| p.into_inner());
                    let had_samples = !guard.samples.is_empty();
                    if had_samples && guard.first_played.is_none() {
                        let ts = info.timestamp();
                        let delay = ts.playback.duration_since(&ts.callback).unwrap_or_default();
                        guard.first_played = Some(std::time::Instant::now() + delay);
                    }
                    let mut last = 0usize;
                    for (i, sample) in output.iter_mut().enumerate() {
                        match guard.samples.pop_front() {
                            Some(v) => {
                                *sample = v;
                                last = i;
                            }
                            None => *sample = 0.0,
                        }
                    }
                    // Latch only on the transition to empty: every later callback pops nothing and
                    // must not move the mark (#1367 review, finding 2).
                    if had_samples && guard.samples.is_empty() {
                        let ts = info.timestamp();
                        guard.drained = Some(
                            crate::flush::drain_after_callback(
                                ts.playback.duration_since(&ts.callback),
                                last / channels + 1,
                                rate,
                            )
                            .map(|d| std::time::Instant::now() + d),
                        );
                    }
                },
                |err| warn!("cpal output error: {err}"),
                None,
            )
            .map_err(|e| self.stream_failed(Direction::Output, device, e))?;

        // play() is deferred to the first write(), so hosts that honour pause start with the
        // frame buffered. On ALSA it changes nothing: cpal's worker starts the device on silence
        // as soon as the stream is built (`play()` is only `pause(false)`), so the callback runs
        // against an empty queue before any write — the drain mark latches only on a
        // non-empty → empty transition for that reason (#1367 review, finding 4).
        debug!(
            "opened cpal output stream on '{}' (playback deferred to first write)",
            dev.name().unwrap_or_default()
        );

        Ok(CpalOutputStream {
            stream,
            started: false,
            buf,
            sample_rate_hz: config.sample_rate,
            channels: config.channels,
            first_write: None,
        })
    }

    /// A stream failed on a device opened by name: forget the name, so the next open resolves in full.
    fn stream_failed(
        &self,
        direction: Direction,
        selector: Option<&str>,
        err: impl std::fmt::Display,
    ) -> AudioError {
        if let Some(selector) = selector {
            self.names.evict(direction, selector);
        }
        AudioError::Stream(err.to_string())
    }

    /// Resolve `selector` to the system name of an output device, without opening a stream.
    ///
    /// Exposed for diagnostics and for testing the resolver: opening a stream perturbs ALSA state,
    /// so a test that opened every device to check reachability would change the very enumeration it
    /// is measuring.
    pub fn resolve_output_name(&self, selector: &str) -> Result<String, AudioError> {
        let names: Vec<String> = self
            .host
            .output_devices()
            .map_err(|e| AudioError::Stream(e.to_string()))?
            .filter_map(|d| d.name().ok())
            .collect();
        resolve_name_from(&names, selector)
    }

    /// Resolve `selector` to the system name of an input device, without opening a stream.
    pub fn resolve_input_name(&self, selector: &str) -> Result<String, AudioError> {
        let names: Vec<String> = self
            .host
            .input_devices()
            .map_err(|e| AudioError::Stream(e.to_string()))?
            .filter_map(|d| d.name().ok())
            .collect();
        resolve_name_from(&names, selector)
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
            Some(name) => self.names.select(Direction::Input, name, |target| {
                let devices = self
                    .host
                    .input_devices()
                    .map_err(|e| AudioError::Stream(e.to_string()))?;
                Ok(scan_devices(devices, target))
            })?,
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
            .map_err(|e| self.stream_failed(Direction::Input, device, e))?;

        stream
            .play()
            .map_err(|e| self.stream_failed(Direction::Input, device, e))?;
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
        Ok(Box::new(self.open_output_stream(device, config)?))
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
    buf: Arc<Mutex<OutQueue>>,
    sample_rate_hz: u32,
    channels: u16,
    first_write: Option<std::time::Instant>,
}

impl CpalOutputStream {
    /// From the first `write` to its first sample at the device's output, once it has played.
    fn first_sample_delay(&self) -> Option<Duration> {
        let played = self
            .buf
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .first_played?;
        Some(played.saturating_duration_since(self.first_write?))
    }
}

/// The output queue and, in the same lock, when its last sample leaves the device (#1367).
///
/// `drained` is set by the callback that empties the queue: `Some(Some(t))` when the device reported a
/// trustworthy delay, `Some(None)` when it did not. One mutex, so `flush` can never see the queue
/// empty before the mark is stored. A stream sees one `write` then one `flush` today (every transmit
/// opens its own stream); a caller that writes again after a drain re-arms it by clearing the mark.
#[derive(Default)]
struct OutQueue {
    samples: VecDeque<f32>,
    drained: Option<Option<std::time::Instant>>,
    /// When the first written sample reaches the device's output, as the first callback that pops
    /// one reports it. Write → drained bounds the end of the stream, not its start (key-to-audio
    /// round 2 review, finding 1).
    first_played: Option<std::time::Instant>,
}

impl AudioOutputStream for CpalOutputStream {
    fn write(&mut self, samples: &[f32]) -> Result<(), AudioError> {
        self.first_write.get_or_insert_with(std::time::Instant::now);
        {
            let mut guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
            guard.samples.extend(samples.iter().copied());
            guard.drained = None;
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
            if !guard.samples.is_empty() {
                guard.samples.extend(std::iter::repeat_n(0.0_f32, pad));
                guard.drained = None;
            }
        }
        // Wait until the driver has consumed all buffered samples.
        // Timeout adapts to queued audio length so slow modes can fully drain; see
        // `flush::flush_timeout_seconds` for why the old 60 s cap made that adaptation inert.
        let queued_samples = {
            let guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
            guard.samples.len()
        };
        let timeout_seconds =
            crate::flush::flush_timeout_seconds(queued_samples, self.sample_rate_hz, self.channels);
        let deadline = std::time::Instant::now() + Duration::from_secs_f64(timeout_seconds);
        loop {
            std::thread::sleep(Duration::from_millis(10));
            let guard = self.buf.lock().unwrap_or_else(|p| p.into_inner());
            if guard.samples.is_empty() {
                // The software queue is empty, but the device still holds what it reported as
                // queued. Wait until it says the last sample has played (plus a margin), never
                // longer than the fixed 200 ms this replaced, and the full 200 ms when the device
                // reported nothing trustworthy (#1367).
                let remaining = guard
                    .drained
                    .flatten()
                    .map(|at| at.saturating_duration_since(std::time::Instant::now()));
                drop(guard);
                std::thread::sleep(crate::flush::drain_wait(remaining));
                if let Some(d) = self.first_sample_delay() {
                    debug!(
                        "first sample reached the output {:.1} ms after the first write",
                        d.as_secs_f64() * 1e3
                    );
                }
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

/// Station probe for the key-to-audio gap (work plan M2): how long a transmit's output path takes
/// before its first sample plays, with the device name cached from run 2 on. Every keyed path asserts PTT before `open_output`, so this is dead
/// air on the rig. Writes SILENCE, so nothing is radiated even with VOX on.
///
/// Run on the station, with the rig's output device name as the daemon is configured:
/// `PROBE_DEVICE='<name>' [PROBE_INPUT_DEVICE='<name>'] cargo test --release -p openpulse-audio \
///  --features cpal-backend --lib key_to_audio -- --ignored --nocapture`. Without `PROBE_DEVICE` it
///  opens the system default, which skips the enumeration a configured name pays (review item 5).
#[cfg(test)]
mod key_to_audio_probe {
    use super::*;
    use openpulse_core::audio::AudioBackend;
    use std::time::Instant;

    #[test]
    #[ignore = "station probe: needs a real output device; prints timings"]
    fn key_to_audio_by_stage() {
        let device = std::env::var("PROBE_DEVICE").ok();
        let input = std::env::var("PROBE_INPUT_DEVICE").ok();
        let backend = CpalBackend::new();
        let config = AudioConfig::default();
        let silence = vec![0.0f32; config.sample_rate as usize / 2];
        // Optional: hold the receiver's capture stream open, as a running station does (#1062
        // key-to-audio review, items 6 and 7: an open stream changes what enumeration sees).
        let _capture = input.as_deref().map(|name| {
            backend
                .open_input(Some(name), &config)
                .expect("open the PROBE_INPUT_DEVICE capture stream")
        });
        println!(
            "output {:?}, capture held open: {:?}",
            device
                .as_deref()
                .unwrap_or("<system default: no enumeration on open>"),
            input
        );
        let resolved = device.as_deref().map(|d| {
            backend
                .resolve_output_name(d)
                .expect("resolve the output name")
        });
        for run in 1..=5 {
            // A full scan, what a cold open pays first: every output device, named. The match's
            // index is its place in hint order, which sets what a cached open still pays (round 2
            // review, finding 3).
            let t = Instant::now();
            let names: Vec<String> = backend
                .host
                .output_devices()
                .map(|d| d.filter_map(|d| d.name().ok()).collect())
                .unwrap_or_default();
            let pass1 = t.elapsed();
            let index = resolved
                .as_deref()
                .and_then(|r| names.iter().position(|n| n == r));

            // Production: `open_output` by the configured name. Run 1 resolves in full and caches
            // the name; later runs scan only up to it.
            let t = Instant::now();
            let mut stream = backend
                .open_output_stream(device.as_deref(), &config)
                .expect("open_output");
            let open = t.elapsed();

            // First sample at the device's output, as its callback reports it; and the drain, which
            // bounds the end of the stream rather than its start.
            let t = Instant::now();
            stream.write(&silence).expect("write");
            let _ = stream.flush();
            let audio = Duration::from_secs_f64(silence.len() as f64 / config.sample_rate as f64);
            let drained = t.elapsed().saturating_sub(audio);
            let first = stream.first_sample_delay();
            let ms = |d: Duration| d.as_secs_f64() * 1e3;
            println!(
                "run {run}: full scan {:>7.1} ms ({} devices, match at index {index:?}) | \
                 open_output {:>7.1} ms | write → first sample {} | write → drained − audio {:>7.1} ms | \
                 key-to-first-sample {}",
                ms(pass1),
                names.len(),
                ms(open),
                first.map_or("n/a".into(), |d| format!("{:>7.1} ms", ms(d))),
                ms(drained),
                first.map_or("n/a".into(), |d| format!("{:>7.1} ms", ms(open + d))),
            );
        }
    }
}
