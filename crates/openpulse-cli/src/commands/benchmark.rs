use anyhow::Result;
use openpulse_modem::benchmark::{
    assert_benchmark_regression, run_benchmark, standard_corpus, RegressionPolicy,
};

use crate::BenchmarkCommands;

pub fn run(command: BenchmarkCommands) -> Result<i32> {
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
