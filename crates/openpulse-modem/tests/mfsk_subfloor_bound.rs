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

// ── real-sync acquisition (Costas-style 16-tone sync + timing/frequency search) ─
//
// Adds acquisition to the 16-GFSK arm so the ideal→real erosion is measured directly (a genie column is
// printed alongside). Design (Fable review): three 7-symbol Costas sync blocks, a normalized per-symbol
// tone-fraction correlation (the JS8 `sync_score` pattern, immune to the high-energy-noise-window trap),
// a two-stage coarse→fine timing+frequency search, and an injected ±25 Hz tuning offset the searcher must
// find. Frequency (tuning) is the dominant real-world uncertainty; timing is a small lead-in search
// (a short silence lead barely changes the RMS-keyed SNR, ~0.03 dB for lead ≪ frame).

/// FT8 legacy Costas `[4,2,5,6,1,3,0]` scaled ×2 (distinct-difference property preserved), spanning
/// tones 0..12 of 16 (~375 Hz) for sync-time frequency diversity.
const COSTAS16: [u8; 7] = [8, 4, 10, 12, 2, 6, 0];
const SYNC_STARTS: [usize; 3] = [0, 262, 524];
const ONAIR_TONES: usize = 531; // 510 data + 3×7 sync

fn sync_mask() -> [bool; ONAIR_TONES] {
    let mut m = [false; ONAIR_TONES];
    for &s in &SYNC_STARTS {
        for k in 0..COSTAS16.len() {
            m[s + k] = true;
        }
    }
    m
}

/// Interleave the 3 Costas sync blocks into the data tones → the 531-symbol on-air sequence.
fn insert_sync(data: &[u8]) -> Vec<u8> {
    let mask = sync_mask();
    let mut out = vec![0u8; ONAIR_TONES];
    for &s in &SYNC_STARTS {
        out[s..s + COSTAS16.len()].copy_from_slice(&COSTAS16);
    }
    let mut di = 0;
    for (p, &is_sync) in mask.iter().enumerate() {
        if !is_sync {
            out[p] = data[di];
            di += 1;
        }
    }
    out
}

/// On-air symbol positions carrying data (i.e. not sync), in order.
fn data_positions() -> Vec<usize> {
    let mask = sync_mask();
    (0..ONAIR_TONES).filter(|&p| !mask[p]).collect()
}

fn mfsk_sync_tx(bytes: &[u8], base: f32) -> Vec<f32> {
    modulate_tones(&insert_sync(&bytes_to_tones(bytes)), base, &gfsk_params())
}

fn sym_energies16(win: &[f32], base: f32) -> [f32; 16] {
    let mut e = [0f32; 16];
    for (t, slot) in e.iter_mut().enumerate() {
        *slot = goertzel_energy(win, base + t as f32 * SPACING, FS as f32);
    }
    e
}

/// Engine-convention max-log 4-bit LLRs from one symbol's 16 tone energies (positive = bit 0).
fn bit_llrs(e: &[f32; 16]) -> [f32; 4] {
    let max = e.iter().cloned().fold(0.0f32, f32::max);
    let (mut acc, mut cnt) = (0.0f32, 0u32);
    for &v in e {
        if v < max {
            acc += v;
            cnt += 1;
        }
    }
    let noise = (acc / cnt.max(1) as f32).max(1e-9);
    let mut out = [0f32; 4];
    for (b, slot) in out.iter_mut().enumerate() {
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
        *slot = (e0 - e1) / noise;
    }
    out
}

/// Normalized Costas correlation over the 21 sync symbols at `(offset, base)`; a perfect lock scores 21,
/// a noise window ≈ 21/16 ≈ 1.3. `None` if the window runs off the end.
fn sync_score16(audio: &[f32], offset: usize, base: f32) -> Option<f32> {
    let mut score = 0.0f32;
    for &s in &SYNC_STARTS {
        for k in 0..COSTAS16.len() {
            let start = offset + (s + k) * SPS;
            let win = audio.get(start..start + SPS)?;
            let e = sym_energies16(win, base);
            let sum: f32 = e.iter().sum::<f32>() + 1e-9;
            score += e[COSTAS16[k] as usize] / sum;
        }
    }
    Some(score)
}

