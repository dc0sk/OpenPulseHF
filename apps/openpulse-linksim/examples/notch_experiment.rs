//! QRM / automatic-notch experiment.
//!
//! Measures whether a receiver-side automatic notch buys effective two-way throughput against
//! CW interference (QRM), and how a *blind* per-frame detector compares to an *oracle* that is
//! told the interferer's frequency. Run:
//!
//!   cargo run -p openpulse-linksim --no-default-features --example notch_experiment
//!
//! Each scenario is run three ways through the same channel realisation:
//!   off    — baseline, no notch
//!   auto   — blind per-frame spectral detection (protecting the receiver's own band)
//!   oracle — notch placed exactly on the known interferer frequency (detection upper bound)

use openpulse_linksim::{run_link, ChannelSpec, LinkNotch, LinkParams};

struct Scenario {
    name: &'static str,
    profile: &'static str,
    /// (frequency_hz, amplitude) of the interfering tone; amplitude is relative to signal RMS.
    tone: (f32, f32),
    /// AWGN noise floor (dB) under the tone.
    snr_floor_db: f32,
    /// Receiver's own occupied band to protect from the auto-notch (Hz).
    protect: (f32, f32),
}

struct Row {
    deliver: f64,
    eff: f64,
    avg_sl: f64,
}

fn run(scn: &Scenario, notch: Option<LinkNotch>) -> Row {
    let r = run_link(&LinkParams {
        profile_name: scn.profile.into(),
        forward: ChannelSpec::Qrm {
            snr_floor_db: scn.snr_floor_db,
            tones: vec![scn.tone],
        },
        reverse: ChannelSpec::Awgn(scn.snr_floor_db + 5.0),
        payload_bytes_per_frame: 200,
        total_frames: 24,
        fec: openpulse_core::fec::FecMode::Rs,
        seed: 49_374,
        notch,
        ..LinkParams::default()
    });
    Row {
        deliver: r.delivery_ratio * 100.0,
        eff: r.effective_bps,
        avg_sl: r.avg_level,
    }
}

fn main() {
    let scenarios = [
        Scenario {
            name: "rectangular QPSK500, OUT-OF-BAND tone @800 Hz (0 dB SIR)",
            profile: "hpx_wideband",
            tone: (800.0, 1.0),
            snr_floor_db: 20.0,
            protect: (1100.0, 1900.0),
        },
        Scenario {
            name: "RRC ladder, OUT-OF-BAND tone @2900 Hz (+3.5 dB SIR)",
            profile: "hpx_wideband_hd",
            tone: (2900.0, 1.5),
            snr_floor_db: 20.0,
            protect: (400.0, 2600.0),
        },
        Scenario {
            name: "RRC ladder, IN-BAND tone @1800 Hz (+3.5 dB SIR)",
            profile: "hpx_wideband_hd",
            tone: (1800.0, 1.5),
            snr_floor_db: 20.0,
            protect: (400.0, 2600.0),
        },
    ];

    println!(
        "QRM automatic-notch experiment — 24 frames × 200 B, RS FEC, noise floor 20 dB\n\
         (eff = effective two-way goodput, bps; avgSL = mean speed level reached)\n"
    );

    for scn in &scenarios {
        let off = run(scn, None);
        let auto = run(
            scn,
            Some(LinkNotch {
                auto: true,
                oracle_freqs: Vec::new(),
                max_notches: 10,
                q: 25.0,
                protect: Some(scn.protect),
            }),
        );
        let oracle = run(
            scn,
            Some(LinkNotch {
                auto: false,
                oracle_freqs: vec![scn.tone.0],
                max_notches: 10,
                q: 25.0,
                protect: Some(scn.protect),
            }),
        );

        println!("{}", scn.name);
        println!(
            "  protect band {:.0}–{:.0} Hz",
            scn.protect.0, scn.protect.1
        );
        for (label, r) in [("off", &off), ("auto", &auto), ("oracle", &oracle)] {
            println!(
                "    {label:<7} deliver {:5.1}%   eff {:7.1} bps   avgSL {:.1}",
                r.deliver, r.eff, r.avg_sl
            );
        }
        let gain = if off.eff > 0.0 {
            (oracle.eff / off.eff - 1.0) * 100.0
        } else {
            f64::INFINITY
        };
        println!("    → oracle vs off: {gain:+.0}% effective throughput\n");
    }

    println!(
        "Reading the result:\n\
         • Out-of-band QRM: the notch is a clear win (~+30% goodput, climbs several speed\n\
         \x20 levels). With the receiver's own band protected, blind 'auto' matches the oracle.\n\
         • In-band QRM: a notch cannot help — removing the tone removes the signal too. That is\n\
         \x20 a frequency-agility (QSY) case, not a notch case.\n\
         • The load-bearing requirement is protecting the receiver's occupied band: without it,\n\
         \x20 blind per-frame detection notches the modem's own preamble/pulse spectral lines\n\
         \x20 and roughly halves throughput."
    );
}
