//! RESEARCH HARNESS — does a work-based budget reconcile #1058's debug/release verdict split?
//!
//! #1058 records the saturating-floor family passing in release and failing in debug, mechanism
//! unestablished. #1066 established that `receive_with_timeout_fec` is wall-clock bounded in two
//! places, so the number of scan positions examined depends on machine speed — and a debug build is
//! roughly 5x slower, which would make both budgets do proportionally less work.
//!
//! That predicts something specific and cheap to check: under a WORK-based budget the two profiles
//! should reach the same verdict, because the work is then identical by construction. If they do,
//! that is evidence for the wall clock being the mechanism ON THIS FIXTURE — not for the family.
//!
//! **RUN 2026-08-09 (3 repetitions per profile, tables byte-identical across all three).** The
//! prediction holds, and the split is fully deterministic rather than load-noise:
//!
//! | lead 120 000 | RELEASE | DEBUG |
//! |---|---|---|
//! | wall-clock | 5 condemnations, **decodes** | 0 condemnations, **fails** |
//! | work-budgeted | 5 condemnations, decodes | 5 condemnations, decodes |
//!
//! Leads 40 000 and 80 000 agree across profiles in both regimes (4/126 and 4/236, decoding). So on
//! this fixture the wall clock IS the mechanism behind #1058's debug/release split, and the work
//! budget removes it exactly — the budgeted rows match to the condemnation count.
//!
//! **One detail that does NOT fit the simple "a slower profile does proportionally less of
//! everything" story, recorded rather than explained away:** at the diverging row the rho-rejection
//! count is *identical* across profiles (361 both), while condemnations differ 5 vs 0. Less total
//! work would have moved both. Whatever produces condemnations is where the divergence is
//! concentrated, which is consistent with the third wall-clock site (`retry_due`, scheduled on
//! elapsed SECONDS) being the operative clock rather than the outer deadline — but that is a
//! hypothesis this table does not establish.
//!
//! Scope is unchanged by the run: three leads of ONE fixture. The paragraph below still governs —
//! a budget fitted here does not transfer to the no-template path, which needs roughly four times
//! this.
//! The five failing fixtures diverge inside the shared entry point (one is QPSK500 with no veto at
//! all, so its work per position differs), and a budget fitted to one of them is a constant fitted
//! to one fixture. Measured: a 4 000-iteration budget reconciles the BPSK fixtures and is far too
//! small for the no-template path, which needs ~260 condemnations where these need four or five.
//!
//! Parameters are taken from the gate this reproduces
//! (`preamble_correlation_settle::the_receiver_never_settles_on_a_saturating_noise_floor`): same
//! corpus, same leads, same mode, same FEC. Run it in BOTH profiles and compare.

use bpsk_plugin::BpskPlugin;
use openpulse_core::fec::FecMode;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::channel_sim::ChannelSimHarness;
use std::time::Duration;

mod common;
/// The gate's own parameters, by reference. Transcribing them under a doc-comment claiming fidelity
/// is the construct `CLAUDE.md` bans by name: a comment cannot fail, and a reproduction that drifts
/// then measures something else while still claiming to be the gate. Sharing the source makes the
/// claim compiler-checked — this file stops building if the gate's fixture changes.
use common::saturating_floor as fixture;

#[test]
#[ignore = "verification"]
fn j1_work_budget_verdicts() {
    let hot = load_corpus(fixture::CORPUS).expect("corpus");
    assert!(
        hot.mean_sq() > fixture::GATE_CEILING_MEAN_SQ,
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
        for lead in fixture::LEADS {
            let mut h = ChannelSimHarness::new();
            for eng in [&mut h.tx_engine, &mut h.rx_engine] {
                eng.register_plugin(Box::new(BpskPlugin::new()))
                    .expect("reg");
            }
            if budgeted {
                // Sized to reproduce the RELEASE wall-clock work level on THIS fixture (4-5
                // condemnations), not chosen for roundness — and deliberately not reused
                // family-wide: the no-template path needs roughly four times this.
                h.rx_engine.set_deterministic_scan_positions(Some(3_000));
                h.rx_engine.set_deterministic_max_iterations(Some(4_000));
            }
            h.tx_engine
                .transmit_with_fec_mode(fixture::PAYLOAD, fixture::MODE, FecMode::Rs, None)
                .expect("tx");
            h.route_embedded_in_capture(&hot, lead, fixture::TRAIL, fixture::EMBED_LEVEL);
            let decoded = h
                .rx_engine
                .receive_with_fec_mode_timeout(
                    fixture::MODE,
                    FecMode::Rs,
                    None,
                    Duration::from_millis(fixture::TIMEOUT_MS),
                )
                .map(|v| v == fixture::PAYLOAD)
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