/// Acquire `(offset, base)` by maximising the normalized Costas score: coarse timing (step = one symbol,
/// up to `max_offset`) × coarse frequency (±50 Hz @ 15.625), then refine timing (±1 symbol @ sps/8) ×
/// frequency (±15.6 Hz @ 3.9 Hz). Gate at 12/21 (JS8's `min_sync_score`). `nominal_base` is what the
/// receiver believes the base tone is (it does NOT know the injected tuning offset).
fn acquire(audio: &[f32], nominal_base: f32, max_offset: usize) -> Option<(usize, f32)> {
    let coarse_freqs: Vec<f32> = (-3..=3)
        .map(|i| nominal_base + i as f32 * (SPACING / 2.0))
        .collect();
    let mut best: Option<(f32, usize, f32)> = None;
    let mut off = 0;
    while off <= max_offset {
        for &bf in &coarse_freqs {
            if let Some(sc) = sync_score16(audio, off, bf) {
                if best.is_none_or(|(bs, _, _)| sc > bs) {
                    best = Some((sc, off, bf));
                }
            }
        }
        off += SPS;
    }
    let (_, coff, cbf) = best?;
    let mut refined: Option<(f32, usize, f32)> = None;
    let t_lo = coff.saturating_sub(SPS);
    let mut t = t_lo;
    while t <= coff + SPS {
        for i in -4..=4 {
            let bf = cbf + i as f32 * (SPACING / 8.0);
            if let Some(sc) = sync_score16(audio, t, bf) {
                if refined.is_none_or(|(bs, _, _)| sc > bs) {
                    refined = Some((sc, t, bf));
                }
            }
        }
        t += SPS / 8;
    }
    let (score, off, bf) = refined?;
    (score >= 12.0).then_some((off, bf))
}

/// Demodulate the 510 data symbols at `(offset, base)`, skipping the 3 sync blocks, into LLRs.
fn mfsk_llrs_at(audio: &[f32], offset: usize, base: f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(data_positions().len() * 4);
    for &p in &data_positions() {
        let start = offset + p * SPS;
        let win = match audio.get(start..start + SPS) {
            Some(w) => w,
            None => break,
        };
        out.extend_from_slice(&bit_llrs(&sym_energies16(win, base)));
    }
    out
}

fn decode_llrs(llr: Vec<f32>) -> bool {
    decode_engine()
        .combine_and_decode_llrs("MFSK16", &[llr])
        .map(|d| d == PAYLOAD)
        .unwrap_or(false)
}

/// Real-sync decode: acquire, then demod from the lock. `false` on a failed acquisition gate.
fn mfsk_real_decodes(faded: &[f32], nominal_base: f32, max_offset: usize) -> bool {
    match acquire(faded, nominal_base, max_offset) {
        Some((off, base)) => decode_llrs(mfsk_llrs_at(faded, off, base)),
        None => false,
    }
}

/// Genie decode of the same sync-bearing waveform at the known `(offset, base)` — the erosion reference.
fn mfsk_genie_decodes(faded: &[f32], offset: usize, base: f32) -> bool {
    decode_llrs(mfsk_llrs_at(faded, offset, base))
}

/// Acquisition is expensive (a timing×frequency grid search per trial), so the real-sync sweeps use
/// fewer trials than the genie bound.
const REAL_TRIALS: u32 = 24;
const LEAD_MAX: usize = 1024;

