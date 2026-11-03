//! A ladder frame with the sender's station ID behind it in the same stream, received in a burst that
//! opened before the frame, decodes as the LADDER frame with an ACK.
//!
//! Two things are pinned. (1) Not the ID: the review of the one-stream ID (2026-11-03, S2) asked
//! whether the uncoded fallback could decode the trailing ID and return early; it did not, in any
//! case here. (2) Not nothing: SL5 and SL6 behind a 0.25–0.7 s lead, read in 800-sample ticks,
//! decoded NOTHING, with or without the ID, until the ladder scan's energy-rise onsets reached past
//! the walk's `scan_end` (the walk ends near the trigger, the frame lay beyond it).

use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;

fn engine(bk: &LoopbackBackend, level: SpeedLevel) -> ModemEngine {
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
    e.ota_lock_level(level);
    e
}

fn noise(n: usize, amp: f32, st: &mut u64) -> Vec<f32> {
    (0..n)
        .map(|_| {
            *st ^= *st << 13;
            *st ^= *st >> 7;
            *st ^= *st << 17;
            ((*st >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * amp * 3.46
        })
        .collect()
}

const DATA: &[u8] = b"the ladder frame the ID rides behind";

/// (payload decoded, ACK keyed) for a data+ID burst with `lead` samples of noise in front, the first
/// block of the burst `first_block` samples long.
fn receive(level: SpeedLevel, lead: usize, first_block: usize) -> (Option<Vec<u8>>, bool) {
    let bk = LoopbackBackend::new();
    let mut tx = engine(&bk, level);
    let mode = tx.ota_tx_mode().unwrap().to_owned();
    let fec = tx.ota_tx_fec();
    tx.transmit_burst(None, |e| {
        e.transmit_with_fec_mode(DATA, &mode, fec, None)?;
        e.transmit(b"DE ONESTR", "BPSK250", None)
    })
    .unwrap();
    let burst = bk.drain_samples();
    let mut st = 0x9e37_79b9_7f4a_7c15_u64;
    let quiet = noise(24_000, 0.002, &mut st);
    let mut signal = noise(lead, 0.03, &mut st);
    let floor = noise(burst.len(), 0.002, &mut st);
    signal.extend(burst.iter().zip(&floor).map(|(s, n)| s + n));
    signal.extend(noise(16_000, 0.002, &mut st));

    let rx_bk = LoopbackBackend::new();
    let mut rx = engine(&rx_bk, level);
    let mut out = (None, false);
    let feed = |rx: &mut ModemEngine, chunk: &[f32], out: &mut (Option<Vec<u8>>, bool)| {
        if let Some(b) = rx
            .accumulate_capture(Some("BPSK250"), chunk.to_vec())
            .unwrap()
        {
            if let Ok(r) = rx.ota_decode_burst(&b, "DC0SK", None) {
                if r.payload.is_some() && out.0.is_none() {
                    *out = (r.payload, r.ack.is_some());
                }
            }
        }
    };
    for c in quiet.chunks(800) {
        feed(&mut rx, c, &mut out);
    }
    let (first, rest) = signal.split_at(first_block.min(signal.len()));
    feed(&mut rx, first, &mut out);
    for c in rest.chunks(800) {
        feed(&mut rx, c, &mut out);
    }
    out
}

#[test]
fn the_data_frame_not_its_trailing_id_is_decoded() {
    for level in [SpeedLevel::Sl5, SpeedLevel::Sl6] {
        for lead in [0usize, 2_000, 5_600] {
            for first_block in [800usize, 16_000] {
                let (payload, ack) = receive(level, lead, first_block);
                println!(
                    "S2 {level:?} lead {lead} first block {first_block}: payload {:?}, ack {ack}",
                    payload.as_deref().map(String::from_utf8_lossy)
                );
                assert_eq!(
                    payload.as_deref(),
                    Some(DATA),
                    "{level:?}, lead {lead}, first block {first_block}: the receiver decoded \
                     {:?} instead of the ladder frame",
                    payload.as_deref().map(String::from_utf8_lossy)
                );
                assert!(
                    ack,
                    "{level:?}, lead {lead}, first block {first_block}: no ACK keyed"
                );
            }
        }
    }
}
