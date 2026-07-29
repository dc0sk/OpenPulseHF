//! One-way channel simulation harness for integration tests.
//!
//! Routes samples from a TX [`ModemEngine`] through an
//! [`openpulse_channel::ChannelModel`] into an RX [`ModemEngine`].
//! The harness is intentionally **unidirectional**: one call to `route()`
//! drains TX samples and fills the RX buffer in a single direction with no
//! shared timebase or concurrent reverse path.  It validates one-way modem
//! correctness under realistic HF propagation without requiring real audio
//! hardware, but does not model full-duplex timing behaviour.

use openpulse_audio::LoopbackBackend;
use openpulse_channel::ChannelModel;

use crate::ModemEngine;

/// Route one station's pending TX samples through `channel` into another
/// station's RX buffer.
///
/// The plugin-agnostic channel-bridge primitive for harnesses that own two
/// independent engines (e.g. a bidirectional OTA link or a future interactive
/// twin-station rig): drain `src`'s loopback, apply the channel, fill `dst`'s
/// loopback. Returns the number of samples routed. Unlike
/// [`ChannelSimHarness::route`] this operates on externally-owned backends so a
/// forward and a reverse direction can each carry their own channel model.
pub fn bridge_through(
    src: &LoopbackBackend,
    dst: &LoopbackBackend,
    channel: &mut dyn ChannelModel,
) -> usize {
    let samples = src.drain_samples();
    let n = samples.len();
    dst.fill_samples(&channel.apply(&samples));
    n
}

/// A one-way test harness that routes TX samples through a pluggable channel model into an RX engine.
///
/// Each call to [`route`](Self::route) drains the TX loopback buffer, applies
/// the channel model, and fills the RX loopback buffer.  There is no reverse
/// path; to simulate a bidirectional exchange create two harnesses and call
/// `route` on each in alternation.
///
/// # Usage
///
/// ```no_run
/// use openpulse_modem::channel_sim::ChannelSimHarness;
/// use openpulse_channel::{AwgnConfig, awgn::AwgnChannel};
///
/// let mut harness = ChannelSimHarness::new();
/// let mut channel = AwgnChannel::new(AwgnConfig { snr_db: 20.0, seed: Some(1) }).unwrap();
///
/// harness.tx_engine.transmit(b"hello", "BPSK250", None).unwrap();
/// harness.route(&mut channel);
/// let rx = harness.rx_engine.receive("BPSK250", None).unwrap();
/// assert_eq!(rx, b"hello");
/// ```
pub struct ChannelSimHarness {
    /// The transmitting engine. Call `transmit()` on this.
    pub tx_engine: ModemEngine,
    /// The receiving engine. Call `receive()` on this after `route()`.
    pub rx_engine: ModemEngine,
    tx_loopback: LoopbackBackend,
    rx_loopback: LoopbackBackend,
}

impl ChannelSimHarness {
    /// Create a harness with two independent loopback engines and no channel
    /// distortion until `route` is called with a model.
    pub fn new() -> Self {
        let tx_loopback = LoopbackBackend::new();
        let rx_loopback = LoopbackBackend::new();
        let tx_engine = ModemEngine::new(Box::new(tx_loopback.clone_shared()));
        let rx_engine = ModemEngine::new(Box::new(rx_loopback.clone_shared()));
        Self {
            tx_engine,
            rx_engine,
            tx_loopback,
            rx_loopback,
        }
    }

    /// Move TX samples through `channel` and deliver the result to the RX engine.
    ///
    /// Returns the number of TX samples routed, which can be divided by the sample
    /// rate (8000 Hz) to obtain the theoretical on-air duration for throughput calculations.
    ///
    /// Call this after `tx_engine.transmit()` and before `rx_engine.receive()`.
    pub fn route(&mut self, channel: &mut dyn ChannelModel) -> usize {
        let samples = self.tx_loopback.drain_samples();
        let n = samples.len();
        let processed = channel.apply(&samples);
        self.rx_loopback.fill_samples(&processed);
        n
    }

