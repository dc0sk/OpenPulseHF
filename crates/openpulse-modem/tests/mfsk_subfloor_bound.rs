//! Kill-first measurement for the robust narrowband weak-signal rung (REQ-WSIG-01).
//!
//! Candidate: a **constant-envelope non-coherent 16-GFSK**, 31.25 baud (= BPSK31's symbol rate), 31.25 Hz
//! tone spacing, 256 samples/symbol at 8 kHz, 16 tones → **500 Hz occupied**, 4 bits/symbol → 125 bps raw
//! (4× BPSK31). It reuses the JS8 tone synth (`modulate_tones`) and Goertzel energy detector
//! (`goertzel_energy`), and the engine's audio-free union decode (`combine_and_decode_llrs`) — so both
//! arms run the **same RS(255,223) + Frame/CRC** decode (matched FEC by construction).
//!
//! The claim under test: a non-coherent, constant-envelope waveform collects the ~14 dB
//! implementation+fading tax the coherent BPSK31 chain pays (carrier tracking through fades, Doppler),
//! **without** paying it back in PAPR (ΔPAPR ≈ 0, a *credit* vs BPSK's +1.44 dB under the RMS-keyed
//! channel). The 500 Hz span also gives per-symbol frequency diversity on moderate/poor multipath.
//!
//! This is the **ideal (genie-sync) bound**: both arms are symbol-aligned and frequency-exact, so it is
//! valid for the kill decision, not the ship decision (a real receiver adds acquisition, ~2–3 dB erosion
//! per #864). Pre-registered ship bar (roadmap): the *ideal* must clear ≥5 dB at the moderate_f1
//! 0.5-crossing (3 dB ship bar + ~2 dB ideal→real erosion), with no good_f1 regression; else honest
//! no-ship. AWGN known-answer sanity: at matched Eb/N0, 16-FSK must land within ~±1.5 dB of BPSK31 (M-ary
//! gain ≈ non-coherent penalty at M=16); an MFSK AWGN win ≥2 dB = a fairness bug.
//!
//! Run: `cargo test -p openpulse-modem --no-default-features --test mfsk_subfloor_bound -- --ignored --nocapture`

use bpsk_plugin::BpskPlugin;
use js8_plugin::demodulate::goertzel_energy;
use js8_plugin::modulate::{modulate_tones, GfskParams};
use openpulse_audio::LoopbackBackend;
use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::{watterson::WattersonChannel, AwgnConfig, ChannelModel, WattersonConfig};
use openpulse_core::fec::FecCodec;
use openpulse_core::frame::Frame;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_modem::ModemEngine;

const PAYLOAD: &[u8] = b"Weak-signal robust-narrowband rung bake-off, sixty-four byte payload AAA";
const FS: u32 = 8000;
const SPS: usize = 256; // 8000 / 31.25 = 256 exact
const SPACING: f32 = 31.25;
const MFSK_BASE: f32 = 1000.0; // 16 tones → 1000..1469 Hz
const BPSK_FC: f32 = 1500.0;
const TRIALS: u32 = 40;

// ── shared FEC frame (identical wire bits on both arms) ───────────────────────

fn fec_bytes() -> Vec<u8> {
    let frame = Frame::new(0, PAYLOAD.to_vec()).expect("frame").encode();
    FecCodec::new().encode(&frame)
}

fn decode_engine() -> ModemEngine {
    // `combine_and_decode_llrs` never touches the plugin registry; a bare engine suffices.
    ModemEngine::new(Box::new(LoopbackBackend::new()))
}

// ── BPSK31 arm (coherent, the neighbour) ──────────────────────────────────────

fn bpsk_tx(bytes: &[u8]) -> Vec<f32> {
    BpskPlugin::new()
        .modulate(
            bytes,
            &ModulationConfig {
                mode: "BPSK31".into(),
                center_frequency: BPSK_FC,
                sample_rate: FS,
                ..Default::default()
            },
        )
        .expect("bpsk modulate")
}

fn bpsk_decodes(tx: &[f32], faded: &[f32]) -> bool {
    let _ = tx;
    let llr = BpskPlugin::new()
        .demodulate_soft(
            faded,
            &ModulationConfig {
                mode: "BPSK31".into(),
                center_frequency: BPSK_FC,
                sample_rate: FS,
                ..Default::default()
            },
        )
        .unwrap_or_default();
    decode_engine()
        .combine_and_decode_llrs("BPSK31", &[llr])
        .map(|d| d == PAYLOAD)
        .unwrap_or(false)
}

