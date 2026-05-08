//! Structured TOML configuration for OpenPulse TNC binaries.
//!
//! Reads `~/.config/openpulse/config.toml` and returns an [`OpenpulseConfig`]
//! with built-in defaults applied for any missing fields.
//!
//! Precedence: CLI flag overrides > config file > built-in defaults.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("identity key file has wrong length (expected 32 bytes)")]
    IdentityKeyLength,
}

/// Top-level configuration.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct OpenpulseConfig {
    pub station: StationConfig,
    pub audio: AudioConfig,
    pub modem: ModemConfig,
    pub radio: RadioConfig,
    pub repeater: RepeaterConfig,
    pub ardop: ArdopConfig,
    pub kiss: KissConfig,
    pub logging: LoggingConfig,
    pub relay: RelayConfig,
    pub trust: TrustConfig,
    pub mesh: MeshConfig,
    pub qsy: QsyConfig,
    pub tx_levels: TxLevelsConfig,
}

/// QSY frequency-agility settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct QsyConfig {
    /// When false, all incoming QSY_REQ frames are rejected.
    pub enabled: bool,
    /// Trust levels whose QSY_REQ frames are accepted ("verified", "psk_verified", "unknown").
    /// Reserved for future trust-gating; not yet enforced by the QSY session layer.
    pub allow_trustlevels: Vec<String>,
    /// Candidate frequencies to scan during QSY negotiation (Hz).
    pub candidate_freqs_hz: Vec<u64>,
    /// Time to dwell on each candidate frequency while reading the S-meter (ms).
    pub scan_dwell_ms: u64,
    /// Seconds after QSY_ACK before both stations switch frequency.
    pub switchover_offset_s: u64,
}

/// Per-band TX attenuation memory (FF-8).
///
/// Maps amateur band names (e.g. `"40m"`, `"20m"`) to a dB attenuation value.
/// `"default"` is used for frequencies that fall outside all registered bands.
/// TX samples are scaled by `10^(db / 20)` before being sent to the audio backend.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TxLevelsConfig(pub HashMap<String, f32>);

/// Station identity.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct StationConfig {
    pub callsign: String,
    pub grid_square: String,
}

/// Audio backend selection.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Audio backend: `cpal` (real hardware), `loopback` (testing), or
    /// `default` (use cpal if compiled in, loopback otherwise).
    pub backend: String,
    /// Soft-limiter threshold applied to TX audio before the output backend.
    /// Each sample `s` becomes `threshold * tanh(s / threshold)`.
    /// Set to `0.0` (default) to disable. Typical value: `1.5 * RMS`.
    pub tx_limiter_threshold: f32,
}

/// Modem defaults shared by all TNC binaries.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ModemConfig {
    /// Default modulation mode (e.g. `"BPSK250"`).
    pub mode: String,
    /// PTT backend: `none`, `rts`, `dtr`, `vox`, or `rigctld`.
    pub ptt_backend: String,
}

/// Per-rig CAT settings (used in `[radio.rig_a]` / `[radio.rig_b]` sections).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RigConfig {
    /// rigctld TCP address for this rig (default `"127.0.0.1:4532"`).
    pub rigctld_addr: String,
}

/// Rig CAT control settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RadioConfig {
    /// rigctld TCP address for single-rig PTT-only use (default `"127.0.0.1:4532"`).
    pub rigctld_addr: String,
    /// Primary rig (RX/TX for normal operation and cross-band relay receive).
    pub rig_a: RigConfig,
    /// Secondary rig (TX for cross-band relay).  `None` if not configured.
    pub rig_b: Option<RigConfig>,
}

/// Cross-band repeater settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RepeaterConfig {
    /// Enable the cross-band repeater.  Requires `[radio.rig_a]` and `[radio.rig_b]`.
    pub enabled: bool,
    /// Modulation mode used for both RX (rig_a) and TX (rig_b).
    pub mode: String,
    /// Milliseconds to hold PTT after the last byte is transmitted.
    pub tx_hang_ms: u64,
}

