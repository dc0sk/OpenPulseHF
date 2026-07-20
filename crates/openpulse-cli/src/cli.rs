use clap::{Parser, Subcommand};

/// Clap value parser for `--profile`, seeded from the profile registry.
///
/// `PossibleValuesParser` alone would be wrong: `SessionProfile::by_name` is case-insensitive and
/// treats `-` and `_` as interchangeable, so an exact-match parser rejects `HPX-OFDM-HF`, which is
/// valid. This validates through `by_name` (preserving that flexibility) while still reporting the
/// canonical names to clap, so `--help` lists all of them and the list cannot drift.
#[derive(Clone)]
struct ProfileNameParser;

impl clap::builder::TypedValueParser for ProfileNameParser {
    type Value = String;

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let raw = value.to_string_lossy();
        if openpulse_core::profile::SessionProfile::by_name(&raw).is_some() {
            return Ok(raw.into_owned());
        }
        Err(clap::Error::raw(
            clap::error::ErrorKind::InvalidValue,
            format!(
                "unknown session profile '{raw}'\n  expected one of: {}\n",
                openpulse_core::profile::SessionProfile::PROFILE_NAMES.join(", ")
            ),
        ))
    }

    fn possible_values(
        &self,
    ) -> Option<Box<dyn Iterator<Item = clap::builder::PossibleValue> + '_>> {
        Some(Box::new(
            openpulse_core::profile::SessionProfile::PROFILE_NAMES
                .iter()
                .map(|n| clap::builder::PossibleValue::new(*n)),
        ))
    }
}

fn profile_name_parser() -> ProfileNameParser {
    ProfileNameParser
}

use crate::commands;
use crate::output::DiagnosticOptions;

#[derive(Parser)]
#[command(
    name = "openpulse",
    about = "OpenPulse software modem for amateur radio data transmission",
    long_about = "OpenPulse software modem for amateur radio data transmission.",
    author,
    version
)]
pub struct Cli {
    /// Audio backend to use.
    #[arg(long, global = true, default_value = "default")]
    pub backend: String,

    /// Verbosity level: error | warn | info | debug | trace.
    #[arg(long, global = true, default_value = "info")]
    pub log: String,

    /// PKI API base URL used by identity/trust diagnostics.
    #[arg(long, global = true, default_value = "http://127.0.0.1:8787")]
    pub pki_url: String,

    /// PTT backend: none | rts | dtr | vox | rigctld | cm108 | gpio.
    #[arg(long, global = true, default_value = "none")]
    pub ptt: String,

    /// PTT target for --ptt: serial path (rts/dtr), rigctld addr, /dev/hidrawN (cm108), or chip:line (gpio).
    #[arg(long, global = true, default_value = "")]
    pub rig: String,

    /// Path to TOML rig-definition file for the generic serial CAT backend.
    #[arg(long, global = true, default_value = "")]
    pub rig_file: String,

