//! MEASUREMENT for #1142 — how much does burst lead-in depress the rate-control SNR?
//!
//! `ota_decode_and_ack_inner` computes the SNR that drives the receiver's rate decision over the
//! **entire gathered burst** (`self.rx_snr_db(m, &samples.samples)`), lead-in included.
//! `samples.samples` is everything `accumulate_capture` flushed: the frame plus however much
//! pre-frame audio the DCD let through. Lead-in is by definition the part with no signal in it, so
//! including it can only depress the estimate — the question this answers is *by how much*, and
//! whether that is enough to cross a rung boundary in `hpx_hf`.
//!
//! **The comparison scale is taken from the profile, not typed in** (`SessionProfile::hpx_hf`), so
//! a floor change cannot silently invalidate the conclusion drawn here.
//!
//! Two arms:
//!
//! 1. **Synthetic lead, real noise** — a real engine-transmitted frame embedded in the recorded
//!    IC-9700 idle floor at controlled lead lengths. Real noise matters: a digital-silence lead
//!    would understate the bias, because silence is not what depresses an SNR estimate the way band
//!    noise does. (That exact flaw was found in #1139's harness.)
//! 2. **Real capture** — the whole recorded on-air frame capture versus its burst span, located
//!    by energy rather than transcribed. Nothing about it is modelled.
//!
//! Assert-free: it prints tables. Run:
//! `cargo test -p openpulse-modem --no-default-features --test snr_lead_in_bias -- --ignored --nocapture`

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::ModemEngine;

const MODE: &str = "BPSK250";
/// A payload of the size ordinary traffic uses, so the frame span is representative.
const PAYLOAD: &[u8] = b"SNR LEAD-IN BIAS PROBE 1142 - sixty four bytes of payload here..";
/// Lead-ins in samples at 8 kHz. 4032 is the lead-in measured on the real #1021 on-air capture;
/// 800 is one daemon rx tick (100 ms); the rest bracket it.
const LEADS: [usize; 6] = [0, 400, 800, 4_032, 8_000, 24_000];
/// Trail is held constant so the only variable is the lead.
const TRAIL: usize = 800;

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register BPSK");
    e
}

/// A real engine-transmitted frame: framing, whitening and modulation exactly as on air.
fn transmitted_frame() -> Vec<f32> {
    let backend = LoopbackBackend::new();
    let handle = backend.clone_shared();
    let mut tx = ModemEngine::new(Box::new(backend));
    tx.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register BPSK");
    tx.transmit(PAYLOAD, MODE, None).expect("transmit");
    handle.drain_samples()
}

/// Compose a burst the way `ChannelSimHarness::route_embedded_in_capture` does: real recorded idle
/// before and after a gain-scaled frame.
fn burst(idle: &Capture, frame: &[f32], lead: usize, gain: f32) -> Vec<f32> {
    let mut buf = Vec::with_capacity(lead + frame.len() + TRAIL);
    buf.extend(idle.cycled(0, lead));
    buf.extend(frame.iter().map(|&s| s * gain));
    buf.extend(idle.cycled(lead, TRAIL));
    buf
}

/// The tightest gap between adjacent `hpx_hf` SNR floors — the scale a bias has to clear to move a
/// rung decision. Read from the profile so it tracks the ladder.
fn tightest_floor_gap() -> (f32, String) {
    let p = SessionProfile::hpx_hf();
    let levels = [
        SpeedLevel::Sl2,
        SpeedLevel::Sl3,
        SpeedLevel::Sl4,
        SpeedLevel::Sl5,
        SpeedLevel::Sl6,
        SpeedLevel::Sl7,
    ];
    let mut tightest = (f32::MAX, String::new());
    for w in levels.windows(2) {
        if let (Some(a), Some(b)) = (p.snr_floor_for_level(w[0]), p.snr_floor_for_level(w[1])) {
            let gap = b - a;
            if gap < tightest.0 {
                tightest = (gap, format!("{:?}->{:?}", w[0], w[1]));
            }
        }
    }
    tightest
}

