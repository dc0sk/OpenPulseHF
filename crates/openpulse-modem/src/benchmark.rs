//! HPX benchmark harness.

use openpulse_core::hpx::{HpxEvent, HpxState, HpxTransition};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::engine::ModemEngine;
use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkScenario {
    pub name: String,
    pub events: Vec<BenchmarkEvent>,
    pub expected_terminal_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkEvent {
    pub event: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub scenario: String,
    pub passed: bool,
    pub terminal_state: String,
    pub expected_terminal_state: String,
    pub transition_count: usize,
    pub wall_ms: u64,
    pub transitions: Vec<TransitionMetric>,
    pub error_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionMetric {
    pub from_state: String,
    pub to_state: String,
    pub event: String,
    pub reason_code: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub mean_transitions: f64,
    pub mean_wall_ms: f64,
    pub scenarios: Vec<ScenarioResult>,
}

fn bev(event: &str, elapsed_ms: u64) -> BenchmarkEvent {
    BenchmarkEvent {
        event: event.into(),
        elapsed_ms,
    }
}

/// Returns the standard benchmark scenario corpus used for regression gating.
///
/// Each scenario is a complete HPX path from Idle driven via `hpx_apply_event`.
/// - `TransferComplete` from `ActiveTransfer` → `Teardown`; a second from
///   `Teardown` → `Idle`.
/// - Recovery exhaustion fires on the 5th entry into `Recovery` from a
///   non-`Recovery` state (counter > MAX_RECOVERY_ATTEMPTS=4).
pub fn standard_corpus() -> Vec<BenchmarkScenario> {
    vec![
        BenchmarkScenario {
            name: "happy_path".into(),
            events: vec![
                bev("StartSession", 0),
                bev("DiscoveryOk", 10),
                bev("TrainingOk", 10),
                bev("TransferComplete", 50), // ActiveTransfer → Teardown
                bev("TransferComplete", 1),  // Teardown → Idle
            ],
            expected_terminal_state: "idle".into(),
        },
        BenchmarkScenario {
            name: "discovery_timeout".into(),
            events: vec![bev("StartSession", 0), bev("DiscoveryTimeout", 30000)],
            expected_terminal_state: "failed".into(),
        },
        BenchmarkScenario {
            name: "training_timeout".into(),
            events: vec![
                bev("StartSession", 0),
                bev("DiscoveryOk", 10),
                bev("TrainingTimeout", 30000),
            ],
            expected_terminal_state: "failed".into(),
        },
        BenchmarkScenario {
            name: "sig_reject_discovery".into(),
            events: vec![
                bev("StartSession", 0),
                bev("SignatureVerificationFailed", 5),
            ],
            expected_terminal_state: "failed".into(),
        },
        BenchmarkScenario {
            name: "sig_reject_active_transfer".into(),
            events: vec![
                bev("StartSession", 0),
                bev("DiscoveryOk", 10),
                bev("TrainingOk", 10),
                bev("SignatureVerificationFailed", 5),
            ],
            expected_terminal_state: "recovery".into(),
        },
        BenchmarkScenario {
            name: "quality_drop_then_recovery".into(),
            events: vec![
                bev("StartSession", 0),
                bev("DiscoveryOk", 10),
                bev("TrainingOk", 10),
                bev("QualityDrop", 5),
                bev("RecoveryOk", 20),
                bev("TransferComplete", 50), // ActiveTransfer → Teardown
                bev("TransferComplete", 1),  // Teardown → Idle
            ],
            expected_terminal_state: "idle".into(),
        },
        BenchmarkScenario {
            name: "recovery_exhaustion".into(),
            // 5th entry into Recovery from non-Recovery > MAX(4) → Failed.
            events: vec![
                bev("StartSession", 0),
                bev("DiscoveryOk", 10),
                bev("TrainingOk", 10),
                bev("QualityDrop", 5),
                bev("RecoveryOk", 5),
                bev("QualityDrop", 5),
                bev("RecoveryOk", 5),
                bev("QualityDrop", 5),
                bev("RecoveryOk", 5),
                bev("QualityDrop", 5),
                bev("RecoveryOk", 5),
                bev("QualityDrop", 5), // 5th attempt > MAX → Failed
            ],
            expected_terminal_state: "failed".into(),
        },
        BenchmarkScenario {
            name: "local_cancel_teardown".into(),
            events: vec![
                bev("StartSession", 0),
                bev("DiscoveryOk", 10),
                bev("TrainingOk", 10),
                bev("LocalCancel", 5),
                bev("TransferComplete", 5), // Teardown → Idle
            ],
            expected_terminal_state: "idle".into(),
        },
        BenchmarkScenario {
            name: "remote_teardown".into(),
            events: vec![
                bev("StartSession", 0),
                bev("DiscoveryOk", 10),
                bev("TrainingOk", 10),
                bev("RemoteTeardown", 5),
            ],
            expected_terminal_state: "teardown".into(),
        },
        BenchmarkScenario {
            name: "relay_activation".into(),
            events: vec![
                bev("StartSession", 0),
                bev("DiscoveryOk", 10),
                bev("TrainingOk", 10),
                bev("RelayRouteFound", 5),
                bev("TrainingOk", 10),
                bev("TransferComplete", 50), // ActiveTransfer → Teardown
                bev("TransferComplete", 1),  // Teardown → Idle
            ],
            expected_terminal_state: "idle".into(),
        },
    ]
}

fn parse_event(s: &str) -> Option<HpxEvent> {
    match s {
        "StartSession" => Some(HpxEvent::StartSession),
        "LocalCancel" => Some(HpxEvent::LocalCancel),
        "RemoteTeardown" => Some(HpxEvent::RemoteTeardown),
        "DiscoveryOk" => Some(HpxEvent::DiscoveryOk),
        "DiscoveryTimeout" => Some(HpxEvent::DiscoveryTimeout),
        "TrainingOk" => Some(HpxEvent::TrainingOk),
        "TrainingTimeout" => Some(HpxEvent::TrainingTimeout),
        "TransferComplete" => Some(HpxEvent::TransferComplete),
        "TransferError" => Some(HpxEvent::TransferError),
        "QualityDrop" => Some(HpxEvent::QualityDrop),
        "RecoveryOk" => Some(HpxEvent::RecoveryOk),
        "RecoveryTimeout" => Some(HpxEvent::RecoveryTimeout),
        "RelayRouteFound" => Some(HpxEvent::RelayRouteFound),
        "RelayPolicyFailed" => Some(HpxEvent::RelayPolicyFailed),
        "SignatureVerificationFailed" => Some(HpxEvent::SignatureVerificationFailed),
        _ => None,
    }
}

fn hpx_state_to_str(s: HpxState) -> &'static str {
    match s {
        HpxState::Idle => "idle",
        HpxState::Discovery => "discovery",
        HpxState::Training => "training",
        HpxState::ActiveTransfer => "activetransfer",
        HpxState::Recovery => "recovery",
        HpxState::RelayActive => "relayactive",
        HpxState::Teardown => "teardown",
        HpxState::Failed => "failed",
    }
}

fn transition_to_metric(t: &HpxTransition) -> TransitionMetric {
    TransitionMetric {
        from_state: format!("{:?}", t.from_state).to_lowercase(),
        to_state: format!("{:?}", t.to_state).to_lowercase(),
        event: format!("{:?}", t.event).to_lowercase(),
        reason_code: format!("{:?}", t.reason_code).to_lowercase(),
        timestamp_ms: t.timestamp_ms,
    }
}

/// Run a single scenario from Idle via raw HPX events.
pub fn run_scenario(scenario: &BenchmarkScenario) -> ScenarioResult {
    let audio = Box::new(LoopbackBackend::new());
    let mut engine = ModemEngine::new(audio);
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("BPSK plugin registration failed");

    let wall_start = Instant::now();
    let ts_base: u64 = 1_000_000;
    let mut cursor_ms = ts_base;
    let mut error_count = 0usize;

    for bev in &scenario.events {
        cursor_ms = cursor_ms.saturating_add(bev.elapsed_ms);
        if let Some(event) = parse_event(&bev.event) {
            if engine.hpx_apply_event(event, cursor_ms).is_err() {
                error_count += 1;
            }
        }
    }

    let wall_ms = wall_start.elapsed().as_millis() as u64;
    let terminal = engine.hpx_state();
    let terminal_str = hpx_state_to_str(terminal).to_string();
    let passed = terminal_str == scenario.expected_terminal_state;
    let transitions: Vec<TransitionMetric> = engine
        .hpx_transitions()
        .iter()
        .map(transition_to_metric)
        .collect();
    let transition_count = transitions.len();

    ScenarioResult {
        scenario: scenario.name.clone(),
        passed,
        terminal_state: terminal_str,
        expected_terminal_state: scenario.expected_terminal_state.clone(),
        transition_count,
        wall_ms,
        transitions,
        error_count,
    }
}

/// Run all scenarios and return aggregate metrics.
pub fn run_benchmark(corpus: &[BenchmarkScenario]) -> BenchmarkReport {
    let results: Vec<ScenarioResult> = corpus.iter().map(run_scenario).collect();
    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = total - passed;
    let mean_transitions = if total == 0 {
        0.0
    } else {
        results
            .iter()
            .map(|r| r.transition_count as f64)
            .sum::<f64>()
            / total as f64
    };
    let mean_wall_ms = if total == 0 {
        0.0
    } else {
        results.iter().map(|r| r.wall_ms as f64).sum::<f64>() / total as f64
    };

    BenchmarkReport {
        total,
        passed,
        failed,
        mean_transitions,
        mean_wall_ms,
        scenarios: results,
    }
}

// ── regression gate ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RegressionPolicy {
    /// Minimum fraction of scenarios that must pass (0.0–1.0).
    pub min_pass_rate: f64,
    /// Maximum allowed mean transition count per scenario.
    pub max_mean_transitions: f64,
}

impl Default for RegressionPolicy {
    fn default() -> Self {
        Self {
            min_pass_rate: 1.0,
            max_mean_transitions: 20.0,
        }
    }
}

/// Assert that `report` meets `policy`; panics with diagnostics if not.
pub fn assert_benchmark_regression(report: &BenchmarkReport, policy: &RegressionPolicy) {
    let pass_rate = if report.total == 0 {
        1.0
    } else {
        report.passed as f64 / report.total as f64
    };

    if pass_rate < policy.min_pass_rate {
        let failed_names: Vec<&str> = report
            .scenarios
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.scenario.as_str())
            .collect();
        panic!(
            "benchmark regression: pass rate {:.1}% < required {:.1}%. Failed: {:?}",
            pass_rate * 100.0,
            policy.min_pass_rate * 100.0,
            failed_names,
        );
    }

    if report.mean_transitions > policy.max_mean_transitions {
        panic!(
            "benchmark regression: mean transitions {:.1} > allowed {:.1}",
            report.mean_transitions, policy.max_mean_transitions,
        );
    }
}
