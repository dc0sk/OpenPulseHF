use openpulse_modem::benchmark::{
    assert_benchmark_regression, run_benchmark, standard_corpus, RegressionPolicy,
};

/// Smoke-test: every scenario in the standard corpus must pass and the
/// aggregate metrics must satisfy the default regression policy.
#[test]
fn standard_corpus_passes_regression_gate() {
    let corpus = standard_corpus();
    let report = run_benchmark(&corpus);

    // Emit a structured summary so CI logs are readable.
    for s in &report.scenarios {
        println!(
            "[benchmark] {} → terminal={} expected={} transitions={} wall={}ms {}",
            s.scenario,
            s.terminal_state,
            s.expected_terminal_state,
            s.transition_count,
            s.wall_ms,
            if s.passed { "PASS" } else { "FAIL" },
        );
    }
    println!(
        "[benchmark] total={} passed={} failed={} mean_transitions={:.1} mean_wall={:.1}ms",
        report.total, report.passed, report.failed, report.mean_transitions, report.mean_wall_ms
    );

    assert_benchmark_regression(&report, &RegressionPolicy::default());
}

/// Each scenario must complete with zero state-machine errors (invalid
/// transitions).  Any error indicates a corpus/engine mismatch.
#[test]
fn standard_corpus_has_no_invalid_transitions() {
    let corpus = standard_corpus();
    let report = run_benchmark(&corpus);

    for s in &report.scenarios {
        assert_eq!(
            s.error_count, 0,
            "scenario '{}' had {} invalid transition(s)",
            s.scenario, s.error_count
        );
    }
}

/// The benchmark report must be serialisable to JSON without loss of data.
#[test]
fn benchmark_report_serialises_to_json() {
    let corpus = standard_corpus();
    let report = run_benchmark(&corpus);
    let json = serde_json::to_string_pretty(&report).expect("report must serialise to JSON");
    let reparsed: serde_json::Value =
        serde_json::from_str(&json).expect("serialised report must be valid JSON");

    assert_eq!(
        reparsed["total"].as_u64().expect("total field"),
        report.total as u64
    );
    assert_eq!(
        reparsed["passed"].as_u64().expect("passed field"),
        report.passed as u64
    );
    assert!(reparsed["scenarios"].is_array());
}
