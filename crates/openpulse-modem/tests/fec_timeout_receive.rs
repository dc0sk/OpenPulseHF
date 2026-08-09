//! `receive_with_fec_mode_timeout`: timeout-scanning reception of FEC-protected
//! frames (the path the CLI/loopback uses), validated through the channel sim.
use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::AwgnConfig;
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use pilot_plugin::PilotPlugin;
use scfdma_plugin::ScFdmaPlugin;

/// #1079/#1066: bound the receive search in WORK, not wall clock. These tests assert a DECODE
/// (`rx == payload`), never a latency; the millisecond argument only bounded how much searching a
/// given machine got to do before the verdict. With both budgets set, the outer deadline and the
/// retry budget are bypassed entirely (`engine.rs` — `deterministic_max_iterations` /
/// `deterministic_scan_positions`), so the work performed is identical on every host.
///
/// **Swept, and the sweep found something rather than a calibration: this file passes at EVERY
/// budget from 2/5 to 800/2000.** The positive cases decode at position ~0 because `route()` hands
/// the receiver a buffer that *is* the frame. Measured across all three files converted here, the
/// decode needs **2 outer iterations** — the deadline allowed however many the host could fit into
/// 4 s, which is far more than 2, so it was not binding on this host. #1079's Class A *shape* is
/// right; "one slow runner away" is not what the floor measurement shows for these files. The
/// conversion is still correct — it removes the host dependence in principle, and here it also
/// makes the file faster. What scales with the budget
/// is cost, via the one NEGATIVE case (`turbo_rejected_on_receive`), which must exhaust the budget
/// to conclude "did not decode": 0.83 s at 2/5, 1.73 s at 50/100, 31.97 s at 800/2000. So
/// over-provisioning is NOT free here — that inverts the "raising the ceiling costs nothing"
/// reasoning, which holds for a wall-clock deadline and not for a work budget.
///
/// 50/100 is ~25x the smallest budget the positive cases clear, and runs the file in 1.73 s —
/// faster than the 4 s deadline it replaces, and host-independent. If this file ever moves to
/// `route_embedded`, the frame stops being at position 0 and these values must be re-swept.
///
/// The negative case is non-vacuous *as a file*: it shares a run and a budget with seven positive
/// cases that do decode, which is the same-run baseline that makes "did not decode" mean something
/// other than "did not look". Sabotage-verified — driving `none_path_unchanged` below its decode
/// floor fails the file at this budget.
const SCAN_POSITIONS: usize = 50;
const MAX_ITERATIONS: usize = 100;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    h.rx_engine
        .set_deterministic_scan_positions(Some(SCAN_POSITIONS));
    h.rx_engine
        .set_deterministic_max_iterations(Some(MAX_ITERATIONS));
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(ScFdmaPlugin::new())).unwrap();
        eng.register_plugin(Box::new(PilotPlugin::new())).unwrap();
    }
    h
}

fn roundtrip(mode: &str, fec: FecMode, snr: f32, payload: &[u8]) -> bool {
    let mut h = harness();
    h.tx_engine
        .transmit_with_fec_mode(payload, mode, fec, None)
        .unwrap();
    let mut ch = AwgnChannel::new(AwgnConfig::new(snr, Some(7))).unwrap();
    h.route(&mut ch);
    matches!(
        h.rx_engine
            .receive_with_fec_mode_timeout(mode, fec, None, Duration::from_millis(4000)),
        Ok(rx) if rx == payload
    )
}

fn roundtrip_sro(mode: &str, fec: FecMode, ppm: f32, payload: &[u8]) -> bool {
    let mut h = harness();
    h.tx_engine
        .transmit_with_fec_mode(payload, mode, fec, None)
        .unwrap();
    h.route_with_sro(ppm);
    matches!(
        h.rx_engine
            .receive_with_fec_mode_timeout(mode, fec, None, Duration::from_millis(6000)),
        Ok(rx) if rx == payload
    )
}

#[test]
fn bpsk_rs_interleaved_timeout() {
    let payload = b"fec timeout receive: rs-interleaved over BPSK250";
    assert!(roundtrip("BPSK250", FecMode::RsInterleaved, 15.0, payload));
}

#[test]
fn scfdma_hom_soft_concatenated_timeout() {
    // The realistic HOM config: soft LLRs + RS+soft-Viterbi, decoded via the
    // timeout-scanning path. 18 dB is comfortably above its threshold.
    let payload: Vec<u8> = (0..64).map(|i| (i * 53 + 7) as u8).collect();
    assert!(roundtrip(
        "SCFDMA52-16QAM",
        FecMode::SoftConcatenated,
        18.0,
        &payload
    ));
}

#[test]
fn pilot_hom_soft_concatenated_timeout() {
    // The pilot dense rungs are structurally compatible with RS+soft-Viterbi:
    // the demod emits genuine LLRs that round-trip through the byte-exact
    // soft-concatenated path via the timeout scanner. (This documents that the
    // combination is valid in sim across AWGN and SRO; on the dual-clock
    // hardware cable the convolutional inner code loses resync and LDPC is the
    // recommended pilot soft FEC -- see docs/dev/dualcard-loopback.md.)
    let payload: Vec<u8> = (0..64).map(|i| (i * 37 + 11) as u8).collect();
    assert!(roundtrip(
        "PILOT-16QAM500",
        FecMode::SoftConcatenated,
        18.0,
        &payload
    ));
}

#[test]
fn pilot_hom_soft_concatenated_tolerates_sro() {
    // Pure sample-rate offset (the dual-clock effect) up to a realistic
    // two-soundcard 200 ppm: the pilot soft-concatenated path round-trips, so
    // the combination is not geometry-incompatible.
    let payload: Vec<u8> = (0..64).map(|i| (i * 53 + 7) as u8).collect();
    assert!(roundtrip_sro(
        "PILOT-8PSK500",
        FecMode::SoftConcatenated,
        200.0,
        &payload
    ));
}

#[test]
fn none_path_unchanged() {
    let payload = b"no-fec timeout path still works";
    assert!(roundtrip("BPSK250", FecMode::None, 20.0, payload));
}

#[test]
fn concatenated_timeout() {
    // Concatenated (Conv½ + RS, hard) now works through the scanning timeout path.
    let payload = b"fec timeout receive: concatenated over BPSK250";
    assert!(roundtrip("BPSK250", FecMode::Concatenated, 15.0, payload));
}

#[test]
fn rs_strong_timeout() {
    let payload = b"fec timeout receive: rs-strong over BPSK250";
    assert!(roundtrip("BPSK250", FecMode::RsStrong, 15.0, payload));
}

#[test]
fn turbo_timeout_does_not_decode() {
    // Turbo is a fixed-block code (QPP block = llrs.len()/3), so the scanning
    // receive can't feed it the exact LLR count — it's single-shot only. (The
    // prior bug was wasting a soft demodulation on it before failing; it is now
    // excluded from the soft set and rejected by the dispatch.)
    let payload = b"turbo single-shot only";
    assert!(!roundtrip("BPSK250", FecMode::Turbo, 20.0, payload));
}
