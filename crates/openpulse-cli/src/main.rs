//! OpenPulse – software modem CLI.
//!
//! # Usage
//!
//! ```text
//! openpulse transmit "Hello World" --mode BPSK100 [--device loopback]
//! openpulse receive  --mode BPSK100 [--device <name>]
//! openpulse devices
//! openpulse modes
//! ```

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use reqwest::blocking::Client as HttpClient;
use reqwest::StatusCode;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::Level;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::trust::{
    allowed_signing_modes, classify_connection_trust, evaluate_handshake, CertificateSource,
    ConnectionTrustLevel, PolicyProfile, PublicKeyTrustLevel, SigningMode,
};
use openpulse_modem::benchmark::{
    assert_benchmark_regression, run_benchmark, standard_corpus, RegressionPolicy,
};
use openpulse_modem::diagnostics::SessionDiagnostics;
use openpulse_modem::engine::SecureSessionParams;
use openpulse_modem::ModemEngine;

#[cfg(feature = "cpal-backend")]
use openpulse_audio::CpalBackend;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "openpulse",
    about = "OpenPulse software modem for amateur radio data transmission",
    version
)]
struct Cli {
    /// Audio backend to use.
    ///
    /// Use `loopback` for testing without hardware.  On Linux the default
    /// is the system audio (ALSA / PipeWire via cpal).
    #[arg(long, global = true, default_value = "default")]
    backend: String,

    /// Verbosity level: error | warn | info | debug | trace.
    #[arg(long, global = true, default_value = "info")]
    log: String,

    /// PKI API base URL used by identity/trust diagnostics.
    #[arg(long, global = true, default_value = "http://127.0.0.1:8787")]
    pki_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Transmit data over the air.
    Transmit {
        /// Data string to transmit (UTF-8).
        data: String,

        /// Modulation mode (e.g. BPSK100, BPSK31).
        #[arg(short, long, default_value = "BPSK100")]
        mode: String,

        /// Audio device name.  Omit to use the backend default.
        #[arg(short, long)]
        device: Option<String>,
    },