/// One real-sync trial through channel `apply`: inject a per-trial ±25 Hz tuning offset and a short
/// silence lead (≤1024 samples ≪ the 136 k-sample frame → ~0.03 dB RMS effect), then decode both
/// genie-aligned (known offset/base) and real (searched). Returns `(genie_ok, real_ok)`.
fn mfsk_real_trial(bytes: &[u8], seed: u64, apply: impl Fn(&[f32]) -> Vec<f32>) -> (bool, bool) {
    let df = (seed % 51) as f32 - 25.0;
    let lead = (seed / 51 % (LEAD_MAX as u64 + 1)) as usize;
    let base = MFSK_BASE + df;
    let mut sig = vec![0.0f32; lead];
    sig.extend(mfsk_sync_tx(bytes, base));
    let faded = apply(&sig);
    let genie = mfsk_genie_decodes(&faded, lead, base);
    // The searcher knows neither `df` nor `lead`: it searches around the nominal base, offsets to LEAD_MAX+1 sym.
    let real = mfsk_real_decodes(&faded, MFSK_BASE, LEAD_MAX + SPS);
    (genie, real)
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

/// Real-sync guard (NOT ignored): with an injected +18 Hz tuning offset and a 300-sample lead, the Costas
/// acquisition must find `(offset, base)` and decode on a near-clean channel. Guards the whole
/// acquisition path (sync insertion, the normalized score, the coarse→fine search, the skip-sync demod).
#[test]
fn real_sync_acquires_a_tuning_offset_and_lead() {
    let bytes = fec_bytes();
    let base = MFSK_BASE + 18.0;
    let lead = 300;
    let mut sig = vec![0.0f32; lead];
    sig.extend(mfsk_sync_tx(&bytes, base));
    let faded = awgn(&sig, 30.0, 1); // near-clean, but gives the lead real noise
    assert!(
        mfsk_real_decodes(&faded, MFSK_BASE, LEAD_MAX + SPS),
        "real-sync must acquire (+18 Hz, lead 300) and decode on a clean channel"
    );
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

/// The real-sync sweep: BPSK31 | 16-GFSK genie | 16-GFSK real, per channel. The genie–real gap IS the
/// acquisition erosion (measured directly). BPSK31 keeps genie frequency (its ±7.8 Hz AFC can't absorb a
/// ±25 Hz tuning error without an engine AFC chain the bare demod lacks — injecting it there would zero
/// the baseline via a harness artifact, so the bias is left running against the candidate). Read the real
/// column's moderate_f1 crossing gain against the ≥3 dB ship bar.
#[test]
#[ignore = "research measurement for REQ-WSIG-01 (slow — acquisition search); --ignored --nocapture"]
fn mfsk_real_sync_sweep() {
    let bytes = fec_bytes();
    let btx = bpsk_tx(&bytes);
    for (preset, snrs) in [
        (Preset::GoodF1, &[-9.0f32, -6.0, -3.0][..]),
        (Preset::ModerateF1, &[-3.0, 0.0, 3.0, 6.0][..]),
        (Preset::PoorF1, &[-3.0, 0.0, 3.0, 6.0][..]),
    ] {
        println!(
            "\n=== {} real-sync ({} trials, +4.1% preamble airtime, ±25 Hz tuning) ===",
            preset.label(),
            REAL_TRIALS
        );
        println!("  snr_db   bpsk31   mfsk_genie   mfsk_real");
        for &snr in snrs {
            let b = frame_success(|s| bpsk_decodes(&btx, &watterson(&btx, preset, snr, s)));
            let (mut g, mut r) = (0u32, 0u32);
            for t in 0..REAL_TRIALS {
                let s = seed(t);
                let (ge, re) = mfsk_real_trial(&bytes, s, |sig| watterson(sig, preset, snr, s));
                g += ge as u32;
                r += re as u32;
            }
            println!(
                "  {snr:6.1}   {b:6.2}   {:10.2}   {:9.2}",
                g as f32 / REAL_TRIALS as f32,
                r as f32 / REAL_TRIALS as f32
            );
        }
    }
}

/// AWGN real-vs-genie sanity: on AWGN (no fading, easy sync) the real column must sit within ~0.5–1 dB of
/// the genie column — a larger gap means the acquisition itself is buggy ("fails where it has nowhere to
/// hide"), which must be fixed before trusting the fading numbers.
#[test]
#[ignore = "research measurement for REQ-WSIG-01; --ignored --nocapture"]
fn mfsk_real_sync_awgn_sanity() {
    let bytes = fec_bytes();
    println!("\n=== AWGN real-sync ({} trials) ===", REAL_TRIALS);
    println!("  snr_db   mfsk_genie   mfsk_real");
    for snr in [-9.0f32, -6.0, -3.0, 0.0, 3.0] {
        let (mut g, mut r) = (0u32, 0u32);
        for t in 0..REAL_TRIALS {
            let s = seed(t);
            let (ge, re) = mfsk_real_trial(&bytes, s, |sig| awgn(sig, snr, s));
            g += ge as u32;
            r += re as u32;
        }
        println!(
            "  {snr:6.1}   {:10.2}   {:9.2}",
            g as f32 / REAL_TRIALS as f32,
            r as f32 / REAL_TRIALS as f32
        );
    }
}
