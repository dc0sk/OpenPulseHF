//! THE #1384 GATE: the OTA coded arm scans with slices sized for the CODED frame.
//!
//! Both OTA onset scans took `max_frame_samples` from `burst_onset_scan_bounds`, which returns the
//! plugin's **raw** geometry — sized for one RS block plus envelope. Offset 0 is exempt (the attempt
//! before the scan decodes the whole burst), so the defect only bites at a **non-zero** onset, which
//! is why it sat unseen behind a green suite.
//!
//! **MEASURED before it was fixed, because the issue was filed as a code read.** On BPSK250 the raw
//! geometry is **74 624** samples; a coded frame past the one-block boundary is **131 840**. Decode
//! of a frame at a non-zero onset, by payload size:
//!
//! | payload | RS input (4 + payload + 10) | blocks | frame samples | decoded before the fix |
//! |---|---|---|---|---|
//! | 200 B | 214 | 1 | 66 560 | yes |
//! | 205 B | 219 | 1 | 66 560 | yes |
//! | 210 B | 224 | **2** | **131 840** | **no** |
//! | 255 B | 269 | 2 | 131 840 | **no** |
//!
//! **The boundary is payload ≤ 209 B, not 213 B**, and getting that wrong is worth recording:
//! `FecCodec::encode` prepends a 4-byte big-endian length prefix (`PREFIX_LEN`) BEFORE blocking, so
//! the RS input is `4 + payload + WIRE_OVERHEAD(10)` and one block holds 223 of it. An earlier
//! statement of this boundary in #1310 PR1b omitted the prefix and said 213 B. The gates built on it
//! are unaffected — 200 B and 255 B sit on the correct sides of 209 either way — but the arithmetic
//! was wrong in the records and is corrected here and there.

use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;

const MODE: &str = "BPSK250";
const SESSION: &str = "gate-1384";
/// Lead-in silence, so the frame does NOT start at sample 0 — the exempt case.
const LEAD_IN: usize = 4_000;

fn rig() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(openpulse_audio::LoopbackBackend::new()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register bpsk");
    e.start_ota_session(SessionProfile::hpx_hf());
    e.ota_lock_level(SpeedLevel::Sl5); // rx_candidates() -> (BPSK250, Rs)
    e
}

fn coded_burst(len: usize) -> (Vec<u8>, AudioSamples) {
    let payload: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    let lb = openpulse_audio::LoopbackBackend::new();
    let mut tx = ModemEngine::new(Box::new(lb.clone_shared()));
    tx.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register bpsk");
    tx.transmit_with_fec_mode(&payload, MODE, FecMode::Rs, None)
        .expect("transmit coded");
    let mut s = lb.drain_samples();
    let mut buf = vec![0.0f32; LEAD_IN];
    buf.append(&mut s);
    (payload, AudioSamples { samples: buf })
}

/// The case the widening exists for: past the one-RS-block boundary, at a non-zero onset.
#[test]
fn a_two_block_coded_frame_is_reachable_at_a_non_zero_onset() {
    let (payload, burst) = coded_burst(255);
    let got = rig()
        .ota_decode_burst(&burst, SESSION, None)
        .expect("ota_decode_burst");
    assert_eq!(
        got.payload.as_deref(),
        Some(payload.as_slice()),
        "a two-block coded frame must decode once the OTA scan sizes its slice through frame_plan"
    );
}

/// The control. One RS block fits a RAW-sized slice, so this passes with or without the widening —
/// which is exactly why it cannot stand in for the test above, and why it is here: it proves a
/// failure of that test is about SIZING and not about coded OTA bursts in general.
#[test]
fn a_one_block_coded_frame_still_decodes() {
    let (payload, burst) = coded_burst(200);
    let got = rig()
        .ota_decode_burst(&burst, SESSION, None)
        .expect("ota_decode_burst");
    assert_eq!(got.payload.as_deref(), Some(payload.as_slice()));
}
