//! The `hpx_hf` high-rate-LDPC top rungs (SL16–SL19) must decode at the AWGN SNR they were calibrated
//! at, and must deliver the throughput that justifies their higher floors.
//!
//! `LdpcHighRate` (r ≈ 8/9) costs +4…+8 dB of floor over `SoftConcatenated` (r ≈ 0.437) and returns
//! 2.03× the rate. That is a *worse* trade than climbing one modulation order — so LDPC earns a rung
//! only above SL15, where 64QAM is already the densest constellation the plugin has and code rate is
//! the last lever left.

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
        e.register_plugin(Box::new(ScFdmaPlugin::new())).unwrap();
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
    let payload: Vec<u8> = (0..213u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect();
    e.transmit_with_fec_mode(&payload, mode, fec, None)
        .unwrap_or_else(|err| panic!("{mode} + {fec:?}: {err}"));
    b.drain_samples().len()
}

/// The measured AWGN floors these rungs were placed from (90 % frame success, 32 frames, 1 dB grid).
/// The profile adds the same +9 dB fading margin the SL11–SL15 rungs carry.
const MEASURED_AWGN_FLOOR_DB: [(SpeedLevel, f32); 4] = [
    (SpeedLevel::Sl16, 14.0),
    (SpeedLevel::Sl17, 15.0),
    (SpeedLevel::Sl18, 19.0),
    (SpeedLevel::Sl19, 21.0),
];

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
/// Adjacent rungs are allowed to *tie*: `SCFDMA52-64QAM-P4` carries 16 pilots to `SCFDMA52-64QAM`'s 13,
/// so its gross rate is only 6 % lower — below the resolution of a whole number of SC-FDMA symbols at
/// any frame a `u8` payload length can express. The pair earns two rungs on P4's fading robustness (its
/// denser pilot comb), exactly as SL14/SL15 do.
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
