//! RESEARCH HARNESS — is the micro-sweep's SECOND cycle capable of anything the first was not?
//!
//! `SETTLE_FAILURE_LIMIT = 2 * SWEEP_OFFSETS` gives a settled anchor two full cycles of the
//! forward-onset sweep before condemning it, on the stated reasoning that the second cycle is "a
//! second chance against a grown buffer".
//!
//! That reasoning is in tension with the counting rule beside it. Failures are only counted once
//! `window_complete` — `accumulated.len() >= onset + arrival_samples` — and past that point the
//! decode window is `(onset, onset + max_frame_samples)`, a fixed slice of already-captured audio
//! that further arrivals cannot change. `afc_correction_hz` is restored after every failure. So if
//! attempt `k + SWEEP_OFFSETS` sees the same onset, the same window length and the same AFC state
//! as attempt `k`, it is a bit-identical repeat and cannot decode where the first failed.
//!
//! That matters well beyond tidiness: it decides whether halving the limit is safe **by
//! construction, on every capture and every channel**, or whether it is a constant fitted to
//! whichever recordings happen to exist. Fitting it to one capture is the archetype this repo has
//! been bitten by before.

use openpulse_core::fec::FecMode;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::channel_sim::ChannelSimHarness;
use std::collections::HashMap;
use std::time::Duration;

const SWEEP_OFFSETS: usize = 9;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
            .expect("register");
    }
    h
}

/// H2 is the case H1 structurally cannot reach, and the one the second cycle exists for.
///
/// `LoopbackBackend::read` with no pacing drains its whole buffer on the first call, so
/// `accumulated` is complete before the first sweep attempt and cannot grow between cycles. That is
/// exactly the regime in which the second cycle is trivially inert — H1 proves inertness only where
/// inertness was guaranteed by the fixture.
///
/// It matters because `window_complete` is measured against `arrival_samples` while the decode
/// window ends at `min(onset + max_frame_samples, accumulated.len())`, and `arrival_samples` is the
/// *raw* geometry while `max_frame_samples` is the widened slice reserve. When the former is
/// smaller, an anchor can be counted as fully buffered while its window is still growing — and then
/// attempt `k + SWEEP_OFFSETS` genuinely sees more audio than attempt `k`.
///
/// Paced delivery is what makes that reachable. If repeats differ here, the second cycle is NOT
/// inert, and halving the limit is an empirical trade rather than deleting dead work.
#[test]
#[ignore = "verification"]
fn h2_paced_delivery_can_the_window_grow_between_cycles() {
    use openpulse_audio::loopback::LoopbackBackend;
    use openpulse_modem::engine::ModemEngine;

    for (name, hz) in [
        ("ic9700-frame-bpsk250-rs-whitened.wav", 8_000.0f32),
        ("ic9700-frame-bpsk250-rs.wav", 8_000.0),
        ("sdr-ic9700tx-bpsk250-rs-1.wav", 8_000.0),
    ] {
        let Ok(c) = load_corpus(name) else {
            println!("{name}: not loadable");
            continue;
        };
        let backend = LoopbackBackend::new().with_pacing(hz);
        let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
        e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
            .expect("register");
        backend.fill_samples(&c.samples);
        let decoded = e
            .receive_with_fec_mode_timeout(
                "BPSK250",
                FecMode::Rs,
                None,
                Duration::from_millis(40_000),
            )
            .is_ok();

        let mut groups: HashMap<(usize, usize), Vec<(usize, f32)>> = HashMap::new();
        for &(k, onset, win, afc) in e.sweep_attempt_inputs() {
            groups
                .entry((k % SWEEP_OFFSETS, onset))
                .or_default()
                .push((win, afc));
        }
        let (mut repeats, mut differing, mut grew) = (0usize, 0usize, 0usize);
        for entries in groups.values() {
            for x in entries.iter().skip(1) {
                repeats += 1;
                let first = entries[0];
                if x.0 != first.0 || (x.1 - first.1).abs() > f32::EPSILON {
                    differing += 1;
                }
                if x.0 > first.0 {
                    grew += 1;
                }
            }
        }
        println!(
            "  {name:<40} attempts {:>4}  repeats {repeats:>4}  differing {differing:>4}  \
             window GREW {grew:>4}  {}",
            e.sweep_attempt_inputs().len(),
            if decoded { "decoded" } else { "" }
        );
    }
    println!("\n  'window GREW' > 0 means a later cycle saw more audio than the first, so the");
    println!("  second cycle is NOT dead work and the halving is an empirical trade.");
}