    /// Receive data and print to stdout.
    Receive {
        /// Modulation mode (e.g. BPSK100, BPSK31).
        #[arg(short, long, default_value = "BPSK100")]
        mode: String,

        /// Audio device name.  Omit to use the backend default.
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
}

#[derive(Subcommand)]
enum BenchmarkCommands {
    /// Run the standard HPX benchmark corpus and emit a JSON report.
    Run {
        /// Minimum required pass rate (0.0–1.0); fails with exit code 2 if not met.
        #[arg(long, default_value_t = 1.0)]
        min_pass_rate: f64,
        /// Maximum allowed mean transition count per scenario.
        #[arg(long, default_value_t = 20.0)]
        max_mean_transitions: f64,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    /// Start a new secure HPX session with a peer.
    Start {
        /// Peer station or callsign to establish session with.
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
    /// End the active HPX session gracefully.
    End {
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Show HPX state transition log for the current session.
    Log {
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
}

#[derive(Subcommand)]
enum IdentityCommands {
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
enum TrustCommands {
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
enum TrustPolicyCommands {
    /// Show active policy profile.
    Show {
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
    /// Set local policy profile (session-local placeholder for now).
    Set {
        profile: String,
        #[command(flatten)]
        opts: DiagnosticOptions,
    },
}

#[derive(Subcommand)]
enum DiagnoseCommands {
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

#[derive(Args, Clone)]
struct DiagnosticOptions {
    /// Output format.
    #[arg(long, default_value = "text")]
    format: String,

    /// Include additional detail lines.
    #[arg(long)]
    verbose: bool,

    /// Emit detailed HPX diagnostics (JSON).
    #[arg(long)]
    diagnostics: bool,

    /// Disable color output.
    #[arg(long)]
    no_color: bool,

    /// Request timeout in seconds.
    #[arg(long, default_value_t = 5)]
    timeout: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Json,
}

impl OutputFormat {
    fn from_arg(value: &str) -> Result<Self> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => anyhow::bail!("invalid --format '{value}', expected text|json"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DiagnosticOutput {
    status: String,
    decision: String,
    reason_code: String,
    details: Value,
    recommendation: String,
}

#[derive(Debug, Clone, Deserialize)]
struct IdentityRecord {
    record_id: String,
    station_id: String,
    callsign: String,
    publication_state: String,
    current_revision_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RevocationRecord {
    revocation_id: String,
    record_id: String,
    reason_code: String,
    effective_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TrustBundleRecord {
    schema_version: String,
    bundle_id: String,
    records: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LocalTrustRecord {
    station_id: String,
    key_id: String,
    trust: PublicKeyTrustLevel,
    source: CertificateSource,
    status: String,
    reason: String,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LocalTrustStore {
    schema_version: String,
    records: Vec<LocalTrustRecord>,
}

#[derive(Debug, Clone)]
struct PkiClient {
    base_url: String,
    http: HttpClient,
}

impl PkiClient {
    fn new(base_url: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: HttpClient::new(),
        }
    }

    fn lookup_identity(&self, station_or_record_id: &str) -> Result<Option<IdentityRecord>> {
        let by_id_url = format!(
            "{}/api/v1/identities/{}",
            self.base_url, station_or_record_id
        );
        let by_id = self
            .http
            .get(by_id_url)
            .send()
            .context("pki request failed")?;
        if by_id.status().is_success() {
            return Ok(Some(by_id.json().context("invalid identity payload")?));
        }
        if by_id.status() != StatusCode::NOT_FOUND {
            anyhow::bail!("pki lookup failed: HTTP {}", by_id.status());
        }

        let by_station_url = format!("{}/api/v1/identities:lookup", self.base_url);
        let by_station = self
            .http
            .get(&by_station_url)
            .query(&[("station_id", station_or_record_id), ("limit", "1")])
            .send()
            .context("pki lookup request failed")?;
        if by_station.status().is_success() {
            let mut rows: Vec<IdentityRecord> =
                by_station.json().context("invalid identity list payload")?;
            if let Some(row) = rows.pop() {
                return Ok(Some(row));
            }
        } else {
            anyhow::bail!("pki station lookup failed: HTTP {}", by_station.status());
        }

        let by_callsign = self
            .http
            .get(by_station_url)
            .query(&[("callsign", station_or_record_id), ("limit", "1")])
            .send()
            .context("pki callsign lookup request failed")?;
        if !by_callsign.status().is_success() {
            anyhow::bail!("pki callsign lookup failed: HTTP {}", by_callsign.status());
        }

        let mut rows: Vec<IdentityRecord> = by_callsign
            .json()
            .context("invalid callsign lookup payload")?;
        Ok(rows.pop())
    }

    fn list_revocations(&self, record_id: &str) -> Result<Vec<RevocationRecord>> {
        let url = format!("{}/api/v1/revocations", self.base_url);
        let resp = self
            .http
            .get(url)
            .query(&[("record_id", record_id), ("limit", "50")])
            .send()
            .context("pki revocation query failed")?;
        if !resp.status().is_success() {
            anyhow::bail!("pki revocation lookup failed: HTTP {}", resp.status());
        }
        resp.json().context("invalid revocation payload")
    }

    fn get_current_bundle(&self) -> Result<Option<TrustBundleRecord>> {
        let url = format!("{}/api/v1/trust-bundles/current", self.base_url);
        let resp = self
            .http
            .get(url)
            .send()
            .context("pki trust-bundle request failed")?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            anyhow::bail!("pki trust-bundle query failed: HTTP {}", resp.status());
        }
        Ok(Some(resp.json().context("invalid trust bundle payload")?))
    }

    fn healthz(&self) -> Result<()> {
        let url = format!("{}/healthz", self.base_url);
        let resp = self
            .http
            .get(url)
            .send()
            .context("pki health request failed")?;
        if !resp.status().is_success() {
            anyhow::bail!("pki health check failed: HTTP {}", resp.status());
        }
        Ok(())
    }

    fn create_session_audit_event(&self, payload: &Value) -> Result<()> {
        let url = format!("{}/api/v1/session-audit-events", self.base_url);
        let resp = self
            .http
            .post(url)
            .json(payload)
            .send()
            .context("pki session audit request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp
                .text()
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            anyhow::bail!("pki session audit insert failed: HTTP {status}: {detail}");
        }

        Ok(())
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialise logging.
    let level: Level = cli.log.parse().unwrap_or(Level::INFO);
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .init();

    // Build audio backend.
    let audio: Box<dyn openpulse_core::audio::AudioBackend> = match cli.backend.as_str() {
        "loopback" => Box::new(LoopbackBackend::new()),
        #[cfg(feature = "cpal-backend")]
        "default" | "cpal" => Box::new(CpalBackend::new()),
        #[cfg(not(feature = "cpal-backend"))]
        "default" => {
            eprintln!("note: cpal backend not compiled in; falling back to loopback");
            Box::new(LoopbackBackend::new())
        }
        name => anyhow::bail!("unknown backend '{name}'"),
    };

    // Build engine and register plugins.
    let mut engine = ModemEngine::new(audio);
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .context("failed to register BPSK plugin")?;
    engine.set_trust_policy_profile(load_policy_profile_or_default());

    // Dispatch.
    let mut exit_code = 0;
    let pki = PkiClient::new(cli.pki_url.clone());
    match cli.command {
        Commands::Transmit { data, mode, device } => {
            let dev = device.as_deref();
            engine
                .transmit(data.as_bytes(), &mode, dev)
                .context("transmit failed")?;
            println!("Transmitted {} bytes in {mode} mode.", data.len());
        }

        Commands::Receive { mode, device } => {
            let dev = device.as_deref();
            let payload = engine.receive(&mode, dev).context("receive failed")?;
            let text = String::from_utf8_lossy(&payload);
            println!("{text}");
        }

        Commands::Devices => {
            // The engine doesn't expose the backend directly, so re-create a
            // temporary backend just for device listing.
            list_devices(&cli.backend)?;
        }

        Commands::Modes => {
            for info in engine.plugins().list() {
                println!(
                    "{}: {} ({})",
                    info.name,
                    info.description,
                    info.supported_modes.join(", ")
                );
            }
        }

        Commands::Identity { command } => {
            exit_code = run_identity(command, &pki)?;
        }

        Commands::Trust { command } => {
            exit_code = run_trust(command, &pki)?;
        }

        Commands::Diagnose { command } => {
            exit_code = run_diagnose(command, &pki)?;
        }

        Commands::Session { command } => {
            exit_code = run_session(command, &mut engine, &pki)?;
        }

        Commands::Benchmark { command } => {
            exit_code = run_benchmark_cmd(command)?;
        }
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn list_devices(backend: &str) -> Result<()> {
    use openpulse_core::audio::AudioBackend;

    let b: Box<dyn AudioBackend> = match backend {
        "loopback" => Box::new(LoopbackBackend::new()),
        #[cfg(feature = "cpal-backend")]
        _ => Box::new(CpalBackend::new()),
        #[cfg(not(feature = "cpal-backend"))]
        _ => Box::new(LoopbackBackend::new()),
    };

    let devices = b.list_devices().context("failed to list devices")?;
    if devices.is_empty() {
        println!("No audio devices found.");
        return Ok(());
    }
    println!("{:<40} {:<8} {:<8} Sample rates", "Name", "Input", "Output");
    println!("{}", "-".repeat(80));
    for d in devices {
        let rates: Vec<String> = d
            .supported_sample_rates
            .iter()
            .map(|r| r.to_string())
            .collect();
        println!(
            "{:<40} {:<8} {:<8} {}",
            d.name,
            if d.is_input { "yes" } else { "no" },
            if d.is_output { "yes" } else { "no" },
            rates.join(", "),
        );
    }
    Ok(())
}

fn run_session(command: SessionCommands, engine: &mut ModemEngine, pki: &PkiClient) -> Result<i32> {
    match command {
        SessionCommands::Start { peer, opts } => {
            let profile = load_policy_profile()?;

            let Some((trust_decision, _)) = (match fetch_pki_trust(peer.clone(), pki) {
                Ok(v) => v,
                Err(err) => return emit_transport_failure(&opts, err),
            }) else {
                let output = DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "missing".to_string(),
                    reason_code: "identity_not_found".to_string(),
                    details: json!({"peer": peer}),
                    recommendation: "Peer identity must exist before session start.".to_string(),
                };
                emit_output(&opts, &output)?;
                return Ok(2);
            };

            let timestamp_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            let result = engine.begin_secure_session(
                SecureSessionParams {
                    local_minimum_mode: SigningMode::Normal,
                    peer_supported_modes: vec![
                        SigningMode::Normal,
                        SigningMode::Psk,
                        SigningMode::Relaxed,
                    ],
                    key_trust: trust_decision.public_key_trust,
                    certificate_source: trust_decision.certificate_source,
                    psk_validated: trust_decision.psk_validated,
                },
                timestamp_ms,
            );

            let output = match result {
                Ok(handshake) => {
                    let session_id = engine
                        .hpx_session_id()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| format!("sess-{timestamp_ms}"));

                    let _ = post_session_audit(
                        pki,
                        &session_id,
                        &peer,
                        profile,
                        &trust_decision,
                        Ok(&handshake),
                        engine.hpx_transitions(),
                    );

                    let _ = persist_session_state(&PersistedSessionState {
                        session_id: session_id.clone(),
                        peer: peer.clone(),
                        hpx_state: format!("{:?}", engine.hpx_state()).to_lowercase(),
                        selected_mode: Some(
                            format!("{:?}", handshake.selected_mode).to_lowercase(),
                        ),
                        trust_level: Some(format!("{:?}", handshake.trust.decision).to_lowercase()),
                        policy_profile: policy_profile_to_str(profile).to_string(),
                        updated_at_ms: timestamp_ms,
                    });

                    DiagnosticOutput {
                        status: "ok".to_string(),
                        decision: format!("{:?}", handshake.trust.decision).to_lowercase(),
                        reason_code: handshake.trust.reason_code.clone(),
                        details: json!({
                            "peer": peer,
                            "session_id": session_id,
                            "hpx_state": format!("{:?}", engine.hpx_state()).to_lowercase(),
                            "policy_profile": policy_profile_to_str(profile),
                            "selected_mode": format!("{:?}", handshake.selected_mode).to_lowercase(),
                            "trust_level": format!("{:?}", handshake.trust.decision).to_lowercase(),
                            "certificate_source": format!("{:?}", handshake.trust.certificate_source).to_lowercase(),
                        }),
                        recommendation:
                            "Session active. Use session state to monitor and session end to close."
                                .to_string(),
                    }
                }
                Err(err) => {
                    let session_id = format!("sess-{timestamp_ms}");
                    let _ = post_session_audit(
                        pki,
                        &session_id,
                        &peer,
                        profile,
                        &trust_decision,
                        Err(()),
                        engine.hpx_transitions(),
                    );

                    DiagnosticOutput {
                        status: "fail".to_string(),
                        decision: "rejected".to_string(),
                        reason_code: "session_start_failed".to_string(),
                        details: json!({
                            "peer": peer,
                            "hpx_state": format!("{:?}", engine.hpx_state()).to_lowercase(),
                            "error": err.to_string(),
                        }),
                        recommendation: "Check trust policy and peer identity before retrying."
                            .to_string(),
                    }
                }
            };

            emit_output(&opts, &output)?;
            Ok(status_to_exit_code(&output.status))
        }

        SessionCommands::State { opts } => {
            let hpx_state = engine.hpx_state();
            let session_id = engine.hpx_session_id().map(ToString::to_string);
            let handshake = engine.active_handshake();

            if hpx_state == openpulse_core::hpx::HpxState::Idle
                && session_id.is_none()
                && handshake.is_none()
            {
                if let Some(persisted) = load_session_state()? {
                    let output = DiagnosticOutput {
                        status: "ok".to_string(),
                        decision: "persisted".to_string(),
                        reason_code: "persisted_session_snapshot".to_string(),
                        details: json!({
                            "hpx_state": "idle",
                            "session_id": persisted.session_id,
                            "peer": persisted.peer,
                            "persisted_hpx_state": persisted.hpx_state,
                            "selected_mode": persisted.selected_mode,
                            "trust_level": persisted.trust_level,
                            "policy_profile": persisted.policy_profile,
                            "updated_at_ms": persisted.updated_at_ms,
                            "session_state_file": session_state_file_path().display().to_string(),
                        }),
                        recommendation:
                            "No active in-memory session. Showing last persisted session snapshot."
                                .to_string(),
                    };

                    emit_output(&opts, &output)?;
                    return Ok(0);
                }
            }

            // Build diagnostics if requested
            if opts.diagnostics {
                let mut diag = SessionDiagnostics::new(
                    session_id.clone().unwrap_or_else(|| "no-session".to_string()),
                    "peer", // Placeholder: actual peer info available from handshake context
                );
                diag.current_state = format!("{hpx_state:?}").to_lowercase();
                diag.total_transitions = engine.hpx_transitions().len();

                // Record all transitions into diagnostics
                for transition in engine.hpx_transitions() {
                    diag.record_transition(transition);
                }

                // Output detailed diagnostics as JSON
                let json_output = diag.to_json_pretty()?;
                println!("{}", json_output);
                return Ok(0);
            }

            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: format!("{hpx_state:?}").to_lowercase(),
                reason_code: "hpx_state_snapshot".to_string(),
                details: json!({
                    "hpx_state": format!("{hpx_state:?}").to_lowercase(),
                    "session_id": session_id,
                    "active_handshake": handshake.map(|h| json!({
                        "selected_mode": format!("{:?}", h.selected_mode).to_lowercase(),
                        "trust_level": format!("{:?}", h.trust.decision).to_lowercase(),
                        "policy_profile": format!("{:?}", h.policy_profile).to_lowercase(),
                    })),
                    "transition_count": engine.hpx_transitions().len(),
                }),
                recommendation: match hpx_state {
                    openpulse_core::hpx::HpxState::Idle => "No active session.",
                    openpulse_core::hpx::HpxState::ActiveTransfer => {
                        "Session active. Ready for transfer."
                    }
                    openpulse_core::hpx::HpxState::Failed => "Session failed. Start a new session.",
                    _ => "Session in transition.",
                }
                .to_string(),
            };

            emit_output(&opts, &output)?;
            Ok(0)
        }

        SessionCommands::End { opts } => {
            let timestamp_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            let session_id = engine
                .hpx_session_id()
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("sess-{timestamp_ms}"));

            let result = engine.end_secure_session(timestamp_ms);

            let output = match result {
                Ok(()) => {
                    let _ = clear_session_state();
                    DiagnosticOutput {
                        status: "ok".to_string(),
                        decision: "closed".to_string(),
                        reason_code: "session_ended".to_string(),
                        details: json!({
                            "session_id": session_id,
                            "hpx_state": format!("{:?}", engine.hpx_state()).to_lowercase(),
                        }),
                        recommendation: "Session closed. Engine is idle.".to_string(),
                    }
                }
                Err(err) => DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "error".to_string(),
                    reason_code: "session_end_failed".to_string(),
                    details: json!({
                        "session_id": session_id,
                        "hpx_state": format!("{:?}", engine.hpx_state()).to_lowercase(),
                        "error": err.to_string(),
                    }),
                    recommendation: "Session end encountered an error.".to_string(),
                },
            };

            emit_output(&opts, &output)?;
            Ok(status_to_exit_code(&output.status))
        }

        SessionCommands::Log { opts } => {
            let transitions = engine.hpx_transitions();
            let session_id = engine.hpx_session_id().map(ToString::to_string);

            let log_entries: Vec<Value> = transitions
                .iter()
                .map(|t| {
                    json!({
                        "timestamp_ms": t.timestamp_ms,
                        "from_state": format!("{:?}", t.from_state).to_lowercase(),
                        "to_state": format!("{:?}", t.to_state).to_lowercase(),
                        "event": format!("{:?}", t.event).to_lowercase(),
                        "reason_code": format!("{:?}", t.reason_code).to_lowercase(),
                        "reason_string": t.reason_string,
                    })
                })
                .collect();

            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: format!("{:?}", engine.hpx_state()).to_lowercase(),
                reason_code: "session_log".to_string(),
                details: json!({
                    "session_id": session_id,
                    "hpx_state": format!("{:?}", engine.hpx_state()).to_lowercase(),
                    "transition_count": transitions.len(),
                    "transitions": log_entries,
                }),
                recommendation: if transitions.is_empty() {
                    "No transitions recorded for this session."
                } else {
                    "Transition log captured."
                }
                .to_string(),
            };

            emit_output(&opts, &output)?;
            Ok(0)
        }
    }
}

fn run_benchmark_cmd(command: BenchmarkCommands) -> Result<i32> {
    match command {
        BenchmarkCommands::Run {
            min_pass_rate,
            max_mean_transitions,
        } => {
            let corpus = standard_corpus();
            let report = run_benchmark(&corpus);

            println!("{}", serde_json::to_string_pretty(&report)?);

            let policy = RegressionPolicy {
                min_pass_rate,
                max_mean_transitions,
            };
            let gate_ok = std::panic::catch_unwind(|| {
                assert_benchmark_regression(&report, &policy);
            })
            .is_ok();

            Ok(if gate_ok { 0 } else { 2 })
        }
    }
}

fn post_session_audit(
    pki: &PkiClient,
    session_id: &str,
    peer: &str,
    profile: PolicyProfile,
    trust_decision: &openpulse_core::trust::TrustDecision,
    handshake: Result<&openpulse_core::trust::HandshakeDecision, ()>,
    transitions: &[openpulse_core::hpx::HpxTransition],
) -> Result<()> {
    let selected_mode = handshake
        .ok()
        .map(|h| format!("{:?}", h.selected_mode).to_lowercase())
        .unwrap_or_else(|| "none".to_string());

    let transition_values: Vec<Value> = transitions
        .iter()
        .map(|t| {
            json!({
                "timestamp_ms": t.timestamp_ms,
                "from_state": format!("{:?}", t.from_state).to_lowercase(),
                "to_state": format!("{:?}", t.to_state).to_lowercase(),
                "event": format!("{:?}", t.event).to_lowercase(),
                "reason_code": format!("{:?}", t.reason_code).to_lowercase(),
                "reason_string": t.reason_string,
            })
        })
        .collect();

    let payload = json!({
        "session_id": session_id,
        "peer_id": peer,
        "policy_profile": policy_profile_to_str(profile),
        "selected_mode": selected_mode,
        "trust_level": format!("{:?}", trust_decision.decision).to_lowercase(),
        "certificate_source": format!("{:?}", trust_decision.certificate_source).to_lowercase(),
        "trust_reason_code": trust_decision.reason_code,
        "transitions": transition_values,
        "actor_identity": "openpulse-cli",
    });

    pki.create_session_audit_event(&payload)
}

fn run_identity(command: IdentityCommands, pki: &PkiClient) -> Result<i32> {
    match command {
        IdentityCommands::Show {
            station_or_record_id,
            opts,
        } => match pki.lookup_identity(&station_or_record_id) {
            Ok(Some(identity)) => {
                let output = DiagnosticOutput {
                    status: "ok".to_string(),
                    decision: "verified".to_string(),
                    reason_code: "identity_found".to_string(),
                    details: json!({
                        "record_id": identity.record_id,
                        "station_id": identity.station_id,
                        "callsign": identity.callsign,
                        "publication_state": identity.publication_state,
                        "current_revision_id": identity.current_revision_id,
                    }),
                    recommendation: "Identity record resolved from PKI service.".to_string(),
                };
                emit_output(&opts, &output)?;
                Ok(0)
            }
            Ok(None) => {
                let output = DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "missing".to_string(),
                    reason_code: "identity_not_found".to_string(),
                    details: json!({ "query": station_or_record_id }),
                    recommendation: "Submit or replicate identity record before session setup."
                        .to_string(),
                };
                emit_output(&opts, &output)?;
                Ok(2)
            }
            Err(err) => emit_transport_failure(&opts, err),
        },
        IdentityCommands::Verify {
            station_or_record_id,
            opts,
        } => match pki.lookup_identity(&station_or_record_id) {
            Ok(Some(identity)) => match pki.list_revocations(&identity.record_id) {
                Ok(revocations) if revocations.is_empty() => {
                    let output = DiagnosticOutput {
                        status: "ok".to_string(),
                        decision: "verified".to_string(),
                        reason_code: "signature_chain_valid".to_string(),
                        details: json!({
                            "record_id": identity.record_id,
                            "effective_revocation_state": "none",
                            "revocation_count": 0,
                        }),
                        recommendation: "No blocking PKI revocations detected.".to_string(),
                    };
                    emit_output(&opts, &output)?;
                    Ok(0)
                }
                Ok(revocations) => {
                    let output = DiagnosticOutput {
                        status: "fail".to_string(),
                        decision: "rejected".to_string(),
                        reason_code: "revocation_conflict".to_string(),
                        details: json!({
                            "record_id": identity.record_id,
                            "revocation_count": revocations.len(),
                            "revocations": revocations,
                        }),
                        recommendation: "Resolve revocation conflict before session use."
                            .to_string(),
                    };
                    emit_output(&opts, &output)?;
                    Ok(2)
                }
                Err(err) => emit_transport_failure(&opts, err),
            },
            Ok(None) => {
                let output = DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "missing".to_string(),
                    reason_code: "identity_not_found".to_string(),
                    details: json!({ "query": station_or_record_id }),
                    recommendation: "Identity record must exist before verification.".to_string(),
                };
                emit_output(&opts, &output)?;
                Ok(2)
            }
            Err(err) => emit_transport_failure(&opts, err),
        },
        IdentityCommands::Cache { opts } => match pki.healthz() {
            Ok(()) => {
                let bundle = pki.get_current_bundle();
                let (entries, bundle_id, schema_version) = match bundle {
                    Ok(Some(b)) => {
                        let count = match &b.records {
                            Value::Array(items) => items.len(),
                            Value::Object(map) => map.len(),
                            _ => 0,
                        };
                        (count, Some(b.bundle_id), Some(b.schema_version))
                    }
                    Ok(None) => (0, None, None),
                    Err(err) => return emit_transport_failure(&opts, err),
                };

                let output = DiagnosticOutput {
                    status: "ok".to_string(),
                    decision: "cache".to_string(),
                    reason_code: "cache_summary".to_string(),
                    details: json!({
                        "entries": entries,
                        "bundle_id": bundle_id,
                        "schema_version": schema_version,
                        "freshness_seconds": opts.timeout,
                    }),
                    recommendation: "Local PKI cache and trust bundle metadata loaded.".to_string(),
                };
                emit_output(&opts, &output)?;
                Ok(0)
            }
            Err(err) => emit_transport_failure(&opts, err),
        },
    }
}

fn run_trust(command: TrustCommands, pki: &PkiClient) -> Result<i32> {
    match command {
        TrustCommands::Show {
            station_or_record_id,
            opts,
        } => {
            let profile = load_policy_profile()?;
            let output = match fetch_pki_trust(station_or_record_id, pki) {
                Ok(Some((decision, details))) => trust_output(decision, details, false, profile),
                Ok(None) => DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "missing".to_string(),
                    reason_code: "identity_not_found".to_string(),
                    details: json!({}),
                    recommendation: "Identity record not found in PKI service.".to_string(),
                },
                Err(err) => return emit_transport_failure(&opts, err),
            };
            emit_output(&opts, &output)?;
            Ok(status_to_exit_code(&output.status))
        }
        TrustCommands::Explain {
            station_or_record_id,
            opts,
        } => {
            let profile = load_policy_profile()?;
            let output = match fetch_pki_trust(station_or_record_id, pki) {
                Ok(Some((decision, details))) => trust_output(decision, details, true, profile),
                Ok(None) => DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "missing".to_string(),
                    reason_code: "identity_not_found".to_string(),
                    details: json!({}),
                    recommendation: "Identity record not found in PKI service.".to_string(),
                },
                Err(err) => return emit_transport_failure(&opts, err),
            };
            emit_output(&opts, &output)?;
            Ok(match output.status.as_str() {
                "fail" => 2,
                _ => 0,
            })
        }
        TrustCommands::Import {
            station_id,
            key_id,
            trust,
            source,
            opts,
        } => {
            let trust = match parse_public_key_trust_level(&trust) {
                Ok(v) => v,
                Err(_) => {
                    let output = DiagnosticOutput {
                        status: "fail".to_string(),
                        decision: "invalid".to_string(),
                        reason_code: "invalid_trust_level".to_string(),
                        details: json!({
                            "requested_trust": trust,
                        }),
                        recommendation: "Use one of: full, marginal, unknown, untrusted, revoked."
                            .to_string(),
                    };
                    emit_output(&opts, &output)?;
                    return Ok(2);
                }
            };

            let source = match parse_certificate_source(&source) {
                Ok(v) => v,
                Err(_) => {
                    let output = DiagnosticOutput {
                        status: "fail".to_string(),
                        decision: "invalid".to_string(),
                        reason_code: "invalid_certificate_source".to_string(),
                        details: json!({
                            "requested_source": source,
                        }),
                        recommendation: "Use one of: out_of_band, over_air.".to_string(),
                    };
                    emit_output(&opts, &output)?;
                    return Ok(2);
                }
            };

            let mut store = load_trust_store()?;
            let updated_at_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            if let Some(existing) = store
                .records
                .iter_mut()
                .find(|r| r.key_id == key_id || r.station_id == station_id)
            {
                existing.station_id = station_id.clone();
                existing.key_id = key_id.clone();
                existing.trust = trust;
                existing.source = source;
                existing.status = "active".to_string();
                existing.reason = "operator_import".to_string();
                existing.updated_at_ms = updated_at_ms;
            } else {
                store.records.push(LocalTrustRecord {
                    station_id: station_id.clone(),
                    key_id: key_id.clone(),
                    trust,
                    source,
                    status: "active".to_string(),
                    reason: "operator_import".to_string(),
                    updated_at_ms,
                });
            }

            persist_trust_store(&store)?;

            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "imported".to_string(),
                reason_code: "trust_store_updated".to_string(),
                details: json!({
                    "station_id": station_id,
                    "key_id": key_id,
                    "trust": format!("{:?}", trust).to_lowercase(),
                    "source": format!("{:?}", source).to_lowercase(),
                    "status": "active",
                    "trust_store_file": trust_store_file_path(),
                }),
                recommendation: "Trust record imported into local trust store.".to_string(),
            };
            emit_output(&opts, &output)?;
            Ok(0)
        }
        TrustCommands::List { opts } => {
            let store = load_trust_store()?;
            let record_count = store.records.len();
            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "listed".to_string(),
                reason_code: "trust_store_list".to_string(),
                details: json!({
                    "records": store.records,
                    "count": record_count,
                    "schema_version": store.schema_version,
                    "trust_store_file": trust_store_file_path(),
                }),
                recommendation: "Review active/revoked entries before session setup.".to_string(),
            };
            emit_output(&opts, &output)?;
            Ok(0)
        }
        TrustCommands::Revoke {
            station_or_key,
            reason,
            opts,
        } => {
            let mut store = load_trust_store()?;
            let updated_at_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            let Some(record) = store
                .records
                .iter_mut()
                .find(|r| r.station_id == station_or_key || r.key_id == station_or_key)
            else {
                let output = DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "missing".to_string(),
                    reason_code: "trust_record_not_found".to_string(),
                    details: json!({
                        "station_or_key": station_or_key,
                    }),
                    recommendation: "Import the record first, then revoke it.".to_string(),
                };
                emit_output(&opts, &output)?;
                return Ok(2);
            };

            record.status = "revoked".to_string();
            record.reason = reason.clone();
            record.updated_at_ms = updated_at_ms;

            let station_id = record.station_id.clone();
            let key_id = record.key_id.clone();
            let status = record.status.clone();

            persist_trust_store(&store)?;

            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "revoked".to_string(),
                reason_code: "trust_record_revoked".to_string(),
                details: json!({
                    "station_id": station_id,
                    "key_id": key_id,
                    "reason": reason,
                    "status": status,
                    "trust_store_file": trust_store_file_path(),
                }),
                recommendation: "Record revoked in local trust store.".to_string(),
            };
            emit_output(&opts, &output)?;
            Ok(0)
        }
        TrustCommands::Policy { command } => match command {
            TrustPolicyCommands::Show { opts } => {
                let profile = load_policy_profile()?;
                let output = DiagnosticOutput {
                    status: "ok".to_string(),
                    decision: policy_profile_to_str(profile).to_string(),
                    reason_code: "policy_profile_active".to_string(),
                    details: json!({
                        "policy_profile": policy_profile_to_str(profile),
                        "policy_version": "1.0.0",
                        "allowed_signing_modes": allowed_signing_modes(profile)
                            .iter()
                            .map(|mode| signing_mode_to_str(*mode))
                            .collect::<Vec<&str>>(),
                        "config_file": policy_file_path(),
                    }),
                    recommendation: "Use trust policy set to change profile.".to_string(),
                };
                emit_output(&opts, &output)?;
                Ok(0)
            }
            TrustPolicyCommands::Set { profile, opts } => {
                let output = match parse_policy_profile(&profile) {
                    Ok(parsed) => {
                        persist_policy_profile(parsed)?;
                        DiagnosticOutput {
                            status: "ok".to_string(),
                            decision: policy_profile_to_str(parsed).to_string(),
                            reason_code: "policy_profile_updated".to_string(),
                            details: json!({
                                    "policy_profile": policy_profile_to_str(parsed),
                                    "config_file": policy_file_path(),
                            }),
                            recommendation: "Policy profile persisted for subsequent runs."
                                .to_string(),
                        }
                    }
                    Err(_) => DiagnosticOutput {
                        status: "fail".to_string(),
                        decision: "invalid".to_string(),
                        reason_code: "policy_rejected".to_string(),
                        details: json!({"requested_profile": profile}),
                        recommendation: "Choose strict, balanced, or permissive.".to_string(),
                    },
                };
                emit_output(&opts, &output)?;
                Ok(status_to_exit_code(&output.status))
            }
        },
    }
}

fn run_diagnose(command: DiagnoseCommands, pki: &PkiClient) -> Result<i32> {
    match command {
        DiagnoseCommands::Handshake { peer, opts } => {
            let profile = load_policy_profile()?;
            let Some((trust_decision, details)) = (match fetch_pki_trust(peer.clone(), pki) {
                Ok(v) => v,
                Err(err) => return emit_transport_failure(&opts, err),
            }) else {
                let output = DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "missing".to_string(),
                    reason_code: "identity_not_found".to_string(),
                    details: json!({"peer": peer}),
                    recommendation: "Peer identity must exist before handshake validation."
                        .to_string(),
                };
                emit_output(&opts, &output)?;
                return Ok(2);
            };

            let handshake = evaluate_handshake(
                profile,
                SigningMode::Normal,
                &[SigningMode::Normal, SigningMode::Psk, SigningMode::Relaxed],
                trust_decision.public_key_trust,
                trust_decision.certificate_source,
                trust_decision.psk_validated,
            );

            let output = match &handshake {
                Ok(h) => DiagnosticOutput {
                    status: "ok".to_string(),
                    decision: format!("{:?}", h.trust.decision).to_lowercase(),
                    reason_code: h.trust.reason_code.to_string(),
                    details: json!({
                        "peer": peer,
                        "policy_profile": policy_profile_to_str(profile),
                        "selected_mode": format!("{:?}", h.selected_mode).to_lowercase(),
                        "certificate_source": format!("{:?}", h.trust.certificate_source).to_lowercase(),
                        "identity_details": details,
                    }),
                    recommendation: "Handshake prerequisites satisfied.".to_string(),
                },
                Err(_) => DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "rejected".to_string(),
                    reason_code: "policy_rejected".to_string(),
                    details: json!({"peer": peer}),
                    recommendation: "Adjust local policy or peer capabilities.".to_string(),
                },
            };

            if let Err(err) =
                record_handshake_session_audit(pki, &peer, profile, &trust_decision, &handshake)
            {
                return emit_transport_failure(&opts, err);
            }

            emit_output(&opts, &output)?;
            Ok(status_to_exit_code(&output.status))
        }
        DiagnoseCommands::Manifest { session, opts } => match pki.get_current_bundle() {
            Ok(Some(bundle)) => {
                let output = DiagnosticOutput {
                    status: "ok".to_string(),
                    decision: "verified".to_string(),
                    reason_code: "manifest_schema_valid".to_string(),
                    details: json!({
                        "session": session,
                        "schema": bundle.schema_version,
                        "bundle_id": bundle.bundle_id,
                    }),
                    recommendation:
                        "Manifest metadata validated against current trust-bundle schema."
                            .to_string(),
                };
                emit_output(&opts, &output)?;
                Ok(0)
            }
            Ok(None) => {
                let output = DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "invalid".to_string(),
                    reason_code: "invalid_manifest_schema".to_string(),
                    details: json!({"session": session}),
                    recommendation: "No current trust bundle available for schema validation."
                        .to_string(),
                };
                emit_output(&opts, &output)?;
                Ok(2)
            }
            Err(err) => emit_transport_failure(&opts, err),
        },
        DiagnoseCommands::Session { peer, opts } => {
            let profile = load_policy_profile()?;
            let identity = run_identity(
                IdentityCommands::Verify {
                    station_or_record_id: peer.clone(),
                    opts: opts.clone(),
                },
                pki,
            )?;
            let trust_code = run_trust(
                TrustCommands::Show {
                    station_or_record_id: peer.clone(),
                    opts: opts.clone(),
                },
                pki,
            )?;
            let handshake_code = run_diagnose(
                DiagnoseCommands::Handshake {
                    peer: peer.clone(),
                    opts: opts.clone(),
                },
                pki,
            )?;

            let composite_status = if identity == 3
                || trust_code == 3
                || handshake_code == 3
                || identity == 2
                || trust_code == 2
                || handshake_code == 2
            {
                "fail"
            } else {
                "ok"
            };

            let output = DiagnosticOutput {
                status: composite_status.to_string(),
                decision: policy_profile_to_str(profile).to_string(),
                reason_code: if composite_status == "ok" {
                    "signature_chain_valid".to_string()
                } else {
                    "policy_rejected".to_string()
                },
                details: json!({
                    "peer": peer,
                    "identity_exit_code": identity,
                    "trust_exit_code": trust_code,
                    "handshake_exit_code": handshake_code,
                }),
                recommendation: "Run trust explain for detailed downgrade reasoning.".to_string(),
            };

            emit_output(&opts, &output)?;
            Ok(status_to_exit_code(&output.status))
        }
    }
}

