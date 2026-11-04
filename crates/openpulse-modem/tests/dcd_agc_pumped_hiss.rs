//! The DCD separates frames again after the band's hiss steps up, with a receiver AGC that pumps.
//!
//! On air (task 21, run a2g) station B's audio between frames went from digital silence to steady
//! hiss (rms 0.021) while its AGC held the hiss 31 dB lower during each frame. The squelch, learned
//! on silence, sat below the hiss; nothing below the squelch was left to learn from, so the carrier
//! never dropped; the 38 s length cap then committed frames and gaps together, and the per-bin low
//! quantile took the AGC-suppressed hiss from the frame windows: a floor at half the gap hiss, a
//! squelch still below it, for good. No frame boundary, no NACK, no demotion, the sender gave up.

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
    e.ota_lock_level(SpeedLevel::Sl6);
    e
}

/// Band-limited hiss (a 300–2700 Hz FIR over white noise) at RMS `rms`.
fn hiss(n: usize, rms: f32, st: &mut u64) -> Vec<f32> {
    let white: Vec<f32> = (0..n + 64)
        .map(|_| {
            *st ^= *st << 13;
            *st ^= *st >> 7;
            *st ^= *st << 17;
            (*st >> 40) as f32 / (1u64 << 24) as f32 - 0.5
        })
        .collect();
    let taps: Vec<f32> = (0..64)
        .map(|k| {
            let t = k as f32 - 31.5;
            let lp = |fc: f32| 2.0 * fc / 8000.0 * sinc(2.0 * fc / 8000.0 * t);
            let w = 0.54 - 0.46 * (2.0 * std::f32::consts::PI * k as f32 / 63.0).cos();
            (lp(2700.0) - lp(300.0)) * w
        })
        .collect();
    let y: Vec<f32> = (0..n)
        .map(|i| taps.iter().enumerate().map(|(k, h)| h * white[i + k]).sum())
        .collect();
    let r = (y.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
    y.iter().map(|s| s * rms / r).collect()
}

fn sinc(x: f32) -> f32 {
    if x.abs() < 1e-6 {
        1.0
    } else {
        (std::f32::consts::PI * x).sin() / (std::f32::consts::PI * x)
    }
}

/// Frames decoded from one minute after the hiss steps up, with the AGC's hang `hang_s` seconds,
/// and how many bursts ran to the length cap after the step.
fn run(hang_s: f32) -> (usize, usize, usize) {
    let bk = LoopbackBackend::new();
    let mut tx = engine(&bk);
    let mode = tx.ota_tx_mode().unwrap().to_owned();
    let fec = tx.ota_tx_fec();
    tx.transmit_with_fec_mode(b"a frame in pumped hiss", &mode, fec, None)
        .unwrap();
    let frame: Vec<f32> = bk.drain_samples().iter().map(|s| s * 0.08).collect();

    let mut st = 0x9e37_79b9_7f4a_7c15_u64;
    // The on-air cadence (task 21, a2g): a frame every 13.8 s, frames ~30 % of the time.
    let period = 110_400usize;
    let gap = period.saturating_sub(frame.len());
    // A quiet, gated channel first (as B's was before the restore), with frames.
    let mut audio = Vec::new();
    for _ in 0..4 {
        audio.extend(
            hiss(frame.len(), 0.0001, &mut st)
                .iter()
                .zip(&frame)
                .map(|(n, s)| n + s),
        );
        audio.extend(hiss(gap, 0.0001, &mut st));
    }
    let step_at = audio.len();
    // Then the hiss steps up to 0.021 in the gaps. The AGC holds it 31 dB down under each frame and
    // releases it over the hang after it: down for `hang_s`, then back to full over the next second
    // (measured on the recording: the total RMS recovers in 0.58 s median, the band away from the
    // signal stays more than 5 dB down for about 1 s).
    let hang = (hang_s * 8000.0) as usize;
    for _ in 0..12 {
        audio.extend(
            hiss(frame.len(), 0.021 * 0.028, &mut st)
                .iter()
                .zip(&frame)
                .map(|(n, s)| n + s),
        );
        let h = hiss(gap, 0.021, &mut st);
        audio.extend(h.iter().enumerate().map(|(i, v)| {
            let g = if i < hang {
                0.028
            } else {
                (0.028 + (1.0 - 0.028) * (i - hang) as f32 / 8000.0).min(1.0)
            };
            v * g
        }));
    }

    let rx_bk = LoopbackBackend::new();
    let mut rx = engine(&rx_bk);
    let (mut capped_after, mut decoded_after_first_minute) = (0usize, 0usize);
    let mut pos = 0usize;
    for chunk in audio.chunks(800) {
        pos += chunk.len();
        if let Some(b) = rx
            .accumulate_capture(Some("BPSK250"), chunk.to_vec())
            .unwrap()
        {
            let after = pos > step_at;
            if after && b.samples.len() > 200_000 {
                capped_after += 1;
            }
            let ok = rx
                .ota_decode_burst(&b, "DC0SK", None)
                .is_ok_and(|r| r.payload.is_some());
            if ok && pos > step_at + 60 * 8000 {
                decoded_after_first_minute += 1;
            }
        }
    }
    println!(
        "hang {hang_s} s: {capped_after} capped bursts after the step; {decoded_after_first_minute} \
         frames decoded from the step + 60 s on (of {})",
        (audio.len() - step_at - 60 * 8000) / period
    );
    (
        decoded_after_first_minute,
        capped_after,
        (audio.len() - step_at - 60 * 8000) / period,
    )
}

#[test]
fn frames_separate_again_after_the_hiss_steps_up_under_a_pumping_agc() {
    // The hang measured on air: the band commit's lower edge must skip it.
    let (decoded, capped, of) = run(0.6);
    assert!(
        decoded + 2 >= of,
        "the DCD did not recover frame separation after the hiss stepped up under a pumping AGC: \
         {decoded} of {of} frames decoded from the step + 60 s on, {capped} bursts ran to the length \
         cap (on-air task 21, run a2g)"
    );
}

/// Where the band commit's lower edge stops covering the hang: a slow AGC (2.5 s, the FT-991A's slow
/// setting on HF, UNCHECKED on a recording) takes more than a tenth of a capped burst. A
/// measurement, recorded in `dcd-raised-floor.md`, not a gate.
#[test]
#[ignore = "a measurement of where the fix stops covering, not a gate"]
fn where_a_slow_agc_hang_defeats_the_band_commit() {
    for hang in [0.6f32, 1.0, 1.5, 2.0, 2.5] {
        let _ = run(hang);
    }
}
