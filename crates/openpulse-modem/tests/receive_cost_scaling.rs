//! Diagnostic harness for the daemon receive cost (work plan M2, found 2026-10-01).
//!
//! Drives one coded frame through the daemon's production receive entry: `accumulate_capture` in
//! 400-sample reads, then `ota_decode_burst` on the flushed burst. Prints the time each stage takes
//! per rung, so the stage that scales with frame length is named by measurement, not by reading.
//! `#[ignore]`d: it is a probe, not a gate.
//!
//! Run on the station computer, release build:
//! `PROBE_ENTRY_RUNGS=1 cargo test --release -p openpulse-modem --no-default-features \
//!  --test receive_cost_scaling -- --ignored --nocapture`.
//! `PROBE_PN=1` runs the BPSK rungs on the #1062 PN-63 candidate (design row F1c).
//! `PROBE_NO_FALLBACK=1` removes the #1123 uncoded fallback scan, to show its share of the cost.
//! Measured 2026-10-02 (x86 container, release, fallback `BPSK250` as the daemon passes it): decode
//! 0.85 s (SL6) to 1.72 s (SL2) per frame; SL2 without the fallback scan 0.77 s.

use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;
use std::time::Instant;

/// Samples per `accumulate_capture` read. The daemon reads whatever buffered since its last tick, so
/// set `PROBE_READ` to the station's typical read (larger reads widen the onset scan via `onset_bound`).
fn read_size() -> usize {
    std::env::var("PROBE_READ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400)
}

/// The ladder under test: `fast`, or with `PROBE_PN` its BPSK rungs as the #1062 PN-63 candidate
/// (`-PN` modes, design row F1c). Same FEC and floors either way.
fn profile() -> SessionProfile {
    if std::env::var_os("PROBE_PN").is_none() {
        return SessionProfile::fast();
    }
    let fast = SessionProfile::fast();
    let rungs: Vec<_> = [
        (SpeedLevel::Sl2, "BPSK31-PN"),
        (SpeedLevel::Sl3, "BPSK63-PN"),
        (SpeedLevel::Sl4, "BPSK100-PN"),
        (SpeedLevel::Sl5, "BPSK250-PN"),
        (SpeedLevel::Sl6, "QPSK250-D"),
    ]
    .into_iter()
    .map(|(l, m)| (l, m, fast.fec_for(l), None, None))
    .collect();
    SessionProfile::from_rungs(&rungs, SpeedLevel::Sl2, 3)
}

fn engine(backend: &LoopbackBackend, level: SpeedLevel) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::pn_candidate()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e.start_ota_session(profile());
    e.ota_lock_level(level);
    e
}

fn probe(level: SpeedLevel, payload: usize) {
    let profile = profile();
    let mode = profile.mode_for(level).unwrap();
    let fec = profile.fec_for(level);

    let tx_bk = LoopbackBackend::new();
    let mut tx = engine(&tx_bk, level);
    let data: Vec<u8> = (0..payload).map(|i| i as u8).collect();
    tx.transmit_with_fec_mode(&data, mode, fec, None)
        .expect("transmit");
    let mut audio = tx_bk.drain_samples();
    let frame_len = audio.len();
    audio.extend(std::iter::repeat_n(0.0, 8 * read_size()));

    let rx_bk = LoopbackBackend::new();
    let mut rx = engine(&rx_bk, level);
    // Warm the noise floor on silence first, as a running daemon would be.
    for _ in 0..40 {
        let _ = rx.accumulate_capture(Some(mode), vec![0.0; read_size()]);
    }

    let t = Instant::now();
    let mut burst = None;
    let mut reads = 0;
    for chunk in audio.chunks(read_size()) {
        reads += 1;
        if let Some(b) = rx.accumulate_capture(Some(mode), chunk.to_vec()).unwrap() {
            burst = Some(b);
            break;
        }
    }
    let accumulate = t.elapsed();
    let Some(burst) = burst else {
        println!(
            "{level:?} {mode} {fec:?} payload {payload}: no burst flushed after {reads} reads"
        );
        return;
    };

    let t = Instant::now();
    // PROBE_NO_FALLBACK deletes the #1123 uncoded fallback scan, to see whether it is the cost.
    // The daemon passes the station's configured `[modem] mode` (default BPSK250), not the rung.
    let fallback = std::env::var_os("PROBE_NO_FALLBACK")
        .is_none()
        .then_some("BPSK250");
    let r = rx.ota_decode_burst(&burst, "probe", fallback).unwrap();
    let decode = t.elapsed();
    println!(
        "{level:?} {mode:<14} {fec:?} payload {payload:>3}: frame {frame_len:>7} samples ({:>5.1} s audio) \
         burst {:>7} | accumulate {:>8.1} ms ({reads} reads) | decode {:>9.1} ms | ok={}",
        frame_len as f64 / 8000.0,
        burst.samples.len(),
        accumulate.as_secs_f64() * 1e3,
        decode.as_secs_f64() * 1e3,
        r.payload.as_deref() == Some(&data[..]),
    );
}

#[test]
#[ignore = "diagnostic probe for the receive-cost item; prints timings"]
fn receive_cost_by_rung_and_length() {
    if std::env::var_os("PROBE_ENTRY_RUNGS").is_some() {
        for level in [
            SpeedLevel::Sl6,
            SpeedLevel::Sl5,
            SpeedLevel::Sl4,
            SpeedLevel::Sl3,
            SpeedLevel::Sl2,
        ] {
            probe(level, 24);
        }
        return;
    }
    for (level, payloads) in [
        (SpeedLevel::Sl7, &[24usize, 200][..]),
        (SpeedLevel::Sl6, &[24, 200][..]),
        (SpeedLevel::Sl5, &[24, 200][..]),
    ] {
        for &p in payloads {
            probe(level, p);
        }
    }
}
