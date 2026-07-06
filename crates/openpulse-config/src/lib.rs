//! Structured TOML configuration for OpenPulse TNC binaries.
//!
//! Reads `~/.config/openpulse/config.toml` and returns an [`OpenpulseConfig`]
//! with built-in defaults applied for any missing fields.
//!
//! Precedence: CLI flag overrides > config file > built-in defaults.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod logging;
pub mod secret_file;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("identity key file has wrong length (expected 32 bytes)")]
    IdentityKeyLength,
    #[error(
        "insecure permissions on secret file {path}: {mode:o} (expected owner-only, e.g. 600)"
    )]
    InsecureSecretPermissions { path: String, mode: u32 },
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
    pub daemon: DaemonConfig,
    pub logbook: LogbookConfig,
    pub observability: ObservabilityConfig,
}

/// Observability / audit-mode settings (REQ-OBS-01). Opt-in; off by default.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ObservabilityConfig {
    /// Enable audit mode: the daemon records its control-event stream (and, in later
    /// slices, per-session diagnostics + a startup snapshot) under `archive_dir`.
    pub audit_mode: bool,
    /// Directory for audit artifacts (`events.ndjson`, …). `~` is expanded.
    pub archive_dir: String,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            audit_mode: false,
            archive_dir: "~/.local/share/openpulse/audit".into(),
        }
    }
}

/// Automatic ADIF logbook settings (opt-in; one record per completed contact).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LogbookConfig {
    /// Append an ADIF record per QSO (a connect→disconnect session). Default `false`.
    pub enabled: bool,
    /// Path to the `.adi` logbook file (created with a header on first write).
    pub adif_path: String,
    /// Optional callsign → Maidenhead grid lookup, used to fill the worked station's `GRIDSQUARE`
    /// when the peer's grid isn't exchanged on air. Keys are matched case-insensitively.
    pub peer_grids: std::collections::BTreeMap<String, String>,
}

impl Default for LogbookConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            adif_path: "~/.local/share/openpulse/openpulse.adi".into(),
            peer_grids: std::collections::BTreeMap::new(),
        }
    }
}

/// `openpulse-daemon` runtime settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DaemonConfig {
    /// Bind address for the TCP control port.
    pub tcp_bind_addr: String,
    /// TCP control port (default 9000).
    pub tcp_port: u16,
    /// Bind address for the WebSocket control port.
    pub websocket_bind_addr: String,
    /// WebSocket control port (default 9001).
    pub websocket_port: u16,
    /// Modem-engine receive ticker interval (ms). Lower = more responsive QSY, higher CPU.
    pub receive_tick_ms: u64,
}

/// QSY frequency-agility settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct QsyConfig {
    /// When false, all incoming QSY_REQ frames are rejected.
    pub enabled: bool,
    /// Trust levels whose QSY_REQ frames are accepted.
    /// Accepted values: "rejected", "low", "unverified", "reduced", "psk_verified", "verified"
    /// (kebab-case variants are also accepted).
    pub allow_trustlevels: Vec<String>,
    /// Bandplan mode for QSY and operating-mode guardrails.
    pub bandplan_mode: String,
    /// Enable bandplan awareness checks before selecting QSY frequencies.
    pub bandplan_awareness_enabled: bool,
    /// Enforce per-segment maximum occupied channel width.
    pub enforce_max_channel_width: bool,
    /// Enforce convention-bound digital/data segments.
    pub enforce_segment_conventions: bool,
    /// Candidate frequencies to scan during QSY negotiation (Hz).
    pub candidate_freqs_hz: Vec<u64>,
    /// Time to dwell on each candidate frequency while reading the S-meter (ms).
    pub scan_dwell_ms: u64,
    /// Seconds after QSY_ACK before both stations switch frequency.
    pub switchover_offset_s: u64,
    /// Allow invoking the rig's integrated tuner when SWR is high.
    pub allow_integrated_tuner_on_high_swr: bool,
    /// Auto-initiate a QSY when the receiver notch confirms a persistent **in-band** interferer
    /// (one a notch can't remove). Requires `[modem] notch_enabled` + `notch_persistence > 0` and
    /// `candidate_freqs_hz`. Default false.
    pub auto_qsy_on_interference: bool,
}

