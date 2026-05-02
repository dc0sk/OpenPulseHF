use anyhow::Result;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

use openpulse_modem::diagnostics::{DiagnosticFormatter, SessionDiagnostics};
use openpulse_modem::engine::SecureSessionParams;
use openpulse_modem::ModemEngine;

use crate::{
    output::{
        emit_output, emit_transport_failure, status_to_exit_code, trust_output, DiagnosticOutput,
        OutputFormat,
    },
    pki::{fetch_pki_trust, PkiClient},
    state::{
        append_session_log_entry, clear_session_state, load_policy_profile, load_session_log,
        load_session_state, parse_policy_profile, persist_session_log, persist_session_state,
        policy_profile_to_str, session_log_entry_to_value, session_log_from_transitions,
        session_state_file_path, PersistedSessionLogEntry, PersistedSessionState,
    },
    DiagnoseCommands, SessionCommands,
};

use openpulse_core::trust::{PolicyProfile, SigningMode};

// ── Session command handler ───────────────────────────────────────────────────

pub fn run(command: SessionCommands, engine: &mut ModemEngine, pki: &PkiClient) -> Result<i32> {
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
                    let _ = persist_session_log(&session_log_from_transitions(
                        engine.hpx_transitions(),
                    ));

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
                    if !engine.hpx_transitions().is_empty() {
                        let _ = persist_session_log(&session_log_from_transitions(
                            engine.hpx_transitions(),
                        ));
                    }

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

            if opts.diagnostics {
                let persisted = load_session_state()?;
                let diag = build_session_diagnostics(engine, persisted.as_ref());
                let format = OutputFormat::from_arg(&opts.format)?;

                match format {
                    OutputFormat::Json => {
                        println!("{}", diag.to_json_pretty()?);
                    }
                    OutputFormat::Text => {
                        let formatter = DiagnosticFormatter::new(opts.verbose);
                        println!("{}", formatter.format_summary(&diag));

                        if diag.events.is_empty() {
                            println!("events: none");
                        } else {
                            for event in &diag.events {
                                println!("{}", formatter.format_event(event));
                            }
                        }

                        if opts.verbose {
                            if let Some(metrics) = &diag.pipeline_metrics {
                                println!("pipeline_metrics: {}", serde_json::to_string(metrics)?);
                            }
                        }
                    }
                }

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

        SessionCommands::Resume { opts } => {
            if engine.hpx_state() != openpulse_core::hpx::HpxState::Idle
                || engine.hpx_session_id().is_some()
                || engine.active_handshake().is_some()
            {
                let output = DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "active".to_string(),
                    reason_code: "active_session_present".to_string(),
                    details: json!({
                        "hpx_state": format!("{:?}", engine.hpx_state()).to_lowercase(),
                        "session_id": engine.hpx_session_id().map(ToString::to_string),
                    }),
                    recommendation:
                        "End the current session before attempting to resume from snapshot."
                            .to_string(),
                };
                emit_output(&opts, &output)?;
                return Ok(status_to_exit_code(&output.status));
            }

            let Some(persisted) = load_session_state()? else {
                let output = DiagnosticOutput {
                    status: "fail".to_string(),
                    decision: "missing".to_string(),
                    reason_code: "session_snapshot_not_found".to_string(),
                    details: json!({
                        "session_state_file": session_state_file_path().display().to_string(),
                    }),
                    recommendation:
                        "Start a session first so a snapshot can be persisted for resume."
                            .to_string(),
                };
                emit_output(&opts, &output)?;
                return Ok(status_to_exit_code(&output.status));
            };

            if let Ok(profile) = parse_policy_profile(&persisted.policy_profile) {
                engine.set_trust_policy_profile(profile);
            }

            let _ = append_session_log_entry(PersistedSessionLogEntry {
                timestamp_ms: persisted.updated_at_ms,
                from_state: persisted.hpx_state.clone(),
                to_state: persisted.hpx_state.clone(),
                event: "resume".to_string(),
                reason_code: "session_snapshot_resumed".to_string(),
                reason_string: "session metadata restored from persisted snapshot".to_string(),
            });

            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "resumed".to_string(),
                reason_code: "session_snapshot_resumed".to_string(),
                details: json!({
                    "session_id": persisted.session_id,
                    "peer": persisted.peer,
                    "persisted_hpx_state": persisted.hpx_state,
                    "selected_mode": persisted.selected_mode,
                    "trust_level": persisted.trust_level,
                    "policy_profile": persisted.policy_profile,
                    "updated_at_ms": persisted.updated_at_ms,
                    "resumed_from_snapshot": true,
                }),
                recommendation:
                    "Session metadata restored from snapshot. Start a new secure session handshake to re-attach runtime state."
                        .to_string(),
            };

            emit_output(&opts, &output)?;
            Ok(0)
        }

        SessionCommands::List { opts } => {
            let mut sessions: Vec<serde_json::Value> = vec![];

            if let Some(session_id) = engine.hpx_session_id() {
                sessions.push(json!({
                    "session_id": session_id,
                    "peer": "live",
                    "hpx_state": format!("{:?}", engine.hpx_state()).to_lowercase(),
                    "source": "in_memory",
                }));
            }

            if let Some(persisted) = load_session_state()? {
                let persisted_id = persisted.session_id.clone();
                let already_listed = sessions
                    .iter()
                    .any(|s| s["session_id"].as_str() == Some(persisted_id.as_str()));
                if !already_listed {
                    sessions.push(json!({
                        "session_id": persisted.session_id,
                        "peer": persisted.peer,
                        "hpx_state": persisted.hpx_state,
                        "selected_mode": persisted.selected_mode,
                        "trust_level": persisted.trust_level,
                        "policy_profile": persisted.policy_profile,
                        "updated_at_ms": persisted.updated_at_ms,
                        "source": "persisted",
                    }));
                }
            }

            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "listed".to_string(),
                reason_code: "session_list".to_string(),
                details: json!({
                    "sessions": sessions,
                    "count": sessions.len(),
                }),
                recommendation: "Use `session state` or `session resume` for a specific snapshot."
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

            let persisted_before_end = load_session_state()?;

            let session_id = engine
                .hpx_session_id()
                .map(ToString::to_string)
                .or_else(|| {
                    persisted_before_end
                        .as_ref()
                        .map(|state| state.session_id.clone())
                })
                .unwrap_or_else(|| format!("sess-{timestamp_ms}"));

            let result = engine.end_secure_session(timestamp_ms);

            let output = match result {
                Ok(()) => {
                    if !engine.hpx_transitions().is_empty() {
                        let _ = persist_session_log(&session_log_from_transitions(
                            engine.hpx_transitions(),
                        ));
                    } else if let Some(state) = &persisted_before_end {
                        let _ = append_session_log_entry(PersistedSessionLogEntry {
                            timestamp_ms,
                            from_state: state.hpx_state.clone(),
                            to_state: "idle".to_string(),
                            event: "localcancel".to_string(),
                            reason_code: "session_ended".to_string(),
                            reason_string: "session closed from persisted snapshot".to_string(),
                        });
                    }
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

        SessionCommands::Log {
            follow,
            follow_timeout_ms,
            poll_interval_ms,
            opts,
        } => {
            use std::thread;
            use std::time::Duration;

            let transitions = engine.hpx_transitions();
            let session_id = engine.hpx_session_id().map(ToString::to_string);

            if !transitions.is_empty() {
                persist_session_log(&session_log_from_transitions(transitions))?;
            }

            let mut log_entries = load_session_log()?
                .unwrap_or_default()
                .into_iter()
                .map(session_log_entry_to_value)
                .collect::<Vec<serde_json::Value>>();

            if follow {
                let started_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let deadline_ms = started_ms.saturating_add(follow_timeout_ms);
                let mut seen_len = log_entries.len();

                loop {
                    let now_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(deadline_ms);
                    if now_ms >= deadline_ms {
                        break;
                    }

                    let latest = load_session_log()?.unwrap_or_default();
                    if latest.len() > seen_len {
                        log_entries = latest
                            .iter()
                            .cloned()
                            .map(session_log_entry_to_value)
                            .collect();
                        seen_len = latest.len();
                    }

                    thread::sleep(Duration::from_millis(poll_interval_ms));
                }
            }

            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: format!("{:?}", engine.hpx_state()).to_lowercase(),
                reason_code: "session_log".to_string(),
                details: json!({
                    "session_id": session_id,
                    "hpx_state": format!("{:?}", engine.hpx_state()).to_lowercase(),
                    "schema_version": "1.0.0",
                    "transition_count": log_entries.len(),
                    "follow": follow,
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

// ── Diagnose command handler ──────────────────────────────────────────────────

pub fn run_diagnose(command: DiagnoseCommands, pki: &PkiClient) -> Result<i32> {
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
                    recommendation: "Peer identity must be resolvable before handshake diagnosis."
                        .to_string(),
                };
                emit_output(&opts, &output)?;
                return Ok(2);
            };

            let output = trust_output(trust_decision, details, opts.verbose, profile);
            emit_output(&opts, &output)?;
            Ok(status_to_exit_code(&output.status))
        }

        DiagnoseCommands::Manifest { session, opts } => {
            let bundle = match pki.get_current_bundle() {
                Ok(Some(b)) => b,
                Ok(None) => {
                    let output = DiagnosticOutput {
                        status: "fail".to_string(),
                        decision: "invalid".to_string(),
                        reason_code: "invalid_manifest_schema".to_string(),
                        details: json!({
                            "session": session,
                            "detail": "no current trust bundle available",
                        }),
                        recommendation: "Ensure the PKI service has a published trust bundle."
                            .to_string(),
                    };
                    emit_output(&opts, &output)?;
                    return Ok(2);
                }
                Err(err) => return emit_transport_failure(&opts, err),
            };

            let output = DiagnosticOutput {
                status: "ok".to_string(),
                decision: "verified".to_string(),
                reason_code: "manifest_shape_valid".to_string(),
                details: json!({
                    "session": session,
                    "bundle_id": bundle.bundle_id,
                    "manifest_fields": ["session_id", "peer_id", "policy_profile", "selected_mode"],
                    "signature_placeholder": "manifest signing not yet implemented",
                }),
                recommendation: "Manifest shape is valid (signature verification is a stub)."
                    .to_string(),
            };
            emit_output(&opts, &output)?;
            Ok(0)
        }

        DiagnoseCommands::Session { peer, opts } => {
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
                    recommendation: "Peer identity must be resolvable before session diagnosis."
                        .to_string(),
                };
                emit_output(&opts, &output)?;
                return Ok(2);
            };

            let _ = record_handshake_session_audit(pki, &peer, profile, &trust_decision);

            let output = trust_output(trust_decision, details, opts.verbose, profile);
            emit_output(&opts, &output)?;
            Ok(status_to_exit_code(&output.status))
        }
    }
}

