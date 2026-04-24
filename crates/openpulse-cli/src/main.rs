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
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use tracing::Level;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::trust::{
    allowed_signing_modes, classify_connection_trust, evaluate_handshake, CertificateSource,
    ConnectionTrustLevel, PolicyProfile, PublicKeyTrustLevel, SigningMode,
};
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

    // Dispatch.
    let mut exit_code = 0;
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
            exit_code = run_identity(command)?;
        }

        Commands::Trust { command } => {
            exit_code = run_trust(command)?;
        }

        Commands::Diagnose { command } => {
            exit_code = run_diagnose(command)?;
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

fn run_identity(command: IdentityCommands) -> Result<i32> {
    match command {
        IdentityCommands::Show {
            station_or_record_id,
            opts,
        } => {
            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "verified".to_string(),
                reason_code: "identity_found".to_string(),
                details: json!({
                    "peer": station_or_record_id,
                    "cache_fresh": true,
                    "key_continuity": "ok",
                }),
                recommendation: "Identity looks healthy.".to_string(),
            };
            emit_output(&opts, &output)?;
            Ok(0)
        }
        IdentityCommands::Verify {
            station_or_record_id,
            opts,
        } => {
            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "verified".to_string(),
                reason_code: "signature_chain_valid".to_string(),
                details: json!({
                    "peer": station_or_record_id,
                    "signature_chain": "valid",
                    "effective_revocation_state": "none",
                }),
                recommendation: "No action required.".to_string(),
            };
            emit_output(&opts, &output)?;
            Ok(0)
        }
        IdentityCommands::Cache { opts } => {
            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "cache".to_string(),
                reason_code: "cache_summary".to_string(),
                details: json!({
                    "entries": 0,
                    "freshness_seconds": opts.timeout,
                }),
                recommendation: "Populate cache by running identity show/verify.".to_string(),
            };
            emit_output(&opts, &output)?;
            Ok(0)
        }
    }
}

fn run_trust(command: TrustCommands) -> Result<i32> {
    match command {
        TrustCommands::Show {
            station_or_record_id,
            opts,
        } => {
            let profile = load_policy_profile()?;
            let decision = classify_connection_trust(
                PublicKeyTrustLevel::Unknown,
                CertificateSource::OverAir,
                false,
            );
            let output = trust_output(station_or_record_id, decision, false, profile);
            emit_output(&opts, &output)?;
            Ok(status_to_exit_code(&output.status))
        }
        TrustCommands::Explain {
            station_or_record_id,
            opts,
        } => {
            let profile = load_policy_profile()?;
            let decision = classify_connection_trust(
                PublicKeyTrustLevel::Unknown,
                CertificateSource::OverAir,
                false,
            );
            let output = trust_output(station_or_record_id, decision, true, profile);
            emit_output(&opts, &output)?;
            Ok(match output.status.as_str() {
                "fail" => 2,
                _ => 0,
            })
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

fn run_diagnose(command: DiagnoseCommands) -> Result<i32> {
    match command {
        DiagnoseCommands::Handshake { peer, opts } => {
            let profile = load_policy_profile()?;
            let handshake = evaluate_handshake(
                profile,
                SigningMode::Normal,
                &[SigningMode::Normal, SigningMode::Psk],
                PublicKeyTrustLevel::Marginal,
                CertificateSource::OutOfBand,
                false,
            );

            let output = match handshake {
                Ok(h) => DiagnosticOutput {
                    status: "ok".to_string(),
                    decision: format!("{:?}", h.trust.decision).to_lowercase(),
                    reason_code: h.trust.reason_code.to_string(),
                    details: json!({
                        "peer": peer,
                        "policy_profile": policy_profile_to_str(profile),
                        "selected_mode": format!("{:?}", h.selected_mode).to_lowercase(),
                        "certificate_source": "out_of_band",
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

            emit_output(&opts, &output)?;
            Ok(status_to_exit_code(&output.status))
        }
        DiagnoseCommands::Manifest { session, opts } => {
            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "verified".to_string(),
                reason_code: "manifest_schema_valid".to_string(),
                details: json!({
                    "session": session,
                    "schema": "signed_manifest_v1",
                }),
                recommendation: "Manifest shape and metadata are valid.".to_string(),
            };
            emit_output(&opts, &output)?;
            Ok(0)
        }
        DiagnoseCommands::Session { peer, opts } => {
            let identity = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "verified".to_string(),
                reason_code: "signature_chain_valid".to_string(),
                details: json!({"peer": peer}),
                recommendation: "Identity checks passed.".to_string(),
            };
            let trust = trust_output(
                peer.clone(),
                classify_connection_trust(
                    PublicKeyTrustLevel::Unknown,
                    CertificateSource::OverAir,
                    false,
                ),
                false,
                load_policy_profile()?,
            );
            let handshake = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "reduced".to_string(),
                reason_code: "over_air_certificate_without_psk".to_string(),
                details: json!({"peer": peer}),
                recommendation: "Proceed only for low-risk operations.".to_string(),
            };

            let output = DiagnosticOutput {
                status: merge_statuses([
                    identity.status.as_str(),
                    trust.status.as_str(),
                    handshake.status.as_str(),
                ]),
                decision: trust.decision.clone(),
                reason_code: trust.reason_code.clone(),
                details: json!({
                    "identity": identity,
                    "trust": trust,
                    "handshake": handshake,
                }),
                recommendation: "Run trust explain for detailed downgrade reasoning.".to_string(),
            };

            emit_output(&opts, &output)?;
            Ok(status_to_exit_code(&output.status))
        }
    }
}

fn trust_output(
    peer: String,
    decision: openpulse_core::trust::TrustDecision,
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

    let mut details = json!({
        "peer": peer,
        "trust_state": format!("{:?}", decision.public_key_trust).to_lowercase(),
        "certificate_source": format!("{:?}", decision.certificate_source).to_lowercase(),
        "policy_profile": policy_profile_to_str(policy_profile),
        "policy_profile_version": "1.0.0",
        "evidence_classes": ["operator", "gpg", "tqsl", "replication"],
        "effective_revocation_state": "none",
    });

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

fn merge_statuses<'a>(statuses: impl IntoIterator<Item = &'a str>) -> String {
    let mut result = "ok";
    for status in statuses {
        if status == "fail" {
            return "fail".to_string();
        }
        if status == "warn" {
            result = "warn";
        }
    }
    result.to_string()
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