/// ARDOP TNC service settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ArdopConfig {
    pub bind_addr: String,
    pub cmd_port: u16,
    pub data_port: u16,
}

/// KISS TNC service settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct KissConfig {
    pub bind_addr: String,
    pub port: u16,
}

/// Logging configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// `tracing` level filter: `error`, `warn`, `info`, `debug`, or `trace`.
    pub level: String,
}

/// Multi-hop relay settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RelayConfig {
    pub enabled: bool,
    pub max_hops: u8,
}

/// Trust store settings.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct TrustConfig {
    /// Path to the local trust store. Empty string uses the platform default.
    pub store_path: String,
}

/// Mesh daemon settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MeshConfig {
    /// Enable the mesh relay daemon.
    pub enabled: bool,
    /// Maximum relay hop count (envelope dropped when hop_index reaches this).
    pub max_hops: u8,
    /// Relay trust policy string: `"strict"`, `"balanced"`, or `"permissive"`.
    /// Reserved for future trust-level filtering; `RelayTrustPolicy` currently
    /// models a deny-list of peer IDs and does not yet enforce this value.
    pub relay_policy: String,
    /// Store-and-forward frame TTL in seconds (passed to `RelayForwarder` as ttl_ms).
    pub store_forward_ttl_s: u64,
    /// Peer discovery beacon interval in seconds; 0 disables beacons.
    pub beacon_interval_s: u64,
    /// Maximum entries in the local peer cache.
    pub peer_cache_capacity: usize,
    /// Peer cache entry TTL in seconds.
    pub peer_cache_ttl_s: u64,
}

// ── Defaults ──────────────────────────────────────────────────────────────────

impl Default for StationConfig {
    fn default() -> Self {
        Self {
            callsign: "N0CALL".into(),
            grid_square: "AA00".into(),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            backend: "default".into(),
            tx_limiter_threshold: 0.0,
        }
    }
}

impl Default for ModemConfig {
    fn default() -> Self {
        Self {
            mode: "BPSK250".into(),
            ptt_backend: "none".into(),
        }
    }
}

impl Default for RigConfig {
    fn default() -> Self {
        Self {
            rigctld_addr: "127.0.0.1:4532".into(),
        }
    }
}

impl Default for RadioConfig {
    fn default() -> Self {
        Self {
            rigctld_addr: "127.0.0.1:4532".into(),
            rig_a: RigConfig::default(),
            rig_b: None,
        }
    }
}

impl Default for RepeaterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "BPSK250".into(),
            tx_hang_ms: 500,
        }
    }
}

impl Default for ArdopConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1".into(),
            cmd_port: 8515,
            data_port: 8516,
        }
    }
}

impl Default for KissConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1".into(),
            port: 8100,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
        }
    }
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_hops: 3,
        }
    }
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_hops: 3,
            relay_policy: "balanced".into(),
            store_forward_ttl_s: 300,
            beacon_interval_s: 60,
            peer_cache_capacity: 256,
            peer_cache_ttl_s: 3600,
        }
    }
}

impl Default for QsyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allow_trustlevels: vec!["verified".into(), "psk_verified".into()],
            candidate_freqs_hz: vec![],
            scan_dwell_ms: 500,
            switchover_offset_s: 5,
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Returns the platform-standard config file path.
///
/// On Linux: `~/.config/openpulse/config.toml`
/// On macOS: `~/Library/Application Support/openpulse/config.toml`
/// On Windows: `%APPDATA%\openpulse\config.toml`
pub fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("openpulse").join("config.toml"))
}

/// Load config from the platform-standard path. Returns `OpenpulseConfig::default()`
/// if the file does not exist.
pub fn load() -> Result<OpenpulseConfig, ConfigError> {
    match default_config_path() {
        Some(path) => load_from(&path),
        None => Ok(OpenpulseConfig::default()),
    }
}

/// Load config from `path`. Returns `OpenpulseConfig::default()` if the file does
/// not exist.
pub fn load_from(path: &Path) -> Result<OpenpulseConfig, ConfigError> {
    if !path.exists() {
        return Ok(OpenpulseConfig::default());
    }
    let content = std::fs::read_to_string(path)?;
    let config: OpenpulseConfig = toml::from_str(&content)?;
    Ok(config)
}