/// Station identity.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct StationConfig {
    pub callsign: String,
    pub grid_square: String,
    /// Path to the 32-byte Ed25519 identity seed used to sign handshake (CONREQ/CONACK) frames.
    /// Empty = the platform default (`~/.config/openpulse/identity.key`); set an explicit path to
    /// give co-located stations (e.g. the twin-station rig) distinct identities.
    pub identity_key_path: String,
    /// Periodic station-ID interval in seconds (REQ-REG-10). While transmitting, the daemon keys up
    /// and sends the callsign at least this often. `600` = every 10 minutes (US Part 97 default);
    /// `0` disables auto-ID entirely (operator IDs manually). A pure-receive station never keys up.
    pub auto_id_interval_secs: u64,
    /// End-of-exchange (sign-off) ID: seconds of TX quiet after the station has transmitted before a
    /// final ID is sent (REQ-REG-10 "at the end of a communication"). `10` = ID ~10 s after the last
    /// transmission of an exchange; `0` disables the sign-off ID (interval ID only). Only active when
    /// `auto_id_interval_secs > 0`.
    pub auto_id_signoff_idle_secs: u64,
}

/// Audio backend selection.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Audio backend: `cpal` (real hardware), `loopback` (testing), or
    /// `default` (use cpal if compiled in, loopback otherwise).
    pub backend: String,
    /// Audio device name (cpal backend). Empty = the system default device.
    /// Pin a specific full-duplex device here to target an `snd-aloop` PCM — the
    /// real-audio twin-station rig sets station A and B to crossed PCMs.
    pub device: String,
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
    /// Adaptive session profile (e.g. `"hpx_hf"`, `"hpx_ofdm_hf"`).
    ///
    /// Selects the SpeedLevel→mode ladder the rate controller and mode advisor use.
    /// See `SessionProfile::PROFILE_NAMES` in `openpulse-core`.
    pub profile: String,
    /// PTT backend: `none`, `rts`, `dtr`, `vox`, or `rigctld`.
    pub ptt_backend: String,
    /// Receiver-led OTA adaptive rate-stepping. When `true`, the daemon starts an
    /// OTA session at launch and drives it on the RX path.
    pub ota_enabled: bool,
    /// OTA session profile; falls back to [`profile`](Self::profile) when empty.
    pub ota_profile: String,
    /// Lowest OTA speed level (e.g. `"SL3"`); empty = the profile's natural floor.
    pub ota_min_level: String,
    /// Highest OTA speed level (e.g. `"SL10"`); empty = the profile's natural cap.
    pub ota_max_level: String,
    /// Lock OTA to a fixed speed level (e.g. `"SL6"`); empty = adapt normally.
    pub ota_lock_level: String,
    /// A2 gate: minimum queued TX bytes before an upgrade is acted on (0 = off).
    pub ota_min_backlog: usize,
    /// A3 gate: suppress this many upgrades after a downgrade (0 = off).
    pub ota_upgrade_hold_frames: u32,
    /// Aggressiveness preset (`conservative`/`balanced`/`aggressive`) that sets the
    /// A2/A3 gates together; empty = use the individual `ota_min_backlog` /
    /// `ota_upgrade_hold_frames` values above. The preset, when set, takes precedence.
    pub ota_aggressiveness: String,
    /// Default DCD/squelch RMS threshold (carrier-present level for channel-busy
    /// detection, CSMA, and burst-capture flush). Raise it above a band's noise
    /// floor. Applied at startup and as the fallback when no per-band value matches.
    pub dcd_squelch: f32,
    /// Per-band DCD/squelch override, keyed by band label (`"20m"`, `"2m"`, …).
    /// When the rig tunes into a listed band, that threshold is applied; otherwise
    /// `dcd_squelch` is used. Empty (default) = always use `dcd_squelch`.
    pub dcd_squelch_bands: std::collections::BTreeMap<String, f32>,
    /// CE-SSB TX envelope conditioning (raises average power at a fixed peak on
    /// high-PAPR multicarrier modes). Default `true`; it only acts on modes that
    /// benefit (OFDM/SC-FDMA), so it is a no-op for single-carrier modes.
    pub cessb_enabled: bool,
    /// Receiver-side automatic notch: removes out-of-band CW interference (QRM) before demod.
    /// Default `false`. The protected band tracks the active mode so the signal is never
    /// notched; an in-band interferer still can't be removed (that is a QSY case).
    pub notch_enabled: bool,
    /// Max simultaneous notches.
    pub notch_max: usize,
    /// Notch sharpness (BW ≈ f0 / q).
    pub notch_q: f32,
    /// Notch persistence: a tone must appear in this many signal-absent (silence) blocks before
    /// it counts as a confirmed external interferer. `0` (default) disables persistence tracking.
    /// When on, the notch nulls confirmed externals robustly and logs in-band ones as QSY hints.
    pub notch_persistence: u32,
}

