//! #1456 review round 2, finding 2: into how many pieces, and how long, does `accumulate_capture` flush a
//! BPSK31 + Rs frame at the edge of decoding over the recorded 250 Hz idle? A length floor on failed
//! bursts must not silence those pieces.

use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::ModemEngine;

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

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[test]
#[ignore = "measurement (#1456 review finding 2); run with --release --ignored --nocapture"]
fn marginal_bpsk31_frame_pieces_over_250hz_idle() {
    let idle = load_corpus("ic9700-idle-250hz.wav").expect("corpus");
    let tx_lb = LoopbackBackend::new();
    let mut tx = engine(&tx_lb);
    let data: Vec<u8> = (0..32u8).collect();
    let mode = std::env::var("PIECE_MODE").unwrap_or_else(|_| "BPSK31".into());
    let level = match mode.as_str() {
        "QPSK250-D" => SpeedLevel::Sl6,
        "BPSK250" => SpeedLevel::Sl5,
        _ => SpeedLevel::Sl2,
    };
    tx.transmit_with_fec_mode(&data, &mode, FecMode::Rs, None)
        .unwrap();
    let frame = tx_lb.drain_samples();
    let noise_rms = rms(&idle.samples);
    let gains: Vec<f32> = std::env::var("PIECE_SNR_DB")
        .ok()
        .map(|v| v.split(',').filter_map(|s| s.parse().ok()).collect())
        .unwrap_or_else(|| vec![6.0, 3.0, 0.0, -3.0, -6.0]);
    for (k, snr_db) in gains.iter().enumerate() {
        let g = noise_rms * 10f32.powf(snr_db / 20.0) / rms(&frame);
        let lead = 20 * 8000;
        let mut input = idle.cycled(k * 7_919, lead + frame.len() + 20 * 8000);
        for (i, s) in frame.iter().enumerate() {
            input[lead + i] += g * s;
        }
        let rx_lb = LoopbackBackend::new();
        let mut rx = engine(&rx_lb);
        rx.start_ota_session(SessionProfile::fast());
        rx.ota_lock_level(level);
        let (mut pieces, mut decoded) = (Vec::new(), false);
        let mut fed = 0usize;
        for chunk in input.chunks(TICK) {
            fed += chunk.len();
            if let Ok(Some(b)) = rx.accumulate_capture(Some("BPSK250"), chunk.to_vec()) {
                let post = b.samples.len() - rx.last_flush_lead();
                let end = fed;
                let r = rx
                    .ota_decode_burst(&b, "peer", Some("BPSK250"))
                    .expect("decode");
                decoded |= r.payload.as_deref() == Some(&data[..]);
                // Only pieces that overlap the frame's span.
                if end > lead && end - post < lead + frame.len() + 8000 {
                    pieces.push((post, r.ack.is_some()));
                }
            }
        }
        println!(
            "{mode} SNR {snr_db:+.0} dB (in-band ratio to the idle's RMS): decoded {decoded}; pieces over the frame (post-lead samples, counted as evidence) {pieces:?}"
        );
    }
}
