//! #1118 — the daemon's streaming receive path must acquire a real carrier offset.
//!
//! `REQ-PHY-03` requires the demodulator to track station-to-station offsets of up to ±50 Hz without
//! operator intervention. Before this, the shipping daemon failed at exactly 50 Hz: its burst path
//! scanned onsets but never estimated frequency, so it decoded 0 Hz and 20 Hz and nothing beyond.
//! The CLI path (`receive_with_timeout_fec`) has always acquired, which is why the gap survived —
//! every corpus capture sits at +2 to ~+12 Hz, inside BPSK250's tolerance, so no existing test could
//! see it. The measurement that exposed it is
//! `daemon_vs_cli_on_real_captures::m2_carrier_offset_sweep_cli_vs_daemon`.
//!
//! **Which arm was broken, measured rather than assumed.** The uncoded arm (`decode_burst`) already
//! tolerated 50 Hz natively; the coded arm (`ota_decode_burst`) failed from 50 Hz, which is the
//! requirement bound, so that is where the defect lived. Measured on this file's own fixture,
//! uncoded frame through the uncoded arm: 0 Hz and 50 Hz decode with **0** settles; 200 Hz and
//! 400 Hz decode with 126.
//!
//! **Corrected 2026-09-23 (#1428): this used to add "it needs the acquisition pass only past
//! ~200 Hz", an interpolation across those four points that is false.** Measured in review (Fable,
//! 2026-09-22/23) across all 64 sub-symbol alignments of this fixture: the uncancelled arm (which the
//! uncoded arm runs) decodes 40/64 at 50 Hz, 0/64 at 62.5, 75, 100 and 200 Hz, and 52/64 at 250 Hz;
//! the cancelled arm decodes 0/64 below 250 Hz and 49/64 there. #1428's own daemon sweep of this
//! fixture (six realisations) needed the acquisition pass on all six at 75–200 Hz and on three at
//! 300 Hz. A review MODEL — not a measurement — has the crossfade term rotating against the symbol's
//! own energy by ~0.4°/Hz, which would make native tolerance non-monotonic in offset. It is also
//! frame-dependent: the same review measured the preamble timing correlation at ~5 % of its peak at
//! 50 Hz, and a different 200 B frame mis-locked and failed both arms at 25 Hz. **Do not read
//! "decodes natively at X Hz" from this fixture as a property of the receiver** — the acquisition
//! pass is what REQ-PHY-03 rests on. Both arms get the
//! pass, because both are reachable on a shipping station and the uncoded one is what carries
//! station ID, filexfer, handshake, QSY and relay traffic.
//!
//! These are the gates, driven through the **production entry** (`accumulate_capture` in tick-sized
//! chunks, then `ota_decode_burst`) on audio built from a real engine-transmitted frame, shifted by
//! the shipped `CfoChannel`, embedded in **real recorded idle** so the receiver must locate the frame
//! as well as acquire it.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::engine::ModemEngine;

const MODE: &str = "BPSK250";
const FEC: FecMode = FecMode::Rs;
const PAYLOAD: &[u8] = b"REQ-PHY-03 daemon acquisition";
/// The measured lead-in of the real #1021 on-air capture — the frame does not start at sample 0.
const LEAD: usize = 4_032;
const TRAIL: usize = 1_600;
const SAMPLE_RATE: usize = 8_000;

/// REQ-PHY-03's bound. The gate asserts *at* it, not comfortably inside it: this is the number the
/// requirement names, and the daemon failed at exactly this offset before #1118.
const REQUIRED_OFFSET_HZ: f32 = 50.0;

/// An offset where NEITHER hard-decision arm decodes without the acquisition pass, so a decode here
/// is attributable to acquisition and nothing else (#1428).
///
/// The differential detector's decision variable rotates by 360°·f/250 per symbol at 250 baud:
/// 100 Hz is 144° (cos −0.81), deep in the inverted lobe, so neither arm can decode it unaided. Two
/// nearby values are avoided on purpose — 62.5 Hz is exactly 90°, the detector's own null and a
/// knife-edge; 250 Hz is 360°, where the detector decodes natively again.
///
/// Measured, from two sources: across all 64 sub-symbol alignments of this fixture both arms decode
/// 0/64 at 100 Hz (review, Fable, 2026-09-22/23); and #1428's daemon sweep spent the full acquisition
/// pass on every realisation at 100 Hz, in both the union build and a variant-0-only build.
const ACQUISITION_OFFSET_HZ: f32 = 100.0;

/// A burst as the daemon would hear it: real recorded idle, a frame shifted by `offset_hz`, more idle.
fn burst_at(offset_hz: f32) -> Vec<f32> {
    burst_at_fec(offset_hz, FEC)
}

fn burst_at_fec(offset_hz: f32, fec: FecMode) -> Vec<f32> {
    let idle = load_corpus("ic9700-idle-hot.wav").expect("corpus idle");
    let mut tx = ChannelSimHarness::new();
    tx.tx_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    tx.tx_engine
        .transmit_with_fec_mode(PAYLOAD, MODE, fec, None)
        .expect("transmit");
    let mut cfo = openpulse_channel::cfo::CfoChannel::new(openpulse_channel::cfo::CfoConfig::new(
        offset_hz,
        SAMPLE_RATE as f32,
    ))
    .expect("finite offset");
    let (_, frame) = tx.route_tapped(&mut cfo);
    let mut buf = Vec::with_capacity(LEAD + frame.len() + TRAIL);
    buf.extend(idle.cycled(0, LEAD));
    buf.extend_from_slice(&frame);
    buf.extend(idle.cycled(LEAD, TRAIL));
    buf
}