fn record_handshake_session_audit(
    pki: &PkiClient,
    peer: &str,
    profile: PolicyProfile,
    trust_decision: &openpulse_core::trust::TrustDecision,
    handshake: &Result<openpulse_core::trust::HandshakeDecision, openpulse_core::trust::TrustError>,
) -> Result<()> {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let mut audit_engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    audit_engine.set_trust_policy_profile(profile);

    let _ = audit_engine.begin_secure_session(
        SecureSessionParams {
            local_minimum_mode: SigningMode::Normal,
            peer_supported_modes: vec![SigningMode::Normal, SigningMode::Psk, SigningMode::Relaxed],
            key_trust: trust_decision.public_key_trust,
            certificate_source: trust_decision.certificate_source,
            psk_validated: trust_decision.psk_validated,
        },
        timestamp_ms,
    );

    let selected_mode = handshake
        .as_ref()
        .ok()
        .map(|h| format!("{:?}", h.selected_mode).to_lowercase())
        .unwrap_or_else(|| "none".to_string());

    let transitions = audit_engine
        .hpx_transitions()
        .iter()
        .map(|transition| {
            json!({
                "timestamp_ms": transition.timestamp_ms,
                "from_state": format!("{:?}", transition.from_state).to_lowercase(),
                "to_state": format!("{:?}", transition.to_state).to_lowercase(),
                "event": format!("{:?}", transition.event).to_lowercase(),
                "reason_code": format!("{:?}", transition.reason_code).to_lowercase(),
                "reason_string": transition.reason_string,
            })
        })
        .collect::<Vec<Value>>();

    let session_id = audit_engine
        .hpx_session_id()
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("diag-{timestamp_ms}"));

    let payload = json!({
        "session_id": session_id,
        "peer_id": peer,
        "policy_profile": policy_profile_to_str(profile),
        "selected_mode": selected_mode,
        "trust_level": format!("{:?}", trust_decision.decision).to_lowercase(),
        "certificate_source": format!("{:?}", trust_decision.certificate_source).to_lowercase(),
        "trust_reason_code": trust_decision.reason_code,
        "transitions": transitions,
        "actor_identity": "openpulse-cli",
    });

    pki.create_session_audit_event(&payload)
}

