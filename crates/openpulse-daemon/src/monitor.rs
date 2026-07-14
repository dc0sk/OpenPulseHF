//! Simultaneous multi-mode receive (REQ-RX-01).
//!
//! A best-effort "monitor" that decodes several registered modulation modes from a copy of the capture
//! stream, independent of the active RX session's single mode — for a discovery/monitor role (see what
//! else is on frequency). It mirrors how the JS8 discovery dwell tees raw audio to an independent
//! decoder: the daemon rx-tick hands each completed burst to the monitor, which tries every configured
//! mode against it.
//!
//! Design: one throwaway [`ModemEngine`] (all CPU plugins registered) separate from the daemon's live RX
//! engine, so its AFC/DCD/rate state never touches the session. `ModulationPlugin`s are stateless
//! (`&self`); `decode_burst` is the public onset-scanning buffer decoder. `reset_afc` before each mode
//! isolates acquisition between modes. Off by default (`[monitor] enabled = false`).

use openpulse_audio::LoopbackBackend;
use openpulse_modem::pipeline::AudioSamples;
use openpulse_modem::ModemEngine;

/// Register the standard CPU modulation plugins on `engine` (mirrors the daemon's live-engine set, minus
/// GPU — the monitor is best-effort). Panics on a plugin trait-version mismatch, exactly like the live
/// engine's startup registration.
fn register_standard_plugins(engine: &mut ModemEngine) {
    engine
        .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register BPSK");
    engine
        .register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .expect("register QPSK");
    engine
        .register_plugin(Box::new(psk8_plugin::Psk8Plugin::new()))
        .expect("register 8PSK");
    engine
        .register_plugin(Box::new(qam64_plugin::Qam64Plugin::new()))
        .expect("register 64QAM");
    engine
        .register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .expect("register FSK4");
    engine
        .register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .expect("register OFDM");
    engine
        .register_plugin(Box::new(scfdma_plugin::ScFdmaPlugin::new()))
        .expect("register SC-FDMA");
    engine
        .register_plugin(Box::new(pilot_plugin::PilotPlugin::new()))
        .expect("register pilot");
}

/// Decodes a set of configured modes from each capture burst, independent of the live RX session.
pub struct MonitorRuntime {
    engine: ModemEngine,
    modes: Vec<String>,
}

impl std::fmt::Debug for MonitorRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonitorRuntime")
            .field("modes", &self.modes)
            .finish()
    }
}

impl MonitorRuntime {
    /// Build a monitor for the given modes. Returns `None` if `modes` is empty (nothing to monitor).
    pub fn new(modes: Vec<String>) -> Option<Self> {
        if modes.is_empty() {
            return None;
        }
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        register_standard_plugins(&mut engine);
        Some(Self { engine, modes })
    }

    /// The modes this monitor tries.
    pub fn modes(&self) -> &[String] {
        &self.modes
    }

    /// Try every configured mode against one completed burst; return `(mode, payload)` for each that
    /// decoded. A burst carries one transmission, so at most one mode normally matches — but an unknown
    /// stream of mixed-mode bursts is decoded correctly over time, each tagged by its mode.
    pub fn decode_all(&mut self, burst: &[f32]) -> Vec<(String, Vec<u8>)> {
        let samples = AudioSamples {
            samples: burst.to_vec(),
        };
        let mut out = Vec::new();
        for mode in &self.modes {
            // Isolate each mode's acquisition — a previous mode's successful AFC must not bias the next.
            self.engine.reset_afc();
            if let Ok(payload) = self.engine.decode_burst(mode, &samples) {
                if !payload.is_empty() {
                    out.push((mode.clone(), payload));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Produce the raw modulated audio for one `mode` frame (no FEC — the `decode_burst` path).
    fn burst_for(mode: &str, payload: &[u8]) -> Vec<f32> {
        let lb = LoopbackBackend::new();
        let mut tx = ModemEngine::new(Box::new(lb.clone_shared()));
        register_standard_plugins(&mut tx);
        tx.transmit(payload, mode, None).expect("transmit");
        lb.drain_samples()
    }

    #[test]
    fn new_is_none_without_modes() {
        assert!(MonitorRuntime::new(vec![]).is_none());
    }

    #[test]
    fn decodes_each_mode_and_does_not_false_positive() {
        // REQ-RX-01 acceptance: a monitor configured for two modes decodes a burst of either, tags it by
        // its mode, and the non-matching mode does NOT false-positive on a foreign burst.
        let mut mon =
            MonitorRuntime::new(vec!["BPSK250".into(), "QPSK500".into()]).expect("monitor");

        let a = burst_for("BPSK250", b"hello-monitor-bpsk");
        assert_eq!(
            mon.decode_all(&a),
            vec![("BPSK250".to_string(), b"hello-monitor-bpsk".to_vec())]
        );

        let b = burst_for("QPSK500", b"hello-monitor-qpsk");
        assert_eq!(
            mon.decode_all(&b),
            vec![("QPSK500".to_string(), b"hello-monitor-qpsk".to_vec())]
        );
    }

    #[test]
    fn silence_decodes_nothing() {
        let mut mon = MonitorRuntime::new(vec!["BPSK250".into()]).expect("monitor");
        assert!(mon.decode_all(&vec![0.0f32; 8000]).is_empty());
    }
}
