//! A ladder frame behind noise that opened its burst is tried where the burst's energy rises, before
//! the onset walk (`docs/dev/design/ladder-scan-ranked-onsets.md`).
//!
//! On air (task 19, run a2d) an SL3 burst opened 0.7 s before its frame, and the walk from onset 0 in
//! 128-sample steps reached the frame at its 66th onset: 68 attempts, about 11 s on a Pi 4, past the
//! ACK deadline, so the frame was never answered and the ladder held at SL3 for 6.5 minutes.

use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;

fn engine(bk: &LoopbackBackend) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(bk.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e.start_ota_session(SessionProfile::robust());
    e.ota_lock_level(SpeedLevel::Sl3);
    e
}

/// Deterministic noise of RMS about `amp`.
fn noise(n: usize, amp: f32, state: &mut u64) -> Vec<f32> {
    (0..n)
        .map(|_| {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            ((*state >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * amp * 3.46
        })
        .collect()
}

/// Decodes an SL3 frame behind 0.7 s of noise of RMS `opener` (or `opener_of_frame` times the frame's
/// RMS) and returns the decode attempts it took.
fn attempts_behind_an_opener(opener: Option<f32>, opener_of_frame: f32) -> u64 {
    let bk = LoopbackBackend::new();
    let mut tx = engine(&bk);
    let mode = tx.ota_tx_mode().unwrap().to_owned();
    assert_eq!(mode, "BPSK63", "precondition: SL3 is BPSK63 in `robust`");
    let fec = tx.ota_tx_fec();
    tx.transmit_with_fec_mode(b"sl3 frame behind noise", &mode, fec, None)
        .unwrap();
    let frame = bk.drain_samples();
    let frame_rms = (frame.iter().map(|x| x * x).sum::<f32>() / frame.len() as f32).sqrt();
    let opener = opener.unwrap_or(opener_of_frame * frame_rms);

    let mut st = 0x9e37_79b9_7f4a_7c15_u64;
    let mut audio = noise(24_000, 0.002, &mut st); // a quiet band: the floor warms
    audio.extend(noise(5_600, opener, &mut st)); // 0.7 s of noise above the squelch: opens the burst
    let floor = noise(frame.len(), 0.002, &mut st);
    audio.extend(frame.iter().zip(&floor).map(|(s, n)| s + n));
    audio.extend(noise(16_000, 0.002, &mut st));

    let rx_bk = LoopbackBackend::new();
    let mut rx = engine(&rx_bk);
    let mut result = None;
    for chunk in audio.chunks(800) {
        if let Some(b) = rx
            .accumulate_capture(Some("BPSK250"), chunk.to_vec())
            .unwrap()
        {
            if b.samples.len() < frame.len() {
                let _ = rx.ota_decode_burst(&b, "DC0SK", None);
                continue;
            }
            let before = rx.decode_attempts();
            let res = rx.ota_decode_burst(&b, "DC0SK", None).unwrap();
            result = Some((res.payload, rx.decode_attempts() - before, b.samples.len()));
        }
    }
    let (payload, attempts, len) = result.expect("the frame flushed as a burst");
    println!(
        "opener RMS {opener:.3} (frame {frame_rms:.3}): burst {len} samples, {attempts} attempts"
    );
    assert_eq!(payload.as_deref(), Some(&b"sl3 frame behind noise"[..]));
    attempts
}

#[test]
fn an_sl3_frame_behind_noise_is_found_where_the_energy_rises() {
    let attempts = attempts_behind_an_opener(Some(0.03), 0.0);
    assert!(
        attempts <= RANKED_BUDGET,
        "{attempts} decode attempts on a frame behind 0.7 s of noise: the scan walked to it from \
         onset 0 instead of trying where the burst's energy rises (budget {RANKED_BUDGET})"
    );
}

/// The opener at a2d's level: its windows peaked at 0.43 of the burst's 80th-percentile window RMS
/// (the review's measurement of `rx-b-a2d`). This pins `ENERGY_RISE_FRACTION` from below: a fraction
/// that lets the opener's level count as the frame's puts the rise on the opener.
#[test]
fn an_sl3_frame_behind_a_loud_opener_is_found_where_the_energy_rises() {
    let attempts = attempts_behind_an_opener(None, 0.4);
    assert!(
        attempts <= RANKED_BUDGET,
        "{attempts} decode attempts on a frame behind an opener at 0.4 of its RMS: the energy rise \
         was placed on the opener (ENERGY_RISE_FRACTION too low?) (budget {RANKED_BUDGET})"
    );
}

/// Measured at the commit that introduced it: 2 attempts with the ranking, 66 with the energy-rise
/// ranking removed (sabotage run). 12 leaves room for upstream changes and still fails the walk.
const RANKED_BUDGET: u64 = 12;
