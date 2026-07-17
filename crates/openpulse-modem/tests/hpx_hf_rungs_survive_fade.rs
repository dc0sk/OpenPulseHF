//! Every `hpx_hf` rung must decode on a fading HF channel — the property the ladder exists for.
//!
//! `hpx_hf` is *the* HF profile, so fading is its design case, not an edge case. It used not to hold:
//! calibrated on AWGN, the ladder shipped four rungs that decoded ~0 % of Watterson `moderate_f1`
//! frames at any SNR (QPSK250/QPSK500 uncoded, 8PSK500+Rs, SCFDMA26-32QAM), and the uncoded BPSK
//! rungs — including SL2, the `initial_level` every session starts on — decoded ~0 % at their own
//! SNR floors. Nothing caught it because every gate measured AWGN. This one measures the fade.
//!
//! Deliberately a *weak* bar (≥ 0.25 at floor+4 dB, well under the measured values): it is a
//! dead-rung tripwire, not a floor calibration. `snr_floor_calibration.rs` is where the numbers get
//! tuned; this is what fails if a rung stops working on a fade at all.

use openpulse_channel::{watterson::WattersonChannel, WattersonConfig};
use openpulse_core::profile::SessionProfile;
use openpulse_modem::channel_sim::ChannelSimHarness;

const PAYLOAD: &[u8] = b"hpx_hf fade gate payload, sixty-four bytes in total AAAAAAAAAAAA";
const FRAMES: u32 = 12;
/// Rungs must clear this on `moderate_f1` at floor+4 dB. Deep-fade outage makes 1.0 unreachable —
/// a fraction of realisations fade the whole frame out at any SNR — so this is a "works at all" bar.
const MIN_DECODE: f32 = 0.25;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for e in [&mut h.tx_engine, &mut h.rx_engine] {
        e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
            .ok();
        e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
            .ok();
        e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
            .ok();
    }
    h
}

fn decode_rate(mode: &str, fec: openpulse_core::fec::FecMode, snr_db: f32) -> f32 {
    let mut ok = 0u32;
    for f in 0..FRAMES {
        let mut h = harness();
        if h.tx_engine
            .transmit_with_fec_mode(PAYLOAD, mode, fec, None)
            .is_err()
        {
            continue;
        }
        let mut cfg = WattersonConfig::moderate_f1(Some(8100 + f as u64));
        cfg.snr_db = snr_db;
        let Ok(mut ch) = WattersonChannel::new(cfg) else {
            continue;
        };
        h.route(&mut ch);
        if h.rx_engine
            .receive_with_fec_mode(mode, fec, None)
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / FRAMES as f32
}

/// SL2 is `initial_level` — every session starts there. **Uncoded it decoded 0.00 on `moderate_f1` at
/// 3, 6 AND 9 dB**, so a fading link could not reliably get started at all. With `Rs` it decodes 0.25
/// at its 3 dB floor and 0.50 at 6 dB — modest, but the distinction that matters is *usable under ARQ*
/// vs *never*, and this bar tests exactly that.
///
/// The bar is 0.2, not 0.5, deliberately. `RsStrong` would make this rung 1.00 and is free for
/// payloads ≤191 B — but at 192–223 B it needs a second RS block and doubles the airtime, which drops
/// the ladder's AWGN goodput through the CI floor (310 → 199 bps). The stronger code is the right
/// answer for a rung whose frames stay under 191 B, not a ladder-wide default; see `profile.rs`.
#[test]
fn entry_rung_decodes_on_a_fade() {
    let p = SessionProfile::hpx_hf();
    let level = p.initial_level;
    let mode = p.mode_for(level).expect("initial rung has a mode");
    let fec = p.fec_for(level);
    let floor = p
        .snr_floor_for_level(level)
        .expect("initial rung has a floor");
    let rate = decode_rate(mode, fec, floor);
    assert!(
        rate >= 0.2,
        "the entry rung ({mode} + {fec:?}, SL floor {floor} dB) must decode on a moderate_f1 fade \
         AT its floor — a session starts here and cannot climb off a rung that never decodes \
         (got {rate:.2})"
    );
}

/// The BPSK rungs are differentially decoded, so they ride the fade — but differential needs FEC
/// (#923's law). An uncoded rung here is the defect this ladder was re-seated to remove.
#[test]
fn no_hpx_hf_rung_is_uncoded() {
    let p = SessionProfile::hpx_hf();
    for level in p.defined_levels() {
        assert_ne!(
            p.fec_for(level),
            openpulse_core::fec::FecMode::None,
            "{level:?} ({:?}) is uncoded — on a fade an uncoded rung decodes ~0 % at its own floor",
            p.mode_for(level)
        );
    }
}

/// Every single-carrier / OFDM rung decodes on `moderate_f1`. MFSK16 (SL1) is excluded only because
/// it is ~17 s per frame and already has its own sub-floor gates; it is non-coherent and immune.
#[test]
fn every_rung_decodes_on_moderate_f1() {
    let p = SessionProfile::hpx_hf();
    let mut checked = 0;
    for level in p.defined_levels() {
        let mode = p.mode_for(level).expect("rung mode");
        if mode == "MFSK16" {
            continue; // ~17 s/frame; covered by mfsk16_engine / mfsk16_harq
        }
        // The LHR rungs are the same modes as their SC pairs at a higher code rate; their floors are
        // 23-30 dB and they are throughput rungs for good conditions. The SC pair covers the waveform.
        if p.fec_for(level) == openpulse_core::fec::FecMode::LdpcHighRate {
            continue;
        }
        let fec = p.fec_for(level);
        let floor = p.snr_floor_for_level(level).expect("rung floor");
        let rate = decode_rate(mode, fec, floor + 4.0);
        assert!(
            rate >= MIN_DECODE,
            "{level:?} ({mode} + {fec:?}) decodes {rate:.2} on moderate_f1 at floor+4 ({} dB) — \
             below the {MIN_DECODE} dead-rung bar. A rung the adapter cannot use on a fade does not \
             belong on the HF ladder.",
            floor + 4.0
        );
        checked += 1;
    }
    assert!(
        checked >= 6,
        "expected to check the ladder, checked {checked}"
    );
}
