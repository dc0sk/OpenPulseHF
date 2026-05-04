use clap::{Parser, Subcommand};

use crate::output::DiagnosticOptions;

#[derive(Parser)]
#[command(
    name = "openpulse",
    about = "OpenPulse software modem for amateur radio data transmission",
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
    },
    /// Receive data and print to stdout.
    Receive {
        #[arg(short, long, default_value = "BPSK100")]
        mode: String,
        #[arg(short, long)]
        device: Option<String>,
    },
    /// List available audio devices.
    Devices,
    /// List registered modulation modes.
    Modes,
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