    /// Maximum TX power in watts for regulatory compliance (default: 100).
    #[arg(long, global = true, default_value = "100")]
    pub max_power: f32,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Transmit data over the air.
    Transmit {
        data: String,
        #[arg(short, long, default_value = "BPSK100")]
        mode: String,
        #[arg(short, long)]
        device: Option<String>,
        /// Forward error correction codec. One of: none, rs, rs-interleaved,
        /// concatenated, rs-strong, soft-concatenated, ldpc, turbo. The receiver
        /// must pass the same value.
        #[arg(long, default_value = "none")]
        fec: String,
        /// Audio carrier center frequency in Hz (default: 1500).
        /// Use when the receive side expects the signal at a specific audio
        /// frequency due to rig VFO offset (e.g. --center-frequency 2550).
        #[arg(long, default_value = "1500")]
        center_frequency: f32,
    },
    /// Receive data and print to stdout.
    Receive {
        #[arg(short, long, default_value = "BPSK100")]
        mode: String,
        #[arg(short, long)]
        device: Option<String>,
        /// Forward error correction codec; must match the transmitter's `--fec`.
        /// Timeout (`--listen-ms`) reception supports: none, rs, rs-interleaved,
        /// soft-concatenated, ldpc.
        #[arg(long, default_value = "none")]
        fec: String,
        /// Listen for up to this many milliseconds before giving up.
        #[arg(long)]
        listen_ms: Option<u64>,
        /// Audio carrier center frequency in Hz (default: 1500).
        /// Use when the transmitting station's signal arrives at a different
        /// audio frequency due to rig VFO offset (e.g. --center-frequency 450).
        #[arg(long, default_value = "1500")]
        center_frequency: f32,
        /// Disable automatic frequency correction (AFC) settling.
        ///
        /// Use when the transmitter and receiver share the same audio path
        /// (loopback cable, direct USB audio) and no carrier frequency offset
        /// is expected.  AFC can produce spurious corrections when applied to
        /// near-zero-offset signals, shifting the demodulator off the true
        /// carrier.
        #[arg(long, default_value = "false")]
        no_afc: bool,
    },
    /// List available audio devices.
    Devices,
    /// List registered modulation modes.
    Modes {
        /// Print one `MODE<TAB>SECONDS` line per mode: the airtime of the largest frame the mode can
        /// send at 8 kHz. Test harnesses use this to size transmit and listen windows per mode
        /// instead of guessing a fixed one.
        #[arg(long)]
        airtime: bool,
    },
    /// Recommend a speed level and mode for the current SNR.
    ModeAdvisor {
        /// Estimated signal-to-noise ratio in dB.
        #[arg(long)]
        snr: f32,
        /// Session profile (overrides config `[modem] profile`).
        ///
        /// The accepted values come from `SessionProfile::PROFILE_NAMES`, so this list cannot drift
        /// from what `by_name` actually accepts — it previously advertised 7 of the 12.
        #[arg(long, value_parser = profile_name_parser())]
        profile: Option<String>,
    },
    /// Export session performance metrics (throughput, FER, latency, SNR estimate).
    SessionMetrics {
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Package audit-mode artifacts (events.ndjson, snapshot.json, logs) into a .tar.gz (REQ-OBS-03).
    AuditBundle {
        /// Audit archive dir (default: `[observability] archive_dir` from config).
        #[arg(long)]
        archive_dir: Option<String>,
        /// Output directory for the bundle (default: `<archive_dir>/bundles`).
        #[arg(long)]
        output: Option<String>,
        /// Optional label appended to the bundle file name.
        #[arg(long)]
        label: Option<String>,
    },
    /// Identity diagnostics.
    Identity {
        #[command(subcommand)]
        command: IdentityCommands,
    },
    /// Trust diagnostics and policy commands.
    Trust {
        #[command(subcommand)]
        command: TrustCommands,
    },
    /// Session and handshake diagnostics.
    Diagnose {
        #[command(subcommand)]
        command: DiagnoseCommands,
    },
    /// HPX session lifecycle management.
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },
    /// HPX benchmark harness.
    Benchmark {
        #[command(subcommand)]
        command: BenchmarkCommands,
    },
    /// Stream engine events as newline-delimited JSON to stdout.
    Monitor {
        /// Modulation mode to drive the receive loop.
        #[arg(short, long, default_value = "BPSK100")]
        mode: String,
    },
    /// Run an adaptive rate-control session over a simulated channel and report
    /// each speed-level transition (loopback/channel-sim; no hardware required).
    Adaptive {
        /// Session profile (overrides config `[modem] profile`).
        ///
        /// The accepted values come from `SessionProfile::PROFILE_NAMES`, so this list cannot drift
        /// from what `by_name` actually accepts — it previously advertised 7 of the 12.
        #[arg(long, value_parser = profile_name_parser())]
        profile: Option<String>,
        /// Channel model: clean, awgn, watterson-good-f1, watterson-poor-f1.
        #[arg(long, default_value = "clean")]
        channel: String,
        /// AWGN SNR in dB (used by `--channel awgn`, and as the rate-adapter SNR
        /// hint when the receiver cannot measure one).
        #[arg(long)]
        snr: Option<f32>,
        /// Number of frames to send.
        #[arg(long, default_value_t = 8)]
        frames: usize,
        /// Payload length per frame, in bytes.
        #[arg(long, default_value_t = 64)]
        payload_len: usize,
        /// A2 backlog gate: minimum queued TX backlog (bytes) an ACK-UP must see
        /// before it may upgrade the rate; 0 disables the gate. The session feeds
        /// the shrinking backlog automatically as the frame queue drains, so the
        /// final upgrade is withheld once too little data remains to benefit.
        #[arg(long, default_value_t = 0)]
        min_backlog: usize,
        /// Deterministic channel seed.
        #[arg(long)]
        seed: Option<u64>,
        /// Emit newline-delimited JSON instead of human-readable lines.
        #[arg(long)]
        json: bool,
    },
    /// Reliable two-way ARQ over the modem (FSK4 ACK return + retransmit).
    ///
    /// Targets VOX or wired/full-duplex audio paths (keying is per transmission).
    Arq {
        #[command(subcommand)]
        command: ArqCommands,
    },
    /// Configuration management.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Transmit a one-to-many broadcast frame (no ACK, no session required).
    Broadcast {
        /// Payload to broadcast (UTF-8 text or `0x`-prefixed hex bytes).
        #[arg(short, long)]
        payload: String,
        /// Modulation mode.
        #[arg(short, long, default_value = "BPSK250")]
        mode: String,
        /// Maximum relay hops (TTL).
        #[arg(long, default_value_t = 3)]
        ttl: u8,
        /// Station callsign embedded in the frame header.
        #[arg(long, default_value = "")]
        callsign: String,
    },
    /// Send periodic station-ID beacons for regulatory compliance.
    Beacon {
        /// Modulation mode.
        #[arg(short, long, default_value = "BPSK250")]
        mode: String,
        /// Beacon interval in seconds.
        #[arg(long, default_value_t = 600)]
        interval: u64,
        /// Station callsign included in each beacon.
        #[arg(long)]
        callsign: String,
        /// Maximum relay hops (TTL).
        #[arg(long, default_value_t = 1)]
        ttl: u8,
    },
    /// QSY frequency-agility negotiation.
    Qsy {
        #[command(subcommand)]
        command: QsyCommands,
    },
    /// On-device audio/PTT/AFC calibration checks.
    Calibrate {
        #[command(subcommand)]
        command: commands::calibrate::CalibrateCommands,
        /// Write JSON result to this path (in addition to stdout).
        #[arg(long)]
        output: Option<std::path::PathBuf>,
    },
    /// Control a running openpulse-server daemon via its NDJSON-over-TCP port.
    Daemon {
        /// Daemon control address (host:port).
        #[arg(long, default_value = "127.0.0.1:9000")]
        addr: String,
        #[command(subcommand)]
        command: DaemonCommands,
    },
}