/// Per-rig CAT settings (used in `[radio.rig_a]` / `[radio.rig_b]` sections).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RigConfig {
    /// rigctld TCP address for this rig (default `"127.0.0.1:4532"`). The only field the daemon
    /// currently consumes (via `[radio.rig_b]` for the cross-band repeater's TX PTT).
    pub rigctld_addr: String,
    /// **Reserved (multi-rig).** Per-rig CAT backend selector; the daemon reads the *top-level*
    /// `[radio] cat_backend` (`"rigctld"` / `"generic"` / `"none"`), not this per-rig copy.
    pub backend: String,
    /// **Reserved (multi-rig).** The active generic-backend serial port is the *top-level*
    /// `[radio] serial_port`; this per-rig copy is unread until the multi-rig refactor.
    pub serial_port: String,
    /// **Reserved (multi-rig).** The active generic-backend rig file is the *top-level*
    /// `[radio] rig_file`; this per-rig copy is unread until the multi-rig refactor.
    pub rig_file: String,
}

/// Rig CAT control settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RadioConfig {
    /// CAT (frequency/mode) backend: `"rigctld"` (default), `"generic"`, or `"none"`.
    ///
    /// `"none"` runs with no CAT control at all — no connection is attempted — for a TRX that
    /// Hamlib/rigctld does not support. `"generic"` drives a TOML-scripted serial rig (the
    /// `serial_port` and `rig_file` below); it requires the daemon to be built with the
    /// `generic-serial` feature and is Unix-only. The operator sets frequency manually if CAT is
    /// unavailable; PTT still works via the `[modem] ptt_backend` selection (`vox`/`rts`/`dtr`).
    pub cat_backend: String,
    /// rigctld TCP address for single-rig PTT-only use (default `"127.0.0.1:4532"`).
    pub rigctld_addr: String,
    /// Serial device for the `"generic"` CAT backend (e.g. `/dev/ttyUSB0`). Unused for rigctld/none.
    pub serial_port: String,
    /// Rig-definition TOML for the `"generic"` CAT backend (command templates + serial params).
    /// See `docs/config/rig-*.toml`. Unused for rigctld/none.
    pub rig_file: String,
    /// Rig meter (ALC/power-out/SWR) poll interval in milliseconds, emitted as
    /// `RigStatus` events for live operator drive-tuning. `0` disables polling.
    /// Default `500` (2 Hz). Uses a dedicated rigctld connection, so it never
    /// contends with PTT/frequency commands. Raise it for rigs with slow CAT.
    pub meter_poll_ms: u64,
    /// **Currently unused.** The primary rig is configured via the top-level `[radio] rigctld_addr`
    /// above (what the daemon's CAT/PTT and the repeater's RX side actually read); `[radio.rig_a]`
    /// is never consumed. Kept for the planned multi-rig refactor — see roadmap "Config/feature gaps".
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
    /// Milliseconds to hold PTT after the last byte is transmitted (half-duplex only).
    pub tx_hang_ms: u64,
    /// When true, PTT is held for the entire relay session.  `tx_hang_ms` is ignored.
    pub full_duplex: bool,
}

/// ARDOP TNC service settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ArdopConfig {
    pub bind_addr: String,
    pub cmd_port: u16,
    pub data_port: u16,
    /// Opt-in: run an adaptive ARQ session so the rate ladder, ARQBW, and ARQTIMEOUT take effect.
    /// Default false (fixed-mode operation, the historical behaviour).
    pub enable_adaptive_arq: bool,
    /// Session profile name for the adaptive ARQ ladder (e.g. `hpx500`, `hpx_hf`). Empty falls
    /// back to `hpx500`.
    pub adaptive_profile: String,
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
    /// Optional log file path (REQ-OBS-02). When set, logs are also appended to a
    /// daily-rolled file next to this path, in addition to stdout. `~` is expanded.
    #[serde(default)]
    pub file: Option<String>,
}

