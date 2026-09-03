//! The ISS ACK listen must find the ACK *inside* a noisy capture, not only when the ACK is the whole
//! buffer (#1177).
//!
//! Every pre-existing ACK test hands `receive_ota_ack_within` a buffer that IS the ACK, so the
//! whole-buffer `decode_fsk4_ack` succeeds before the in-stream scan ever runs — the same
//! buffer-is-the-frame shape `route_embedded` exists to close on the data path. Under band noise the
//! production path is `decode_fsk4_ack_in_stream`, and it was refusing ~60 % of real ACKs at the ACK
//! channel's own operating point because its window gate thresholded on the whole buffer's PEAK.
//!
//! SNR here is the harness's full-band convention (`AwgnConfig`): FSK4-ACK occupies ~300 Hz of the
//! 4 kHz band, so 4 dB full-band is well above the waveform's in-band floor. It is the same
//! convention `plugins/fsk4/tests/fsk4_integration.rs` uses to call +4 dB the operating point.

use fsk4_plugin::Fsk4Plugin;
use mfsk16_plugin::Mfsk16Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::cfo::{CfoChannel, CfoConfig};
use openpulse_channel::ChannelModel;
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;

const SAMPLE_RATE: f32 = 8000.0;
/// Harness full-band SNR at which `fsk4_integration.rs` calls the ACK channel operational.
const OPERATING_SNR_DB: f32 = 4.0;
/// Seeds per cell. The gate demands every one: with ShortFec at the operating point a single miss is
/// a regression, not a tail.
const SEEDS: u64 = 40;
/// A quarter of the FSK4 tone spacing (100 Hz — `plugins/fsk4/src/lib.rs`). Measured: the decode knee
/// at this SNR is ~35 Hz, so a quarter-spacing offset sits a full cell inside it. The cliff is
/// deliberately NOT pinned; 35-45 Hz are marginal cells and a gate there would re-roll on any change
/// to the waveform.
const OFFSET_HZ: f32 = 25.0;

fn hf_engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine
        .register_plugin(Box::new(Mfsk16Plugin::new()))
        .unwrap();
    engine.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    engine.start_ota_session(SessionProfile::hpx_hf());
    (engine, backend)
}

/// A FSK4 ACK recommending a normal rung (so `transmit_ota_ack` takes the FSK4 branch, not the K=3
/// MFSK16 branch), rendered to audio.
fn ack_audio() -> (Vec<f32>, AckFrame) {
    let (mut irs, irs_bk) = hf_engine();
    let ack = AckFrame::new(AckType::AckDown, "in-noise").with_recommended_level(SpeedLevel::Sl2);
    irs.transmit_ota_ack(&ack, None).expect("transmit FSK4 ACK");
    (irs_bk.drain_samples(), ack)
}

/// Noise at a FIXED sigma set by the ACK's own RMS, so lead/tail length does not change the SNR.
/// (`AwgnChannel` normalises sigma to the whole input, which would make a longer capture a quieter
/// one and confound duration with SNR.) Deterministic per `seed`.
fn noisy_capture(lead: usize, tail: usize, snr_db: f32, seed: u64, offset_hz: f32) -> Vec<f32> {
    let (mut signal, _) = ack_audio();
    if offset_hz != 0.0 {
        let mut cfo = CfoChannel::new(CfoConfig::new(offset_hz, SAMPLE_RATE)).expect("cfo");
        signal = cfo.apply(&signal);
    }
    let n = signal.len();
    let rms = (signal.iter().map(|x| x * x).sum::<f32>() / n as f32).sqrt();
    let sigma = rms / 10f32.powf(snr_db / 20.0);

    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1) | 1;
    let mut uniform = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f32 / (1u64 << 53) as f32
    };
    (0..lead + n + tail)
        .map(|i| {
            let u1 = uniform().max(1e-9);
            let u2 = uniform();
            let g = (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos();
            let s = if i >= lead && i < lead + n {
                signal[i - lead]
            } else {
                0.0
            };
            s + sigma * g
        })
        .collect()
}

/// One listen over an already-captured buffer. Returns whether the ACK was recovered intact.
///
/// `timeout_ms` is short because it cannot change the verdict here: `receive_ota_ack_within` reads
/// before it checks its deadline, and an unpaced `LoopbackBackend` returns the whole capture in that
/// first read — so the outcome is fixed by the first decode attempt and only the *duration* of a
/// failure depends on the timeout. Verified identical at 1, 300 and 4000 ms.
fn listens(capture: &[f32]) -> bool {
    let (mut iss, iss_bk) = hf_engine();
    iss_bk.fill_samples(capture);
    iss.receive_ota_ack_within(None, 50, None)
        .map(|got| {
            got.recommended_level == Some(SpeedLevel::Sl2) && got.ack_type == AckType::AckDown
        })
        .unwrap_or(false)
}

/// The defect: an ACK surrounded by band noise, which is every real capture.
#[test]
fn the_ack_is_found_inside_a_noisy_capture() {
    let found = (0..SEEDS)
        .filter(|s| listens(&noisy_capture(4000, 4000, OPERATING_SNR_DB, 7000 + s, 0.0)))
        .count();
    assert_eq!(
        found as u64, SEEDS,
        "the ISS recovered {found}/{SEEDS} ACKs from a noisy capture at the ACK channel's operating \
         point; every existing ACK test passes only because the ACK is the whole buffer"
    );
}

/// The same, across a carrier offset — the FSK4-ACK-at-offset chain, which had no coverage at all.
#[test]
fn the_ack_is_found_at_a_carrier_offset_inside_a_noisy_capture() {
    let found = (0..SEEDS)
        .filter(|s| {
            listens(&noisy_capture(
                4000,
                4000,
                OPERATING_SNR_DB,
                7000 + s,
                OFFSET_HZ,
            ))
        })
        .count();
    assert_eq!(
        found as u64, SEEDS,
        "the ISS recovered {found}/{SEEDS} ACKs at {OFFSET_HZ} Hz — a quarter of the FSK4 tone \
         spacing, well inside the measured ~35 Hz knee"
    );
}

/// The scan now trial-decodes every window, so the degenerate ones must be refused by the code, not
/// by an energy gate. `silent_window_ack_rejection.rs` pins the mechanism; this pins the behaviour
/// through the production entry point.
#[test]
fn a_silent_capture_does_not_false_accept_an_ack() {
    assert!(
        !listens(&vec![0.0f32; 40_000]),
        "a silent capture must not yield an ACK: an all-zero window demodulates to all-zero bytes, \
         and only the wire whitening stops that being a valid ShortFec+CRC ACK frame"
    );
}
