use serde::{Deserialize, Serialize};

use crate::error::ModemError;

// ── Plugin metadata ───────────────────────────────────────────────────────────

/// Static metadata that every plugin must provide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Short identifier, e.g. `"BPSK"`.
    pub name: String,
    /// Semver version string of the plugin itself.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Plugin author(s).
    pub author: String,
    /// List of mode strings this plugin handles, e.g. `["BPSK31", "BPSK100"]`.
    pub supported_modes: Vec<String>,
}

// ── Modulation configuration ──────────────────────────────────────────────────

/// Runtime configuration passed to a plugin for each encode/decode call.
#[derive(Debug, Clone)]
pub struct ModulationConfig {
    /// Centre (audio) frequency in Hz (typically 1 500 Hz for HF work).
    pub center_frequency: f32,
    /// PCM sample rate of the audio stream in Hz.
    pub sample_rate: u32,
    /// Mode string that selects parameters inside the plugin, e.g. `"BPSK31"`.
    pub mode: String,
}

impl Default for ModulationConfig {
    fn default() -> Self {
        Self {
            center_frequency: 1500.0,
            sample_rate: 8000,
            mode: "BPSK100".to_string(),
        }
    }
}

// ── Plugin trait ──────────────────────────────────────────────────────────────

/// A modulation / demodulation plugin.
///
/// Implement this trait to add a new waveform to OpenPulse.  Plugins are
/// registered with [`PluginRegistry`] at startup.
pub trait ModulationPlugin: Send + Sync {
    /// Return this plugin's static metadata.
    fn info(&self) -> &PluginInfo;

    /// Encode `data` bytes into a vector of normalised audio samples (`-1.0 …
    /// +1.0`).
    fn modulate(
        &self,
        data: &[u8],
        config: &ModulationConfig,
    ) -> Result<Vec<f32>, ModemError>;

    /// Decode audio samples back to the original bytes.
    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError>;

    /// Return `true` when this plugin can handle `mode` (case-insensitive).
    fn supports_mode(&self, mode: &str) -> bool {
        self.info()
            .supported_modes
            .iter()
            .any(|m| m.eq_ignore_ascii_case(mode))
    }
}

// ── Plugin registry ───────────────────────────────────────────────────────────

/// A runtime collection of modulation plugins.
///
/// Plugins are registered once at startup and then looked up by mode string.
#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<Box<dyn ModulationPlugin>>,
}

impl PluginRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a plugin.  Later registrations shadow earlier ones for the same
    /// mode string.
    pub fn register(&mut self, plugin: Box<dyn ModulationPlugin>) {
        self.plugins.push(plugin);
    }

    /// Look up the first plugin that supports `mode`.
    pub fn get(&self, mode: &str) -> Option<&dyn ModulationPlugin> {
        self.plugins
            .iter()
            .rev() // later registrations take precedence
            .find(|p| p.supports_mode(mode))
            .map(|p| p.as_ref())
    }

    /// Return metadata for every registered plugin.
    pub fn list(&self) -> Vec<&PluginInfo> {
        self.plugins.iter().map(|p| p.info()).collect()
    }
}
