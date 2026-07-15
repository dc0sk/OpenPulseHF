//! MFSK16 HARQ diversity gate (REQ-WSIG-01, PR-2): the HARQ-combine gate now admits the MFSK16 sub-floor
//! rung (soft-capable but plain RS), so its 17 s frames MAP-combine across NACK retransmissions. Fable's
//! guardrail 4: the gate ships only with a measured gain — combining two moderate-fade attempts must beat a
//! single attempt (the standalone-first union bounds the downside; this proves the upside is real).

use mfsk16_plugin::Mfsk16Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_modem::engine::ModemEngine;

const FS: u32 = 8000;
const FC: f32 = 1500.0;

fn cfg() -> ModulationConfig {
    ModulationConfig {
        mode: "MFSK16".into(),
        center_frequency: FC,
        sample_rate: FS,
        ..Default::default()
    }
}

/// One MFSK16 data frame (Frame + RS, one 255-byte block) as audio, plus the payload it carries.
fn mfsk16_frame_audio() -> (Vec<f32>, Vec<u8>) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(Mfsk16Plugin::new())).unwrap();
    let payload: Vec<u8> = (0..120u8).collect();
    e.transmit_with_fec(&payload, "MFSK16", None)
        .expect("transmit MFSK16 data frame");
    (backend.drain_samples(), payload)
}

fn faded(tx: &[f32], snr: f32, seed: u64) -> Vec<f32> {
    let mut c = WattersonConfig::moderate_f1(Some(seed));
    c.snr_db = snr;
    WattersonChannel::new(c).expect("watterson").apply(tx)
}

fn soft(plugin: &Mfsk16Plugin, faded: &[f32]) -> Option<Vec<f32>> {
    plugin.demodulate_soft(faded, &cfg()).ok()
}

/// Combining two moderate-fade retransmissions decodes strictly more often than a single attempt.
#[test]
fn mfsk16_harq_diversity_beats_single_attempt() {
    let (tx, payload) = mfsk16_frame_audio();
    let plugin = Mfsk16Plugin::new();
    let trials = 30u64;
    let snr = -3.0;
    let (mut single_ok, mut combined_ok) = (0u32, 0u32);

    for t in 0..trials {
        // Two retransmissions land in independent fade realizations (distinct seeds).
        let a = faded(&tx, snr, 100 + t * 2);
        let b = faded(&tx, snr, 101 + t * 2);
        let la = soft(&plugin, &a);
        let lb = soft(&plugin, &b);

        if let Some(la) = &la {
            if decode_matches(&[la.clone()], &payload) {
                single_ok += 1;
            }
        }
        if let (Some(la), Some(lb)) = (&la, &lb) {
            if decode_matches(&[la.clone(), lb.clone()], &payload) {
                combined_ok += 1;
            }
        }
    }

    assert!(
        combined_ok > single_ok,
        "MFSK16 HARQ combining must beat a single attempt on moderate_f1 @{snr} dB \
         (single {single_ok}/{trials}, combined {combined_ok}/{trials})"
    );
}

/// A fresh bare engine per decode (union decode is stateless w.r.t. the registry).
fn decode_matches(llrs: &[Vec<f32>], payload: &[u8]) -> bool {
    ModemEngine::new(Box::new(LoopbackBackend::new()))
        .combine_and_decode_llrs("MFSK16", llrs)
        .map(|d| d == payload)
        .unwrap_or(false)
}