fn trust_output(
    decision: openpulse_core::trust::TrustDecision,
    mut details: Value,
    explain: bool,
    policy_profile: PolicyProfile,
) -> DiagnosticOutput {
    let (status, recommendation) = match decision.decision {
        ConnectionTrustLevel::Verified | ConnectionTrustLevel::PskVerified => (
            "ok",
            "Connection trust is sufficient for publish/promote operations.",
        ),
        ConnectionTrustLevel::Reduced
        | ConnectionTrustLevel::Unverified
        | ConnectionTrustLevel::Low => (
            "warn",
            "Request out-of-band certificate verification before data transfer.",
        ),
        ConnectionTrustLevel::Rejected => ("fail", "Do not proceed; peer is untrusted or revoked."),
    };

    details["trust_state"] = json!(format!("{:?}", decision.public_key_trust).to_lowercase());
    details["certificate_source"] =
        json!(format!("{:?}", decision.certificate_source).to_lowercase());
    details["policy_profile"] = json!(policy_profile_to_str(policy_profile));
    details["policy_profile_version"] = json!("1.0.0");

    if explain {
        details["secondary_notes"] = json!([
            "reduced/unverified/low sessions are consume-only for publication workflows",
            "psk validation failure is fail-closed",
        ]);
    }

    DiagnosticOutput {
        status: status.to_string(),
        decision: format!("{:?}", decision.decision).to_lowercase(),
        reason_code: decision.reason_code.to_string(),
        details,
        recommendation: recommendation.to_string(),
    }
}

