//! Small frames get free `RsStrong` on the weak rungs, and the OTA receiver decodes them — #934 follow-up.
//!
//! The weak `hpx_hf` rungs are RS-coded (t=16). `RsStrong` (t=32) roughly doubles their fading decode
//! and is *free on the wire* for small frames — same 255-byte RS block. But the profile FEC is a
//! per-level constant with no per-frame signalling, so the sender strengthens opportunistically
//! (`free_rs_strengthening`) and the receiver must try both codes. This proves the receive half: an
//! `RsStrong`-sent frame decodes through the OTA path whose rung is `Rs`. Without the candidate
//! expansion in `ota_decode_and_ack_inner`, the receiver only tries `Rs` and this fails.

use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::fec::{free_rs_strengthening, FecMode};
use openpulse_core::frame::Frame;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::pipeline::AudioSamples;
use openpulse_modem::ModemEngine;

const SESSION: &str = "rs-strong-sess";
const MODE: &str = "BPSK250"; // hpx_hf SL5, Rs-coded, and fast enough for a clean round-trip.

fn engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register");
    e.start_ota_session(SessionProfile::hpx_hf());
    e.ota_lock_level(SpeedLevel::Sl5); // rx_candidates() → (BPSK250, Rs)
    (e, backend)
}

/// The sender's choice: a small frame is strengthened to RsStrong, a 200-byte frame stays Rs (the
/// size that would need a second block — the v0.14.0 goodput regression).
#[test]
fn sender_strengthens_small_frames_only() {
    let small = 32usize;
    let big = 200usize;
    assert_eq!(
        free_rs_strengthening(FecMode::Rs, small + Frame::WIRE_OVERHEAD),
        FecMode::RsStrong,
        "a small frame must be strengthened"
    );
    assert_eq!(
        free_rs_strengthening(FecMode::Rs, big + Frame::WIRE_OVERHEAD),
        FecMode::Rs,
        "a 200-byte frame must stay Rs (RsStrong would need a 2nd block)"
    );
}

/// The receiver's half, where it actually matters — on a fade. A frame the sender strengthened to
/// RsStrong decodes through the OTA receive path (rung FEC = Rs) far more often than the rung's own
/// t=16 code could, because the candidate expansion also tries t=32.
///
/// On a clean channel this can't be tested: the data is intact, so even a mismatched-code decode
/// recovers it by passthrough. The dual-decode only bites where there are burst errors to correct —
/// which is the whole reason RsStrong is worth using. Measured on `moderate_f1` @6 dB: an
/// RsStrong-sent frame decodes ~0.7 as RsStrong vs ~0.2 as Rs; the OTA path (trying both) must reach
/// the RsStrong rate, well above what the rung's Rs alone gives.
#[test]
fn receiver_decodes_rsstrong_on_a_fade_that_rs_cannot() {
    let payload = b"a short control frame".to_vec();
    assert_eq!(
        free_rs_strengthening(FecMode::Rs, payload.len() + Frame::WIRE_OVERHEAD),
        FecMode::RsStrong,
        "the sender would strengthen this frame"
    );
    const FRAMES: u32 = 16;
    let mut ota_ok = 0u32;
    for seed in 0..FRAMES {
        let (mut tx, tx_backend) = engine();
        tx.transmit_with_fec_mode(&payload, MODE, FecMode::RsStrong, None)
            .expect("transmit");
        let clean = tx_backend.drain_samples();
        let mut ch = WattersonChannel::new({
            let mut cfg = WattersonConfig::moderate_f1(Some(seed as u64));
            cfg.snr_db = 6.0;
            cfg
        })
        .expect("channel");
        let faded = AudioSamples {
            samples: ch.apply(&clean),
        };
        // The production OTA path: rung FEC = Rs, plus the RsStrong candidate the fix adds. With only
        // Rs tried (pre-fix) this decodes ~0.12; with the expansion, ~0.7.
        let (mut rx, _) = engine();
        if rx
            .ota_decode_burst(&faded, SESSION)
            .ok()
            .and_then(|r| r.payload)
            .as_deref()
            == Some(payload.as_slice())
        {
            ota_ok += 1;
        }
    }
    assert!(
        ota_ok * 2 >= FRAMES,
        "the OTA path must decode the RsStrong-strengthened frame on a moderate_f1 fade via the \
         candidate expansion: only {ota_ok}/{FRAMES}. The rung's Rs code alone gets ~2/16 here, so \
         this bar fails without the RsStrong candidate (that is the point of the fix)."
    );
}

/// Backward compatibility: an ordinary Rs frame still decodes (the expansion adds candidates, never
/// removes the profile's own).
#[test]
fn receiver_still_decodes_a_plain_rs_frame() {
    let payload = b"plain rs frame".to_vec();
    let (mut tx, tx_backend) = engine();
    tx.transmit_with_fec_mode(&payload, MODE, FecMode::Rs, None)
        .expect("transmit");
    let burst = AudioSamples {
        samples: tx_backend.drain_samples(),
    };
    let (mut rx, _) = engine();
    let out = rx.ota_decode_burst(&burst, SESSION).expect("decode");
    assert_eq!(out.payload.as_deref(), Some(payload.as_slice()));
}