#[test]
#[ignore = "verification"]
fn lead_in_bias_on_real_noise() {
    let idle = load_corpus("ic9700-idle-hot.wav").expect("corpus idle");
    let frame = transmitted_frame();
    let rx = engine();

    // Gain set so the frame sits ~8 dB over the recorded floor — the margin the real on-air capture
    // measured (README, `ic9700-frame-bpsk250-rs.wav`: "8.3 dB above floor"; the whitened one is
    // 7.2 dB). Note this composition puts NO noise under the frame, so the frame-span column is a
    // clean-frame reference and the bias column is the upper regime; the real-capture arm below,
    // where noise genuinely sits under the frame, is the conservative one.
    let frame_ms: f32 = frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32;
    let gain = ((idle.mean_sq() * 10f32.powf(8.3 / 10.0)) / frame_ms).sqrt();

    let (gap, which) = tightest_floor_gap();
    println!("\n#1142: rate-control SNR over the whole burst vs over the frame span");
    println!(
        "mode {MODE}, real IC-9700 idle noise, frame at ~8.3 dB over the floor, gain {gain:.3}"
    );
    println!("tightest hpx_hf floor gap: {gap:.1} dB ({which}) — the scale a bias must clear\n");
    println!(
        "{:>8} {:>10} {:>12} {:>12} {:>12} {:>9}",
        "lead", "lead frac", "whole burst", "frame+trail", "frame span", "bias dB"
    );

    for lead in LEADS {
        let b = burst(&idle, &frame, lead, gain);
        let whole = rx.rx_snr_db(MODE, &b);
        // frame+trail isolates the LEAD's contribution from the trailing noise, which is present in
        // every row including lead 0 and would otherwise be charged to the lead.
        let no_lead = rx.rx_snr_db(MODE, &b[lead..]);
        let framed = rx.rx_snr_db(MODE, &b[lead..lead + frame.len()]);
        let frac = lead as f32 / b.len() as f32;
        println!(
            "{lead:>8} {:>9.1}% {whole:>12.2} {no_lead:>12.2} {framed:>12.2} {:>9.2}",
            frac * 100.0,
            framed - whole
        );
    }
    println!("\n(bias = how much the whole-burst estimate understates the frame-span estimate)");
}

/// Locate the burst by short-time energy: the first and last 100 ms block whose mean-square is
/// 6 dB over the file's 10th-percentile block (its idle floor).
///
/// The 6 dB threshold is positive-controlled below, not robust: the weakest capture in the corpus
/// sits 7.2 dB over its floor, so a hotter-floored future capture could fall under it. If a span
/// stops matching the README's, this constant is the first thing to check.
///
/// Derived from the signal, never typed in. The corpus README documents a span for one file only;
/// hand-transcribing a span for the others would be a constant fitted to nothing, and the README's
/// own span is used below as the positive control that this detector agrees with reality.
fn detect_burst(samples: &[f32], fs: f32) -> Option<(usize, usize)> {
    let blk = (fs * 0.1) as usize;
    let ms: Vec<f32> = samples
        .chunks(blk)
        .map(|c| c.iter().map(|s| s * s).sum::<f32>() / c.len().max(1) as f32)
        .collect();
    let mut sorted = ms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = sorted[sorted.len() / 10];
    let thr = floor * 10f32.powf(6.0 / 10.0);
    let first = ms.iter().position(|&m| m > thr)?;
    let last = ms.iter().rposition(|&m| m > thr)?;
    Some((first * blk, ((last + 1) * blk).min(samples.len())))
}

#[test]
#[ignore = "verification"]
fn lead_in_bias_on_the_real_capture() {
    let files = [
        "ic9700-frame-bpsk250-rs-whitened.wav",
        "ic9700-frame-bpsk250-none-whitened.wav",
        "ic9700-frame-bpsk250-rs.wav",
        "ic9700-frame-bpsk250-none.wav",
    ];
    let rx = engine();
    println!(
        "\n#1142: same comparison on the REAL on-air captures (burst span detected by energy)"
    );
    println!(
        "positive control: the README documents ic9700-frame-bpsk250-rs-whitened at t≈10.3–18.6 s"
    );
    println!(
        "{:<40} {:>13} {:>9} {:>11} {:>11} {:>8}",
        "capture", "detected span", "lead frac", "whole file", "burst span", "bias dB"
    );
    for name in files {
        let c = load_corpus(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        let fs = c.sample_rate as f32;
        let Some((a, b)) = detect_burst(&c.samples, fs) else {
            println!("{name:<40} no burst detected");
            continue;
        };
        let whole = rx.rx_snr_db(MODE, &c.samples);
        let span = rx.rx_snr_db(MODE, &c.samples[a..b]);
        println!(
            "{name:<40} {:>6.1}-{:<6.1} {:>8.1}% {whole:>11.2} {span:>11.2} {:>8.2}",
            a as f32 / fs,
            b as f32 / fs,
            100.0 * a as f32 / c.samples.len() as f32,
            span - whole
        );
    }
    println!(
        "\n(these files are a 45 s listen, not a daemon burst — the lead fraction is far larger"
    );
    println!(" than `accumulate_capture` would produce, so read the SIGN and the mechanism, not the size)");
}
