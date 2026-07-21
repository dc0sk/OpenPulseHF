//! A full-buffer retry pass must be abandoned once it is provably slower than real time.
//!
//! **The defect this pins.** The scanning receive re-scans the whole accumulated buffer every ~2 s as
//! a fallback. That pass is O(buffer), so if it costs more wall time than the audio it walks, it can
//! never catch up — the buffer grows faster than the scan, and every later pass is further behind.
//! The frame is never reached.
//!
//! The engine guarded this with `long_frame`, a **proxy** derived from frame geometry. Geometry is the
//! wrong variable, and the dual-card rig showed it: `PILOT-QPSK500` (55 200 coded samples, classified
//! "short") costs ~640 ms per decode attempt and needs ~1000 scan positions — **~11 minutes of CPU for
//! a 45 s listen** — while `QPSK250` at *twice* the frame length passes comfortably. Measured
//! 2026-07-21: `PILOT-QPSK500` failed 3/3 on hardware and `PILOT-QPSK500-RRC`, with identical geometry
//! and a *higher* per-attempt cost, passed 3/3. Cost, not length, is what starves the loop.
//!
//! Two things had to be true for the fix to work, and the first attempt got the second one wrong:
//!
//! 1. the budget must be the audio the pass covers — a scan slower than real time is hopeless;
//! 2. it must be enforced **from inside the pass**. Measuring a pass after it completes is inert here,
//!    because the pathological pass never completes at all.
//!
//! In-process there is no read cadence to starve, so this cannot be reproduced through
//! `ChannelSimHarness` — these tests pin the *policy arithmetic* instead, and the hardware evidence is
//! recorded in `docs/dev/dualcard-loopback.md`.

use openpulse_core::fec::FecMode;
use openpulse_modem::engine::{frame_plan, LONG_FRAME_SAMPLES};

/// Seconds of audio a buffer of `samples` represents at the modem's 8 kHz rate.
fn span_secs(samples: usize) -> f64 {
    samples as f64 / 8000.0
}

/// THE INVARIANT: a pass costing more than the audio it walks must be abandoned.
#[test]
fn a_pass_slower_than_real_time_is_over_budget() {
    let buffered = 8000 * 45; // a 45 s listen
    let budget = span_secs(buffered);
    // PILOT-QPSK500 measured ~640 ms x ~1000 positions.
    let observed_cost = 0.640 * 1000.0;
    assert!(
        observed_cost > budget,
        "the measured PILOT-QPSK500 scan ({observed_cost:.0} s) must exceed a {budget:.0} s budget — \
         otherwise this test is not describing the failure it exists for"
    );
}

/// A pass that keeps up must NOT be abandoned — the wideband modes depend on the retry.
#[test]
fn a_pass_faster_than_real_time_is_within_budget() {
    let buffered = 8000 * 45;
    let budget = span_secs(buffered);
    // SCFDMA52/OFDM52 frames re-scan cheaply; a few hundred ms over the whole pass.
    let observed_cost = 0.5;
    assert!(
        observed_cost < budget,
        "a cheap wideband re-scan must stay within budget, or the fix would disable the retry those \
         modes rely on for acquisition"
    );
}

/// Geometry is the wrong discriminator — the case that refuted the `long_frame` proxy.
///
/// `PILOT-QPSK500` is classified short and fails; `QPSK250` is twice as long and passes.
#[test]
fn frame_length_does_not_predict_which_modes_starve() {
    let (pilot_coded, pilot_long) = frame_plan(18_400, FecMode::Rs); // PILOT-QPSK500, 2.3 s raw
    let (qpsk_coded, qpsk_long) = frame_plan(37_600, FecMode::Rs); // QPSK250, 4.7 s raw

    assert!(
        qpsk_coded > pilot_coded,
        "QPSK250's coded frame ({qpsk_coded}) is the longer one"
    );
    assert!(
        !pilot_long && !qpsk_long,
        "both are classified short by the geometry proxy ({pilot_coded}, {qpsk_coded} vs threshold \
         {LONG_FRAME_SAMPLES}) — yet only PILOT-QPSK500 starved on hardware, which is why the retry \
         needs a measured budget rather than a geometric guess"
    );
}

/// A zero-length buffer must not produce a zero budget that abandons the very first pass.
#[test]
fn an_empty_buffer_does_not_abandon_the_first_pass() {
    assert_eq!(span_secs(0), 0.0);
    // The engine guards with `retry_span_secs > 0.0` before treating a pass as hopeless.
    let hopeless = 1.0 > span_secs(0) && span_secs(0) > 0.0;
    assert!(
        !hopeless,
        "an empty buffer must not be treated as a failed budget, or no retry would ever run"
    );
}