// ── 16-GFSK arm (constant-envelope non-coherent) ──────────────────────────────

fn gfsk_params() -> GfskParams {
    GfskParams {
        samples_per_symbol: SPS,
        tone_spacing_hz: SPACING,
        sample_rate: FS,
        bt: js8_plugin::modulate::DEFAULT_BT,
    }
}

/// Bytes → LSB-first bits → 4-bit tones (tone = Σ bit_{4j+b}·2^b), matching the demod's bit order.
fn bytes_to_tones(bytes: &[u8]) -> Vec<u8> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for k in 0..8 {
            bits.push((b >> k) & 1);
        }
    }
    while bits.len() % 4 != 0 {
        bits.push(0);
    }
    bits.chunks(4)
        .map(|c| c[0] | (c[1] << 1) | (c[2] << 2) | (c[3] << 3))
        .collect()
}

fn mfsk_tx(bytes: &[u8]) -> Vec<f32> {
    modulate_tones(&bytes_to_tones(bytes), MFSK_BASE, &gfsk_params())
}

/// Non-coherent soft demod: per symbol, 16 Goertzel energies → engine-convention max-log bit LLRs
/// (positive = bit 0, negative = bit 1; LSB-first), scaled by 1/noise (mean of the 15 non-winner tones).
fn mfsk_llrs(faded: &[f32], n_tones: usize) -> Vec<f32> {
    let fs = FS as f32;
    let mut out = Vec::with_capacity(n_tones * 4);
    for j in 0..n_tones {
        let win = match faded.get(j * SPS..j * SPS + SPS) {
            Some(w) => w,
            None => break,
        };
        let mut e = [0f32; 16];
        for (t, slot) in e.iter_mut().enumerate() {
            *slot = goertzel_energy(win, MFSK_BASE + t as f32 * SPACING, fs);
        }
        let max = e.iter().cloned().fold(0.0f32, f32::max);
        let noise = {
            let (mut acc, mut cnt) = (0.0f32, 0u32);
            for &v in &e {
                if v < max {
                    acc += v;
                    cnt += 1;
                }
            }
            (acc / cnt.max(1) as f32).max(1e-9)
        };
        for b in 0..4 {
            let mask = 1usize << b;
            let mut e0 = f32::NEG_INFINITY;
            let mut e1 = f32::NEG_INFINITY;
            for (t, &energy) in e.iter().enumerate() {
                if t & mask != 0 {
                    e1 = e1.max(energy);
                } else {
                    e0 = e0.max(energy);
                }
            }
            // Engine convention: positive ⇒ bit 0 (best bit-0 tone stronger).
            out.push((e0 - e1) / noise);
        }
    }
    out
}

fn mfsk_decodes(faded: &[f32], n_tones: usize) -> bool {
    let llr = mfsk_llrs(faded, n_tones);
    decode_engine()
        .combine_and_decode_llrs("MFSK16", &[llr])
        .map(|d| d == PAYLOAD)
        .unwrap_or(false)
}

// ── channels ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Preset {
    GoodF1,
    ModerateF1,
    PoorF1,
}
impl Preset {
    fn config(self, seed: u64) -> WattersonConfig {
        match self {
            Preset::GoodF1 => WattersonConfig::good_f1(Some(seed)),
            Preset::ModerateF1 => WattersonConfig::moderate_f1(Some(seed)),
            Preset::PoorF1 => WattersonConfig::poor_f1(Some(seed)),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Preset::GoodF1 => "good_f1",
            Preset::ModerateF1 => "moderate_f1",
            Preset::PoorF1 => "poor_f1",
        }
    }
}

fn watterson(tx: &[f32], p: Preset, snr_db: f32, seed: u64) -> Vec<f32> {
    let mut c = p.config(seed);
    c.snr_db = snr_db;
    WattersonChannel::new(c).expect("watterson").apply(tx)
}

fn awgn(tx: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    AwgnChannel::new(AwgnConfig::new(snr_db, Some(seed)))
        .expect("awgn")
        .apply(tx)
}

fn papr_db(x: &[f32]) -> f32 {
    // Envelope PAPR via |analytic|² (hilbert at the waveform's center).
    let (i, q) = openpulse_core::iq::hilbert_iq(x, 1200.0, FS as f32);
    let p: Vec<f32> = i.iter().zip(&q).map(|(a, b)| a * a + b * b).collect();
    let peak = p.iter().cloned().fold(0.0f32, f32::max);
    let mean = p.iter().sum::<f32>() / p.len().max(1) as f32;
    10.0 * (peak / mean.max(1e-12)).log10()
}

