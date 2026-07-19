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