/// Load or generate the node's 32-byte Ed25519 signing key seed.
///
/// Reads `identity.key` from the platform config directory (`~/.config/openpulse/`).
/// If absent, generates a fresh random seed, persists it, then returns it.
/// The caller derives `peer_id = SigningKey::from_bytes(&seed).verifying_key().to_bytes()`.
pub fn load_or_generate_identity() -> Result<[u8; 32], ConfigError> {
    let path = default_identity_path().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cannot determine config directory",
        )
    })?;
    load_identity_from(&path)
}

/// Load or generate an identity seed at an explicit path (useful in tests).
pub fn load_identity_from(path: &Path) -> Result<[u8; 32], ConfigError> {
    if path.exists() {
        let bytes = std::fs::read(path)?;
        if bytes.len() != 32 {
            return Err(ConfigError::IdentityKeyLength);
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&bytes);
        return Ok(seed);
    }

    use rand::RngCore;
    let mut seed = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut seed);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, seed)?;
    Ok(seed)
}

/// Returns the platform-standard identity key file path.
///
/// On Linux: `~/.config/openpulse/identity.key`
fn default_identity_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("openpulse").join("identity.key"))
}

/// Returns a commented TOML configuration template for `openpulse config init`.
pub fn init_template() -> String {
    r#"# OpenPulse configuration file
# Generated by: openpulse config init
#
# Place this file at:
#   Linux/BSDs : ~/.config/openpulse/config.toml
#   macOS      : ~/Library/Application Support/openpulse/config.toml
#   Windows    : %APPDATA%\openpulse\config.toml
#
# All fields are optional; built-in defaults are used for any missing values.

[station]
# Your amateur radio callsign.
callsign = "N0CALL"
# Maidenhead grid square locator.
grid_square = "AA00"

[audio]
# Audio backend: default | cpal | loopback
#   default  — use cpal if compiled in, loopback otherwise (recommended)
#   cpal     — always use the real sound card (error if not compiled in)
#   loopback — software loopback for testing only, no audio hardware required
backend = "default"
# Soft TX limiter threshold (0.0 = disabled). Typical value: 1.5 × RMS of the
# modulated signal. Prevents ADC clipping and reduces PA non-linearity on peaks.
# tx_limiter_threshold = 0.0

[modem]
# Default modulation mode used when no --mode flag is provided.
# Available: BPSK31, BPSK63, BPSK100, BPSK250, QPSK125, QPSK250, QPSK500,
#            QPSK1000, 8PSK500, 8PSK1000, FSK4-ACK
mode = "BPSK250"
# PTT backend: none | rts | dtr | vox | rigctld
ptt_backend = "none"

[radio]
# rigctld TCP address for single-rig PTT-only use.
rigctld_addr = "127.0.0.1:4532"

# Primary rig (RX/TX for normal operation; also the receive side of cross-band relay).
[radio.rig_a]
rigctld_addr = "127.0.0.1:4532"

# Secondary rig (TX side of cross-band relay).  Uncomment to enable dual-rig.
# [radio.rig_b]
# rigctld_addr = "127.0.0.1:4533"

[repeater]
# Enable the cross-band repeater (requires [radio.rig_a] and [radio.rig_b]).
enabled = false
# Modulation mode used for both RX (rig_a) and TX (rig_b).
mode = "BPSK250"
# Milliseconds to hold PTT after the last byte is transmitted.
tx_hang_ms = 500

[ardop]
# IP address the ARDOP TNC listens on.
bind_addr = "127.0.0.1"
# ARDOP command port.
cmd_port = 8515
# ARDOP data port.
data_port = 8516

[kiss]
# IP address the KISS TNC listens on.
bind_addr = "127.0.0.1"
# KISS TCP port.
port = 8100

[logging]
# Log verbosity: error | warn | info | debug | trace
level = "info"

[relay]
# Enable multi-hop relay forwarding.
enabled = false
# Maximum relay hop count.
max_hops = 3

[mesh]
# Enable the openpulse-mesh daemon relay stack.
enabled = false
# Maximum relay hop count before a frame is dropped.
max_hops = 3
# Relay trust policy: strict | balanced | permissive
relay_policy = "balanced"
# Store-and-forward frame TTL in seconds.
store_forward_ttl_s = 300
# Peer discovery beacon interval in seconds.
beacon_interval_s = 60
# Maximum peer cache entries.
peer_cache_capacity = 256
# Peer cache entry TTL in seconds.
peer_cache_ttl_s = 3600

[trust]
# Path to the local trust store directory. Empty = platform default.
store_path = ""

# [qsy]
# Enable QSY frequency-agility negotiation.  Requires hamlib rigctld configured in [radio].
# enabled = false
# Trust levels allowed to initiate QSY with this station.
# allow_trustlevels = ["verified", "psk_verified"]
# Candidate frequencies to evaluate during a QSY scan (Hz).
# candidate_freqs_hz = [14070000, 14074000, 14077000]
# How long to dwell on each candidate while reading the S-meter (ms).
# scan_dwell_ms = 500
# Seconds between QSY_ACK and the actual frequency switch.
# switchover_offset_s = 5
"#
    .to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn unique_tmp(suffix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "openpulse_cfg_{}_{}.toml",
            std::process::id(),
            suffix
        ))
    }

    #[test]
    fn load_defaults_when_no_file() {
        let path = unique_tmp("defaults");
        let _ = std::fs::remove_file(&path);
        let cfg = load_from(&path).unwrap();
        assert_eq!(cfg.station.callsign, "N0CALL");
        assert_eq!(cfg.ardop.cmd_port, 8515);
        assert_eq!(cfg.ardop.data_port, 8516);
        assert_eq!(cfg.kiss.port, 8100);
        assert_eq!(cfg.modem.mode, "BPSK250");
        assert_eq!(cfg.logging.level, "info");
        assert!(!cfg.relay.enabled);
        assert_eq!(cfg.relay.max_hops, 3);
    }

    #[test]
    fn cli_override_pattern() {
        // CLI flag > config > default: simulate by loading config then applying
        // an Option<T> override, the pattern used by TNC binaries.
        let path = unique_tmp("override");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "[ardop]").unwrap();
            writeln!(f, "cmd_port = 9000").unwrap();
            writeln!(f, "data_port = 9001").unwrap();
        }
        let mut cfg = load_from(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.ardop.cmd_port, 9000);

        // CLI flag supplied → overrides config value.
        let cli_cmd_port: Option<u16> = Some(7777);
        if let Some(p) = cli_cmd_port {
            cfg.ardop.cmd_port = p;
        }
        assert_eq!(cfg.ardop.cmd_port, 7777);
        // Unset CLI flag → config value retained.
        assert_eq!(cfg.ardop.data_port, 9001);
    }

    #[test]
    fn missing_fields_get_defaults() {
        let path = unique_tmp("partial");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            // Only set callsign; everything else should come from Default.
            writeln!(f, "[station]").unwrap();
            writeln!(f, r#"callsign = "K1ABC""#).unwrap();
        }
        let cfg = load_from(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.station.callsign, "K1ABC");
        // Fields not in the file use built-in defaults.
        assert_eq!(cfg.station.grid_square, "AA00");
        assert_eq!(cfg.ardop.cmd_port, 8515);
        assert_eq!(cfg.modem.ptt_backend, "none");
    }

    #[test]
    fn load_twice_returns_same_seed() {
        // Use a unique temp dir to avoid polluting the real config directory.
        let tmp = std::env::temp_dir().join(format!("openpulse_id_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let key_path = tmp.join("identity.key");
        let _ = std::fs::remove_file(&key_path);

        // First call creates the key file.
        let seed1 = load_identity_from(&key_path).unwrap();
        assert_eq!(seed1.len(), 32);
        // Second call reads the same seed.
        let seed2 = load_identity_from(&key_path).unwrap();
        assert_eq!(seed1, seed2);

        let _ = std::fs::remove_file(&key_path);
        let _ = std::fs::remove_dir(&tmp);
    }
}
