//! How often does idle behind a 250 Hz filter produce a burst that today counts as ladder evidence?
//!
//! The reply-window redesign (#1456/#1460) rests on this rate; its review asked for it measured
//! rather than estimated. Feeds the recorded IC-9700 250 Hz idle, cycled from staggered offsets,
//! through `accumulate_capture` with a `fast` session, and counts flushed bursts that
//! `ota_decode_burst` answers with an ACK frame (a keyed NACK on air today). Cycling repeats the
//! recording, so the bursts are not independent draws: the rate is an indication, not a statistic.

use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::ModemEngine;

const TICK: usize = 400;

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
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

#[test]
#[ignore = "measurement (#1456); run with --release --ignored --nocapture"]
fn idle_flicker_evidence_per_hour() {
    // FLICKER_CAPTURE picks the idle: the 250 Hz filter (default), 500 Hz, or the wide control.
    let capture =
        std::env::var("FLICKER_CAPTURE").unwrap_or_else(|_| "ic9700-idle-250hz.wav".into());
    let idle = load_corpus(&capture).expect("corpus");
    let minutes: usize = std::env::var("FLICKER_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let mut e = engine();
    e.start_ota_session(SessionProfile::fast());
    // FLICKER_LEVEL=7 locks SL7 (OFDM52, the shortest recognition window); default: the entry rung.
    if let Some(l) = std::env::var("FLICKER_LEVEL")
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .and_then(SpeedLevel::from_u8)
    {
        e.ota_lock_level(l);
    }
    let start = e.ota_rx_recommended_level();
    let total = minutes * 60 * 8000;
    let (mut bursts, mut evidence, mut lens) = (0usize, 0usize, Vec::new());
    let mut fed = 0usize;
    let mut offset = 0usize;
    while fed < total {
        // A fresh 45 s pass from a staggered offset each time.
        let pass = idle.cycled(offset, idle.samples.len());
        offset += 7_919;
        for chunk in pass.chunks(TICK) {
            fed += chunk.len();
            if let Ok(Some(b)) = e.accumulate_capture(Some("BPSK250"), chunk.to_vec()) {
                bursts += 1;
                // Post-lead length: the lead is pre-trigger ring audio, not the burst.
                let len = b.samples.len() - e.last_flush_lead();
                let r = e
                    .ota_decode_burst(&b, "idle", Some("BPSK250"))
                    .expect("decode");
                if r.ack.is_some() {
                    evidence += 1;
                    lens.push(len);
                }
            }
        }
    }
    let hours = fed as f64 / 8000.0 / 3600.0;
    println!(
        "{minutes} min of {capture} at {start:?}: {bursts} bursts flushed, {evidence} answered with an ACK \
         ({:.1}/h); evidence post-lead lengths {lens:?}; level now {:?}",
        evidence as f64 / hours,
        e.ota_rx_recommended_level()
    );
}