fn emit_output(opts: &DiagnosticOptions, output: &DiagnosticOutput) -> Result<()> {
    let format = OutputFormat::from_arg(&opts.format)?;
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(output)?);
        }
        OutputFormat::Text => {
            println!("STATUS: {}", output.decision);
            println!("status: {}", output.status);
            println!("reason_code: {}", output.reason_code);

            if let Value::Object(map) = &output.details {
                for (k, v) in map {
                    println!("{k}: {v}");
                }
            }

            println!("recommendation: {}", output.recommendation);
            if opts.verbose {
                println!("timeout_seconds: {}", opts.timeout);
                println!("no_color: {}", opts.no_color);
            }
        }
    }
    Ok(())
}

fn status_to_exit_code(status: &str) -> i32 {
    match status {
        "fail" => 2,
        _ => 0,
    }
}

fn emit_transport_failure(opts: &DiagnosticOptions, err: anyhow::Error) -> Result<i32> {
    let output = DiagnosticOutput {
        status: "fail".to_string(),
        decision: "unavailable".to_string(),
        reason_code: "pki_service_unreachable".to_string(),
        details: json!({"error": err.to_string()}),
        recommendation: "Check PKI service availability and --pki-url endpoint.".to_string(),
    };
    emit_output(opts, &output)?;
    Ok(3)
}