fn seed(trial: u32) -> u64 {
    5000 + trial as u64 * 7
}

// ── the clean-channel plumbing guard (NOT ignored) ────────────────────────────

/// Both arms must round-trip on a clean channel. This guards the whole harness — in particular the
/// 16-tone LLR convention (positive = bit 0, LSB-first) against the engine's `hard_decide`.
#[test]
fn both_arms_round_trip_on_a_clean_channel() {
    let bytes = fec_bytes();
    // MFSK clean: no channel, demod the exact tones.
    let mtx = mfsk_tx(&bytes);
    let n_tones = bytes_to_tones(&bytes).len();
    assert!(
        mfsk_decodes(&mtx, n_tones),
        "16-GFSK must round-trip clean (LLR convention / bit order)"
    );
    // BPSK31 clean.
    let btx = bpsk_tx(&bytes);
    assert!(bpsk_decodes(&btx, &btx), "BPSK31 must round-trip clean");
}

// ── the sweeps (ignored research measurements) ────────────────────────────────

fn frame_success<F: Fn(u64) -> bool>(trial_fn: F) -> f32 {
    let ok: u32 = (0..TRIALS).map(|t| trial_fn(seed(t)) as u32).sum();
    ok as f32 / TRIALS as f32
}

fn watterson_sweep(preset: Preset, snrs: &[f32]) {
    let bytes = fec_bytes();
    let btx = bpsk_tx(&bytes);
    let mtx = mfsk_tx(&bytes);
    let n_tones = bytes_to_tones(&bytes).len();
    println!(
        "\n=== {} ({} trials) — ΔPAPR {:+.2} dB (mfsk {:.2} − bpsk {:.2}) ===",
        preset.label(),
        TRIALS,
        papr_db(&mtx) - papr_db(&btx),
        papr_db(&mtx),
        papr_db(&btx),
    );
    println!("  snr_db   bpsk31   mfsk16   (Eb/N0: bpsk +26.5, mfsk +20.5 dB vs label)");
    for &snr in snrs {
        let b = frame_success(|s| bpsk_decodes(&btx, &watterson(&btx, preset, snr, s)));
        let m = frame_success(|s| mfsk_decodes(&watterson(&mtx, preset, snr, s), n_tones));
        println!("  {snr:6.1}   {b:6.2}   {m:6.2}");
    }
}

/// The primary REQ-WSIG-01 kill-first sweep: 16-GFSK vs BPSK31 coded frame-success on Watterson
/// good/moderate/poor at matched average power, ideal (genie) sync. Read the moderate_f1 crossing gain
/// against the ≥5 dB ideal bar.
#[test]
#[ignore = "research measurement for REQ-WSIG-01; run with --ignored --nocapture"]
fn mfsk_vs_bpsk31_watterson_sweep() {
    watterson_sweep(Preset::GoodF1, &[-12.0, -9.0, -6.0, -3.0, 0.0]);
    watterson_sweep(Preset::ModerateF1, &[-9.0, -6.0, -3.0, 0.0, 3.0]);
    watterson_sweep(Preset::PoorF1, &[-6.0, -3.0, 0.0, 3.0, 6.0]);
}

/// AWGN known-answer sanity: at the SAME label SNR the 16-GFSK's 4× rate means it runs 6 dB *higher*
/// Eb/N0, so on the label axis it should decode at a HIGHER SNR than BPSK31 by roughly (6 dB − the M-ary
/// coding gain). The load-bearing check is that MFSK does NOT beat BPSK31 on AWGN at the same label by a
/// wide margin — a fading-only lever must not win on AWGN (that would be a fairness/accounting bug).
#[test]
#[ignore = "research measurement for REQ-WSIG-01; run with --ignored --nocapture"]
fn awgn_known_answer_sanity() {
    let bytes = fec_bytes();
    let btx = bpsk_tx(&bytes);
    let mtx = mfsk_tx(&bytes);
    let n_tones = bytes_to_tones(&bytes).len();
    println!("\n=== AWGN (label SNR; mfsk runs +6 dB Eb/N0 vs bpsk at equal label) ===");
    println!("  snr_db   bpsk31   mfsk16");
    for snr in [-12.0f32, -9.0, -6.0, -3.0, 0.0, 3.0, 6.0] {
        let b = frame_success(|s| bpsk_decodes(&btx, &awgn(&btx, snr, s)));
        let m = frame_success(|s| mfsk_decodes(&awgn(&mtx, snr, s), n_tones));
        println!("  {snr:6.1}   {b:6.2}   {m:6.2}");
    }
}
