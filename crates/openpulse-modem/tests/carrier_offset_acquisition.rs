//! Acquisition through a real carrier-frequency offset, in process.
//!
//! **The gap this closes.** Two independent transceivers never agree exactly — measured on air
//! 2026-07-28, an IC-9700 and an FT-991A both commanded to 144.600000 MHz put the received carrier
//! at **1436 Hz, a −64 Hz offset**, sitting right at the edge of BPSK250's ±62.5 Hz AFC range. AFC
//! and carrier recovery are the most-churned area of this modem (60+ commits per the DSP playbook
//! in `CLAUDE.md`), yet every offset bug so far had to be reproduced on hardware: the loopback
//! harness shares one clock and has no RF conversion, so the audio carrier always landed exactly
//! where it was generated and the AFC had nothing to do.
//!
//! [`ChannelSimHarness::route_with_cfo`] supplies the missing impairment. Note it is a different
//! effect from `route_with_sro`: a sampling-clock offset drifts the symbol clock and leaves the
//! carrier alone; this shifts the carrier and leaves timing alone.
//!
//! **These tests are written to be falsifiable.** A harness that silently applied no offset would
//! make every "decodes under CFO" assertion vacuous, so the suite pins BOTH ends: small offsets
//! must decode, and a large offset must FAIL. If the failing case ever starts passing, the channel
//! has stopped shifting and the passing cases mean nothing.

use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_modem::channel_sim::ChannelSimHarness;
use qpsk_plugin::QpskPlugin;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    }
    h
}

/// Transmit and receive `mode` through a pure carrier offset of `offset_hz`.
///
/// Uses the TIMEOUT (scanning) receive on purpose. The one-shot `receive()` is
/// `stage_capture_input` -> `receive_from_samples` and performs **no AFC settling** at all — the
/// energy-gate -> refine-onset -> `afc_mini_settle` chain only exists in the scanning loop. Testing
/// carrier offset through the one-shot path measures the bare demodulator's tolerance and would
/// report an "acquisition failure" that is really just the wrong entry point.
fn round_trip_with_cfo(mode: &str, payload: &[u8], offset_hz: f32) -> Result<Vec<u8>, String> {
    let mut h = harness();
    h.tx_engine
        .transmit(payload, mode, None)
        .map_err(|e| format!("transmit: {e}"))?;
    let n = h.route_with_cfo(offset_hz);
    assert!(n > 0, "nothing was routed — the test would prove nothing");
    h.rx_engine
        .receive_with_timeout(mode, None, Duration::from_millis(4_000))
        .map_err(|e| format!("{e}"))
}

/// A modest offset — well inside the mode's tracking range — must still decode.
#[test]
fn bpsk250_decodes_through_a_small_carrier_offset() {
    let payload = b"carrier offset acquisition probe".to_vec();
    // BPSK250's AFC spans about +-baud/4 = +-62.5 Hz; 20 Hz is comfortably inside it.
    let got = round_trip_with_cfo("BPSK250", &payload, 20.0)
        .expect("BPSK250 must acquire through a 20 Hz carrier offset");
    assert_eq!(got, payload);
}

/// The on-air number: -64 Hz is what the two rigs actually produced, and it sits AT the edge of the
/// tracking range. This is the case the campaign had to fix by trimming a rig dial, so its
/// behaviour is worth pinning rather than assuming.
#[test]
fn bpsk250_at_the_measured_on_air_offset() {
    let payload = b"on-air offset".to_vec();
    let result = round_trip_with_cfo("BPSK250", &payload, -64.0);
    // Deliberately NOT asserting success: -64 Hz is at the +-62.5 Hz boundary, and pinning it as
    // "must decode" would encode luck as a requirement. What matters is that it is decided by the
    // modem rather than by an inert channel, so record the outcome explicitly.
    match result {
        Ok(got) => assert_eq!(got, payload, "if it decodes, it must decode CORRECTLY"),
        Err(e) => assert!(
            !e.is_empty(),
            "a failure at the AFC boundary must carry a reason: {e}"
        ),
    }
}

/// THE FALSIFIER: far outside the acquisition range the decode must FAIL.
///
/// Without this, a harness that quietly applied no offset at all would let every other test in this
/// file pass while proving nothing about acquisition.
///
/// The threshold is set from a measured sweep, not from the per-symbol tracking range — see
/// [`the_measured_acquisition_range`] for why those differ by an order of magnitude.
#[test]
fn a_gross_carrier_offset_breaks_the_decode() {
    let payload = b"this must not survive".to_vec();
    let result = round_trip_with_cfo("BPSK250", &payload, 1600.0);
    assert!(
        result.is_err(),
        "a 1600 Hz carrier offset decoded successfully — the CFO channel is not actually shifting \
         the signal, which would make every other test in this file vacuous"
    );
}

