use anyhow::Result;
use clap::Args;
use serde::Serialize;
use serde_json::{json, Value};

use openpulse_core::trust::{ConnectionTrustLevel, PolicyProfile};

use crate::state::policy_profile_to_str;

#[derive(Args, Clone)]
pub struct DiagnosticOptions {
    /// Output format.
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Include additional detail lines.
    #[arg(long)]
    pub verbose: bool,

    /// Emit detailed HPX diagnostics (JSON).
    #[arg(long)]
    pub diagnostics: bool,

    /// Disable color output.
    #[arg(long)]
    pub no_color: bool,

    /// Request timeout in seconds.
    #[arg(long, default_value_t = 5)]
    pub timeout: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

impl OutputFormat {
    pub fn from_arg(value: &str) -> Result<Self> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => anyhow::bail!("invalid --format '{value}', expected text|json"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticOutput {
    pub status: String,
    pub decision: String,
    pub reason_code: String,
    pub details: Value,
    pub recommendation: String,
}

pub fn emit_output(opts: &DiagnosticOptions, output: &DiagnosticOutput) -> Result<()> {
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

pub fn status_to_exit_code(status: &str) -> i32 {
    match status {
        "fail" => 2,
        _ => 0,
    }
}

pub fn emit_transport_failure(opts: &DiagnosticOptions, err: anyhow::Error) -> Result<i32> {
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

pub fn trust_output(
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

pub fn signing_mode_to_str(mode: openpulse_core::trust::SigningMode) -> &'static str {
    use openpulse_core::trust::SigningMode;
    match mode {
        SigningMode::Normal => "normal",
        SigningMode::Psk => "psk",
        SigningMode::Relaxed => "relaxed",
        SigningMode::Paranoid => "paranoid",
        SigningMode::Pq => "pq",
        SigningMode::Hybrid => "hybrid",
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
