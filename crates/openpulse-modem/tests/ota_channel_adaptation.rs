//! Real-stack OTA adaptive rate-stepping through a SIMULATED CHANNEL.
//!
//! `ota_rate_lockstep.rs` proves the receiver-led lockstep over a CLEAN wire with
//! an injected SNR. This harness goes one layer closer to reality: two
//! `ModemEngine`s run the **real** `OtaRateController` / `RateAdapter`, bridged
//! forward (data) and reverse (ACK) through `openpulse_channel` models, and the
//! IRS derives its rate decision from the **real M2M4 SNR estimate** on the
//! faded/noisy capture (no injected SNR). This is the headless deterministic
//! validation of the explicitly-deferred hard problem — RX-side lockstep
//! adaptation under loss/fading — that `openpulse-linksim` only models.
//!
//! It exercises paths a clean-loopback test cannot:
//! - the M2M4 estimator reading a real channel SNR and driving the ladder;
//! - channel-induced ACK loss (a faded FSK4 ACK that fails CRC), not artificial;
//! - the candidate fallback + absolute recommendation recovering after a fade.

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::watterson::WattersonChannel;
use openpulse_channel::{AwgnConfig, ChannelModel, WattersonConfig};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::channel_sim::bridge_through;
use openpulse_modem::engine::ModemEngine;
use qpsk_plugin::QpskPlugin;

fn make_engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    engine.start_ota_session(SessionProfile::hpx500());
    (engine, backend)
}

/// A bidirectional OTA link over two independent channel models. The IRS uses its
/// built-in M2M4 SNR estimate (no injected SNR) — exactly what we are validating.
struct OtaLink {
    iss: ModemEngine,
    irs: ModemEngine,
    iss_lb: LoopbackBackend,
    irs_lb: LoopbackBackend,
}

/// Per-frame result of one ISS→IRS exchange.
struct FrameResult {
    decoded: bool,
    tx_level: SpeedLevel,
}

impl OtaLink {
    fn new() -> Self {
        let (iss, iss_lb) = make_engine();
        let (irs, irs_lb) = make_engine();
        Self {
            iss,
            irs,
            iss_lb,
            irs_lb,
        }
    }

    /// One data frame ISS→IRS through `fwd`, reverse ACK IRS→ISS through `rev`.
    /// `respond_arq_ota` always transmits an ACK (even a Nack on decode failure),
    /// so the reverse path is always routed and the sender always gets a chance to
    /// adopt the absolute recommendation. The IRS's recommendation is driven by the
    /// real M2M4 estimate on the post-channel capture.
    fn exchange(
        &mut self,
        payload: &[u8],
        fwd: &mut dyn ChannelModel,
        rev: &mut dyn ChannelModel,
    ) -> FrameResult {
        let tx_mode = self
            .iss
            .ota_tx_mode()
            .expect("OTA session active")
            .to_owned();
        self.iss.transmit(payload, &tx_mode, None).unwrap();
        bridge_through(&self.iss_lb, &self.irs_lb, fwd);

        let decoded = self.irs.respond_arq_ota("ota", None);
        // The ACK (Ok or Nack) is now queued on the IRS backend — route it back.
        bridge_through(&self.irs_lb, &self.iss_lb, rev);
        if let Ok(ack) = self.iss.receive_ack_with_short_fec(None) {
            self.iss.apply_ota_ack(&ack);
        }

        FrameResult {
            decoded: decoded.as_deref().ok() == Some(payload),
            tx_level: self.iss.ota_tx_level().unwrap(),
        }
    }
}

const PAYLOAD: &[u8] = b"real-stack OTA adaptation through a simulated HF channel";

/// Run `frames` exchanges, building a fresh seeded channel per direction.
fn run_awgn(frames: usize, snr_db: f32, seed: u64) -> Vec<FrameResult> {
    let mut link = OtaLink::new();
    let mut out = Vec::with_capacity(frames);
    for i in 0..frames {
        // Fresh seeded channels per frame keep each direction independent and the
        // whole run deterministic (seed varies by frame so fades are not identical).
        let mut fwd = AwgnChannel::new(AwgnConfig::new(snr_db, Some(seed + i as u64))).unwrap();
        let mut rev =
            AwgnChannel::new(AwgnConfig::new(snr_db, Some(seed + 1000 + i as u64))).unwrap();
        out.push(link.exchange(PAYLOAD, &mut fwd, &mut rev));
    }
    out
}

