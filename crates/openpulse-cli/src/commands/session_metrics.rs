use anyhow::Result;
use serde_json::json;

use openpulse_modem::ModemEngine;

use crate::output::{emit_output, status_to_exit_code, DiagnosticOptions, DiagnosticOutput};
use crate::state::load_session_log;

const BYTES_PER_SUCCESS_FRAME_PROXY: f64 = 223.0;

fn parse_snr_db(reason: &str) -> Option<f64> {
    for marker in ["snr_db=", "snr="] {
        if let Some(pos) = reason.find(marker) {
            let start = pos + marker.len();
            let tail = &reason[start..];
            let token: String = tail
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || *ch == '.' || *ch == '-')
                .collect();
            if token.is_empty() {
                continue;
            }
            if let Ok(value) = token.parse::<f64>() {
                return Some(value);
            }
        }
    }
    None
}

fn is_success_transition(event: &str, from_state: &str) -> bool {
    event == "transfercomplete" && from_state == "activetransfer"
}

fn is_error_transition(event: &str, from_state: &str) -> bool {
    event == "transfererror" && (from_state == "activetransfer" || from_state == "relayactive")
}

pub fn run(engine: &ModemEngine, opts: &DiagnosticOptions) -> Result<i32> {
    let Some(entries) = load_session_log()? else {
        let output = DiagnosticOutput {
            status: "fail".to_string(),
            decision: "missing".to_string(),
            reason_code: "session_metrics_unavailable".to_string(),
            details: json!({
                "session_log": "not_found",
            }),
            recommendation: "Run a session first so metrics can be exported from persisted logs."
                .to_string(),
        };
        emit_output(opts, &output)?;
        return Ok(status_to_exit_code(&output.status));
    };

    if entries.is_empty() {
        let output = DiagnosticOutput {
            status: "fail".to_string(),
            decision: "empty".to_string(),
            reason_code: "session_metrics_unavailable".to_string(),
            details: json!({
                "session_log": "empty",
            }),
            recommendation:
                "No persisted session events yet; run a transfer before exporting metrics."
                    .to_string(),
        };
        emit_output(opts, &output)?;
        return Ok(status_to_exit_code(&output.status));
    }

    let started_at_ms = entries.iter().map(|e| e.timestamp_ms).min().unwrap_or(0);
    let ended_at_ms = entries.iter().map(|e| e.timestamp_ms).max().unwrap_or(0);
    let elapsed_ms = ended_at_ms.saturating_sub(started_at_ms);

    let transfer_ok = entries
        .iter()
        .filter(|e| is_success_transition(&e.event, &e.from_state))
        .count() as u64;
    let transfer_error = entries
        .iter()
        .filter(|e| is_error_transition(&e.event, &e.from_state))
        .count() as u64;

    let attempts = transfer_ok + transfer_error;
    let fer = if attempts > 0 {
        transfer_error as f64 / attempts as f64
    } else {
        0.0
    };

    let latency_ms = if transfer_ok > 0 {
        Some(elapsed_ms as f64 / transfer_ok as f64)
    } else {
        None
    };

    let throughput_bps_upper_bound = if elapsed_ms > 0 {
        let bits = transfer_ok as f64 * BYTES_PER_SUCCESS_FRAME_PROXY * 8.0;
        Some(bits / (elapsed_ms as f64 / 1000.0))
    } else {
        None
    };

    let snr_samples: Vec<f64> = entries
        .iter()
        .filter_map(|e| parse_snr_db(&e.reason_string))
        .collect();
    let snr_db_estimate = if snr_samples.is_empty() {
        None
    } else {
        Some(snr_samples.iter().sum::<f64>() / snr_samples.len() as f64)
    };

    let pipeline_snapshot = engine.pipeline_metrics_snapshot();

    let output = DiagnosticOutput {
        status: "ok".to_string(),
        decision: "exported".to_string(),
        reason_code: "session_metrics_export".to_string(),
        details: json!({
            "session": {
                "events": entries.len(),
                "started_at_ms": started_at_ms,
                "ended_at_ms": ended_at_ms,
                "elapsed_ms": elapsed_ms,
            },
            "metrics": {
                "throughput_bps": throughput_bps_upper_bound,
                "throughput_bps_upper_bound": throughput_bps_upper_bound,
                "throughput_bps_note": "223-byte successful-frame proxy; actual payload throughput may be lower",
                "fer": fer,
                "latency_ms": latency_ms,
                "snr_db_estimate": snr_db_estimate,
                "transfer_ok": transfer_ok,
                "transfer_error": transfer_error,
            },
            "telemetry": {
                "snr_sample_count": snr_samples.len(),
                "afc_offset_hz": engine.last_afc_offset_hz(),
                "pipeline_metrics": pipeline_snapshot,
            },
            "notes": [
                "throughput_bps uses 223-byte successful-frame proxy from persisted transition logs",
                "snr_db_estimate is derived from reason_string markers like snr_db=<value> when present"
            ]
        }),
        recommendation: "Use this JSON export for post-session analysis and trend dashboards."
            .to_string(),
    };

    emit_output(opts, &output)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{is_error_transition, is_success_transition, parse_snr_db};

    #[test]
    fn parse_snr_db_handles_supported_markers() {
        assert_eq!(parse_snr_db("foo snr_db=12.5 bar"), Some(12.5));
        assert_eq!(parse_snr_db("snr=9.0"), Some(9.0));
        assert_eq!(parse_snr_db("no marker"), None);
    }

    #[test]
    fn transition_classifiers_avoid_double_counting() {
        assert!(is_success_transition("transfercomplete", "activetransfer"));
        assert!(!is_success_transition("transfercomplete", "teardown"));

        assert!(is_error_transition("transfererror", "activetransfer"));
        assert!(is_error_transition("transfererror", "relayactive"));
        assert!(!is_error_transition("transfererror", "teardown"));
        assert!(!is_error_transition("qualitydrop", "activetransfer"));
    }
}