/// Multi-hop relay settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RelayConfig {
    pub enabled: bool,
    pub max_hops: u8,
    /// Store-and-forward frame TTL in seconds. Read by `openpulse-daemon`'s relay forwarder.
    pub store_forward_ttl_s: u64,
    /// Peer IDs (lower-hex, 64 chars each) whose frames are dropped at the first relay hop.
    pub deny_list: Vec<String>,
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
    /// Store-and-forward frame TTL in seconds. Read by `openpulse-mesh` daemon.
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
            identity_key_path: String::new(),
            auto_id_interval_secs: 600,
            auto_id_signoff_idle_secs: 10,
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            backend: "default".into(),
            device: String::new(),
            tx_limiter_threshold: 0.0,
        }
    }
}

impl Default for ModemConfig {
    fn default() -> Self {
        Self {
            mode: "BPSK250".into(),
            profile: "hpx_hf".into(),
            ptt_backend: "none".into(),
            ota_enabled: false,
            ota_profile: String::new(),
            ota_min_level: String::new(),
            ota_max_level: String::new(),
            ota_lock_level: String::new(),
            ota_min_backlog: 0,
            ota_upgrade_hold_frames: 0,
            ota_aggressiveness: String::new(),
            dcd_squelch: 0.01, // matches the engine's built-in DcdState default
            dcd_squelch_bands: std::collections::BTreeMap::new(),
            cessb_enabled: true,
            notch_enabled: false,
            notch_max: 10,
            notch_q: 25.0,
            notch_persistence: 0,
        }
    }
}

impl Default for RigConfig {
    fn default() -> Self {
        Self {
            rigctld_addr: "127.0.0.1:4532".into(),
            backend: "rigctld".into(),
            serial_port: String::new(),
            rig_file: String::new(),
        }
    }
}

impl Default for RadioConfig {
    fn default() -> Self {
        Self {
            cat_backend: "rigctld".into(),
            rigctld_addr: "127.0.0.1:4532".into(),
            serial_port: String::new(),
            rig_file: String::new(),
            meter_poll_ms: 500,
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
            full_duplex: false,
        }
    }
}

impl Default for ArdopConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1".into(),
            cmd_port: 8515,
            data_port: 8516,
            enable_adaptive_arq: false,
            adaptive_profile: "hpx500".into(),
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
            file: None,
        }
    }
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_hops: 3,
            store_forward_ttl_s: 300,
            deny_list: Vec::new(),
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
            bandplan_mode: "ham-iaru-r1".into(),
            bandplan_awareness_enabled: true,
            enforce_max_channel_width: true,
            enforce_segment_conventions: true,
            candidate_freqs_hz: vec![],
            scan_dwell_ms: 500,
            switchover_offset_s: 5,
            allow_integrated_tuner_on_high_swr: false,
            auto_qsy_on_interference: false,
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            tcp_bind_addr: "127.0.0.1".into(),
            tcp_port: 9000,
            websocket_bind_addr: "127.0.0.1".into(),
            websocket_port: 9001,
            receive_tick_ms: 50,
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
        // Refuse a group/world-readable identity key (REQ-SEC-CTL-05).
        secret_file::validate_owner_only(path)?;
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
    // Create with owner-only permissions atomically on Unix to avoid a window
    // where the file exists with broader umask-derived permissions.
    #[cfg(unix)]
    {
        use std::io::{ErrorKind, Write};
        use std::os::unix::fs::OpenOptionsExt;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(mut f) => f.write_all(&seed)?,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                // Another process won the race and already created the file.
                let bytes = std::fs::read(path)?;
                if bytes.len() != 32 {
                    return Err(ConfigError::IdentityKeyLength);
                }
                seed.copy_from_slice(&bytes);
            }
            Err(e) => return Err(e.into()),
        }
    }
    #[cfg(not(unix))]
    std::fs::write(path, seed)?;
    Ok(seed)
}
/// Returns the platform-standard identity key file path.
///
/// On Linux: `~/.config/openpulse/identity.key`
fn default_identity_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("openpulse").join("identity.key"))
}

/// Persist updated QSY settings to the platform config file.
///
/// Loads the existing config (falling back to defaults), updates the QSY
/// fields, then rewrites the file. Returns an error when the config directory
/// cannot be determined.
pub fn save_qsy_config(
    qsy_enabled: bool,
    bandplan_mode: &str,
    allow_integrated_tuner_on_high_swr: bool,
) -> Result<(), ConfigError> {
    let path = match default_config_path() {
        Some(p) => p,
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "cannot determine config directory",
            )
            .into())
        }
    };
    save_qsy_config_to_path(
        &path,
        qsy_enabled,
        bandplan_mode,
        allow_integrated_tuner_on_high_swr,
    )
}