#[derive(Subcommand, Clone)]
pub enum DaemonCommands {
    /// Initiate an RF connection to a peer callsign via the TNC.
    ConnectPeer { callsign: String },
    /// Disconnect the current RF peer connection.
    DisconnectPeer,
    /// Switch the active modem mode at runtime (e.g. BPSK250, QPSK500).
    SetMode { mode: String },
    /// Tune the rig frequency (Hz) via CAT. `rig` is the control target (currently `rigctld`).
    SetFreq {
        #[arg(long, default_value = "rigctld")]
        rig: String,
        freq_hz: u64,
    },
    /// Assert PTT (key the transmitter).
    PttAssert,
    /// Release PTT (unkey the transmitter).
    PttRelease,
    /// Print the daemon's current PTT state (`{"active": true|false}`) — a resync for a client that
    /// missed an edge-triggered PttChanged event.
    PttState,
    /// Accept a pending incoming QSY negotiation by token.
    AcceptQsy { token: String },
    /// Reject a pending incoming QSY negotiation by token.
    RejectQsy { token: String },
    /// Queue an outgoing message to a peer callsign for RF delivery.
    SendMessage {
        #[arg(long)]
        to: String,
        #[arg(long, default_value = "")]
        subject: String,
        #[arg(long)]
        body: String,
    },
    /// Print the full inbox listing as JSON.
    ListMessages,
    /// Fetch the full body of a single message by ID.
    GetMessage { id: u64 },
    /// Delete a stored message by ID.
    DeleteMessage { id: u64 },
    /// Enable the cross-band repeater.
    EnableRepeater,
    /// Disable the cross-band repeater.
    DisableRepeater,
    /// Enable JS8 station discovery (RX-only: dwell on the band's JS8 calling frequency).
    EnableDiscovery,
    /// Disable JS8 station discovery and (if dwelling) return to the home frequency.
    DisableDiscovery,
    /// Print discovered JS8 stations as JSON (`is_opulse` flags OpenPulse peers).
    Stations,
    /// Print recognized OpenPulse peers (capabilities, quality, trust) from the shared cache as JSON.
    Peers,
    /// Send a local file to a peer callsign over RF (direct P2P `OPFX` transfer).
    SendFile {
        /// Recipient callsign.
        to: String,
        /// Path to the local file to send.
        path: String,
    },
    /// Accept a pending inbound file offer by transfer id.
    AcceptFile { transfer_id: u32 },
    /// Reject a pending inbound file offer by transfer id.
    RejectFile { transfer_id: u32 },
    /// Cancel an in-flight file transfer by transfer id.
    CancelFile { transfer_id: u32 },
    /// Print files received this session as JSON.
    Files,
    /// Stream binary spectrum frames as NDJSON to stdout.
    SubscribeSpectrum {
        /// Frames per second requested from the daemon.
        #[arg(long, default_value_t = 10)]
        fps: u32,
        /// Stop after this many frames; 0 = stream until interrupted.
        #[arg(long, default_value_t = 0)]
        frames: u32,
    },
    /// Print the daemon's current runtime configuration as JSON.
    GetConfig,
    /// Update runtime configuration; omitted fields are preserved.
    SetConfig {
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        tx_attenuation_db: Option<f32>,
        #[arg(long)]
        qsy_enabled: Option<bool>,
        #[arg(long)]
        bandplan_mode: Option<String>,
    },
    /// Start a receiver-led OTA adaptive rate session with the named profile.
    OtaStart {
        /// Session profile (e.g. hpx_hf, hpx_modcod).
        #[arg(long)]
        profile: String,
    },
    /// Stop the active OTA session.
    OtaStop,
    /// Clamp the OTA ladder to a min/max level (e.g. SL3 / SL10). Omit for profile bound.
    OtaBounds {
        #[arg(long)]
        min: Option<String>,
        #[arg(long)]
        max: Option<String>,
    },
    /// Lock OTA to a fixed level (manual override; e.g. SL6).
    OtaLock {
        #[arg(long)]
        level: String,
    },
    /// Release the OTA level lock and resume adapting.
    OtaUnlock,
    /// Tune the rate-adaptation hysteresis (anti-oscillation) gates at runtime.
    OtaHysteresis {
        /// Min queued TX backlog (bytes) before acting on an upgrade; 0 disables.
        #[arg(long)]
        min_backlog: Option<usize>,
        /// Upgrade attempts to suppress after a downgrade; 0 disables.
        #[arg(long)]
        upgrade_hold_frames: Option<u32>,
    },
    /// Apply an aggressiveness preset (sets the A2/A3 gates together).
    OtaAggressiveness {
        /// conservative | balanced | aggressive
        preset: String,
    },
    /// Set the DCD/squelch RMS threshold at runtime (e.g. 0.05 on a noisy band).
    SetDcdSquelch {
        /// RMS threshold (0.0–1.0); raise above the band noise floor.
        threshold: f32,
    },
    /// Enable/disable CE-SSB TX envelope conditioning (multicarrier modes only).
    SetCessb {
        /// true to enable, false to disable.
        #[arg(action = clap::ArgAction::Set)]
        enabled: bool,
    },
    /// Enable/disable the receiver-side automatic notch (removes out-of-band CW interference).
    SetNotch {
        /// true to enable, false to disable.
        #[arg(action = clap::ArgAction::Set)]
        enabled: bool,
    },
    /// Enable/disable the receiver-side streaming AGC (normalises captured level before demod).
    SetAgc {
        /// true to enable, false to disable.
        #[arg(action = clap::ArgAction::Set)]
        enabled: bool,
    },
    /// Enable/disable the automatic ADIF logbook (one record per connect→disconnect).
    SetLogbook {
        /// true to enable, false to disable.
        #[arg(action = clap::ArgAction::Set)]
        enabled: bool,
    },
    /// Set the TX attenuation (dB; 0 = none) for the current band, or a named band.
    SetTxAttenuation {
        /// Attenuation in dB (0.0 = no attenuation).
        db: f32,
        /// Optional band label (e.g. `20m`); omit to apply to the current band.
        #[arg(long)]
        band: Option<String>,
    },
    /// Print one OTA status snapshot as JSON.
    OtaStatus,
}