/// Drive the burst through the daemon's own entry points, and report what it cost.
///
/// Returns `(decoded, settle_attempts)`. The settle count is the tripwire: a gate that only checked
/// the decode would pass while the acquisition pass ran on every burst, which is the property the
/// two-phase design exists to avoid.
fn via_daemon(samples: &[f32]) -> (bool, u64) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    // Lock the rung whose mode IS the transmitted one — searched from the profile, not transcribed,
    // so a profile change cannot silently turn this into a test about candidate coverage.
    let profile = SessionProfile::hpx_hf();
    let level = (1u8..=20)
        .filter_map(SpeedLevel::from_u8)
        .find(|&l| profile.mode_for(l) == Some(MODE))
        .unwrap_or_else(|| panic!("hpx_hf has no rung running {MODE}"));
    e.start_ota_session(profile);
    e.ota_lock_level(level);

    let tick =
        SAMPLE_RATE * openpulse_config::DaemonConfig::default().receive_tick_ms as usize / 1_000;
    let mut bursts = Vec::new();
    for chunk in samples.chunks(tick) {
        if let Ok(Some(b)) = e.accumulate_capture(Some(MODE), chunk.to_vec()) {
            bursts.push(b);
        }
    }
    for _ in 0..8 {
        if let Ok(Some(b)) = e.accumulate_capture(Some(MODE), vec![0.0; tick]) {
            bursts.push(b);
        }
    }
    let mut ok = false;
    for b in &bursts {
        if let Ok(out) = e.ota_decode_burst(b, "gate", Some(MODE)) {
            if out.payload.as_deref().is_some_and(|p| p == PAYLOAD) {
                ok = true;
                break;
            }
        }
    }
    (ok, e.afc_settle_attempts())
}

/// The requirement itself, at the bound it names, on the surface that was failing it.
///
/// **This asserts the decode and nothing about HOW.** It used to also require `settles > 0`, as
/// proof the acquisition pass did the work. Since #1428 the union's uncancelled arm decodes THIS
/// fixture at 50 Hz with no acquisition at all (measured: 0 settles, against 198 for the cancelled
/// arm alone over six realisations), so that guard was measuring the fixture's luck rather than the
/// requirement — and per the header, that luck is frame-dependent. The mechanism is gated
/// separately, at an offset where no arm can reach natively:
/// [`the_acquisition_pass_recovers_a_station_beyond_native_reach`].
///
// VERIFIES: REQ-PHY-03
#[test]
fn the_daemon_decodes_a_station_fifty_hz_off_frequency() {
    let (ok, _settles) = via_daemon(&burst_at(REQUIRED_OFFSET_HZ));
    assert!(
        ok,
        "the daemon did not decode a frame {REQUIRED_OFFSET_HZ} Hz off frequency — REQ-PHY-03 \
         requires tracking station-to-station offsets to ±50 Hz without operator intervention, and \
         this is the streaming path a shipping station actually receives on"
    );
}

/// The acquisition pass itself — the mechanism REQ-PHY-03 actually rests on.
///
/// At [`ACQUISITION_OFFSET_HZ`] neither decision arm decodes without acquisition, so a decode here
/// can only have come from the pass. The settle assertion is the anti-vacuity guard the 50 Hz test
/// had to give up: it is meaningful here and was not there.
///
// VERIFIES: REQ-PHY-03
#[test]
fn the_acquisition_pass_recovers_a_station_beyond_native_reach() {
    let (ok, settles) = via_daemon(&burst_at(ACQUISITION_OFFSET_HZ));
    assert!(
        ok,
        "the daemon did not decode a frame {ACQUISITION_OFFSET_HZ} Hz off frequency — at this \
         offset only the acquisition pass can recover it, so the pass is broken"
    );
    assert!(
        settles > 0,
        "decoded {ACQUISITION_OFFSET_HZ} Hz off with zero settle attempts, so the acquisition pass \
         is not what recovered it. Either a decision arm now reaches this offset natively (re-measure \
         and move ACQUISITION_OFFSET_HZ), or this gate is no longer measuring what it claims"
    );
}

/// The cost property, which is the whole reason the design is two-phase rather than always-settling.
///
/// Without this, a regression that ran the acquisition pass on every burst would leave every decode
/// gate green while roughly doubling the work the receive tick does on a busy band.
///
// VERIFIES: REQ-PHY-03
#[test]
fn an_on_frequency_burst_pays_no_acquisition_cost() {
    let (ok, settles) = via_daemon(&burst_at(0.0));
    assert!(ok, "the on-frequency case must still decode");
    assert_eq!(
        settles, 0,
        "an on-frequency burst spent {settles} settle attempts — phase 1 decoded it, so phase 2 \
         must never have run. (Measured before the fallback split: 129, all of them inside the \
         uncoded fallback trying to decode a coded frame.)"
    );
}
