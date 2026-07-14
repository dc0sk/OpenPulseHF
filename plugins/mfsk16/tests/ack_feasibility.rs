//! MFSK16-ACK feasibility (REQ-WSIG-01, PR-C measure-first gate): the short non-coherent return channel
//! must survive at the *data* rung's operating floor, or the sub-floor rung is broadcast-only.
//!
//! Path: a 5-byte `AckFrame` → `ShortFecCodec::new()` (t=4 → 13 bytes) → `MFSK16-ACK` modulate (40 symbols
//! ≈ 1.28 s) → Watterson → demodulate (13 bytes) → ShortFec decode. Injects a ±25 Hz tuning offset + a
//! short lead so acquisition is exercised, exactly like the data real-sync. The data rung crosses ~0 dB on
//! moderate/poor_f1 (`docs/dev/research/robust-narrowband-measurement.md`); the ACK — ShortFec's ~30% byte
//! tolerance over 13 bytes vs the data path's RS 6.3% — should be *more* robust there. Pre-registered bar:
//! **ACK decode ≥ 0.9 at 0 dB on moderate_f1 and poor_f1.**

use mfsk16_plugin::Mfsk16Plugin;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::fec::ShortFecCodec;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

const ACK_RAW: [u8; 5] = [0x01, 0x23, 0x45, 0x67, 0x89];
const TRIALS: u32 = 40;

fn ack_cfg(center: f32) -> ModulationConfig {
    ModulationConfig {
        mode: "MFSK16-ACK".into(),
        center_frequency: center,
        sample_rate: 8000,
        ..Default::default()
    }
}

fn ack_success(preset: fn(Option<u64>) -> WattersonConfig, snr_db: f32) -> f32 {
    let plugin = Mfsk16Plugin::new();
    let fec = ShortFecCodec::new()
        .encode(&ACK_RAW)
        .expect("short-fec encode"); // 13 bytes
    let mut ok = 0u32;
    for t in 0..TRIALS {
        let seed = 3000 + t as u64 * 5;
        let df = (seed % 51) as f32 - 25.0; // ±25 Hz tuning
        let lead = (seed / 51 % 257) as usize; // ≤ 256-sample lead
        let tx = plugin
            .modulate(&fec, &ack_cfg(1500.0 + df))
            .expect("modulate");
        let mut sig = vec![0.0f32; lead];
        sig.extend(tx);
        let mut cfg = preset(Some(seed));
        cfg.snr_db = snr_db;
        let faded = WattersonChannel::new(cfg).expect("watterson").apply(&sig);
        // Receiver believes fc = 1500; acquisition absorbs the offset + lead.
        let decoded = plugin
            .demodulate(&faded, &ack_cfg(1500.0))
            .ok()
            .and_then(|bytes| ShortFecCodec::new().decode(&bytes).ok());
        if decoded.as_deref() == Some(&ACK_RAW[..]) {
            ok += 1;
        }
    }
    ok as f32 / TRIALS as f32
}

/// The MFSK16-ACK mode functions end-to-end (waveform + acquisition + ShortFec) — it decodes reliably a
/// few dB above the data floor. **The measured finding (see the module docs + the sweep below):** at the
/// *data* floor (~0 dB) it decodes only ~0.6 — a 1.28 s ACK can't fade-average like the 17 s data frame,
/// so the short return channel is ~3–4 dB more fade-sensitive and is the binding constraint for an ARQ
/// rung. Naive tone repetition doesn't fix it (energy-summing a faded copy dilutes; the #694 lesson).
/// A robust ARQ ACK needs proper per-copy LLR diversity — deferred; the rung ships broadcast-first.
#[test]
fn ack_mode_functions_above_the_floor() {
    let rate = ack_success(WattersonConfig::moderate_f1, 6.0);
    assert!(
        rate >= 0.9,
        "MFSK16-ACK on moderate_f1 @6 dB decoded {rate:.2} (< 0.9) — the ACK mode itself is broken"
    );
}

/// Full sweep for the record.
#[test]
#[ignore = "research measurement for REQ-WSIG-01 ACK; --ignored --nocapture"]
fn ack_feasibility_sweep() {
    for (name, preset) in [
        (
            "moderate_f1",
            WattersonConfig::moderate_f1 as fn(Option<u64>) -> WattersonConfig,
        ),
        (
            "poor_f1",
            WattersonConfig::poor_f1 as fn(Option<u64>) -> WattersonConfig,
        ),
    ] {
        println!("\n=== MFSK16-ACK on {name} ({TRIALS} trials) ===");
        println!("  snr_db   ack_success");
        for snr in [-6.0f32, -3.0, 0.0, 3.0] {
            println!("  {snr:6.1}   {:.2}", ack_success(preset, snr));
        }
    }
}