    /// Like [`route`](Self::route) but also returns the drained TX samples and the
    /// post-channel samples, for visualization or diagnostics.
    ///
    /// Returns `(tx_samples, channel_output)`. The RX engine is filled with
    /// `channel_output`, identical to [`route`](Self::route).
    pub fn route_tapped(&mut self, channel: &mut dyn ChannelModel) -> (Vec<f32>, Vec<f32>) {
        let samples = self.tx_loopback.drain_samples();
        let processed = channel.apply(&samples);
        self.rx_loopback.fill_samples(&processed);
        (samples, processed)
    }

    /// Route TX samples with no channel distortion (clean passthrough).
    ///
    /// Returns the number of TX samples routed (same semantics as [`route`](Self::route)).
    pub fn route_clean(&mut self) -> usize {
        let samples = self.tx_loopback.drain_samples();
        let n = samples.len();
        self.rx_loopback.fill_samples(&samples);
        n
    }

    /// Route TX samples with band-limited NOISE padded around them, at a chosen idle level.
    ///
    /// [`route_embedded`](Self::route_embedded) pads *silence*, and silence cannot reproduce a whole
    /// class of real-radio failure: the receiver's energy gate falls back to an absolute floor
    /// (`EnergyGate::ABS_THRESHOLD`) until it has accumulated 32 windows of history, so a real idle
    /// noise floor above that floor makes the gate fire — and AFC settle — on NOISE, before the
    /// frame has arrived. A silence-padded capture always has an idle level of exactly zero, so it
    /// can never trip it, and every in-process test stayed blind to it.
    ///
    /// Measured on air (issue #1021, IC-9700 <-> FT-991A, 2 m): an idle floor of mean-square 4.2e-4
    /// against a 1e-4 cold-start threshold settled AFC at 0.2 s — some 40 000 samples before the
    /// transmission started — and the coded receive never recovered.
    ///
    /// `noise_rms` is the RMS amplitude of the padding (mean-square = `noise_rms²`), and `seed`
    /// makes the noise deterministic so a failure reproduces exactly. Returns the number of TX
    /// samples routed (excluding the padding).
    pub fn route_embedded_noisy(
        &mut self,
        lead: usize,
        trail: usize,
        noise_rms: f32,
        seed: u64,
    ) -> usize {
        // Deterministic xorshift64* rather than a seeded RNG dependency: the padding only has to be
        // reproducible and roughly white, and a test that cannot reproduce its own failure is worse
        // than no test.
        let mut state = seed | 1;
        let mut next_noise = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let u = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64; // uniform [0,1)
            ((u as f32) * 2.0 - 1.0) * noise_rms * 1.732_050_8 // uniform -> RMS = a/sqrt(3)
        };
        let samples = self.tx_loopback.drain_samples();
        let n = samples.len();
        let mut buf = Vec::with_capacity(lead + n + trail);
        buf.extend((0..lead).map(|_| next_noise()));
        buf.extend_from_slice(&samples);
        buf.extend((0..trail).map(|_| next_noise()));
        self.rx_loopback.fill_samples(&buf);
        n
    }

    /// Route TX samples with silence padded around them, so the receiver must LOCATE the frame.
    ///
    /// Every other route fills the RX loopback with a buffer that **is** the frame, which is a
    /// receiver's easiest possible case and not what a real capture looks like: a live receiver
    /// listens for seconds and the frame sits somewhere inside. That gap is why the whole suite
    /// missed a defect where the scanning FEC receive could not decode a frame a shorter capture
    /// decoded fine (measured 2026-07-19 on the dual-card rig, 45 s window vs 7 s).
    ///
    /// Returns the number of TX samples routed (excluding the padding).
    pub fn route_embedded(&mut self, lead_silence: usize, trail_silence: usize) -> usize {
        let samples = self.tx_loopback.drain_samples();
        let n = samples.len();
        let mut buf = Vec::with_capacity(lead_silence + n + trail_silence);
        buf.extend(std::iter::repeat_n(0.0f32, lead_silence));
        buf.extend_from_slice(&samples);
        buf.extend(std::iter::repeat_n(0.0f32, trail_silence));
        self.rx_loopback.fill_samples(&buf);
        n
    }

    /// Hand a recorded capture straight to the RX engine, with no synthetic signal at all.
    ///
    /// This is the highest-fidelity path the suite has: the receiver is fed exactly the audio a rig
    /// produced during a real transmission, so a decode result is a statement about the modem
    /// against reality rather than against a model of a radio.
    pub fn feed_capture(&mut self, capture: &crate::capture_replay::Capture) -> usize {
        self.rx_loopback.fill_samples(&capture.samples);
        capture.samples.len()
    }

    /// Route TX samples embedded in REAL recorded radio audio.
    ///
    /// Every other padding here is synthetic — flat pseudo-random noise at a chosen level. A real
    /// receiver's idle output is not flat: it carries the rig's own noise shaping, whatever spurs
    /// the host puts into the USB audio, and an absolute level set by the rig and mixer rather than
    /// by us. Padding with a recorded capture puts the frame in that actual context, so the test
    /// cannot be wrong about the radio the way a model can.
    ///
    /// `capture` supplies the lead and trail (cycled if shorter than requested), and the frame is
    /// scaled by `signal_gain` so the signal-to-idle ratio can be swept against a fixed real floor.
    ///
    /// Returns the number of TX samples routed (excluding padding).
    pub fn route_embedded_in_capture(
        &mut self,
        capture: &crate::capture_replay::Capture,
        lead: usize,
        trail: usize,
        signal_gain: f32,
    ) -> usize {
        let samples = self.tx_loopback.drain_samples();
        let n = samples.len();
        let mut buf = Vec::with_capacity(lead + n + trail);
        buf.extend(capture.cycled(0, lead));
        buf.extend(samples.iter().map(|&s| s * signal_gain));
        buf.extend(capture.cycled(lead, trail));
        self.rx_loopback.fill_samples(&buf);
        n
    }

    /// Route TX samples through a fixed capture gain, with noise padded around the frame.
    ///
    /// The absolute level a receiver hands the modem is set by the rig's audio output and the host
    /// mixer, not by the channel — and it matters because `EnergyGate` compares against *absolute*
    /// thresholds. Measured on air 2026-07-28: an idle floor of mean-square 0.0154 sat 4.7x above
    /// the gate's maximum threshold, so the gate could never shut, fired on noise, and settled AFC
    /// on it. Every other harness route runs at a normalised level where that cannot happen.
    ///
    /// `idle_mean_sq` sets the noise floor directly (it is the quantity the gate actually compares),
    /// and `capture_gain` scales the whole capture including the frame.
    pub fn route_embedded_at_level(
        &mut self,
        lead: usize,
        trail: usize,
        idle_mean_sq: f32,
        capture_gain: f32,
        seed: u64,
    ) -> usize {
        let noise_rms = idle_mean_sq.max(0.0).sqrt();
        let samples = self.tx_loopback.drain_samples();
        let n = samples.len();
        let mut state = seed | 1;
        let mut next_noise = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let u = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64;
            ((u as f32) * 2.0 - 1.0) * noise_rms * 1.732_050_8
        };
        let mut buf = Vec::with_capacity(lead + n + trail);
        buf.extend((0..lead).map(|_| next_noise()));
        buf.extend_from_slice(&samples);
        buf.extend((0..trail).map(|_| next_noise()));
        if capture_gain != 1.0 {
            for s in buf.iter_mut() {
                *s *= capture_gain;
            }
        }
        self.rx_loopback.fill_samples(&buf);
        n
    }

    /// Route TX samples through a live capture-side AGC.
    ///
    /// An AGC moves the gain *during* a frame: near-harmless to a phase-only waveform, destructive
    /// to one carrying bits in amplitude. That asymmetry once made eight modes look
    /// "analog-path limited" when the real cause was a live mixer AGC on the capture card.
    ///
    /// **The AGC is primed with `idle_secs` of idle noise before the frame, and that is not
    /// cosmetic.** A cold AGC starts at unity and ramps, and the ramp lands exactly on the preamble
    /// the acquisition path depends on. Measured: cold start peaks at **272x** the input over the
    /// first 200 samples; primed with 1 s of idle it peaks at **5.9x**, with the same settled gain
    /// (1.10x) either way — so priming changes the transient and nothing else. The cold transient is
    /// also in the wrong causal direction: a real receiver's AGC has been listening to the noise
    /// floor for minutes and *settles down* onto an arriving signal rather than ramping up from
    /// unity. Left uncorrected, this seam would have mis-measured the very defect it was built to
    /// reproduce (archetype scan 2026-07-29, finding 14).
    ///
    /// Pass `idle_secs = 0.0` deliberately if you want the cold transient itself.
    pub fn route_with_capture_agc(
        &mut self,
        target_rms: f32,
        attack_secs: f32,
        decay_secs: f32,
        idle_secs: f32,
    ) -> usize {
        let mut channel = openpulse_channel::agc::AgcChannel::new(
            openpulse_channel::agc::AgcConfig::agc(target_rms, attack_secs, decay_secs, 8_000.0),
        )
        .expect("valid agc parameters");
        let n_idle = (idle_secs.max(0.0) * 8_000.0) as usize;
        if n_idle > 0 {
            // Deterministic idle noise at a realistic floor, so the AGC arrives at the frame with a
            // settled gain the way a real receiver does.
            let mut state = 0x51ED_2701u64 | 1;
            let idle: Vec<f32> = (0..n_idle)
                .map(|_| {
                    state ^= state >> 12;
                    state ^= state << 25;
                    state ^= state >> 27;
                    let u = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64
                        / (1u64 << 53) as f64;
                    ((u as f32) * 2.0 - 1.0) * 0.02
                })
                .collect();
            let _ = channel.apply(&idle);
        }
        self.route(&mut channel)
    }

    /// Route TX samples through a pure carrier-frequency-offset channel.
    ///
    /// Two independent transceivers never agree exactly: measured on air 2026-07-28, an IC-9700 and
    /// an FT-991A both commanded to 144.600000 MHz put the received carrier at **1436 Hz — a −64 Hz
    /// offset**, at the edge of BPSK250's ±62.5 Hz AFC. Carrier recovery is the most-churned area of
    /// this modem and every offset bug so far had to be found on hardware, because nothing
    /// in-process gave the acquisition path a real frequency offset.
    ///
    /// This is NOT [`route_with_sro`](Self::route_with_sro): a sampling-clock offset drifts the
    /// symbol clock and leaves the carrier alone; this shifts the carrier and leaves timing alone.
    pub fn route_with_cfo(&mut self, offset_hz: f32) -> usize {
        let mut channel = openpulse_channel::cfo::CfoChannel::new(
            openpulse_channel::cfo::CfoConfig::new(offset_hz, 8_000.0),
        )
        .expect("finite offset and sample rate");
        self.route(&mut channel)
    }

    /// Route TX samples through a carrier offset with noise padded around the frame, so the
    /// receiver must both LOCATE the frame in a realistic idle floor and ACQUIRE it off-frequency —
    /// the combination a real receiver actually faces.
    pub fn route_embedded_with_cfo(
        &mut self,
        lead: usize,
        trail: usize,
        noise_rms: f32,
        seed: u64,
        offset_hz: f32,
    ) -> usize {
        let samples = self.tx_loopback.drain_samples();
        let n = samples.len();
        let shifted = if offset_hz == 0.0 {
            samples
        } else {
            let mut ch = openpulse_channel::cfo::CfoChannel::new(
                openpulse_channel::cfo::CfoConfig::new(offset_hz, 8_000.0),
            )
            .expect("finite offset and sample rate");
            ch.apply(&samples)
        };
        let mut state = seed | 1;
        let mut next_noise = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let u = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64;
            ((u as f32) * 2.0 - 1.0) * noise_rms * 1.732_050_8
        };
        let mut buf = Vec::with_capacity(lead + shifted.len() + trail);
        buf.extend((0..lead).map(|_| next_noise()));
        buf.extend_from_slice(&shifted);
        buf.extend((0..trail).map(|_| next_noise()));
        self.rx_loopback.fill_samples(&buf);
        n
    }

    /// Route TX samples through a pure sample-rate-offset (clock-drift) channel.
    ///
    /// `ppm` is the RX-vs-TX clock offset in parts-per-million (positive = RX
    /// faster). This isolates the two-independent-clock effect that distinguishes
    /// the dual-clock hardware loopback from the single-clock virtual loopback.
    /// Returns the number of TX samples routed.
    pub fn route_with_sro(&mut self, ppm: f32) -> usize {
        let mut channel =
            openpulse_channel::sro::SroChannel::new(openpulse_channel::sro::SroConfig::new(ppm))
                .expect("finite ppm");
        self.route(&mut channel)
    }
}

impl Default for ChannelSimHarness {
    fn default() -> Self {
        Self::new()
    }
}
