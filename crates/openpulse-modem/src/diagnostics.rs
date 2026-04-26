//! Structured diagnostics and observability for HPX sessions.

use openpulse_core::hpx::{HpxEvent, HpxReasonCode, HpxTransition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::pipeline::PipelineMetricsSnapshot;

/// A structured HPX event in the diagnostic log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticEvent {
    pub timestamp_ms: u64,
    pub event_source: String,
    pub event: String,
    pub state_before: String,
    pub state_after: Option<String>,
    pub reason_code: String,
    pub reason_string: Option<String>,
    pub session_id: Option<String>,
    pub metadata: HashMap<String, String>,
}

/// Diagnostic snapshot of an HPX session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDiagnostics {
    pub session_id: String,
    pub peer: String,
    pub current_state: String,
    pub total_transitions: usize,
    pub total_events: usize,
    pub elapsed_ms: u64,
    pub error_count: usize,
    pub recovery_count: usize,
    pub pipeline_metrics: Option<PipelineMetricsSnapshot>,
    pub events: Vec<DiagnosticEvent>,
}

impl SessionDiagnostics {
    /// Create a new empty session diagnostic log.
    pub fn new(session_id: impl Into<String>, peer: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            peer: peer.into(),
            current_state: "idle".to_string(),
            total_transitions: 0,
            total_events: 0,
            elapsed_ms: 0,
            error_count: 0,
            recovery_count: 0,
            pipeline_metrics: None,
            events: vec![],
        }
    }

    /// Attach per-stage pipeline metrics snapshot.
    pub fn set_pipeline_metrics(&mut self, metrics: PipelineMetricsSnapshot) {
        self.pipeline_metrics = Some(metrics);
    }

    /// Add a transition to the event log.
    pub fn record_transition(&mut self, transition: &HpxTransition) {
        let ts_ms = transition.timestamp_ms;
        self.current_state = format!("{:?}", transition.to_state).to_lowercase();
        self.total_transitions += 1;
        self.total_events += 1;

        // Track state-machine errors (Timeout, Signature failures, exhausted retries)
        match transition.reason_code {
            HpxReasonCode::Timeout
            | HpxReasonCode::SignatureFailure
            | HpxReasonCode::RetriesExhausted
            | HpxReasonCode::RecoveryTimeout
            | HpxReasonCode::RecoveryAttemptsExhausted
            | HpxReasonCode::ManifestVerificationFailed => {
                self.error_count += 1;
            }
            _ => {}
        }

        // Track recovery events
        if format!("{:?}", transition.to_state).to_lowercase() == "recovery" {
            self.recovery_count += 1;
        }

        self.events.push(DiagnosticEvent {
            timestamp_ms: ts_ms,
            event_source: "transition".to_string(),
            event: format!("{:?}", transition.event).to_lowercase(),
            state_before: format!("{:?}", transition.from_state).to_lowercase(),
            state_after: Some(self.current_state.clone()),
            reason_code: format!("{:?}", transition.reason_code).to_lowercase(),
            reason_string: Some(transition.reason_string.clone()),
            session_id: transition.session_id.clone(),
            metadata: Default::default(),
        });
    }

    /// Add a raw event to the log (without a transition).
    pub fn record_event(
        &mut self,
        event: HpxEvent,
        reason_code: &str,
        metadata: Option<HashMap<String, String>>,
    ) {
        self.total_events += 1;
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        self.events.push(DiagnosticEvent {
            timestamp_ms: ts_ms,
            event_source: "event".to_string(),
            event: format!("{:?}", event).to_lowercase(),
            state_before: self.current_state.clone(),
            state_after: None,
            reason_code: reason_code.to_string(),
            reason_string: None,
            session_id: Some(self.session_id.clone()),
            metadata: metadata.unwrap_or_default(),
        });
    }

    /// Compute elapsed time and update the snapshot.
    pub fn update_elapsed(&mut self, base_time_ms: u64, current_time_ms: u64) {
        self.elapsed_ms = current_time_ms.saturating_sub(base_time_ms);
    }

    /// Get a summary string for quick status reporting.
    pub fn summary(&self) -> String {
        format!(
            "Session {} (peer={}): state={}, transitions={}, events={}, errors={}, recovery_count={}, elapsed={}ms",
            self.session_id,
            self.peer,
            self.current_state,
            self.total_transitions,
            self.total_events,
            self.error_count,
            self.recovery_count,
            self.elapsed_ms
        )
    }

    /// Serialize to JSON for CLI output.
    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

