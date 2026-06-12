use serde::{Deserialize, Serialize};

use crate::error::{ModemError, PluginError};

/// Current plugin trait version.
/// Format: `<major>.<minor>.<patch>`
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
    /// Plugin trait version requirement, e.g. `"1.0"` (format: `<major>.<minor>`).
    /// The plugin is compatible with the framework if:
    /// - framework major version == plugin major version, AND
    /// - framework minor version >= plugin minor version
    pub trait_version_required: String,
}

// ── Pulse shaping ─────────────────────────────────────────────────────────────

/// Amplitude envelope applied during symbol modulation.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum PulseShape {
    /// 50% overlapping raised-cosine crossfade between adjacent symbols.
    /// Default for all modes; equivalent to PSK31 shaping for pure BPSK.
    #[default]
    Hann,
    /// Independent sin² amplitude envelope per symbol (0 → 1 → 0 per period).
    /// Forces amplitude zero at every symbol boundary; achieves null-to-null BW ≈ 2×Rs.
    /// Used by `-HF` mode aliases for HF-legal operation at high baud rates.
    CosineOverlap,
    /// Square-root raised-cosine (SRRC) FIR pulse shaping.
    /// Occupied bandwidth ≈ (1 + alpha) × Rs Hz; requires a matched RRC RX filter.
    /// Used by `-RRC` mode aliases.
    Rrc {
        /// RRC rolloff factor α ∈ [0, 1]; 0.35 is the default for `-RRC` modes.
        alpha: f32,
    },
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
    /// Pulse-shaping envelope; plugins select this based on the mode string.
    pub pulse_shape: PulseShape,
    /// AFC correction already applied to `center_frequency` by the engine, in Hz.
    ///
    /// Non-zero when the engine ran AFC settling before this decode attempt.
    /// Plugins may use this to decide whether carrier-phase drift correction
    /// is appropriate (e.g. QPSK only corrects drift when AFC is active).
    pub afc_correction_hz: f32,
}

impl Default for ModulationConfig {
    fn default() -> Self {
        Self {
            center_frequency: 1500.0,
            sample_rate: 8000,
            mode: "BPSK100".to_string(),
            pulse_shape: PulseShape::Hann,
            afc_correction_hz: 0.0,
        }
    }
}

// ── Frame geometry ────────────────────────────────────────────────────────────

/// Mode-specific frame dimensions used by the receive engine to size its scan
/// step, energy-gate window, and per-attempt demodulation slice.
///
/// All values are in samples at the config's sample rate.  Before this struct
/// existed the engine guessed these from trailing digits of the mode name —
/// wrong for every mode whose name does not end in its baud rate (OFDM52's 52
/// is a subcarrier count; SCFDMA52-64QAM-P4 parsed as 4 baud) — and assumed a
/// 32-symbol preamble (true only for BPSK).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameGeometry {
    /// Scan step: one symbol period (serial-tone modes) or one block-symbol
    /// length (OFDM/SC-FDMA).
    pub symbol_period_samples: usize,
    /// Acquisition span the demodulator needs near the slice front (preamble
    /// or sync sequence).
    pub preamble_samples: usize,
    /// Minimum slice length that can hold one decodable minimal frame.
    pub min_frame_samples: usize,
    /// Slice length that bounds one demodulation attempt: the largest frame
    /// this mode emits (255-byte RS block) plus margin.
    pub max_frame_samples: usize,
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

    /// Decode audio samples and return per-bit soft log-likelihood ratios.
    ///
    /// Returns one `f32` per bit in the decoded stream, with **positive = bit
    /// more likely 0** and negative = bit more likely 1.  Plugins that know
    /// their internal soft values (BPSK I-channel correlation, QPSK I/Q
    /// projections) should override this for maximum coding gain (~1–2 dB).
    ///
    /// The default falls back to [`demodulate`](Self::demodulate) and maps each
    /// hard-decided bit to ±1.0.
    fn demodulate_soft(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<f32>, ModemError> {
        let bytes = self.demodulate(samples, config)?;
        let llrs = bytes
            .iter()
            .flat_map(|&b| (0..8u8).map(move |i| if (b >> i) & 1 == 0 { 1.0f32 } else { -1.0f32 }))
            .collect();
        Ok(llrs)
    }

    /// Frame geometry for `config.mode`, used by the receive engine to size
    /// its scan step, energy-gate window, and demodulation slices.
    ///
    /// Returns `None` (the default) when the plugin does not describe its
    /// geometry; the engine then falls back to a mode-name heuristic that is
    /// only correct for modes named after their baud rate with a 32-symbol
    /// preamble.  Every production plugin should override this.
    fn frame_geometry(&self, _config: &ModulationConfig) -> Option<FrameGeometry> {
        None
    }

    /// Return `true` if this plugin produces genuine soft LLRs from
    /// [`demodulate_soft`](Self::demodulate_soft).
    ///
    /// Plugins that override `demodulate_soft` with proper LLR computation
    /// (e.g. matched-filter projections, per-subcarrier FFT magnitude) should
    /// override this to return `true`.  The default `false` indicates the
    /// fallback ±1.0 hard-decision output, which provides no iteration gain
    /// to soft-input FEC decoders such as LDPC and turbo.
    ///
    /// The modem engine logs a warning when a soft-FEC mode is paired with a
    /// plugin that returns `false`.
    fn supports_soft_demod(&self) -> bool {
        false
    }

    /// Return `true` when this plugin can handle `mode` (case-insensitive).
    fn supports_mode(&self, mode: &str) -> bool {
        self.info()
            .supported_modes
            .iter()
            .any(|m| m.eq_ignore_ascii_case(mode))
    }

    /// Estimate the carrier frequency offset in Hz from the given samples.
    ///
    /// Returns `None` if the plugin does not support AFC or the buffer is too
    /// short.  The default implementation returns `None`.
    fn estimate_afc_hz(&self, _samples: &[f32], _config: &ModulationConfig) -> Option<f32> {
        None
    }

    /// Encode `data` bytes and return baseband I and Q sample vectors.
    ///
    /// The returned vectors have the same length.  `I` maps to the left
    /// channel and `Q` to the right channel of a stereo audio output, which
    /// an SDR upconverts directly to RF with exact sideband suppression.
    ///
    /// The default implementation wraps [`modulate`](Self::modulate) via a
    /// Hilbert-transform baseband shift.  Plugins with a native complex-baseband
    /// path (BPSK, QPSK) override this for efficiency and accuracy.
    fn modulate_iq(
        &self,
        data: &[u8],
        config: &ModulationConfig,
    ) -> Result<(Vec<f32>, Vec<f32>), ModemError> {
        let real = self.modulate(data, config)?;
        let (i_bb, q_bb) =
            crate::iq::hilbert_iq(&real, config.center_frequency, config.sample_rate as f32);
        Ok((i_bb, q_bb))
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
        Self::validate_trait_version(info)?;
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

        let (fw_major_str, fw_rest) = PLUGIN_TRAIT_VERSION.split_once('.').ok_or_else(|| {
            PluginError::InvalidTraitVersionFormat(PLUGIN_TRAIT_VERSION.to_string())
        })?;
        let framework_major = fw_major_str.parse::<u32>().map_err(|_| {
            PluginError::InvalidTraitVersionFormat(PLUGIN_TRAIT_VERSION.to_string())
        })?;
        let framework_minor = fw_rest
            .split_once('.')
            .map_or(fw_rest, |(m, _)| m)
            .parse::<u32>()
            .map_err(|_| {
                PluginError::InvalidTraitVersionFormat(PLUGIN_TRAIT_VERSION.to_string())
            })?;

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