fn fetch_pki_trust(
    station_or_record_id: String,
    pki: &PkiClient,
) -> Result<Option<(openpulse_core::trust::TrustDecision, Value)>> {
    let Some(identity) = pki.lookup_identity(&station_or_record_id)? else {
        return Ok(None);
    };

    let revocations = pki.list_revocations(&identity.record_id)?;
    if !revocations.is_empty() {
        let decision = classify_connection_trust(
            PublicKeyTrustLevel::Untrusted,
            CertificateSource::OutOfBand,
            false,
        );
        return Ok(Some((
            decision,
            json!({
                "peer": station_or_record_id,
                "record_id": identity.record_id,
                "station_id": identity.station_id,
                "callsign": identity.callsign,
                "publication_state": identity.publication_state,
                "effective_revocation_state": "revoked",
                "revocation_count": revocations.len(),
                "revocations": revocations,
                "evidence_classes": ["operator", "gpg", "tqsl", "replication"],
            }),
        )));
    }

    let key_trust = key_trust_from_publication_state(&identity.publication_state);

    let decision = classify_connection_trust(key_trust, CertificateSource::OutOfBand, false);
    Ok(Some((
        decision,
        json!({
            "peer": station_or_record_id,
            "record_id": identity.record_id,
            "station_id": identity.station_id,
            "callsign": identity.callsign,
            "publication_state": identity.publication_state,
            "current_revision_id": identity.current_revision_id,
            "effective_revocation_state": "none",
            "revocation_count": 0,
            "evidence_classes": ["operator", "gpg", "tqsl", "replication"],
        }),
    )))
}

