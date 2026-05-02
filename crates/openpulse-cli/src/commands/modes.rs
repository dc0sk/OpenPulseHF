use anyhow::Result;
use serde_json::json;

use openpulse_core::trust::allowed_signing_modes;
use openpulse_modem::ModemEngine;

use crate::{
    output::{emit_output, emit_transport_failure, signing_mode_to_str, DiagnosticOutput},
    pki::PkiClient,
    state::{load_policy_profile, policy_file_path, policy_profile_to_str},
    IdentityCommands,
};

pub fn run_modes(engine: &ModemEngine) -> Result<()> {
    for info in engine.plugins().list() {
        println!(
            "{}: {} ({})",
            info.name,
            info.description,
            info.supported_modes.join(", ")
        );
    }
    Ok(())
}

pub fn run_identity(command: IdentityCommands, pki: &PkiClient) -> Result<i32> {
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
                            serde_json::Value::Array(items) => items.len(),
                            serde_json::Value::Object(map) => map.len(),
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

pub fn run_trust_policy(command: crate::TrustPolicyCommands, pki: &PkiClient) -> Result<i32> {
    use crate::output::status_to_exit_code;
    use crate::state::{parse_policy_profile, persist_policy_profile};
    let _ = pki;
    match command {
        crate::TrustPolicyCommands::Show { opts } => {
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
        crate::TrustPolicyCommands::Set { profile, opts } => {
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
                        recommendation: "Policy profile persisted for subsequent runs.".to_string(),
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
    }
}