#[test]
#[ignore = "verification"]
fn h1_does_the_second_sweep_cycle_differ_from_the_first() {
    let captures = [
        "ic9700-frame-bpsk250-rs-whitened.wav",
        "ic9700-frame-bpsk250-rs.wav",
        "ic9700-frame-bpsk250-none.wav",
        "ic9700-frame-bpsk250-none-whitened.wav",
        "sdr-ic9700tx-bpsk250-rs-1.wav",
        "sdr-ic9700tx-bpsk250-rs-2.wav",
        "sdr-ic9700tx-bpsk250-rs-3.wav",
        "ic9700-idle-hot.wav",
        "ic9700-tone-1501hz.wav",
        "ft991a-idle.wav",
    ];

    println!(
        "\nH1: do repeated sweep offsets see identical inputs? (fully-buffered attempts only)"
    );
    println!(
        "\n{:<42} {:>9} {:>10} {:>12} {:>14}",
        "capture", "attempts", "repeats", "differing", "max succ idx"
    );

    let (mut total_repeats, mut total_diff) = (0usize, 0usize);
    for name in captures {
        let Ok(c) = load_corpus(name) else {
            println!("{name:<42}   (corpus not loadable)");
            continue;
        };
        let mut h = harness();
        h.feed_capture(&c);
        let decoded = h
            .rx_engine
            .receive_with_fec_mode_timeout(
                "BPSK250",
                FecMode::Rs,
                None,
                Duration::from_millis(40_000),
            )
            .is_ok();

        // Group fully-buffered attempts by (offset index within a cycle, onset). Two attempts in
        // the same group are the same sweep position revisited on a later cycle.
        let mut groups: HashMap<(usize, usize), Vec<(usize, f32)>> = HashMap::new();
        for &(k, onset, win, afc) in h.rx_engine.sweep_attempt_inputs() {
            groups
                .entry((k % SWEEP_OFFSETS, onset))
                .or_default()
                .push((win, afc));
        }
        let (mut repeats, mut differing) = (0usize, 0usize);
        for entries in groups.values() {
            for e in entries.iter().skip(1) {
                repeats += 1;
                let first = entries[0];
                if e.0 != first.0 || (e.1 - first.1).abs() > f32::EPSILON {
                    differing += 1;
                }
            }
        }
        let max_k = h
            .rx_engine
            .sweep_attempt_inputs()
            .iter()
            .map(|&(k, ..)| k)
            .max()
            .map(|v| v.to_string())
            .unwrap_or("-".into());
        total_repeats += repeats;
        total_diff += differing;
        println!(
            "{name:<42} {:>9} {repeats:>10} {differing:>12} {max_k:>14}  {}",
            h.rx_engine.sweep_attempt_inputs().len(),
            if decoded { "decoded" } else { "" }
        );
    }

    println!(
        "\n  totals: {total_repeats} repeated sweep positions, {total_diff} with differing inputs"
    );
    if total_repeats > 0 && total_diff == 0 {
        println!("  => every repeat is bit-identical: the second cycle cannot decode where the");
        println!("     first did not, so SETTLE_FAILURE_LIMIT = 2*SWEEP_OFFSETS spends half its");
        println!("     budget on provably inert work.");
    } else if total_repeats == 0 {
        println!(
            "  => NO repeats observed at all — no anchor here reached a second cycle, so this"
        );
        println!(
            "     corpus cannot answer the question. My filter found nothing; that is not the"
        );
        println!("     same as there being nothing.");
    } else {
        println!("  => repeats DO differ: the second cycle sees different inputs, the inertness");
        println!("     argument is wrong, and the limit is genuinely empirical.");
    }
}