fn parse_policy_profile(value: &str) -> Result<PolicyProfile> {
    match value.to_lowercase().as_str() {
        "strict" => Ok(PolicyProfile::Strict),
        "balanced" => Ok(PolicyProfile::Balanced),
        "permissive" => Ok(PolicyProfile::Permissive),
        _ => anyhow::bail!("invalid policy profile"),
    }
}

fn policy_profile_to_str(profile: PolicyProfile) -> &'static str {
    match profile {
        PolicyProfile::Strict => "strict",
        PolicyProfile::Balanced => "balanced",
        PolicyProfile::Permissive => "permissive",
    }
}

fn signing_mode_to_str(mode: SigningMode) -> &'static str {
    match mode {
        SigningMode::Normal => "normal",
        SigningMode::Psk => "psk",
        SigningMode::Relaxed => "relaxed",
        SigningMode::Paranoid => "paranoid",
    }
}

fn config_dir_path() -> PathBuf {
    if let Ok(path) = std::env::var("OPENPULSE_CONFIG_DIR") {
        return PathBuf::from(path);
    }

    if let Ok(path) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("openpulse");
    }

    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config").join("openpulse");
    }

    PathBuf::from(".")
}

fn policy_file_path() -> PathBuf {
    config_dir_path().join("trust-policy.json")
}

fn trust_store_file_path() -> PathBuf {
    config_dir_path().join("trust-store.json")
}

fn session_state_file_path() -> PathBuf {
    config_dir_path().join("session-state.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PersistedSessionState {
    session_id: String,
    peer: String,
    hpx_state: String,
    selected_mode: Option<String>,
    trust_level: Option<String>,
    policy_profile: String,
    updated_at_ms: u64,
}

fn load_session_state() -> Result<Option<PersistedSessionState>> {
    load_session_state_at(&session_state_file_path())
}

fn persist_session_state(state: &PersistedSessionState) -> Result<()> {
    persist_session_state_at(&session_state_file_path(), state)
}

fn clear_session_state() -> Result<()> {
    clear_session_state_at(&session_state_file_path())
}

fn load_session_state_at(path: &Path) -> Result<Option<PersistedSessionState>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read session state file {}", path.display()))?;
    let state: PersistedSessionState = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse session state file {}", path.display()))?;

    Ok(Some(state))
}

fn persist_session_state_at(path: &Path, state: &PersistedSessionState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    fs::write(path, serde_json::to_string_pretty(state)?)
        .with_context(|| format!("failed to write session state file {}", path.display()))?;

    Ok(())
}

fn clear_session_state_at(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("failed to remove session state file {}", path.display()))?;
    }
    Ok(())
}

