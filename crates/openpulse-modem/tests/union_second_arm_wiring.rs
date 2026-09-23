//! BPSK's second decision arm is reached, and wins frames, through the production entry (#1428).
//!
//! **What this asserts, and what it does not.** It asserts WIRING: that the non-primary arm is
//! reached and can produce a frame on a fade, and that it is never credited on a clean channel.
//! It does NOT assert that the union decodes more frames than variant 0 alone — the tripwire it
//! reads counts "a later arm produced this frame at this attempt", which is not "arm 0 could not
//! have decoded it" (measured 26 credits against 18 genuine rescues; see `alternate_arm_decodes`).
//!
//! The GAIN was measured against a variant-0-only build, paired on identical channel realisations:
//! +10/48 through `receive_with_fec_mode` and +18/96 through `ota_decode_burst` on `moderate_f1`
//! @ 8 dB (200 B, plain `Rs`), with zero frames lost in either. That comparison needs a second
//! build, so it lives in the traceability ledger, not in this file.
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_modem::engine::ModemEngine;

const MODE: &str = "BPSK250";

fn engine() -> (ModemEngine, LoopbackBackend) {
    let b = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(b.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register");
    (e, b)
}

fn payload() -> Vec<u8> {
    (0..200u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect()
}

/// A non-primary arm is reached and produces frames on a fade — counted, not assumed.
#[test]
fn a_non_primary_arm_produces_frames_on_a_fade() {
    let p = payload();
    let tx = {
        let (mut e, b) = engine();
        e.transmit_with_fec_mode(&p, MODE, FecMode::Rs, None)
            .expect("tx");
        b.drain_samples()
    };

    let (mut decoded, mut alt_total) = (0u32, 0u64);
    let seeds = 48u64;
    for seed in 0..seeds {
        let mut cfg = WattersonConfig::moderate_f1(Some(seed));
        cfg.snr_db = 8.0;
        let rx = WattersonChannel::new(cfg).expect("chan").apply(&tx);

        let (mut e, b) = engine();
        b.push_frame(&rx);
        if e.receive_with_fec_mode(MODE, FecMode::Rs, None)
            .is_ok_and(|d| d == p)
        {
            decoded += 1;
        }
        alt_total += e.alternate_arm_decodes();
    }

    println!(
        "moderate_f1 @ 8 dB: {decoded}/{seeds} decoded, {alt_total} produced by a non-primary arm"
    );

    assert!(
        decoded > 0,
        "nothing decoded at all — the fixture is not exercising the decode path"
    );
    assert!(
        alt_total > 0,
        "the union decoded {decoded}/{seeds} frames but NOT ONE came from a non-primary arm. \
         Either the second arm is never reached, or it never wins — and both are \
         indistinguishable from the union being unwired, which is what this tripwire exists to \
         tell apart."
    );
}

/// The tripwire stays at zero on a clean channel, where arm 0 wins every attempt — so a non-zero count above is
/// attributable to the fade, not to the counter incrementing on every decode.
#[test]
fn a_clean_channel_needs_no_second_arm() {
    let p = payload();
    let (mut e, b) = engine();
    e.transmit_with_fec_mode(&p, MODE, FecMode::Rs, None)
        .expect("tx");
    let tx = b.drain_samples();

    let (mut e, b) = engine();
    b.push_frame(&tx);
    let got = e
        .receive_with_fec_mode(MODE, FecMode::Rs, None)
        .expect("rx");
    assert_eq!(got, p, "the clean-channel control must decode");
    assert_eq!(
        e.alternate_arm_decodes(),
        0,
        "arm 0 decoded a clean frame, so no later arm should have been credited — a non-zero \
         count here would mean the tripwire fires on ordinary decodes and proves nothing"
    );
}