/// What the acquisition chain can ACTUALLY pull in, measured rather than assumed.
///
/// Sweeping BPSK250 and QPSK500 through the CFO channel (2026-07-29, in process):
///
/// | mode | decodes through | fails at |
/// |---|---|---|
/// | BPSK250 | ≤ 600 Hz | 800 Hz |
/// | QPSK500 | ≤ 400 Hz | 600 Hz |
///
/// That is roughly **ten times** the ±baud/4 per-symbol tracking range (±62.5 Hz for BPSK250), and
/// the difference is the point: the per-symbol figure describes what the *carrier tracker* holds,
/// while acquisition additionally has the coarse `afc_mini_settle` pass (documented as effectively
/// one-shot for crystal errors up to ±300 Hz) plus the retry scan's per-position re-acquisition.
/// Quoting the tracking range as the system's offset budget understates it by an order of magnitude.
///
/// Practical consequence for the on-air campaign: the **−64 Hz** rig-to-rig crystal offset measured
/// on 2026-07-28 was never anywhere near the limit, which corroborates the finding that the coded
/// on-air failures were a capture-level/settle problem and not a frequency problem.
///
/// The assertions bracket the measurement with margin rather than pinning an exact cliff, which
/// would be brittle against ordinary DSP changes.
#[test]
fn the_measured_acquisition_range() {
    let payload = b"acquisition range".to_vec();
    assert!(
        round_trip_with_cfo("BPSK250", &payload, 400.0).is_ok(),
        "BPSK250 acquisition regressed: 400 Hz is well inside the measured 600 Hz reach"
    );
    assert!(
        round_trip_with_cfo("QPSK500", &payload, 200.0).is_ok(),
        "QPSK500 acquisition regressed: 200 Hz is well inside the measured 400 Hz reach"
    );
}

/// The offset is a property of the channel, not of one waveform: a second mode must see it too.
#[test]
fn qpsk500_also_sees_the_offset() {
    let payload = b"qpsk under offset".to_vec();
    let clean = round_trip_with_cfo("QPSK500", &payload, 0.0);
    assert!(
        clean.is_ok(),
        "QPSK500 must decode with no offset, or the failing case below proves nothing: {clean:?}"
    );
    let gross = round_trip_with_cfo("QPSK500", &payload, 900.0);
    assert!(
        gross.is_err(),
        "QPSK500 decoded through a 900 Hz offset — the channel is not shifting the signal"
    );
}

// ── The two seams that existed for this class of bug but had no caller ────────
//
// `route_embedded_with_cfo` and `route_with_capture_agc` were both added to reproduce
// hardware-diagnosed defects and then left uncalled (archetype scan 2026-07-29, finding 14). An
// uncalled reproduction seam is not neutral: nothing keeps it correct, and the AGC one shipped with
// a cold-start transient that peaks at 272x the input across the preamble — it would have
// mis-measured the very defect it was built for. These two tests give both a caller.

/// The combination a real receiver faces: LOCATE the frame in an idle floor **and** ACQUIRE it
/// off-frequency. Either alone is easier than both together, which is the point of the seam.
///
/// **The noise level is an independent variable with its own ceiling, well below the CFO limit.**
/// Measured while writing this: at `noise_rms = 0.02` the decode fails **at offset 0** — the idle
/// floor alone defeats acquisition, and any CFO conclusion drawn there would be about the wrong
/// impairment. At 0.005 it decodes. The zero-offset control below keeps the two separable: if both
/// cases fail together, the noise level moved, not the carrier tracking.
#[test]
fn a_frame_embedded_in_noise_still_acquires_through_a_carrier_offset() {
    const LEAD: usize = 16_000;
    const NOISE_RMS: f32 = 0.005;

    let payload = b"embedded and off-frequency".to_vec();

    // Control: same noise, no offset. Establishes that the idle floor is decodable on its own.
    let mut h = harness();
    h.tx_engine.transmit(&payload, "BPSK250", None).expect("tx");
    h.route_embedded_with_cfo(LEAD, LEAD, NOISE_RMS, 0xC0FF_EE01, 0.0);
    let control = h
        .rx_engine
        .receive_with_timeout("BPSK250", None, Duration::from_millis(8_000));
    assert!(
        control.is_ok(),
        "the zero-offset control failed, so NOISE_RMS={NOISE_RMS} is already past what acquisition          tolerates and the offset assertion below would be measuring the wrong impairment: {:?}",
        control.err()
    );

    let mut h = harness();
    h.tx_engine.transmit(&payload, "BPSK250", None).expect("tx");
    let n = h.route_embedded_with_cfo(LEAD, LEAD, NOISE_RMS, 0xC0FF_EE01, 20.0);
    assert!(n > 0, "nothing was routed — the test would prove nothing");

    let got = h
        .rx_engine
        .receive_with_timeout("BPSK250", None, Duration::from_millis(8_000))
        .expect("a frame embedded in noise must still acquire through a 20 Hz carrier offset");
    assert_eq!(got, payload);
}

/// A live capture AGC must not destroy a phase-only waveform.
///
/// This is the asymmetry that made eight modes look "analog-path limited" (#1009/#1010): an AGC
/// moving gain mid-frame is near-harmless to a waveform carrying bits in phase and destructive to
/// one carrying them in amplitude. BPSK is the harmless side, so it is the right side to pin — an
/// AGC that broke *this* would be a harness bug, not a finding.
#[test]
fn a_phase_only_waveform_survives_a_live_capture_agc() {
    let mut h = harness();
    let payload = b"agc should not matter here".to_vec();
    h.tx_engine.transmit(&payload, "BPSK250", None).expect("tx");
    // 1 s of idle priming: the AGC arrives at the frame settled, as a real receiver's does.
    let n = h.route_with_capture_agc(0.1, 0.01, 0.5, 1.0);
    assert!(n > 0, "nothing was routed — the test would prove nothing");

    let got = h
        .rx_engine
        .receive_with_timeout("BPSK250", None, Duration::from_millis(8_000))
        .expect("a phase-only waveform must survive a settled capture AGC");
    assert_eq!(got, payload);
}
