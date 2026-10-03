//! #1456: idle flicker behind a narrow filter is not ladder evidence, and neither is an ACK; a frame
//! failing at the decode edge still is. Driven through `accumulate_capture` and `ota_decode_burst`,
//! the daemon receive tick's own path, over the recorded IC-9700 idle
//! (`design/reply-window-evidence.md`).

use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::ModemEngine;

/// The daemon's default read: `receive_tick_ms = 50` at 8 kHz.
const TICK: usize = 400;

fn engine(lb: &LoopbackBackend) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e
}

fn receiver_at_sl6() -> ModemEngine {
    let mut rx = engine(&LoopbackBackend::new());
    rx.start_ota_session(SessionProfile::fast());
    rx.ota_lock_level(SpeedLevel::Sl6);
    rx
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Feed `input` in daemon-sized reads; return the post-lead length of every flushed burst answered
/// with an ACK frame (a keyed NACK on air, for a failed burst).
fn answered(rx: &mut ModemEngine, input: &[f32]) -> Vec<usize> {
    let mut lens = Vec::new();
    for chunk in input.chunks(TICK) {
        if let Ok(Some(b)) = rx.accumulate_capture(Some("BPSK250"), chunk.to_vec()) {
            let post = b.samples.len() - rx.last_flush_lead();
            let r = rx
                .ota_decode_burst(&b, "peer", Some("BPSK250"))
                .expect("decode");
            if r.ack.is_some() {
                lens.push(post);
            }
        }
    }
    lens
}

/// `signal` placed 10 s into the recorded 250 Hz idle at `snr_db` against the idle's RMS.
fn over_idle(signal: &[f32], snr_db: f32) -> Vec<f32> {
    let idle = load_corpus("ic9700-idle-250hz.wav").expect("corpus");
    let g = rms(&idle.samples) * 10f32.powf(snr_db / 20.0) / rms(signal);
    let lead = 10 * 8000;
    let mut input = idle.cycled(0, lead + signal.len() + 10 * 8000);
    for (i, s) in signal.iter().enumerate() {
        input[lead + i] += g * s;
    }
    input
}

/// Measured before #1456: 107 evidence bursts in 30 min at SL6, every one 800–1200 samples after its
/// lead. Without the floor these two minutes key 11 NACKs (sabotage run, 2026-10-02).
#[test]
fn idle_behind_a_250hz_filter_is_not_evidence_at_sl6() {
    let idle = load_corpus("ic9700-idle-250hz.wav").expect("corpus");
    let mut rx = receiver_at_sl6();
    let mut input = Vec::new();
    let mut offset = 0;
    while input.len() < 2 * 60 * 8000 {
        input.extend(idle.cycled(offset, idle.samples.len()));
        offset += 7_919;
    }
    assert_eq!(
        answered(&mut rx, &input),
        Vec::<usize>::new(),
        "idle flicker keyed a NACK"
    );
    assert_eq!(rx.ota_rx_recommended_level(), Some(SpeedLevel::Sl6));
}

/// The other half of the falsifier: a real frame that fails must still draw a NACK. QPSK250-D does
/// not decode over this idle at 0 dB and is flushed as one 4.2 s piece.
#[test]
fn a_frame_failing_at_the_decode_edge_is_still_evidence() {
    let tx_lb = LoopbackBackend::new();
    let mut tx = engine(&tx_lb);
    tx.transmit_with_fec_mode(&[7u8; 32], "QPSK250-D", FecMode::Rs, None)
        .unwrap();
    let input = over_idle(&tx_lb.drain_samples(), 0.0);
    let mut rx = receiver_at_sl6();
    let lens = answered(&mut rx, &input);
    assert!(
        lens.iter().any(|&n| n >= 30_000),
        "the failed frame keyed no NACK; answered bursts {lens:?}"
    );
}

/// Another station's NACK on air is about 0.5 s, so it clears the evidence floor at every rung; it
/// must not be answered, or two idle OTA stations NACK each other. Recognised down to +9 dB over this
/// idle; below that the codeword does not decode and the burst counts like a failed frame.
#[test]
fn an_ack_on_air_is_not_evidence() {
    let tx_lb = LoopbackBackend::new();
    let mut tx = engine(&tx_lb);
    tx.transmit_ack_with_short_fec(&AckFrame::new(AckType::Nack, "OTHER"), None)
        .unwrap();
    let input = over_idle(&tx_lb.drain_samples(), 12.0);
    let mut rx = receiver_at_sl6();
    assert_eq!(
        answered(&mut rx, &input),
        Vec::<usize>::new(),
        "an ACK burst keyed a NACK"
    );
}
