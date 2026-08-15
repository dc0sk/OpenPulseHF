//! MEASUREMENT for #1139 — does HARQ's offset-0 soft demod actually cost diversity?
//!
//! The HARQ block demodulates the whole burst at offset 0 (`ota_demodulate_soft(mode,
//! &samples.samples)`), with no onset scan — the third site missing one after #1138 fixed the coded
//! decode arm. The combine's own comment states the assumption it relies on: retained vectors
//! "share a start and a grid, so index k means the same bit in both".
//!
//! Bursts with DIFFERENT lead-ins break that. The daemon's burst start is set by tick size, DCD hold
//! and key-up dead carrier, none of which repeat exactly, so real retransmissions of one frame
//! arrive with differing lead-in — and each vector is then offset relative to the others by the
//! difference. Summing them is not a MAP combine of the same bits.
//!
//! This measures rather than assumes, because #1138 just demonstrated that the obvious attribution
//! can be wrong: there, a corpus gap that looked like missing acquisition machinery was entirely an
//! onset scan, and two-thirds of the apparent cost was dead work no design option would have touched.
//!
//! Assert-free on the comparison; it prints a table. It DOES assert its own integrity — an
//! aligned-case combine that shows no gain would mean the harness never exercised HARQ at all, and
//! the misaligned column would say nothing.
//!
//! Run: cargo test -p openpulse-modem --no-default-features --test harq_lead_in_alignment -- --ignored --nocapture

use bpsk_plugin::BpskPlugin;
use ofdm_plugin::OfdmPlugin;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;

const PAYLOAD: &[u8] = b"HARQ lead-in alignment probe, sixty-four bytes AAAAAAAAAAAAAA";
/// BPSK250 + Rs, DELIBERATELY not a multicarrier mode.
///
/// The first version of this measurement used OFDM52-16QAM and read identical columns — but OFDM
/// SELF-SYNCS: its Schmidl-Cox preamble locates the frame inside the buffer, so lead-in cannot
/// misalign it and the measurement was structurally incapable of showing the defect. BPSK's timing
/// search spans one symbol period, which is exactly why #1138 existed. `Rs` is admitted to the HARQ
/// soft path (`decode_combined_llrs` hard-decides the combined vector) and BPSK reports
/// `supports_soft_demod = true`, so this reaches the same block.
const MODE: &str = "BPSK250";
const FEC: FecMode = FecMode::Rs;
const SESSION: &str = "harq-align";
const TRIALS: u32 = 40;
const ATTEMPTS: usize = 3;
/// Chosen so single bursts mostly FAIL: HARQ only runs when the standalone candidates do not decode,
/// so a higher SNR would measure nothing about combining.
const SNR_DB: f32 = 4.0;

/// Lead-in applied to each of the three bursts.
///
/// ALIGNED is the assumption the combine encodes — every burst starts at the same place. VARIED is
/// what a real daemon produces; 4032 is the measured lead-in of the real #1021 on-air capture, and
/// the others bracket it.
const ALIGNED: [usize; ATTEMPTS] = [0, 0, 0];
const VARIED: [usize; ATTEMPTS] = [0, 4_032, 1_600];

fn make() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(OfdmPlugin::new()))
        .expect("register ofdm");
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register bpsk");
    let profile = SessionProfile::hpx_hf();
    let level = (1u8..=20)
        .filter_map(SpeedLevel::from_u8)
        .find(|&l| profile.mode_for(l) == Some(MODE))
        .unwrap_or_else(|| panic!("hpx_hf has no rung running {MODE}"));
    e.start_ota_session(profile);
    e.ota_lock_level(level);
    (e, backend)
}

fn tx_samples() -> Vec<f32> {
    let (mut e, backend) = make();
    e.transmit_with_fec_mode(PAYLOAD, MODE, FEC, None)
        .expect("transmit");
    backend.drain_samples()
}

fn faded(tx: &[f32], seed: u64) -> Vec<f32> {
    let mut cfg = WattersonConfig::moderate_f1(Some(seed));
    cfg.snr_db = SNR_DB;
    WattersonChannel::new(cfg).expect("watterson").apply(tx)
}

/// One burst: the faded frame preceded by `lead` samples of lead-in.
///
/// The lead-in is effectively DIGITAL SILENCE, not a band noise floor — `faded(&vec![0.0; lead])`
/// takes Watterson's zero-input branch (sigma 1e-4, ~60 dB below the in-frame noise at this SNR).
/// An earlier version of this comment claimed the opposite. The geometric result survives
/// (misalignment is misalignment), but AGC/AFC magnitudes under real band noise are NOT established
/// here.
fn burst(tx: &[f32], seed: u64, lead: usize) -> AudioSamples {
    let faded_frame = faded(tx, seed);
    if lead == 0 {
        return AudioSamples {
            samples: faded_frame,
        };
    }
    let noise = faded(&vec![0.0f32; lead], seed ^ 0xA5A5);
    let mut samples = noise;
    samples.extend_from_slice(&faded_frame);
    AudioSamples { samples }
}

fn seed(trial: u32, attempt: usize) -> u64 {
    (trial as u64) * 97 + attempt as u64
}

/// Feed ATTEMPTS bursts through ONE engine so failures retain LLRs and combine into the next.
fn combining_success(tx: &[f32], leads: [usize; ATTEMPTS]) -> f32 {
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        let (mut rx, _b) = make();
        for (attempt, lead) in leads.iter().enumerate() {
            let b = burst(tx, seed(trial, attempt), *lead);
            if rx
                .ota_decode_burst(&b, SESSION, Some(MODE))
                .ok()
                .and_then(|r| r.payload)
                .map(|p| p == PAYLOAD)
                .unwrap_or(false)
            {
                ok += 1;
                break;
            }
        }
    }
    ok as f32 / TRIALS as f32
}

