use clap::{Parser, Subcommand};

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

    /// PTT backend: none | rts | dtr | vox | rigctld.
    #[arg(long, global = true, default_value = "none")]
    pub ptt: String,

    /// Serial port path or rigctld address:port for PTT (e.g. /dev/ttyUSB0 or localhost:4532).
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
    Modes,
    /// Recommend a speed level and mode for the current SNR.
    ModeAdvisor {
        /// Estimated signal-to-noise ratio in dB.
        #[arg(long)]
        snr: f32,
    },
    /// Export session performance metrics (throughput, FER, latency, SNR estimate).
    SessionMetrics {
        #[command(flatten)]
        opts: DiagnosticOptions,
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