#[derive(Subcommand)]
pub enum BenchmarkCommands {
    /// Run the standard HPX benchmark corpus and emit a JSON report.
    Run {
        #[arg(long, default_value_t = 1.0)]
        min_pass_rate: f64,
        #[arg(long, default_value_t = 20.0)]
        max_mean_transitions: f64,
    },
}

#[derive(Subcommand)]
pub enum SessionCommands {
    /// Start a new secure HPX session with a peer.
    Start {
        #[arg(long)]
        peer: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Show current HPX session state.
    State {
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Resume from a persisted session snapshot.
    Resume {
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// List available session snapshots.
    List {
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// End the active HPX session gracefully.
    End {
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Show HPX state transition log for the current session.
    Log {
        #[arg(long)]
        follow: bool,
        #[arg(long, default_value_t = 5_000)]
        follow_timeout_ms: u64,
        #[arg(long, default_value_t = 250)]
        poll_interval_ms: u64,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
}

#[derive(Subcommand)]
pub enum IdentityCommands {
    /// Show identity summary for a station/record.
    Show {
        station_or_record_id: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Verify signature chain/key continuity for a station/record.
    Verify {
        station_or_record_id: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Show local identity cache state.
    Cache {
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
}

#[derive(Subcommand)]
pub enum TrustCommands {
    /// Show trust recommendation and evidence summary.
    Show {
        station_or_record_id: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Explain trust decision trace.
    Explain {
        station_or_record_id: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Import or update a local trust-store record.
    Import {
        #[arg(long)]
        station_id: String,
        #[arg(long)]
        key_id: String,
        #[arg(long, default_value = "unknown")]
        trust: String,
        #[arg(long, default_value = "out_of_band")]
        source: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// List local trust-store records.
    List {
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Revoke a local trust-store record by station id or key id.
    Revoke {
        #[arg(long)]
        station_or_key: String,
        #[arg(long, default_value = "operator_revoked")]
        reason: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Trust policy controls.
    Policy {
        #[command(subcommand)]
        command: TrustPolicyCommands,
    },
}

#[derive(Subcommand)]
pub enum TrustPolicyCommands {
    /// Show active policy profile.
    Show {
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Set local policy profile.
    Set {
        profile: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
}

#[derive(Subcommand)]
pub enum DiagnoseCommands {
    /// Dry-run handshake prerequisites for a peer.
    Handshake {
        #[arg(long)]
        peer: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Verify signed manifest shape for a session.
    Manifest {
        #[arg(long)]
        session: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Composite identity+trust+handshake checks.
    Session {
        #[arg(long)]
        peer: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Write a commented configuration template to stdout.
    Init,
}

#[derive(Subcommand)]
pub enum ArqCommands {
    /// ISS: transmit a payload with ARQ, retransmitting until ACK.
    Send {
        /// Payload text to send.
        #[arg(short, long)]
        payload: String,
        /// Modulation mode (start mode when --profile is set).
        #[arg(short, long, default_value = "BPSK250")]
        mode: String,
        /// Adaptive session profile (enables rate stepping).
        #[arg(long, value_parser = profile_name_parser())]
        profile: Option<String>,
        /// Maximum retransmissions before giving up.
        #[arg(long, default_value_t = 3)]
        retries: usize,
        /// Audio device name (backend-specific).
        #[arg(short, long)]
        device: Option<String>,
    },
    /// IRS: receive data frames and ACK each (NACK on decode failure).
    Listen {
        /// Modulation mode (fallback when no adaptive session is active).
        #[arg(short, long, default_value = "BPSK250")]
        mode: String,
        /// Adaptive session profile (enables rate stepping).
        #[arg(long, value_parser = profile_name_parser())]
        profile: Option<String>,
        /// Number of frames to receive before exiting.
        #[arg(long, default_value_t = 1)]
        frames: usize,
        /// Session identifier echoed in ACK frames.
        #[arg(long, default_value = "openpulse-arq")]
        session: String,
        /// Audio device name (backend-specific).
        #[arg(short, long)]
        device: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum QsyCommands {
    /// Initiate a QSY frequency-agility negotiation using the configured rigctld and candidates.
    Init {
        /// rigctld address:port (overrides config file).
        #[arg(long, default_value = "")]
        rig: String,
    },
    /// Show the current QSY configuration.
    Status,
}
