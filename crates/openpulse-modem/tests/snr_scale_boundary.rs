//! The rate ladder's SNR scales are per-waveform-family by *physical necessity*, and this pins the
//! boundary so it can't silently drift — or get "fixed" into a regression.
//!
//! `ModemEngine::rx_snr_db` dispatches to each plugin's `estimate_snr_db`, and the plugins do not —
//! *cannot* — all report the same quantity:
//!
//! - **Single-carrier PSK** (BPSK, after #934) reports ~true additive channel SNR: it removes the
//!   multiplicative channel with a per-window gain and converts symbol-domain Es/N0 to the channel
//!   scale. So `hpx_hf`'s SL2–SL6 floors are true channel SNR and the estimate matches them.
//! - **Multicarrier** (OFDM / SC-FDMA) reports a *saturation-bounded plugin-domain* SNR. Its
//!   zero-forcing equaliser enhances noise on faded subcarriers, so the estimate flattens near
//!   ~16 dB and **physically cannot report the 20–30 dB its top rungs operate at**. `hpx_ofdm_hf` and
//!   `hpx_hf`'s OFDM rungs (SL7+) are therefore calibrated in that plugin-domain scale, deliberately.
//!
//! This is NOT a bug to unify away. Forcing OFDM onto a true-SNR scale would put the top rungs' floors
//! above anything the estimate can ever read → the ladder could never climb the SNR path to them →
//! the exact v0.14.0 "AWGN-scale floors never clear" stall. The evidence-based climb (#934) bridges
//! the gap — it climbs on decode success where the SNR estimate saturates — which is why two scales
//! coexisting is safe. **If a future change makes OFDM's estimate track true SNR, re-derive the OFDM
//! floors in the same breath or this gate fails: the two are one decision.**

use openpulse_channel::{awgn::AwgnChannel, AwgnConfig};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;

const PAYLOAD: &[u8] = b"snr scale boundary probe payload, sixty-four bytes total AAAAAA";

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for e in [&mut h.tx_engine, &mut h.rx_engine] {
        e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
            .ok();
        e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
            .ok();
    }
    h
}

/// Mean reported `rx_snr_db` at a true AWGN channel SNR.
fn reported(mode: &str, fec: FecMode, true_snr: f32, n: u32) -> f32 {
    let mut acc = 0.0f32;
    let mut cnt = 0u32;
    for f in 0..n {
        let mut h = harness();
        if h.tx_engine
            .transmit_with_fec_mode(PAYLOAD, mode, fec, None)
            .is_err()
        {
            continue;
        }
        let Ok(mut ch) = AwgnChannel::new(AwgnConfig {
            snr_db: true_snr,
            seed: Some(700 + f as u64),
        }) else {
            continue;
        };
        let (_, rx) = h.route_tapped(&mut ch);
        acc += h.rx_engine.rx_snr_db(mode, &rx);
        cnt += 1;
    }
    assert!(cnt > 0, "no frames survived for {mode}");
    acc / cnt as f32
}

/// The single-carrier side: BPSK reports ~true channel SNR (the scale its floors are written in).
#[test]
fn single_carrier_reports_true_channel_snr() {
    for true_snr in [5.0f32, 15.0, 25.0] {
        let got = reported("BPSK250", FecMode::Rs, true_snr, 5);
        assert!(
            (got - true_snr).abs() <= 4.0,
            "single-carrier BPSK must read ~true channel SNR: true {true_snr} → {got:.1}"
        );
    }
}

/// The multicarrier side: OFDM's estimate SATURATES and cannot report high true SNR. This documents
/// (as an executable fact) why the OFDM floors are plugin-domain, not true channel SNR — and guards
/// against a change that makes OFDM read true SNR without re-deriving those floors.
#[test]
fn multicarrier_snr_estimate_saturates_and_cannot_report_true() {
    let low = reported("OFDM52", FecMode::SoftConcatenated, 10.0, 5);
    let high = reported("OFDM52", FecMode::SoftConcatenated, 30.0, 5);
    // It must still MOVE with SNR at the low end (else it carries no information at all).
    assert!(
        high > low,
        "OFDM SNR estimate must be monotone at least at the low end: 10 dB → {low:.1}, 30 dB → {high:.1}"
    );
    // But it must NOT reach anywhere near true SNR at the top — it saturates well below.
    assert!(
        high <= 22.0,
        "OFDM's estimate must saturate below true SNR (30 dB → {high:.1}); if this now tracks true, \
         the OFDM ladder floors must be re-derived to the true-SNR scale in the SAME change (#934 \
         v0.14.0 stall). The two scales are one decision — see this file's header."
    );
}

/// The boundary itself: at a high true SNR the two families read on visibly different scales. This is
/// the invariant the ladder lives with — a single scalar cannot be compared across the whole ladder,
/// so each rung's floor is on its own family's scale. Pinning the divergence stops a well-meaning
/// "unify the estimators" change from erasing the boundary and breaking OFDM reachability.
#[test]
fn the_two_families_are_on_different_scales_by_necessity() {
    let bpsk = reported("BPSK250", FecMode::Rs, 30.0, 5);
    let ofdm = reported("OFDM52", FecMode::SoftConcatenated, 30.0, 5);
    assert!(
        bpsk - ofdm >= 6.0,
        "at 30 dB true SNR the single-carrier and multicarrier estimates must diverge (BPSK {bpsk:.1} \
         vs OFDM {ofdm:.1}) — they are on different scales by physical necessity (OFDM saturates), \
         which is why the ladder's floors are per-family, not on one global scale"
    );
}
