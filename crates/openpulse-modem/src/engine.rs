//! The core [`ModemEngine`] struct.

use tracing::{debug, info};

use openpulse_core::audio::{AudioBackend, AudioConfig};
use openpulse_core::error::ModemError;
use openpulse_core::frame::Frame;
use openpulse_core::plugin::{ModulationConfig, PluginRegistry};

/// The modem engine.
///
/// # Example
/// ```no_run
/// use openpulse_modem::ModemEngine;
/// use openpulse_audio::LoopbackBackend;
/// use bpsk_plugin::BpskPlugin;
///
/// let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
/// engine.register_plugin(Box::new(BpskPlugin::new()));
/// engine.transmit(b"Hello", "BPSK100", None).unwrap();
/// let received = engine.receive("BPSK100", None).unwrap();
/// ```
pub struct ModemEngine {
    audio: Box<dyn AudioBackend>,
    plugins: PluginRegistry,
    sequence: u16,
}

impl ModemEngine {
    /// Create a new engine backed by the given audio backend.
    pub fn new(audio: Box<dyn AudioBackend>) -> Self {
        Self {
            audio,
            plugins: PluginRegistry::new(),
            sequence: 0,
        }
    }

    /// Register a modulation plugin.
    pub fn register_plugin(
        &mut self,
        plugin: Box<dyn openpulse_core::plugin::ModulationPlugin>,
    ) {
        info!("registered plugin: {}", plugin.info().name);
        self.plugins.register(plugin);
    }

    /// Return the underlying plugin registry (read-only).
    pub fn plugins(&self) -> &PluginRegistry {
        &self.plugins
    }

    /// Encode `data` into a [`Frame`], modulate it with the plugin that
    /// handles `mode`, and write the resulting audio to the output device.
    ///
    /// Pass `device = None` to use the backend's default output device.
    pub fn transmit(
        &mut self,
        data: &[u8],
        mode: &str,
        device: Option<&str>,
    ) -> Result<(), ModemError> {
        let plugin = self.plugins.get(mode).ok_or_else(|| {
            ModemError::PluginNotFound(mode.to_string())
        })?;

        let frame = Frame::new(self.sequence, data.to_vec());
        self.sequence = self.sequence.wrapping_add(1);
        let wire_bytes = frame.encode();

        debug!(
            "transmitting {} byte frame (seq={}, mode={mode})",
            wire_bytes.len(),
            frame.sequence
        );

        let mod_cfg = ModulationConfig {
            mode: mode.to_string(),
            ..ModulationConfig::default()
        };

        let samples = plugin.modulate(&wire_bytes, &mod_cfg)?;
        info!(
            "modulated {} bytes → {} audio samples",
            wire_bytes.len(),
            samples.len()
        );

        let audio_cfg = AudioConfig::default();
        let mut stream = self
            .audio
            .open_output(device, &audio_cfg)
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        stream
            .write(&samples)
            .map_err(|e| ModemError::Audio(e.to_string()))?;
        stream
            .flush()
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        Ok(())
    }

    /// Read audio from the input device, demodulate with the plugin for
    /// `mode`, and return the decoded frame payload.
    ///
    /// Pass `device = None` to use the backend's default input device.
    pub fn receive(
        &mut self,
        mode: &str,
        device: Option<&str>,
    ) -> Result<Vec<u8>, ModemError> {
        let audio_cfg = AudioConfig::default();
        let mut stream = self
            .audio
            .open_input(device, &audio_cfg)
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        let samples = stream
            .read()
            .map_err(|e| ModemError::Audio(e.to_string()))?;

        info!("received {} audio samples", samples.len());

        let plugin = self.plugins.get(mode).ok_or_else(|| {
            ModemError::PluginNotFound(mode.to_string())
        })?;

        let mod_cfg = ModulationConfig {
            mode: mode.to_string(),
            ..ModulationConfig::default()
        };

        let wire_bytes = plugin.demodulate(&samples, &mod_cfg)?;
        debug!("demodulated {} bytes", wire_bytes.len());

        let frame = Frame::decode(&wire_bytes)?;
        info!("received frame seq={}", frame.sequence);

        Ok(frame.payload)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bpsk_plugin::BpskPlugin;
    use openpulse_audio::LoopbackBackend;

    fn make_engine() -> ModemEngine {
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        engine.register_plugin(Box::new(BpskPlugin::new()));
        engine
    }

    #[test]
    fn transmit_then_receive() {
        let mut engine = make_engine();
        engine.transmit(b"Hello", "BPSK100", None).unwrap();
        let received = engine.receive("BPSK100", None).unwrap();
        assert_eq!(received, b"Hello");
    }

    #[test]
    fn unknown_mode_returns_error() {
        let mut engine = make_engine();
        assert!(engine.transmit(b"x", "UNKNOWN", None).is_err());
    }
}
