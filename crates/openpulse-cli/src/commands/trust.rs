use anyhow::Result;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    output::{
        emit_output, emit_transport_failure, status_to_exit_code, trust_output, DiagnosticOutput,
    },
    pki::{fetch_pki_trust, parse_certificate_source, parse_public_key_trust_level, PkiClient},
    state::{
        load_policy_profile, load_trust_store, persist_trust_store, trust_store_file_path,
        LocalTrustRecord,
    },
    TrustCommands,
};

pub fn run(command: TrustCommands, pki: &PkiClient) -> Result<i32> {
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
                        details: json!({"requested_trust": trust}),
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
                        details: json!({"requested_source": source}),
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
                    details: json!({"station_or_key": station_or_key}),
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

        TrustCommands::Policy { command } => {
            use crate::commands::modes::run_trust_policy;
            run_trust_policy(command, pki)
        }
    }
}
