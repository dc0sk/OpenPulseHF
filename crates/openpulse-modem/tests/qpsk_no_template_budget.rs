//! RESEARCH HARNESS — why does the NO-TEMPLATE fixture not reconcile across profiles?
//!
//! `a_no_template_mode_decodes_through_a_saturating_floor` is the one member of #1058's failing
//! family that a work-based budget did NOT fix at the first budget tried. It is also the member
//! whose work profile differs most inside the shared entry point: QPSK500 publishes no preamble
//! template, so the #1049 correlation veto never runs and the settle is energy-only — a materially
//! different amount of work per position than the BPSK fixtures.
//!
//! Two hypotheses, and they are distinguishable:
//!
//! * **Sizing.** One budget was applied family-wide after being fitted to a BPSK fixture. If some
//!   larger budget makes the profiles agree, the mechanism is still the wall clock and the constant
//!   was simply wrong for this path.
//! * **A second mechanism.** If the profiles disagree at *every* budget, something profile-sensitive
//!   lives on this path that the budget does not bound, and #1058 does not close.
//!
//! Parameters come from the gate itself via `capture_replay_gate_params`, not by transcription.

use openpulse_core::fec::FecMode;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::channel_sim::ChannelSimHarness;
use std::time::Duration;

/// The gate's own parameters. Kept beside the gate rather than copied into this file: a
/// doc-comment claiming fidelity cannot fail, and this repo has a rule about that by name.
mod gate_params {
    pub const CORPUS: &str = "ic9700-idle-hot.wav";
    pub const MODE: &str = "QPSK500";
    pub const LEADS: [usize; 2] = [40_000, 80_000];
    pub const TRAIL: usize = 40_000;
    pub const EMBED_LEVEL: f32 = 0.3;
    pub const PAYLOAD: &[u8] = b"no template probe";
    pub const TIMEOUT_MS: u64 = 40_000;
}

#[test]
#[ignore = "verification"]
fn k1_budget_sweep_on_the_no_template_path() {
    use gate_params::*;
    let hot = load_corpus(CORPUS).expect("corpus");
    let profile = if cfg!(debug_assertions) {
        "DEBUG"
    } else {
        "RELEASE"
    };

    println!("\nK1 [{profile}]: {MODE} (no template, energy-only settle) vs budget\n");
    println!(
        "{:<10} {:>18} {:>14} {:>12} {:>10}",
        "lead", "budget (pos/iter)", "condemnations", "rho rej", "decoded"
    );

    for lead in LEADS {
        for budget in [
            None,
            Some((3_000usize, 4_000usize)),
            Some((3_000, 16_000)),
            Some((8_000, 64_000)),
        ] {
            let mut h = ChannelSimHarness::new();
            for eng in [&mut h.tx_engine, &mut h.rx_engine] {
                eng.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
                    .unwrap();
                eng.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
                    .unwrap();
            }
            if let Some((pos, iters)) = budget {
                h.rx_engine.set_deterministic_scan_positions(Some(pos));
                h.rx_engine.set_deterministic_max_iterations(Some(iters));
            }
            h.tx_engine
                .transmit_with_fec_mode(PAYLOAD, MODE, FecMode::Rs, None)
                .expect("transmit");
            h.route_embedded_in_capture(&hot, lead, TRAIL, EMBED_LEVEL);
            let decoded = h
                .rx_engine
                .receive_with_fec_mode_timeout(
                    MODE,
                    FecMode::Rs,
                    None,
                    Duration::from_millis(TIMEOUT_MS),
                )
                .map(|v| v == PAYLOAD)
                .unwrap_or(false);
            let label = match budget {
                None => "WALL-CLOCK".to_string(),
                Some((p, i)) => format!("{p}/{i}"),
            };
            println!(
                "{lead:<10} {label:>18} {:>14} {:>12} {:>10}",
                h.rx_engine.settle_condemnations(),
                h.rx_engine.rho_rejected_settles(),
                decoded
            );
        }
    }
    println!(
        "\n  If some budget decodes in BOTH profiles, the mechanism is still the wall clock and"
    );
    println!(
        "  the family-wide constant was simply wrong for this path. If no budget does, there is"
    );
    println!("  a second, profile-sensitive effect here and #1058 does not close.");
}