#[test]
fn awgn_high_snr_climbs_above_floor() {
    // High AWGN SNR: the M2M4 estimate clears the lower rungs, so the receiver-led
    // ladder climbs above the SL2 floor and the link keeps decoding.
    let results = run_awgn(16, 30.0, 42);
    let last = results.last().unwrap().tx_level;
    let decoded = results.iter().filter(|r| r.decoded).count();
    assert!(
        last > SpeedLevel::Sl2,
        "good SNR should climb above the floor; final={last:?}"
    );
    assert!(
        decoded >= 12,
        "most frames should decode on a strong channel; {decoded}/16"
    );
}

#[test]
fn awgn_low_snr_does_not_overclimb() {
    // Poor AWGN SNR: the receiver-led ladder must not run away into the dense high-throughput
    // rungs a poor channel cannot carry, and must keep delivering.
    //
    // The bar was `<= Sl4` under the SNR-only controller, which stayed pinned near the advertised
    // floors. With the evidence-based climb (#934) the ladder probes upward on decode success, and it
    // correctly discovers that a rung's *advertised* floor is conservative: those floors carry a
    // fading margin, so coded BPSK250 (SL5, floor 5 dB) actually decodes at 2 dB AWGN ~70% of the
    // time — SL5 nets ~133 effective bps there against SL4's ~87, so climbing to it is
    // throughput-positive, not overclimb. What must NOT happen is a runaway into the OFDM dense
    // section (SL7+): those genuinely cannot carry a 2 dB channel, and reaching them would mean the
    // self-correcting drop is broken. That is the real property, and it is what this now asserts.
    let results = run_awgn(24, 2.0, 7);
    let max_level = results.iter().map(|r| r.tx_level).max().unwrap();
    let decoded = results.iter().filter(|r| r.decoded).count();
    assert!(
        max_level < SpeedLevel::Sl7,
        "a poor channel must not be driven into the dense OFDM rungs (SL7+); reached {max_level:?} —          the evidence climb is not self-correcting"
    );
    assert!(
        decoded * 3 >= results.len() * 2,
        "the link must keep delivering while probing on a poor channel; {decoded}/{} decoded",
        results.len()
    );
}

#[test]
fn watterson_fading_never_desyncs_and_recovers() {
    // Moderate Watterson fading: individual frames may be lost in deep fades, but
    // the lockstep invariant means the link never desyncs — when a frame fails it
    // fails cleanly (no bad-level adoption) and subsequent frames recover. The
    // harness completing at all proves no decode ever panicked the candidate set.
    let mut link = OtaLink::new();
    let frames = 24;
    let mut decoded = 0;
    let mut last_eight_decoded = 0;
    for i in 0..frames {
        let mut fwd =
            WattersonChannel::new(WattersonConfig::moderate_f1(Some(100 + i as u64))).unwrap();
        let mut rev =
            WattersonChannel::new(WattersonConfig::moderate_f1(Some(500 + i as u64))).unwrap();
        let r = link.exchange(PAYLOAD, &mut fwd, &mut rev);
        if r.decoded {
            decoded += 1;
            if i >= frames - 8 {
                last_eight_decoded += 1;
            }
        }
        // Lockstep bound: the TX level the sender uses is always one the IRS can
        // demodulate (its candidate set covers it), so a corrupt frame can never
        // push TX above what the receiver confirmed it can decode.
        assert!(
            r.tx_level <= SpeedLevel::Sl6,
            "TX level must stay within the hpx500 ladder; got {:?}",
            r.tx_level
        );
    }
    assert!(
        decoded > 0,
        "a moderate fading channel should still deliver some frames; {decoded}/{frames}"
    );
    assert!(
        last_eight_decoded > 0,
        "the link must recover (not stay desynced) by the end of the run"
    );
}
