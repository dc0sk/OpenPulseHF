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

/// Feed `input` in daemon-sized reads; return `(post-lead length, answered with an ACK frame)` for
/// every flushed burst, so a test can show which case it exercised as well as its verdict.
fn flushed(rx: &mut ModemEngine, input: &[f32]) -> Vec<(usize, bool)> {
    let mut out = Vec::new();
    for chunk in input.chunks(TICK) {
        if let Ok(Some(b)) = rx.accumulate_capture(Some("BPSK250"), chunk.to_vec()) {
            let post = b.samples.len() - rx.last_flush_lead();
            let r = rx
                .ota_decode_burst(&b, "peer", Some("BPSK250"))
                .expect("decode");
            out.push((post, r.ack.is_some()));
        }
    }
    out
}

fn union_nack() -> Vec<f32> {
    let tx_lb = LoopbackBackend::new();
    let mut tx = engine(&tx_lb);
    tx.transmit_ack_mfsk16_k3(&AckFrame::new(AckType::Nack, "OTHER"), None)
        .unwrap();
    tx_lb.drain_samples()
}

/// A station at SL1 NACKs with the K=3 union ACK: one FSK4 ACK, then three MFSK16-ACK copies after
/// 0.5 s gaps, 5.86 s in all. The DCD's 100 ms hold drops in the gaps, so it is heard in pieces, and
/// a lone MFSK16 copy carries no FSK4 codeword. On air (task 32) the receiver stepped SL9 → SL8 on
/// such a NACK (`design/union-ack-recognition.md`).
#[test]
fn a_union_ack_heard_in_pieces_is_not_evidence() {
    let mut rx = receiver_at_sl6();
    let bursts = flushed(&mut rx, &over_idle(&union_nack(), 12.0));
    assert!(
        bursts.len() >= 2,
        "the fixture did not split the union ACK; bursts {bursts:?}"
    );
    assert!(
        bursts.iter().all(|&(_, answered)| !answered),
        "a piece of a union ACK keyed a NACK; bursts {bursts:?}"
    );
    assert_eq!(rx.ota_rx_recommended_level(), Some(SpeedLevel::Sl6));
}

/// The same NACK heard as ONE burst, as under a receiver AGC whose pumped hiss holds the squelch
/// through the gaps: the gaps are filled with hiss well above the idle. It was past the old
/// 2 × FSK4 bound, so its FSK4 copy was never looked for.
#[test]
fn a_union_ack_heard_whole_is_not_evidence() {
    let ack = union_nack();
    let mut input = over_idle(&ack, 12.0);
    let idle_rms = rms(&load_corpus("ic9700-idle-250hz.wav")
        .expect("corpus")
        .samples);
    let mut seed: u32 = 0x1234_5678;
    for s in &mut input[10 * 8000..10 * 8000 + ack.len()] {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *s += idle_rms * 3.0 * ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 3.46;
    }
    let mut rx = receiver_at_sl6();
    let bursts = flushed(&mut rx, &input);
    assert!(
        bursts.iter().any(|&(post, _)| post >= ack.len() * 9 / 10),
        "the fixture did not hold the union ACK as one burst; bursts {bursts:?}"
    );
    assert!(
        bursts.iter().all(|&(_, answered)| !answered),
        "a whole union ACK keyed a NACK; bursts {bursts:?}"
    );
}

/// Every ACK `transmit_ota_ack` can key is recognised as an ACK, so the guard's waveform set cannot
/// drift from the transmit's. The waveform depends only on whether the recommended level's mode is
/// MFSK16, so the profile's closed level list is swept one level per waveform.
#[test]
fn every_ack_the_transmit_keys_is_not_evidence() {
    let profile = SessionProfile::fast();
    let mut waveforms = Vec::new();
    for level in profile.defined_levels() {
        let union = profile.mode_for(level) == Some("MFSK16");
        if waveforms.contains(&union) {
            continue;
        }
        waveforms.push(union);
        let tx_lb = LoopbackBackend::new();
        let mut tx = engine(&tx_lb);
        tx.start_ota_session(profile.clone());
        tx.transmit_ota_ack(
            &AckFrame::new(AckType::Nack, "OTHER").with_recommended_level(level),
            None,
        )
        .unwrap();
        let mut rx = receiver_at_sl6();
        let bursts = flushed(&mut rx, &over_idle(&tx_lb.drain_samples(), 12.0));
        assert!(
            bursts.iter().all(|&(_, answered)| !answered),
            "the ACK recommending {level:?} keyed a NACK; bursts {bursts:?}"
        );
    }
    assert_eq!(
        waveforms.len(),
        2,
        "the profile no longer keys both ACK waveforms"
    );
}