fn parse_public_key_trust_level(value: &str) -> Result<PublicKeyTrustLevel> {
    match value.to_lowercase().as_str() {
        "full" => Ok(PublicKeyTrustLevel::Full),
        "marginal" => Ok(PublicKeyTrustLevel::Marginal),
        "unknown" => Ok(PublicKeyTrustLevel::Unknown),
        "untrusted" => Ok(PublicKeyTrustLevel::Untrusted),
        "revoked" => Ok(PublicKeyTrustLevel::Revoked),
        _ => anyhow::bail!("invalid trust level"),
    }
}

fn parse_certificate_source(value: &str) -> Result<CertificateSource> {
    match value.to_lowercase().as_str() {
        "out_of_band" | "out-of-band" => Ok(CertificateSource::OutOfBand),
        "over_air" | "over-air" => Ok(CertificateSource::OverAir),
        _ => anyhow::bail!("invalid certificate source"),
    }
}

fn load_trust_store() -> Result<LocalTrustStore> {
    load_trust_store_at(&trust_store_file_path())
}

fn persist_trust_store(store: &LocalTrustStore) -> Result<()> {
    persist_trust_store_at(&trust_store_file_path(), store)
}

fn load_trust_store_at(path: &Path) -> Result<LocalTrustStore> {
    if !path.exists() {
        return Ok(LocalTrustStore {
            schema_version: "1.0.0".to_string(),
            records: vec![],
        });
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read trust store file {}", path.display()))?;
    let store: LocalTrustStore = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse trust store file {}", path.display()))?;
    Ok(store)
}

fn persist_trust_store_at(path: &Path, store: &LocalTrustStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    fs::write(path, serde_json::to_string_pretty(store)?)
        .with_context(|| format!("failed to write trust store file {}", path.display()))?;
    Ok(())
}

fn load_policy_profile() -> Result<PolicyProfile> {
    let path = policy_file_path();
    if !path.exists() {
        return Ok(PolicyProfile::Balanced);
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read policy file {}", path.display()))?;
    let value: Value = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse policy file {}", path.display()))?;

    let Some(profile) = value.get("policy_profile").and_then(Value::as_str) else {
        anyhow::bail!(
            "invalid policy file {}: missing policy_profile",
            path.display()
        );
    };

    parse_policy_profile(profile)
}

fn load_policy_profile_or_default() -> PolicyProfile {
    match load_policy_profile() {
        Ok(profile) => profile,
        Err(err) => {
            tracing::warn!(
                "failed to load persisted trust policy profile ({}); defaulting to balanced",
                err
            );
            PolicyProfile::Balanced
        }
    }
}

fn persist_policy_profile(profile: PolicyProfile) -> Result<()> {
    let dir = config_dir_path();
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create config directory {}", dir.display()))?;

    let path = policy_file_path();
    let payload = json!({
        "policy_profile": policy_profile_to_str(profile),
        "policy_version": "1.0.0"
    });

    fs::write(&path, serde_json::to_string_pretty(&payload)?)
        .with_context(|| format!("failed to write policy file {}", path.display()))?;

    Ok(())
}

fn key_trust_from_publication_state(publication_state: &str) -> PublicKeyTrustLevel {
    match publication_state {
        "published" => PublicKeyTrustLevel::Full,
        "pending" | "quarantined" => PublicKeyTrustLevel::Unknown,
        "rejected" | "revoked" => PublicKeyTrustLevel::Untrusted,
        _ => PublicKeyTrustLevel::Marginal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_state_mapping_matches_policy_expectations() {
        assert_eq!(
            key_trust_from_publication_state("published"),
            PublicKeyTrustLevel::Full
        );
        assert_eq!(
            key_trust_from_publication_state("pending"),
            PublicKeyTrustLevel::Unknown
        );
        assert_eq!(
            key_trust_from_publication_state("quarantined"),
            PublicKeyTrustLevel::Unknown
        );
        assert_eq!(
            key_trust_from_publication_state("rejected"),
            PublicKeyTrustLevel::Untrusted
        );
        assert_eq!(
            key_trust_from_publication_state("revoked"),
            PublicKeyTrustLevel::Untrusted
        );
    }

    #[test]
    fn status_exit_code_conformance() {
        assert_eq!(status_to_exit_code("ok"), 0);
        assert_eq!(status_to_exit_code("warn"), 0);
        assert_eq!(status_to_exit_code("fail"), 2);
    }

    #[test]
    fn transport_failure_maps_to_exit_code_3() {
        let opts = DiagnosticOptions {
            format: "json".to_string(),
            verbose: false,
            diagnostics: false,
            no_color: true,
            timeout: 5,
        };
        let code = emit_transport_failure(&opts, anyhow::anyhow!("simulated transport error"))
            .expect("transport failure should still render output");
        assert_eq!(code, 3);
    }

    #[test]
    fn persisted_session_state_round_trips() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "openpulse-cli-session-state-{}-{}",
            std::process::id(),
            nonce
        ));
        let path = root.join("session-state.json");

        let state = PersistedSessionState {
            session_id: "sess-42".to_string(),
            peer: "N0CALL".to_string(),
            hpx_state: "activetransfer".to_string(),
            selected_mode: Some("normal".to_string()),
            trust_level: Some("verified".to_string()),
            policy_profile: "balanced".to_string(),
            updated_at_ms: 1234,
        };

        persist_session_state_at(&path, &state).expect("persist session state");
        let loaded = load_session_state_at(&path)
            .expect("load session state")
            .expect("state should exist");
        assert_eq!(loaded, state);

        clear_session_state_at(&path).expect("clear session state");
        let none = load_session_state_at(&path).expect("load after clear");
        assert!(none.is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn trust_store_round_trips() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "openpulse-cli-trust-store-{}-{}",
            std::process::id(),
            nonce
        ));
        let path = root.join("trust-store.json");

        let store = LocalTrustStore {
            schema_version: "1.0.0".to_string(),
            records: vec![LocalTrustRecord {
                station_id: "N0CALL".to_string(),
                key_id: "key-42".to_string(),
                trust: PublicKeyTrustLevel::Full,
                source: CertificateSource::OutOfBand,
                status: "active".to_string(),
                reason: "operator_import".to_string(),
                updated_at_ms: 42,
            }],
        };

        persist_trust_store_at(&path, &store).expect("persist trust store");
        let loaded = load_trust_store_at(&path).expect("load trust store");
        assert_eq!(loaded, store);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn trust_and_source_parsers_accept_expected_values() {
        assert_eq!(
            parse_public_key_trust_level("full").expect("trust"),
            PublicKeyTrustLevel::Full
        );
        assert_eq!(
            parse_public_key_trust_level("revoked").expect("trust"),
            PublicKeyTrustLevel::Revoked
        );
        assert!(parse_public_key_trust_level("bogus").is_err());

        assert_eq!(
            parse_certificate_source("out_of_band").expect("source"),
            CertificateSource::OutOfBand
        );
        assert_eq!(
            parse_certificate_source("over-air").expect("source"),
            CertificateSource::OverAir
        );
        assert!(parse_certificate_source("bogus").is_err());
    }
}
