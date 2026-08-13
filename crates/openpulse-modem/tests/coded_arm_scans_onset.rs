//! The daemon's CODED decode arm must locate a frame that does not start at sample 0 (#1138).
//!
//! `ota_decode_and_ack_inner` used to make ONE attempt per candidate, at offset 0, on the whole
//! burst, while its uncoded sibling `decode_burst_inner` scanned onsets. The demodulator's timing
//! search spans a single symbol period (32 samples at BPSK250), so a frame a few thousand samples
//! into a burst was undecodable — and that is where real captures put it. On the on-air corpus the
//! coded arm lost every RS-coded frame the CLI path recovered (daemon 1/7 vs CLI 5/7); with the scan
//! it is 5/7, matching the CLI exactly. See `daemon_vs_cli_on_real_captures.rs`.
//!
//! This is the blind-sibling-path archetype: one arm fixed, its twin left behind. The two now share
//! `burst_onset_scan_bounds`, so the geometry cannot drift apart again — but a shared helper is not
//! a gate, because nothing stops a future edit dropping the scan loop from one caller. This is.
//!
//! Deliberately NOT a corpus replay: the corpus is BPSK250-specific and slow, and this must fail for
//! the mechanism (frame not at offset 0) rather than for anything about a particular recording.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;

const MODE: &str = "BPSK250";
const FEC: FecMode = FecMode::Rs;
const PAYLOAD: &[u8] = b"coded arm onset scan";
const SESSION: &str = "onset-scan";
/// Frame offset inside the burst. 4032 is the measured lead-in of the real #1021 capture — the case
/// this gate exists for — taken from that measurement rather than picked for convenience.
const LEAD_SAMPLES: usize = 4_032;

fn engine_with_ota() -> (LoopbackBackend, ModemEngine) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register bpsk");
    let profile = SessionProfile::hpx_hf();
    // The rung is SEARCHED, not transcribed: `rx_candidates` offers only the recommended and
    // confirmed levels, both the entry rung on a fresh session, so without locking the right rung
    // this test would never try BPSK250+Rs and would pass for the wrong reason.
    let level = (1u8..=20)
        .filter_map(SpeedLevel::from_u8)
        .find(|&l| profile.mode_for(l) == Some(MODE))
        .unwrap_or_else(|| panic!("hpx_hf has no rung running {MODE}"));
    e.start_ota_session(profile);
    e.ota_lock_level(level);
    (backend, e)
}

fn burst_with_frame_at(offset: usize) -> AudioSamples {
    let (backend, mut e) = engine_with_ota();
    e.transmit_with_fec_mode(PAYLOAD, MODE, FEC, None)
        .expect("transmit");
    let tx = backend.drain_samples();
    let mut samples = vec![0.0f32; offset];
    samples.extend_from_slice(&tx);
    AudioSamples { samples }
}

/// CONTROL: a frame at offset 0 decodes. If this fails the offset case below proves nothing — the
/// failure would be about the setup, not about the scan.
#[test]
fn the_coded_arm_decodes_a_frame_at_offset_zero() {
    let burst = burst_with_frame_at(0);
    let (_b, mut e) = engine_with_ota();
    let got = e
        .ota_decode_burst(&burst, SESSION, Some(MODE))
        .expect("decode call must not error");
    assert_eq!(
        got.payload.as_deref(),
        Some(PAYLOAD),
        "a coded frame at offset 0 must decode; without this control the offset test is meaningless"
    );
}

/// THE GATE: the same frame, offset into the burst as real captures deliver it.
#[test]
fn the_coded_arm_decodes_a_frame_that_does_not_start_at_offset_zero() {
    let burst = burst_with_frame_at(LEAD_SAMPLES);
    let (_b, mut e) = engine_with_ota();
    let got = e
        .ota_decode_burst(&burst, SESSION, Some(MODE))
        .expect("decode call must not error");
    assert_eq!(
        got.payload.as_deref(),
        Some(PAYLOAD),
        "the coded arm failed to decode a frame {LEAD_SAMPLES} samples into the burst (#1138). The \
         candidate loop must scan onsets, as `decode_burst_inner` does — the demod's timing search \
         spans one symbol period, so an un-scanned attempt at offset 0 cannot find it. This is the \
         defect that lost every RS-coded frame on the real on-air corpus while the uncoded sibling \
         recovered them."
    );

    // ORDERING, not just the decode. The scan runs AFTER the uncoded fallback, so a coded ladder
    // frame could in principle be claimed by the fallback first and returned as non-ladder traffic
    // — which carries no ACK and never reaches the rate controller. Asserting the ACK exists pins
    // the classification: this frame must come back as LADDER traffic, from the scan.
    assert!(
        got.ack.is_some(),
        "the offset frame decoded but was classified as non-ladder traffic (no ACK). The uncoded \
         fallback claimed it before the scan — ladder frames must keep first claim, and a frame \
         recovered without an ACK never keys the rate controller."
    );
}