/// `samples` of listening time with nothing heard: digital silence, which is cheap, then 60 s of the
/// recorded idle so the noise floor learns the band again (after silence it would hold the hiss as a
/// burst). Bursts the idle flushes are dropped undecoded.
fn listen_quietly(rx: &mut ModemEngine, samples: u64) {
    let warm = 60 * 8_000;
    let mut left = samples.saturating_sub(warm as u64);
    while left > 0 {
        let n = left.min(8_000) as usize;
        assert!(
            matches!(
                rx.accumulate_capture(Some("BPSK250"), vec![0.0; n]),
                Ok(None)
            ),
            "silence flushed a burst"
        );
        left -= n as u64;
    }
    let idle = load_corpus("ic9700-idle-250hz.wav").expect("corpus");
    for chunk in idle.cycled(0, warm).chunks(TICK) {
        let _ = rx.accumulate_capture(Some("BPSK250"), chunk.to_vec());
    }
}

fn failing_qpsk_frame() -> Vec<f32> {
    let tx_lb = LoopbackBackend::new();
    let mut tx = engine(&tx_lb);
    tx.transmit_with_fec_mode(&[7u8; 32], "QPSK250-D", FecMode::Rs, None)
        .unwrap();
    over_idle(&tx_lb.drain_samples(), 0.0)
}

/// On air (task 32) two idle daemons NACKed their own floor, then each other's NACKs and IDs heard too
/// weak to recognise. With no ladder frame decoded in `OTA_PEER_LIVE_SAMPLES` of listening, a failure
/// keys nothing and moves no ladder: the frame that keys a NACK in a live session
/// (`a_frame_failing_at_the_decode_edge_is_still_evidence`), and the other station's FSK4 NACK with its
/// ID behind it at +1.5 dB (`idle-nack-gate.md`, G1).
#[test]
fn with_no_live_peer_a_failure_keys_nothing() {
    let mut rx = receiver_at_sl6();
    listen_quietly(&mut rx, openpulse_modem::OTA_PEER_LIVE_SAMPLES + 8_000);
    let bursts = flushed(&mut rx, &failing_qpsk_frame());
    assert!(
        bursts.iter().any(|&(post, _)| post >= 30_000),
        "the failing frame was never flushed, so this proves nothing; bursts {bursts:?}"
    );
    assert!(
        bursts.iter().all(|&(_, answered)| !answered),
        "a failure with no live peer keyed a NACK; bursts {bursts:?}"
    );

    let tx_lb = LoopbackBackend::new();
    let mut tx = engine(&tx_lb);
    tx.transmit_ack_with_short_fec(&AckFrame::new(AckType::Nack, "OTHER"), None)
        .unwrap();
    tx.transmit(b"DE DD2ZM", "BPSK250", None).unwrap();
    let bursts = flushed(&mut rx, &over_idle(&tx_lb.drain_samples(), 1.5));
    assert!(!bursts.is_empty(), "the weak NACK + ID was never flushed");
    assert!(
        bursts.iter().all(|&(_, answered)| !answered),
        "a weak NACK + ID keyed a NACK; bursts {bursts:?}"
    );
    assert_eq!(rx.ota_rx_recommended_level(), Some(SpeedLevel::Sl6));
}

/// The control: a decoded ladder frame makes the peer live again, and the same failure then keys.
#[test]
fn a_decode_makes_the_peer_live_again() {
    let mut rx = receiver_at_sl6();
    listen_quietly(&mut rx, openpulse_modem::OTA_PEER_LIVE_SAMPLES + 8_000);
    let tx_lb = LoopbackBackend::new();
    let mut tx = engine(&tx_lb);
    tx.transmit_with_fec_mode(&[9u8; 32], "QPSK250-D", FecMode::Rs, None)
        .unwrap();
    let bursts = flushed(&mut rx, &over_idle(&tx_lb.drain_samples(), 20.0));
    assert!(
        bursts.iter().any(|&(_, answered)| answered),
        "the clean frame did not decode; bursts {bursts:?}"
    );
    let lens = answered(&mut rx, &failing_qpsk_frame());
    assert!(
        lens.iter().any(|&n| n >= 30_000),
        "after a decode the failure keyed no NACK; answered bursts {lens:?}"
    );
}