/// Diagnostic output formatter for CLI commands.
pub struct DiagnosticFormatter {
    verbose: bool,
}

impl DiagnosticFormatter {
    /// Create a new formatter with optional verbosity.
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }

    /// Format a diagnostic event for human-readable output.
    pub fn format_event(&self, event: &DiagnosticEvent) -> String {
        if self.verbose {
            format!(
                "[{:>5}ms] {} {} → {} (reason: {})",
                event.timestamp_ms,
                event.event,
                event.state_before,
                event.state_after.as_deref().unwrap_or("(no transition)"),
                event.reason_code
            )
        } else {
            format!(
                "{} → {} ({})",
                event.state_before,
                event.state_after.as_deref().unwrap_or("(no transition)"),
                event.event
            )
        }
    }

    /// Format a session diagnostic summary.
    pub fn format_summary(&self, diag: &SessionDiagnostics) -> String {
        diag.summary()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::HpxState;

    #[test]
    fn session_diagnostics_record_transition() {
        let mut diag = SessionDiagnostics::new("sess-1", "N0CALL");
        assert_eq!(diag.current_state, "idle");
        assert_eq!(diag.total_transitions, 0);

        let transition = HpxTransition {
            timestamp_ms: 100,
            from_state: HpxState::Idle,
            to_state: HpxState::Discovery,
            event: HpxEvent::StartSession,
            reason_code: HpxReasonCode::Success,
            reason_string: "session start".to_string(),
            session_id: Some("sess-1".to_string()),
        };

        diag.record_transition(&transition);
        assert_eq!(diag.current_state, "discovery");
        assert_eq!(diag.total_transitions, 1);
        assert_eq!(diag.events.len(), 1);
    }

    #[test]
    fn diagnostic_formatter_verbose_mode() {
        let event = DiagnosticEvent {
            timestamp_ms: 1234,
            event_source: "transition".to_string(),
            event: "discoverytimeout".to_string(),
            state_before: "discovery".to_string(),
            state_after: Some("failed".to_string()),
            reason_code: "timeout".to_string(),
            reason_string: Some("discovery timeout".to_string()),
            session_id: Some("sess-1".to_string()),
            metadata: Default::default(),
        };

        let formatter_verbose = DiagnosticFormatter::new(true);
        let output = formatter_verbose.format_event(&event);
        assert!(output.contains("1234ms"));
        assert!(output.contains("discovery"));
        assert!(output.contains("failed"));
        assert!(output.contains("reason: timeout"));

        let formatter_brief = DiagnosticFormatter::new(false);
        let output = formatter_brief.format_event(&event);
        assert!(!output.contains("1234ms"));
    }

    #[test]
    fn session_diagnostics_json_serialization() {
        let diag = SessionDiagnostics::new("sess-2", "W1ABC");
        let json = diag.to_json_pretty().expect("serialize to json");
        assert!(json.contains("sess-2"));
        assert!(json.contains("W1ABC"));
        assert!(json.contains("idle"));
    }

    #[test]
    fn session_diagnostics_transition_preserves_structured_fields() {
        let mut diag = SessionDiagnostics::new("sess-9", "K1TEST");

        let transition = HpxTransition {
            timestamp_ms: 250,
            from_state: HpxState::Training,
            to_state: HpxState::ActiveTransfer,
            event: HpxEvent::TrainingOk,
            reason_code: HpxReasonCode::Success,
            reason_string: "training complete".to_string(),
            session_id: Some("sess-9".to_string()),
        };

        diag.record_transition(&transition);

        assert_eq!(diag.total_transitions, 1);
        assert_eq!(diag.total_events, 1);
        assert_eq!(diag.events[0].event_source, "transition");
        assert_eq!(
            diag.events[0].reason_string.as_deref(),
            Some("training complete")
        );
        assert_eq!(diag.events[0].session_id.as_deref(), Some("sess-9"));
    }

    #[test]
    fn session_diagnostics_raw_event_uses_session_context() {
        let mut diag = SessionDiagnostics::new("sess-10", "K1TEST");
        diag.record_event(HpxEvent::QualityDrop, "quality_drop", None);

        assert_eq!(diag.total_events, 1);
        assert_eq!(diag.events[0].event_source, "event");
        assert_eq!(diag.events[0].session_id.as_deref(), Some("sess-10"));
        assert!(diag.events[0].reason_string.is_none());
    }
}