/// H3: the positive control H2 lacked, and the measurement that decides whether cycle 2 ever helps.
///
/// H2 reported zero fully-buffered attempts under paced delivery and I read that as "my filter
/// found nothing". It was worse than that: the harness had never been shown able to produce a
/// non-zero at all, so its zero could equally have meant a pacer that silently drains in one call —
/// H1 wearing H2's name. This plants a case where the counting path MUST engage: paced delivery
/// with continuous interference before the frame, so nothing decodes early and every anchor is
/// counted.
///
/// Two questions, both open after H1/H2:
///
/// 1. Do repeated sweep offsets see a GROWN window under pacing? If yes, cycle 2's inputs really do
///    differ and halving the limit is an empirical trade, not deletion of dead work.
/// 2. Does any decode ever succeed at attempt index > SWEEP_OFFSETS? Until one is observed, "the
///    second cycle is the absorption for early arrival" is design intent, not mechanism — the
///    every-iteration re-decode against a growing buffer may be the real absorber.
#[test]
#[ignore = "verification"]
fn h3_paced_with_interference_does_cycle_two_ever_differ() {
    use openpulse_audio::loopback::LoopbackBackend;
    use openpulse_modem::engine::ModemEngine;
    use std::f32::consts::PI;

    let fs = 8_000.0f32;
    let fc = 1_500.0f32;
    let tx_backend = LoopbackBackend::new();
    let mut tx = ModemEngine::new(Box::new(tx_backend.clone_shared()));
    tx.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register");
    tx.transmit_with_fec_mode(b"paced counting probe", "BPSK250", FecMode::Rs, None)
        .expect("transmit");
    let frame = tx_backend.drain_samples();
    let frame_rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
    let amp = frame_rms / 10.0 * std::f32::consts::SQRT_2;

    println!(
        "\nH3: paced delivery WITH a pre-frame interferer, so the counting path must engage\n"
    );
    println!(
        "{:<12} {:>9} {:>9} {:>10} {:>12} {:>14} {:>9}",
        "lead-in", "paced?", "attempts", "repeats", "window GREW", "max attempt", "decoded"
    );

    for lead_s in [1.0f32, 2.0] {
        let pad = (lead_s * fs) as usize;
        let total = pad + frame.len() + (fs as usize);
        // Asymmetric comb: the veto corroborates it, so the receiver anchors and counting engages.
        let mut sig: Vec<f32> = (0..total)
            .map(|k| {
                let t = k as f32 / fs;
                amp * (2.0 * PI * (fc - 60.0) * t).cos()
                    + 0.8 * amp * (2.0 * PI * (fc + 65.0) * t).cos()
            })
            .collect();
        for (i, s) in frame.iter().enumerate() {
            sig[pad + i] += s;
        }

        let backend = LoopbackBackend::new().with_pacing(fs);
        let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
        e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
            .expect("register");
        backend.fill_samples(&sig);
        let decoded = e
            .receive_with_fec_mode_timeout(
                "BPSK250",
                FecMode::Rs,
                None,
                Duration::from_millis(30_000),
            )
            .is_ok();

        let mut groups: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for &(k, onset, win, _afc) in e.sweep_attempt_inputs() {
            groups
                .entry((k % SWEEP_OFFSETS, onset))
                .or_default()
                .push(win);
        }
        let (mut repeats, mut grew) = (0usize, 0usize);
        for wins in groups.values() {
            for w in wins.iter().skip(1) {
                repeats += 1;
                if *w > wins[0] {
                    grew += 1;
                }
            }
        }
        let attempts = e.sweep_attempt_inputs().len();
        let max_k = e
            .sweep_attempt_inputs()
            .iter()
            .map(|&(k, ..)| k)
            .max()
            .map(|v| v.to_string())
            .unwrap_or("-".into());
        println!(
            "{lead_s:<12} {:>9} {attempts:>9} {repeats:>10} {grew:>12} {max_k:>14} {decoded:>9}",
            // The pacer's own tripwire: with pacing on, a full buffer cannot arrive in one read,
            // so a non-trivial attempt count is itself evidence the pacer is pacing.
            if attempts > 0 { "yes" } else { "UNPROVEN" }
        );
    }
    println!(
        "\n  attempts > 0 is the positive control H2 never had. 'window GREW' > 0 means cycle 2"
    );
    println!(
        "  sees more audio than cycle 1, so it is not dead work. A max index <= 8 across every"
    );
    println!("  NOTE: 'max attempt' is the running attempt counter, NOT the index at which the");
    println!("  decode succeeded — this harness does not record that, so whether a success has");
    println!("  ever landed inside the SECOND cycle remains unmeasured.");
}
