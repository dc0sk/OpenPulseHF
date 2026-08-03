//! RESEARCH HARNESS — does a work-based budget reconcile #1058's debug/release verdict split?
//!
//! #1058 records the saturating-floor family passing in release and failing in debug, mechanism
//! unestablished. #1066 established that `receive_with_timeout_fec` is wall-clock bounded in two
//! places, so the number of scan positions examined depends on machine speed — and a debug build is
//! roughly 5x slower, which would make both budgets do proportionally less work.
//!
//! That predicts something specific and cheap to check: under a WORK-based budget the two profiles
//! should reach the same verdict, because the work is then identical by construction. If they do,
//! #1058's mechanism is the wall clock and it closes essentially for free. If they still disagree,
//! there is a second mechanism in the acquisition machinery and #1058 is a genuine blocker for any
//! change validated against these gates.
//!
//! Parameters are taken from the gate this reproduces
//! (`preamble_correlation_settle::the_receiver_never_settles_on_a_saturating_noise_floor`): same
//! corpus, same leads, same mode, same FEC. Run it in BOTH profiles and compare.

use bpsk_plugin::BpskPlugin;
use openpulse_core::fec::FecMode;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::channel_sim::ChannelSimHarness;
use std::time::Duration;

/// From the gate being reproduced.
const MODE: &str = "BPSK250";
const LEADS: [usize; 3] = [40_000, 80_000, 120_000];
const PAYLOAD: &[u8] = b"correlation gate probe";

#[test]
#[ignore = "verification"]
fn j1_work_budget_verdicts() {
    let hot = load_corpus("ic9700-idle-hot.wav").expect("corpus");
    assert!(
        hot.mean_sq() > 0.0032,
        "corpus floor no longer saturates the gate — the premise of this reproduction is gone"
    );

    let profile = if cfg!(debug_assertions) {
        "DEBUG"
    } else {
        "RELEASE"
    };
    println!("\nJ1 [{profile}]: saturating-floor reproduction under WORK-based budgets\n");
    println!(
        "{:<10} {:>10} {:>14} {:>12} {:>10}",
        "lead", "budgeted", "condemnations", "rho rej", "decoded"
    );

    for budgeted in [false, true] {
        for lead in LEADS {
            let mut h = ChannelSimHarness::new();
            for eng in [&mut h.tx_engine, &mut h.rx_engine] {
                eng.register_plugin(Box::new(BpskPlugin::new()))
                    .expect("reg");
            }
            if budgeted {
                h.rx_engine.set_deterministic_scan_positions(Some(3_000));
                h.rx_engine.set_deterministic_max_iterations(Some(4_000));
            }
            h.tx_engine
                .transmit_with_fec_mode(PAYLOAD, MODE, FecMode::Rs, None)
                .expect("tx");
            h.route_embedded_in_capture(&hot, lead, 40_000, 0.3);
            let decoded = h
                .rx_engine
                .receive_with_fec_mode_timeout(
                    MODE,
                    FecMode::Rs,
                    None,
                    Duration::from_millis(40_000),
                )
                .map(|v| String::from_utf8_lossy(&v) == String::from_utf8_lossy(PAYLOAD))
                .unwrap_or(false);
            println!(
                "{lead:<10} {:>10} {:>14} {:>12} {:>10}",
                if budgeted { "yes" } else { "WALL-CLOCK" },
                h.rx_engine.settle_condemnations(),
                h.rx_engine.rho_rejected_settles(),
                decoded
            );
        }
    }
    println!(
        "\n  Compare this table between profiles. If the 'budgeted' rows match across DEBUG and"
    );
    println!("  RELEASE while the wall-clock rows do not, #1058's mechanism is the wall clock.");
}
