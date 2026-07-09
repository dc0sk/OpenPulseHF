//! The `hpx_hf` high-rate-LDPC top rungs (SL16–SL19) must decode at the AWGN SNR they were calibrated
//! at, and must deliver the throughput that justifies their higher floors.
//!
//! `LdpcHighRate` (r ≈ 8/9) costs +4…+8 dB of floor over `SoftConcatenated` (r ≈ 0.437) and returns
//! 2.03× the rate. That is a *worse* trade than climbing one modulation order — so LDPC earns a rung
//! only above SL15, where 64QAM is already the densest constellation the plugin has and code rate is
//! the last lever left.

use ofdm_plugin::OfdmPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::engine::ModemEngine;
use scfdma_plugin::ScFdmaPlugin;

const PAYLOAD: &[u8] = b"OTA SNR floor calibration payload, sixty-four bytes total AAAA";

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for e in [&mut h.tx_engine, &mut h.rx_engine] {
        // SL10 stays SC-FDMA (narrowband); SL11+ are OFDM after the dense-rung re-seat.
        e.register_plugin(Box::new(ScFdmaPlugin::new())).unwrap();
        e.register_plugin(Box::new(OfdmPlugin::new())).unwrap();
    }
    h
}

fn decode_rate(mode: &str, fec: FecMode, snr_db: f32, frames: u32) -> f32 {
    let mut ok = 0u32;
    for f in 0..frames {
        let mut h = harness();
        h.tx_engine
            .transmit_with_fec_mode(PAYLOAD, mode, fec, None)
            .unwrap();
        let mut ch = AwgnChannel::new(AwgnConfig {
            snr_db,
            seed: Some(1000 + f as u64),
        })
        .unwrap();
        h.route(&mut ch);
        if h.rx_engine
            .receive_with_fec_mode(mode, fec, None)
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / frames as f32
}

/// Samples on air for one `AIRTIME_PAYLOAD` frame — the throughput denominator.
///
/// 213 bytes, not `PAYLOAD`'s 62: a frame is modulated in whole SC-FDMA symbols, and at 62 bytes two
/// adjacent dense rungs can quantise to the same symbol count even though their rates differ by 13 %.
fn airtime(mode: &str, fec: FecMode) -> usize {
    let b = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(b.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(psk8_plugin::Psk8Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(ScFdmaPlugin::new())).unwrap();
    e.register_plugin(Box::new(OfdmPlugin::new())).unwrap();
    let payload: Vec<u8> = (0..213u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect();
    e.transmit_with_fec_mode(&payload, mode, fec, None)
        .unwrap_or_else(|err| panic!("{mode} + {fec:?}: {err}"));
    b.drain_samples().len()
}

/// The measured AWGN floors these rungs decode at (90 % frame success, 32 frames, 1 dB grid), after
/// the SC-FDMA→OFDM re-seat — re-measured with `measure_ofdm_floors`. The conservative SC-FDMA-derived
/// profile floors (SL16–SL19 = 23/24/28/30) are retained for now: OFDM works on fading where SC-FDMA
/// did not, so those floors are a safe upper bound; tightening them to reclaim throughput is a
/// follow-up calibration. (OFDM52-16QAM+LHR is slightly easier than SC-FDMA's was, 32QAM+LHR slightly
/// harder — Fable's PAPR-clipping point on dense constellations over a clean channel.)
const MEASURED_AWGN_FLOOR_DB: [(SpeedLevel, f32); 4] = [
    (SpeedLevel::Sl16, 12.0),
    (SpeedLevel::Sl17, 16.0),
    (SpeedLevel::Sl18, 20.0),
    (SpeedLevel::Sl19, 20.0),
];

/// Probe: find the AWGN SNR at which each re-seated OFDM rung first clears 0.90 decode (32 frames),
/// to recalibrate the floors after the SC-FDMA→OFDM re-seat. Run:
///   cargo test -p openpulse-modem --no-default-features --test ldpc_ladder_rungs measure_ofdm_floors -- --ignored --nocapture
#[test]
#[ignore]
fn measure_ofdm_floors() {
    let rungs: [(&str, FecMode); 7] = [
        ("OFDM52-8PSK", FecMode::SoftConcatenated),
        ("OFDM52-16QAM", FecMode::SoftConcatenated),
        ("OFDM52-32QAM", FecMode::SoftConcatenated),
        ("OFDM52-64QAM", FecMode::SoftConcatenated),
        ("OFDM52-16QAM", FecMode::LdpcHighRate),
        ("OFDM52-32QAM", FecMode::LdpcHighRate),
        ("OFDM52-64QAM", FecMode::LdpcHighRate),
    ];
    println!("\n=== OFDM rung AWGN floors (first SNR clearing 0.90, 32 frames) ===");
    for (mode, fec) in rungs {
        let mut floor = None;
        for snr in [4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26] {
            let r = decode_rate(mode, fec, snr as f32, 32);
            if r >= 0.90 {
                floor = Some(snr);
                break;
            }
        }
        println!("{mode:<14} {fec:<18?} floor {:?} dB", floor);
    }
}

#[test]
fn ldpc_top_rungs_decode_at_their_calibrated_awgn_floor() {
    let p = SessionProfile::hpx_hf();
    for (level, measured_floor) in MEASURED_AWGN_FLOOR_DB {
        let mode = p.mode_for(level).expect("mode");
        let fec = p.fec_for(level);
        assert_eq!(
            fec,
            FecMode::LdpcHighRate,
            "{level:?} must be high-rate LDPC"
        );
        let rate = decode_rate(mode, fec, measured_floor, 16);
        assert!(
            rate >= 0.85,
            "{level:?} ({mode} + {fec:?}) decoded only {rate:.2} of frames at its calibrated \
             {measured_floor} dB AWGN floor"
        );
    }
}

/// Airtime must never *grow* as the ladder climbs, and the LDPC rungs must be decisively faster than
/// the soft-concatenated rung they sit above. That second clause is the entire claim of SL16–SL19.
///
/// Scoped to the SC-FDMA segment (SL10 upward). Below it the ladder's rate column is *gross* modem rate
/// × code rate, which airtime for a short payload does not reproduce: `Rs` and `SoftConcatenated` pad
/// every frame to a 255-byte Reed–Solomon block, so a small payload pays for redundancy it never uses.
/// (That padding is also why `LdpcHighRate` more than doubles throughput here while its code rate is
/// only 2.03× — LDPC's 128-byte blocks waste far less on a short frame.)
///
/// Adjacent rungs are allowed to *tie*: after the OFDM re-seat SL14 and SL15 (and SL18/SL19) are both
/// `OFDM52-64QAM` — the former SC-FDMA P4 dense-pilot rung folded onto plain OFDM64QAM, since OFDM's
/// cyclic prefix makes the dense-pilot delay trick unnecessary — so those pairs have *identical*
/// airtime. They are a redundant step pending a pre-release re-index (see `profile.rs`).
#[test]
fn scfdma_rungs_never_lengthen_the_air_time_and_ldpc_shortens_it_sharply() {
    let p = SessionProfile::hpx_hf();
    let rungs: Vec<SpeedLevel> = p
        .defined_levels()
        .into_iter()
        .filter(|l| *l as usize >= SpeedLevel::Sl10 as usize)
        .filter(|l| p.mode_for(*l).is_some())
        .collect();
    assert_eq!(rungs.len(), 10, "SL10–SL19");

    let air = |level: SpeedLevel| -> usize {
        airtime(p.mode_for(level).expect("mode"), p.fec_for(level))
    };

    let mut prev: Option<(SpeedLevel, usize)> = None;
    for level in rungs {
        let samples = air(level);
        if let Some((prev_level, prev_samples)) = prev {
            assert!(
                samples <= prev_samples,
                "{level:?} takes {samples} samples, more than {prev_level:?}'s {prev_samples} — \
                 climbing the ladder must never cost airtime"
            );
        }
        prev = Some((level, samples));
    }

    // The LDPC rungs' reason to exist: SL15 is the fastest soft-concatenated rung there is.
    let sl15 = air(SpeedLevel::Sl15);
    for level in [
        SpeedLevel::Sl16,
        SpeedLevel::Sl17,
        SpeedLevel::Sl18,
        SpeedLevel::Sl19,
    ] {
        let samples = air(level);
        assert!(
            samples * 4 < sl15 * 3,
            "{level:?} takes {samples} samples against SL15's {sl15} — a high-rate-LDPC rung must be \
             at least 25 % shorter than the densest soft-concatenated rung, or it is not worth the \
             +4…+8 dB of floor it costs"
        );
    }
}
