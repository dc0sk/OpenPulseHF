use serde::{Deserialize, Serialize};

use crate::error::{ModemError, PluginError};

/// Current plugin trait version.
/// Format: "<major>.<minor>.<patch>"
pub const PLUGIN_TRAIT_VERSION: &str = "1.0.0";

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
    /// Plugin trait version requirement, e.g. `"1.0"` (format: "<major>.<minor>").
    /// The plugin is compatible with the framework if:
    /// - framework major version == plugin major version, AND
    /// - framework minor version >= plugin minor version
    pub trait_version_required: String,
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
    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError>;

    /// Decode audio samples back to the original bytes.
    fn demodulate(&self, samples: &[f32], config: &ModulationConfig)
        -> Result<Vec<u8>, ModemError>;

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

    /// Register a plugin, validating trait version compatibility.
    /// Later registrations shadow earlier ones for the same mode string.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the plugin's `trait_version_required` is incompatible
    /// with the framework's `PLUGIN_TRAIT_VERSION`.
    pub fn register(&mut self, plugin: Box<dyn ModulationPlugin>) -> Result<(), PluginError> {
        let info = plugin.info();
        Self::validate_trait_version(&info)?;
        self.plugins.push(plugin);
        Ok(())
    }

    /// Validate that a plugin's trait version is compatible with the framework.
    fn validate_trait_version(info: &PluginInfo) -> Result<(), PluginError> {
        let plugin_parts: Vec<&str> = info.trait_version_required.split('.').collect();
        if plugin_parts.len() != 2 {
            return Err(PluginError::InvalidTraitVersionFormat(
                info.trait_version_required.clone(),
            ));
        }

        let plugin_major = plugin_parts[0].parse::<u32>().map_err(|_| {
            PluginError::InvalidTraitVersionFormat(info.trait_version_required.clone())
        })?;
        let plugin_minor = plugin_parts[1].parse::<u32>().map_err(|_| {
            PluginError::InvalidTraitVersionFormat(info.trait_version_required.clone())
        })?;

        let framework_parts: Vec<&str> = PLUGIN_TRAIT_VERSION.split('.').collect();
        let framework_major = framework_parts[0].parse::<u32>().unwrap();
        let framework_minor = framework_parts[1].parse::<u32>().unwrap();

        // Compatible if: framework major == plugin major AND framework minor >= plugin minor
        if plugin_major != framework_major || framework_minor < plugin_minor {
            return Err(PluginError::IncompatibleTraitVersion {
                plugin: info.name.clone(),
                required: info.trait_version_required.clone(),
                current: PLUGIN_TRAIT_VERSION.to_string(),
            });
        }

        Ok(())
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