/// Persist updated QSY settings to an explicit config path.
///
/// This is used by tests and tooling that need deterministic file locations.
pub fn save_qsy_config_to_path(
    path: &Path,
    qsy_enabled: bool,
    bandplan_mode: &str,
    allow_integrated_tuner_on_high_swr: bool,
) -> Result<(), ConfigError> {
    let mut cfg = load_from(path).unwrap_or_default();
    cfg.qsy.enabled = qsy_enabled;
    cfg.qsy.allow_integrated_tuner_on_high_swr = allow_integrated_tuner_on_high_swr;
    if bandplan_mode == "unrestricted" {
        cfg.qsy.bandplan_awareness_enabled = false;
    } else {
        cfg.qsy.bandplan_awareness_enabled = true;
        cfg.qsy.bandplan_mode = bandplan_mode.to_string();
    }
    let toml_str =
        toml::to_string_pretty(&cfg).map_err(|e| std::io::Error::other(e.to_string()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml_str)?;
    Ok(())
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
# Path to the 32-byte Ed25519 identity seed used to sign handshake (CONREQ/CONACK) frames.
# Empty = the platform default (~/.config/openpulse/identity.key), generated on first run.
# Set an explicit path to give co-located stations (e.g. the twin rig) distinct identities.
identity_key_path = ""
# Periodic station-ID interval in seconds (regulatory: e.g. US Part 97 = every 10 minutes).
# While transmitting, the daemon keys up and sends your callsign at least this often.
# 0 disables auto-ID entirely (you identify manually). A pure-receive station never keys up.
auto_id_interval_secs = 600
# End-of-exchange (sign-off) ID: seconds of transmit quiet before a final ID is sent after you
# have transmitted (regulatory "identify at the end of a communication"). 10 = ID ~10 s after the
# last transmission of an exchange; 0 disables the sign-off ID (interval ID only).
auto_id_signoff_idle_secs = 10

[audio]
# Audio backend: default | cpal | loopback
#   default  — use cpal if compiled in, loopback otherwise (recommended)
#   cpal     — always use the real sound card (error if not compiled in)
#   loopback — software loopback for testing only, no audio hardware required
backend = "default"
# Audio device name for the cpal backend. Empty = the system default device.
# Pin a specific device (e.g. an snd-aloop PCM) to target a fixed full-duplex
# device — the real-audio twin-station rig sets stations A and B to crossed PCMs.
device = ""
# Soft TX limiter threshold (0.0 = disabled). Typical value: 1.5 × RMS of the
# modulated signal. Prevents ADC clipping and reduces PA non-linearity on peaks.
# tx_limiter_threshold = 0.0

[modem]
# Default modulation mode used when no --mode flag is provided.
# Available: BPSK31, BPSK63, BPSK100, BPSK250, QPSK125, QPSK250, QPSK500,
#            QPSK1000, 8PSK500, 8PSK1000, 64QAM500, 64QAM1000, 64QAM2000-RRC,
#            OFDM16, OFDM52, SCFDMA16, SCFDMA52, SCFDMA52-16QAM,
#            SCFDMA52-64QAM, SCFDMA52-64QAM-P4, FSK4-ACK
mode = "BPSK250"
# Adaptive session profile (SpeedLevel ladder) used by the rate controller and
# `openpulse mode-advisor`. Available: hpx500, hpx_hf, hpx_ofdm_hf, hpx_wideband,
# hpx_wideband_hd, hpx_narrowband, hpx_narrowband_hd. hpx_ofdm_hf is the OFDM
# higher-order (high-throughput/high-reliability) HF ladder.
profile = "hpx_hf"
# PTT backend: none | rts | dtr | vox | rigctld
ptt_backend = "none"
# Receiver-led OTA adaptive rate-stepping. When true the daemon starts an OTA
# session at launch and drives it on the RX path (the data receiver leads the
# rate per direction; the sender follows an absolute recommendation in the ACK).
ota_enabled = false
# OTA session profile; empty falls back to `profile` above.
ota_profile = ""
# Clamp the OTA ladder. Empty = the profile's natural floor/cap. e.g. "SL3" / "SL10".
ota_min_level = ""
ota_max_level = ""
# Lock OTA to a fixed level (manual override); empty = adapt normally. e.g. "SL6".
ota_lock_level = ""
# A2 gate: minimum queued TX bytes before an upgrade is acted on (0 = off).
ota_min_backlog = 0
# A3 gate: suppress this many upgrade attempts after a downgrade (0 = off).
ota_upgrade_hold_frames = 0
# Aggressiveness preset: conservative | balanced | aggressive. Sets the A2/A3
# gates together (one knob instead of two). Empty = use the two values above.
# Takes precedence over ota_min_backlog / ota_upgrade_hold_frames when set.
ota_aggressiveness = ""
# DCD/squelch RMS threshold (carrier-present level for channel-busy detection,
# CSMA, and burst-capture flush). Raise above a band's noise floor if the carrier
# never appears to "drop". Applied at startup and as the per-band fallback.
dcd_squelch = 0.01
# Optional per-band squelch overrides (band label → threshold). When the rig tunes
# into a listed band the matching value is applied; otherwise dcd_squelch is used.
# [modem.dcd_squelch_bands]
# "40m" = 0.05
# "20m" = 0.02
# "2m"  = 0.01
# CE-SSB TX envelope conditioning: raises average power at a fixed peak on
# high-PAPR multicarrier modes (OFDM/SC-FDMA). No-op for single-carrier modes.
cessb_enabled = true
# Receiver-side automatic notch: removes out-of-band CW interference (QRM) before
# demod. The protected band tracks the active mode, so the signal is never notched;
# an in-band interferer can't be removed this way (that is a QSY case). Default off.
notch_enabled = false
# Max simultaneous notches, and notch sharpness (bandwidth ~= f0 / notch_q).
notch_max = 10
notch_q = 25.0
# Persistence: a tone must appear in this many silence (signal-absent) blocks before it
# is treated as a confirmed external interferer. 0 = off. When on, externally-confirmed
# tones are notched robustly and confirmed in-band tones are logged as QSY hints.
notch_persistence = 0

[radio]
# CAT (frequency/mode) backend: "rigctld" (default) or "none".
# "none" runs with no CAT control — no rigctld connection is attempted — for a
# TRX that Hamlib/rigctld does not support. Set frequency manually on the radio;
# PTT still works via [modem] ptt_backend (vox/rts/dtr). Set-freq and QSY retune
# are rejected while CAT is disabled.
cat_backend = "rigctld"
# rigctld TCP address for single-rig PTT-only use.
rigctld_addr = "127.0.0.1:4532"
# "generic" CAT backend (TOML-scripted serial; requires the `generic-serial` build feature, Unix):
serial_port = ""           # e.g. /dev/ttyUSB0
rig_file = ""              # e.g. docs/config/rig-icom-ic7300.toml
# Rig meter (ALC / power-out / SWR) poll interval in ms, surfaced as live RigStatus
# events for drive tuning. 0 disables. Uses a separate rigctld connection.
meter_poll_ms = 500

# The "generic" serial CAT backend IS wired: set cat_backend = "generic" with the top-level
# serial_port + rig_file above (build with --features generic-serial, Unix only).
# [radio.rig_a] remains UNUSED — the primary rig is the top-level [radio] config above; rig_a is
# reserved for the planned multi-rig refactor (roadmap "Config/feature gaps").

# Secondary rig: the daemon reads ONLY its rigctld_addr, for the cross-band repeater's TX PTT.
# Uncomment to enable the repeater's second rig.
# [radio.rig_b]
# rigctld_addr = "127.0.0.1:4533"

[repeater]
# Enable the cross-band repeater (RX uses the top-level [radio] rig; TX uses [radio.rig_b]).
enabled = false
# Modulation mode used for both RX (rig_a) and TX (rig_b).
mode = "BPSK250"
# Milliseconds to hold PTT after the last byte is transmitted (half-duplex only).
tx_hang_ms = 500
# Hold PTT for the entire relay session instead of per-frame assert/release.
# full_duplex = false

[ardop]
# IP address the ARDOP TNC listens on.
bind_addr = "127.0.0.1"
# ARDOP command port.
cmd_port = 8515
# ARDOP data port.
data_port = 8516
# Opt-in: run an adaptive ARQ session so the rate ladder + host ARQBW/ARQTIMEOUT take effect.
# Default false = fixed-mode operation (ARQBW/ARQTIMEOUT are accepted-and-echoed no-ops).
enable_adaptive_arq = false
# Session profile for the adaptive ladder (e.g. hpx500, hpx_hf); empty falls back to hpx500.
adaptive_profile = "hpx500"

[kiss]
# IP address the KISS TNC listens on.
bind_addr = "127.0.0.1"
# KISS TCP port.
port = 8100

[logging]
# Log verbosity: error | warn | info | debug | trace
level = "info"
# Optional persistent log file (REQ-OBS-02). When set, logs are appended to a
# daily-rolled file (<path>.YYYY-MM-DD) in addition to stdout. `~` is expanded.
# Read by openpulse-daemon. RUST_LOG still overrides `level`.
# file = "~/.local/share/openpulse/openpulse.log"

[observability]
# Audit mode (REQ-OBS-01): when enabled, openpulse-daemon records its control-event
# stream to <archive_dir>/events.ndjson for later analysis. Off by default. `~` expanded.
audit_mode = false
archive_dir = "~/.local/share/openpulse/audit"

[relay]
# Enable multi-hop relay forwarding (used by openpulse-daemon).
enabled = false
# Maximum relay hop count.
max_hops = 3
# Store-and-forward frame TTL in seconds (read by openpulse-daemon relay forwarder).
store_forward_ttl_s = 300

[mesh]
# Enable the openpulse-mesh daemon relay stack (used by openpulse-mesh binary).
enabled = false
# Maximum relay hop count before a frame is dropped.
max_hops = 3
# Relay trust policy: strict | balanced | permissive
relay_policy = "balanced"
# Store-and-forward frame TTL in seconds (read by openpulse-mesh binary).
store_forward_ttl_s = 300
# Peer discovery beacon interval in seconds.
beacon_interval_s = 60
# Maximum peer cache entries.
peer_cache_capacity = 256
# Peer cache entry TTL in seconds.
peer_cache_ttl_s = 3600

[trust]
# Path to the local trust store file. Empty = platform default.
store_path = ""

[daemon]
# TCP control port (openpulse-daemon).
tcp_bind_addr = "127.0.0.1"
tcp_port = 9000
# WebSocket control port (openpulse-daemon).
websocket_bind_addr = "127.0.0.1"
websocket_port = 9001
# Modem receive ticker interval (ms). Lower = more responsive RF reception, higher CPU.
receive_tick_ms = 50

# [qsy]
# Enable QSY frequency-agility negotiation.  Requires hamlib rigctld configured in [radio].
# enabled = false
# Trust levels allowed to initiate QSY with this station.
# allow_trustlevels = ["verified", "psk_verified"]
# Bandplan-awareness mode: ham-iaru-r1 | ham-iaru-r2 | ham-iaru-r3
# bandplan_mode = "ham-iaru-r1"
# Enforce bandplan guardrails for QSY (enabled by default).
# Set to false only as an explicit responsible-operator compliance override.
# bandplan_awareness_enabled = true
# Enforce per-segment occupied bandwidth limits for the active modem mode.
# enforce_max_channel_width = true
# Enforce convention-bound digital/data segments.
# enforce_segment_conventions = true
# Candidate frequencies to evaluate during a QSY scan (Hz).
# candidate_freqs_hz = [14070000, 14074000, 14077000]
# How long to dwell on each candidate while reading the S-meter (ms).
# scan_dwell_ms = 500
# Seconds between QSY_ACK and the actual frequency switch.
# switchover_offset_s = 5
# Allow integrated tuner operation when high SWR is detected.
# allow_integrated_tuner_on_high_swr = false
# Auto-initiate a QSY when the receiver notch confirms a persistent in-band interferer (one a
# notch can't remove). Requires [modem] notch_enabled + notch_persistence > 0 and candidate_freqs_hz.
# auto_qsy_on_interference = false

[logbook]
# Automatic ADIF logbook: append one record per completed contact (a connect→disconnect
# session) so logs import into standard logging software / LoTW / eQSL. Opt-in.
enabled = false
# Path to the .adi file (a header is written on first record).
adif_path = "~/.local/share/openpulse/openpulse.adi"
# Optional callsign → grid lookup to fill the worked station's GRIDSQUARE (case-insensitive).
# [logbook.peer_grids]
# "DL1ABC" = "JO31aa"
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
        // CAT defaults to rigctld for backward compatibility.
        assert_eq!(cfg.radio.cat_backend, "rigctld");
        // Rig meter polling defaults to 2 Hz (500 ms).
        assert_eq!(cfg.radio.meter_poll_ms, 500);
        // Audio device defaults to empty (system default device).
        assert_eq!(cfg.audio.backend, "default");
        assert_eq!(cfg.audio.device, "");
        // Receiver auto-notch is opt-in.
        assert!(!cfg.modem.notch_enabled);
        assert_eq!(cfg.modem.notch_max, 10);
        assert_eq!(cfg.modem.notch_q, 25.0);
        assert_eq!(cfg.modem.notch_persistence, 0);
        // Auto-QSY on interference is opt-in.
        assert!(!cfg.qsy.auto_qsy_on_interference);
        // ADIF logbook is opt-in.
        assert!(!cfg.logbook.enabled);
        assert!(cfg.logbook.adif_path.ends_with(".adi"));
        assert!(cfg.logbook.peer_grids.is_empty());
    }

    #[test]
    fn dcd_squelch_per_band_parses() {
        let path = unique_tmp("dcd-squelch");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "[modem]").unwrap();
            writeln!(f, "dcd_squelch = 0.02").unwrap();
            writeln!(f, "[modem.dcd_squelch_bands]").unwrap();
            writeln!(f, r#""40m" = 0.05"#).unwrap();
            writeln!(f, r#""2m" = 0.015"#).unwrap();
        }
        let cfg = load_from(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!((cfg.modem.dcd_squelch - 0.02).abs() < 1e-6);
        assert_eq!(cfg.modem.dcd_squelch_bands.get("40m").copied(), Some(0.05));
        assert_eq!(cfg.modem.dcd_squelch_bands.get("2m").copied(), Some(0.015));
    }

    #[test]
    fn cat_backend_none_parses() {
        let path = unique_tmp("cat-none");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "[radio]").unwrap();
            writeln!(f, r#"cat_backend = "none""#).unwrap();
        }
        let cfg = load_from(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.radio.cat_backend, "none");
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
        assert_eq!(cfg.modem.profile, "hpx_hf");
        assert_eq!(cfg.modem.ota_aggressiveness, ""); // empty = use individual A2/A3 knobs
        assert!((cfg.modem.dcd_squelch - 0.01).abs() < 1e-6);
        assert!(cfg.modem.dcd_squelch_bands.is_empty());
    }

    #[test]
    fn modem_profile_loads_and_template_parses() {
        // Override the profile in a config file and confirm it round-trips.
        let path = unique_tmp("profile");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "[modem]").unwrap();
            writeln!(f, r#"profile = "hpx_ofdm_hf""#).unwrap();
        }
        let cfg = load_from(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.modem.profile, "hpx_ofdm_hf");

        // The emitted template must parse and carry the documented default.
        let parsed: OpenpulseConfig = toml::from_str(&init_template()).unwrap();
        assert_eq!(parsed.modem.profile, "hpx_hf");
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

    #[cfg(unix)]
    #[test]
    fn load_identity_refuses_group_readable_key() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("openpulse_id_perm_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let key_path = tmp.join("identity.key");
        std::fs::write(&key_path, [7u8; 32]).unwrap();
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o640)).unwrap();

        assert!(
            matches!(
                load_identity_from(&key_path),
                Err(ConfigError::InsecureSecretPermissions { .. })
            ),
            "a group-readable identity key must be refused (REQ-SEC-CTL-05)"
        );

        let _ = std::fs::remove_file(&key_path);
        let _ = std::fs::remove_dir(&tmp);
    }

    #[test]
    fn save_qsy_config_to_path_persists_unrestricted_mode() {
        let path = unique_tmp("qsy_unrestricted");
        let _ = std::fs::remove_file(&path);

        save_qsy_config_to_path(&path, true, "unrestricted", true).unwrap();

        let cfg = load_from(&path).unwrap();
        assert!(cfg.qsy.enabled);
        assert!(!cfg.qsy.bandplan_awareness_enabled);
        assert!(cfg.qsy.allow_integrated_tuner_on_high_swr);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_qsy_config_to_path_persists_bandplan_mode() {
        let path = unique_tmp("qsy_bandplan");
        let _ = std::fs::remove_file(&path);

        save_qsy_config_to_path(&path, true, "ham-iaru-r2", false).unwrap();

        let cfg = load_from(&path).unwrap();
        assert!(cfg.qsy.enabled);
        assert!(cfg.qsy.bandplan_awareness_enabled);
        assert_eq!(cfg.qsy.bandplan_mode, "ham-iaru-r2");
        assert!(!cfg.qsy.allow_integrated_tuner_on_high_swr);
        let _ = std::fs::remove_file(&path);
    }
}