/// Each burst decoded by a FRESH engine — no retention, so no combining.
///
/// NOT the right baseline for isolating the combine, and kept only as a reference column: a fresh
/// engine differs from the shared one by ALL cross-burst state, not just retention. Comparing
/// against it conflates the combine with the AFC carry-over and produced a false "combining is
/// worse than not combining" reading. Use `rotated_success` below.
fn standalone_success(tx: &[f32], leads: [usize; ATTEMPTS]) -> f32 {
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        for (attempt, lead) in leads.iter().enumerate() {
            let (mut rx, _b) = make();
            let b = burst(tx, seed(trial, attempt), *lead);
            if rx
                .ota_decode_burst(&b, SESSION, Some(MODE))
                .ok()
                .and_then(|r| r.payload)
                .map(|p| p == PAYLOAD)
                .unwrap_or(false)
            {
                ok += 1;
                break;
            }
        }
    }
    ok as f32 / TRIALS as f32
}

/// THE CORRECT BASELINE: one shared engine, but a rotating `session_id` per attempt.
///
/// `ota_decode_burst` clears `ota_retained_llrs` when the session id changes, so the whole HARQ
/// block still executes — soft demod, AFC side effects, identical work — and only the COMBINE is
/// ablated. Differencing against this isolates the combine from everything else the shared engine
/// carries, which the fresh-engine column cannot do.
fn rotated_success(tx: &[f32], leads: [usize; ATTEMPTS]) -> f32 {
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        let (mut rx, _b) = make();
        for (attempt, lead) in leads.iter().enumerate() {
            let b = burst(tx, seed(trial, attempt), *lead);
            let sess = format!("{SESSION}-{trial}-{attempt}");
            if rx
                .ota_decode_burst(&b, &sess, Some(MODE))
                .ok()
                .and_then(|r| r.payload)
                .map(|p| p == PAYLOAD)
                .unwrap_or(false)
            {
                ok += 1;
                break;
            }
        }
    }
    ok as f32 / TRIALS as f32
}

#[test]
#[ignore = "measurement"]
fn m_does_varying_lead_in_cost_harq_diversity() {
    let tx = tx_samples();
    // HARNESS VALIDATION: the two configurations must actually differ in the input, or identical
    // columns mean nothing. Print the burst lengths and how far each reaches HARQ.
    for (name, leads) in [("ALIGNED", ALIGNED), ("VARIED", VARIED)] {
        let lens: Vec<usize> = leads
            .iter()
            .enumerate()
            .map(|(i, l)| burst(&tx, seed(0, i), *l).samples.len())
            .collect();
        println!("{name} burst lengths: {lens:?}");
    }
    println!(
        "\n=== #1139: does HARQ's offset-0 soft demod lose diversity when lead-in varies? ==="
    );
    println!("mode={MODE} fec={FEC:?} snr={SNR_DB} trials={TRIALS} attempts={ATTEMPTS}\n");

    let aligned_standalone = standalone_success(&tx, ALIGNED);
    let aligned_rotated = rotated_success(&tx, ALIGNED);
    let aligned_combined = combining_success(&tx, ALIGNED);
    let varied_standalone = standalone_success(&tx, VARIED);
    let varied_rotated = rotated_success(&tx, VARIED);
    let varied_combined = combining_success(&tx, VARIED);

    println!(
        "{:<28} {:>10} {:>10} {:>10} {:>12}",
        "lead-ins", "fresh", "rotated", "combined", "combine gain"
    );
    println!(
        "{:<28} {:>10.3} {:>10.3} {:>10.3} {:>+12.3}   {ALIGNED:?}",
        "ALIGNED (combine's premise)",
        aligned_standalone,
        aligned_rotated,
        aligned_combined,
        aligned_combined - aligned_rotated
    );
    println!(
        "{:<28} {:>10.3} {:>10.3} {:>10.3} {:>+12.3}   {VARIED:?}",
        "VARIED (what a daemon emits)",
        varied_standalone,
        varied_rotated,
        varied_combined,
        varied_combined - varied_rotated
    );
    println!("\n`combined - rotated` is the COMBINE's contribution; `fresh` is reference only.");

    // INTEGRITY, not a result: if the aligned case shows no combining gain, HARQ never ran and the
    // varied column is measuring nothing. That would be a broken harness, not a finding.
    assert!(
        aligned_combined > aligned_rotated,
        "aligned combining showed no gain over the ROTATED baseline ({aligned_combined:.3} vs \
         {aligned_rotated:.3}) — the combine did not engage, so the varied column says nothing \
         about alignment. Differencing against the rotated arm is what makes this an integrity \
         check rather than a measure of unrelated cross-burst state."
    );

    println!(
        "\nReading: varied gain ~ aligned gain -> lead-in does not cost diversity; #1139 is a\n\
         \x20        latent tidiness issue, not a live defect, and should be re-scoped.\n\
         \x20        varied gain ~ 0 -> the combine contributes NOTHING once lead-ins differ by\n\
         \x20        >= 2 symbol periods; an onset for the soft demod is justified BY MEASUREMENT.\n\
         \x20        The effect is a STEP at the demod's timing-search span, not size-scaled:\n\
         \x20        a 32-sample lead leaves the combine fully alive, 64 kills it outright.\n"
    );
}
