//! Two-engine channel simulation harness.
//!
//! Connects a TX [`ModemEngine`] to an RX [`ModemEngine`] through an
//! [`openpulse_channel::ChannelModel`], enabling integration tests that
//! exercise realistic HF propagation without real audio hardware.

use openpulse_audio::LoopbackBackend;
use openpulse_channel::ChannelModel;

use crate::ModemEngine;

/// A test harness that wires two modem engines through a pluggable channel model.
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
    /// Call this after `tx_engine.transmit()` and before `rx_engine.receive()`.
    pub fn route(&mut self, channel: &mut dyn ChannelModel) {
        let samples = self.tx_loopback.drain_samples();
        let processed = channel.apply(&samples);
        self.rx_loopback.fill_samples(&processed);
    }

    /// Route TX samples with no channel distortion (clean passthrough).
    pub fn route_clean(&mut self) {
        let samples = self.tx_loopback.drain_samples();
        self.rx_loopback.fill_samples(&samples);
    }
}

impl Default for ChannelSimHarness {
    fn default() -> Self {
        Self::new()
    }
}