// ── Session helpers ───────────────────────────────────────────────────────────

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

    let transition_values: Vec<serde_json::Value> = transitions
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

fn record_handshake_session_audit(
    pki: &PkiClient,
    peer: &str,
    profile: PolicyProfile,
    trust_decision: &openpulse_core::trust::TrustDecision,
) -> Result<()> {
    use openpulse_audio::LoopbackBackend;

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
        .collect::<Vec<serde_json::Value>>();

    let session_id = audit_engine
        .hpx_session_id()
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("diag-{timestamp_ms}"));

    let payload = json!({
        "session_id": session_id,
        "peer_id": peer,
        "policy_profile": policy_profile_to_str(profile),
        "selected_mode": "none",
        "trust_level": format!("{:?}", trust_decision.decision).to_lowercase(),
        "certificate_source": format!("{:?}", trust_decision.certificate_source).to_lowercase(),
        "trust_reason_code": trust_decision.reason_code,
        "transitions": transitions,
        "actor_identity": "openpulse-cli",
    });

    pki.create_session_audit_event(&payload)
}

fn build_session_diagnostics(
    engine: &ModemEngine,
    persisted: Option<&PersistedSessionState>,
) -> SessionDiagnostics {
    let mut diag = SessionDiagnostics::new(
        engine
            .hpx_session_id()
            .map(ToString::to_string)
            .or_else(|| persisted.map(|state| state.session_id.clone()))
            .unwrap_or_else(|| "no-session".to_string()),
        diagnostics_peer(engine, persisted),
    );
    diag.current_state = format!("{:?}", engine.hpx_state()).to_lowercase();
    diag.set_pipeline_metrics(engine.pipeline_metrics_snapshot());

    for transition in engine.hpx_transitions() {
        diag.record_transition(transition);
    }

    diag
}

fn diagnostics_peer(engine: &ModemEngine, persisted: Option<&PersistedSessionState>) -> String {
    if let Some(state) = persisted {
        return state.peer.clone();
    }
    if engine.hpx_session_id().is_some() {
        return "live".to_string();
    }
    "unknown".to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_audio::LoopbackBackend;

    #[test]
    fn build_session_diagnostics_uses_persisted_peer_and_transition_counts() {
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        engine
            .hpx_apply_event(openpulse_core::hpx::HpxEvent::StartSession, 100)
            .expect("start session transition");

        let persisted = PersistedSessionState {
            session_id: "sess-100".to_string(),
            peer: "W1ABC".to_string(),
            hpx_state: "discovery".to_string(),
            selected_mode: None,
            trust_level: None,
            policy_profile: "balanced".to_string(),
            updated_at_ms: 100,
        };

        let diag = build_session_diagnostics(&engine, Some(&persisted));

        assert_eq!(diag.peer, "W1ABC");
        assert_eq!(diag.current_state, "discovery");
        assert_eq!(diag.total_transitions, 1);
        assert_eq!(diag.total_events, 1);
        assert_eq!(diag.events.len(), 1);
    }
}
