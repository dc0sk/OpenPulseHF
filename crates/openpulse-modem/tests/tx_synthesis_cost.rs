//! Diagnostic harness for the key-to-audio gap (work plan M2, found 2026-10-02).
//!
//! Every keyed path asserts PTT and THEN calls the engine's transmit, which modulates, conditions and
//! opens the output before the first sample reaches the device. Whatever that takes is dead air on a
//! keyed rig. This times one `transmit_with_fec_mode` per `fast` rung, and the two ACK waveforms,
//! into the loopback backend, whose open and write cost nothing, so the number is synthesis alone.
//! A real device adds its own stream-open and buffer latency on top. `#[ignore]`d: a probe, not a
//! gate.
//!
//! Run on the station computer, release build:
//! `cargo test --release -p openpulse-modem --no-default-features --test tx_synthesis_cost -- --ignored --nocapture`.

use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;
use std::time::Instant;

fn engine(backend: &LoopbackBackend) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e
}

/// Median of `runs` timings of `f`, in ms, with the audio seconds it emitted.
fn time_it(bk: &LoopbackBackend, runs: usize, mut f: impl FnMut()) -> (f64, f64) {
    let mut ms = Vec::with_capacity(runs);
    let mut audio_s = 0.0;
    for _ in 0..runs {
        let t = Instant::now();
        f();
        ms.push(t.elapsed().as_secs_f64() * 1e3);
        audio_s = bk.drain_samples().len() as f64 / 8_000.0;
    }
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (ms[runs / 2], audio_s)
}

#[test]
#[ignore = "diagnostic probe for the key-to-audio gap; prints timings"]
fn tx_synthesis_by_rung() {
    let runs = 5;
    let profile = SessionProfile::fast();
    for n in 1u8..=14 {
        let Some(level) = SpeedLevel::from_u8(n) else {
            continue;
        };
        let Some(mode) = profile.mode_for(level) else {
            continue;
        };
        let fec = profile.fec_for(level);
        for payload in [24usize, 200] {
            let bk = LoopbackBackend::new();
            let mut e = engine(&bk);
            let data: Vec<u8> = (0..payload).map(|i| i as u8).collect();
            let (ms, audio_s) = time_it(&bk, runs, || {
                e.transmit_with_fec_mode(&data, mode, fec, None)
                    .expect("transmit");
            });
            println!(
                "{level:?} {mode:<16} {fec:?} payload {payload:>3}: synthesis {ms:>8.1} ms for {audio_s:>6.2} s of audio"
            );
        }
    }
    let ack = AckFrame::new(AckType::AckOk, "probe");
    let bk = LoopbackBackend::new();
    let mut e = engine(&bk);
    let (ms, audio_s) = time_it(&bk, runs, || {
        e.transmit_ack_with_short_fec(&ack, None).expect("fsk4 ack");
    });
    println!("FSK4 ACK: synthesis {ms:>8.1} ms for {audio_s:>6.2} s of audio");
    let (ms, audio_s) = time_it(&bk, runs, || {
        e.transmit_ack_mfsk16_k3(&ack, None).expect("mfsk16 ack");
    });
    println!("MFSK16 K=3 ACK: synthesis {ms:>8.1} ms for {audio_s:>6.2} s of audio");
}
