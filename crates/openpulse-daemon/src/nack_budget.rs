//! The receiver's keyed-NACK budget (#1456).
//!
//! Caps a keyed NACK storm — two OTA-active ends answering each other's ACK or QRM bursts — and a
//! §97 babbling transmitter on repetitive co-channel QRM. Before #1456 the count reset only on a
//! decode, so three failed bursts during idle muted the receiver until the next decode: the peer's
//! first failed frame got silence and its ISS abandoned after two silent windows. The count now also
//! leaks one per `LEAK_SAMPLES` of listening time.

use openpulse_core::error::ModemError;
use openpulse_core::ota_rate::NackVerdict;
use openpulse_modem::pipeline::AudioSamples;
use openpulse_modem::{ModemEngine, OtaRxResult};

/// Keyed NACKs allowed in a row with no decode and no leak.
pub(crate) const OTA_NACK_BUDGET: u32 = 3;

/// Listening time per leaked NACK: 10 min at 8 kHz. Bounds idle keying at 3 + 6 per hour.
pub(crate) const LEAK_SAMPLES: u64 = 10 * 60 * 8_000;

/// NACKs keyed since the last decode, leaking with listening time.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct NackBudget {
    failures: u32,
    leaked_to: u64,
}

impl NackBudget {
    /// What [`on_ladder_burst`](Self::on_ladder_burst) would do with a failure heard at `listening`,
    /// without changing anything. Known before the decode, because the decode does not advance the
    /// listening clock; the engine uses it so a muted failure moves no ladder (#1460 round 4b).
    pub(crate) fn failure_verdict(&self, listening: u64) -> NackVerdict {
        let leaks = listening.saturating_sub(self.leaked_to) / LEAK_SAMPLES;
        let after_leak = self
            .failures
            .saturating_sub(u32::try_from(leaks).unwrap_or(u32::MAX));
        if after_leak >= OTA_NACK_BUDGET {
            NackVerdict::Muted
        } else if self.failures >= OTA_NACK_BUDGET {
            NackVerdict::Leaked
        } else {
            NackVerdict::Keyed
        }
    }

    /// Account one ladder burst heard at `listening` samples; true when its ACK may be keyed.
    pub(crate) fn on_ladder_burst(&mut self, decoded: bool, listening: u64) -> bool {
        let leaks = listening.saturating_sub(self.leaked_to) / LEAK_SAMPLES;
        self.failures = self
            .failures
            .saturating_sub(u32::try_from(leaks).unwrap_or(u32::MAX));
        self.leaked_to += leaks * LEAK_SAMPLES;
        if decoded {
            self.failures = 0;
            self.leaked_to = listening;
            return true;
        }
        if self.failures == 0 {
            self.leaked_to = listening;
        }
        // A failure heard while muted is not added: it keyed nothing, and a debt that outlasts the
        // leak would let steady QRM mute the receiver for good, which is the defect this fixes.
        if self.failures >= OTA_NACK_BUDGET {
            return false;
        }
        self.failures += 1;
        true
    }
}

/// One OTA burst through the receiver's ladder accounting, shared by `server::run`'s rx tick and the
/// #1460 probes so they cannot drift apart. The budget's verdict reaches the engine before the decode
/// (a station without a valid callsign keys nothing, so its failures are muted); the budget is
/// charged after it, except for a failure the preamble detect confirmed as a frame, which is keyed
/// and not charged. Returns the decode and whether its ACK may be keyed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_ota_burst(
    engine: &mut ModemEngine,
    budget: &mut NackBudget,
    callsign_valid: bool,
    burst: &AudioSamples,
    session_id: &str,
    mode: &str,
    channel_busy: bool,
) -> Result<(OtaRxResult, bool), ModemError> {
    // `channel_busy`: the channel still carried a signal right after this burst flushed, so the
    // sender is still transmitting and the burst is a piece of its frame. A NACK keyed now would go
    // out over that frame, unheard: measured on air (task 15), station B flushed 56.5 s of a 66.6 s
    // frame and keyed a NACK over the rest. Muted, so the failure neither keys nor moves the ladder
    // (#1460) and the budget is left alone. A decode is still ACKed.
    //
    // The budget's verdict goes in as the budget's: a failed burst the preamble detect confirmed as a
    // frame is exempt from it (`ofdm-preamble-detect.md`, "The budget exemption"). On air (f2h) three
    // NACKs spent the budget and A then muted 54 decodable-frame failures for the 10-min leak.
    if callsign_valid && !channel_busy {
        engine.set_ota_budget_verdict(budget.failure_verdict(engine.listening_samples()));
    } else {
        engine.set_ota_failure_verdict(NackVerdict::Muted);
    }
    // `mode` is this station's ACTIVE mode: what its own non-ladder traffic (station ID, filexfer,
    // handshake, QSY, relay) is sent at. Passing it enables the uncoded fallback for exactly that
    // traffic (#1123); without it an OTA-enabled daemon cannot receive any of it.
    let res = engine.ota_decode_burst(burst, session_id, Some(mode))?;
    if channel_busy && res.payload.is_none() {
        tracing::info!(
            "OTA: a burst failed while the channel was still busy; no NACK keyed, the ladder holds"
        );
        return Ok((res, false));
    }
    // A ladder frame resets the budget and is always ACKed; a failed decode is a NACK, keyed only
    // while the leaking budget allows. A `None` ack is non-ladder traffic the uncoded fallback
    // recovered: no evidence about the ladder either way, so it leaves the budget alone (#1123).
    // A P-confirmed failure is keyed and not charged: it neither spends the budget nor resets it.
    let exempt = res.payload.is_none() && res.p_evidence;
    let within_budget = res.ack.is_some()
        && (exempt || budget.on_ladder_burst(res.payload.is_some(), engine.listening_samples()));
    // Audit F6 (§97.119): without a valid MYID the daemon cannot auto-ID, so it keys no ACK.
    Ok((res, within_budget && callsign_valid))
}

// Declared here, not in `lib.rs`: `ptt_keys_every_daemon_transmit` scans `lib.rs` only up to its first
// `#[cfg(test)]`, so a test module declared near its top hides the production code from that scan.
#[cfg(test)]
#[path = "ladder_evidence_probe.rs"]
mod ladder_evidence_probe;

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed burst flushed while the channel is still busy keys no NACK and spends no budget; the
    /// same burst on a quiet channel keys one (task 15: station B NACKed over the frame it was
    /// receiving). Fails if `channel_busy` is ignored.
    #[test]
    fn a_failure_while_the_channel_is_busy_keys_nothing_and_spends_no_budget() {
        let mut engine = ModemEngine::new(Box::new(openpulse_audio::LoopbackBackend::new()));
        engine
            .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
            .unwrap();
        engine
            .register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
            .unwrap();
        engine.start_ota_session(openpulse_core::profile::SessionProfile::robust());
        // Deterministic noise: no frame in it, so every decode fails.
        let mut state = 0x2545_f491_u64;
        let noise: Vec<f32> = (0..16_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                ((state >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.2
            })
            .collect();
        let burst = AudioSamples { samples: noise };
        let mut budget = NackBudget::default();

        let (res, keyed) = decode_ota_burst(
            &mut engine,
            &mut budget,
            true,
            &burst,
            "peer",
            "BPSK250",
            true,
        )
        .unwrap();
        assert!(res.payload.is_none(), "noise must not decode");
        assert!(
            !keyed,
            "a NACK keyed while the channel is busy goes out over the sender's frame"
        );

        // The budget is untouched: the three NACKs it allows are all still there.
        let keyed: Vec<bool> = (0..4)
            .map(|_| {
                decode_ota_burst(
                    &mut engine,
                    &mut budget,
                    true,
                    &burst,
                    "peer",
                    "BPSK250",
                    false,
                )
                .unwrap()
                .1
            })
            .collect();
        assert_eq!(keyed, [true, true, true, false]);
    }

    /// An engine on the `fast` profile locked at SL7, so the preamble detect runs.
    fn sl7_engine() -> ModemEngine {
        let mut engine = ModemEngine::new(Box::new(openpulse_audio::LoopbackBackend::new()));
        engine
            .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
            .unwrap();
        engine
            .register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
            .unwrap();
        engine
            .register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
            .unwrap();
        engine.start_ota_session(openpulse_core::profile::SessionProfile::fast());
        engine.ota_lock_level(openpulse_core::rate::SpeedLevel::Sl7);
        engine
    }

    /// Deterministic noise, `n` samples at peak `amp`: no frame in it.
    fn noise(n: usize, amp: f32, mut state: u64) -> Vec<f32> {
        (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                ((state >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * amp
            })
            .collect()
    }

    /// 12 s of quiet noise, then a genuine SL7 frame cut after ten data symbols, then 4 s of quiet:
    /// the preamble detect confirms the frame (its `acquire` coherence clears the evidence threshold)
    /// and the cut frame fails to decode. Fed through the accumulator so the flushed burst carries
    /// P's flags; returns that burst.
    fn p_confirmed_failure(engine: &mut ModemEngine) -> AudioSamples {
        let lb = openpulse_audio::LoopbackBackend::new();
        let mut tx = ModemEngine::new(Box::new(lb.clone_shared()));
        tx.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
            .unwrap();
        tx.transmit_with_fec_mode(
            &[7u8; 64],
            "OFDM52",
            openpulse_core::fec::FecMode::SoftConcatenated,
            None,
        )
        .unwrap();
        let frame = lb.drain_samples();
        let fragment = &frame[..11 * 288];
        let mut x = noise(8_000 * 12, 0.02, 0x1b87_3593);
        let tail = noise(8_000 * 4 + fragment.len(), 0.02, 0x9e37_79b9);
        x.extend(fragment.iter().zip(&tail).map(|(v, n)| v * 0.5 + n));
        x.extend_from_slice(&tail[fragment.len()..]);
        let mut bursts = Vec::new();
        for chunk in x.chunks(800) {
            if let Ok(Some(b)) = engine.accumulate_capture(Some("OFDM52"), chunk.to_vec()) {
                bursts.push(b);
            }
        }
        assert_eq!(bursts.len(), 1, "the fixture must flush exactly one burst");
        bursts.remove(0)
    }

    /// The verdict the engine reported for the last OTA decision.
    fn last_verdict(
        rx: &mut tokio::sync::broadcast::Receiver<openpulse_modem::EngineEvent>,
    ) -> Option<NackVerdict> {
        let mut last = None;
        while let Ok(ev) = rx.try_recv() {
            if let openpulse_modem::EngineEvent::OtaRateDecision { verdict, .. } = ev {
                last = Some(verdict);
            }
        }
        last
    }

    /// The budget exemption (`ofdm-preamble-detect.md`, Implementation): with the budget spent, a
    /// failed burst the preamble detect confirmed as a frame is keyed, moves the ladder as a keyed
    /// NACK, and is not charged; a failure without P's evidence is still charged and muted. On air
    /// (f2h) three NACKs spent the budget and A muted 54 decodable-frame failures for 10 min.
    /// Sabotage: removing the exemption turns it red.
    #[test]
    fn a_p_confirmed_failure_keys_past_the_budget_and_is_not_charged() {
        let mut engine = sl7_engine();
        let mut rx = engine.subscribe();
        let burst = AudioSamples {
            samples: noise(16_000, 0.2, 0x2545_f491),
        };
        let mut budget = NackBudget::default();
        let decode = |engine: &mut ModemEngine, budget: &mut NackBudget, b: &AudioSamples, busy| {
            decode_ota_burst(engine, budget, true, b, "peer", "BPSK250", busy).unwrap()
        };
        let keyed: Vec<bool> = (0..4)
            .map(|_| decode(&mut engine, &mut budget, &burst, false).1)
            .collect();
        assert_eq!(
            keyed,
            [true, true, true, false],
            "the budget must be spent first"
        );
        assert_eq!(last_verdict(&mut rx), Some(NackVerdict::Muted));

        let p_burst = p_confirmed_failure(&mut engine);
        let before = budget.clone();
        let (res, keyed) = decode(&mut engine, &mut budget, &p_burst, false);
        assert!(res.payload.is_none(), "the cut frame must not decode");
        assert!(
            res.p_evidence,
            "the fixture is not a P-confirmed failure: vacuous"
        );
        assert!(
            keyed,
            "a P-confirmed failure was muted by a spent NACK budget"
        );
        assert_eq!(
            last_verdict(&mut rx),
            Some(NackVerdict::Keyed),
            "the engine moved the ladder as for a muted failure"
        );
        assert_eq!(
            budget, before,
            "a P-confirmed failure was charged to the budget"
        );

        // A failure without P's evidence is still muted: the exemption reset nothing.
        let (res, keyed) = decode(&mut engine, &mut budget, &burst, false);
        assert!(
            !res.p_evidence && !keyed,
            "a noise failure keyed past the spent budget"
        );
        assert_eq!(last_verdict(&mut rx), Some(NackVerdict::Muted));

        // The busy-channel mute stands for a P-confirmed failure too.
        let p_burst = p_confirmed_failure(&mut engine);
        let (res, keyed) = decode(&mut engine, &mut budget, &p_burst, true);
        assert!(res.p_evidence, "second fixture is not P-confirmed: vacuous");
        assert!(
            !keyed,
            "a P-confirmed failure keyed while the channel was still busy"
        );
        assert_eq!(
            last_verdict(&mut rx),
            Some(NackVerdict::Muted),
            "the exemption overrode the busy-channel mute"
        );
    }

    #[test]
    fn three_failures_key_and_the_fourth_does_not() {
        let mut b = NackBudget::default();
        let keyed: Vec<bool> = (0..4)
            .map(|i| b.on_ladder_burst(false, i * 8_000))
            .collect();
        assert_eq!(keyed, [true, true, true, false]);
    }

    #[test]
    fn a_decode_resets_the_count() {
        let mut b = NackBudget::default();
        for i in 0..4 {
            b.on_ladder_burst(false, i);
        }
        assert!(b.on_ladder_burst(true, 10));
        assert!(b.on_ladder_burst(false, 11));
    }

    #[test]
    fn listening_time_leaks_one_failure_per_period() {
        let mut b = NackBudget::default();
        for i in 0..4 {
            b.on_ladder_burst(false, i);
        }
        assert!(
            !b.on_ladder_burst(false, LEAK_SAMPLES - 1),
            "not yet leaked"
        );
        assert!(
            b.on_ladder_burst(false, LEAK_SAMPLES + 4),
            "one leak ends the mute"
        );
        assert!(
            !b.on_ladder_burst(false, LEAK_SAMPLES + 5),
            "and allows one NACK"
        );
    }

    #[test]
    fn the_verdict_agrees_with_the_budget_and_changes_nothing() {
        // Failures every 10 s for an hour, then a decode, then more failures: every verdict must
        // match what the budget then does, and reading it must not move the budget.
        let mut b = NackBudget::default();
        let mut leaked = 0;
        let times = (0..6 * LEAK_SAMPLES).step_by(8_000 * 10);
        for (i, t) in times.enumerate() {
            let decoded = i == 200;
            let before = b.clone();
            let verdict = b.failure_verdict(t);
            assert_eq!(b, before, "reading the verdict changed the budget");
            let keyed = b.on_ladder_burst(decoded, t);
            if decoded {
                continue;
            }
            assert_eq!(verdict != NackVerdict::Muted, keyed, "burst {i} at {t}");
            leaked += usize::from(verdict == NackVerdict::Leaked);
        }
        assert!(
            leaked >= 4,
            "the run must exercise the leak ({leaked} leaked)"
        );
    }

    #[test]
    fn idle_keying_is_bounded_by_the_leak() {
        let mut b = NackBudget::default();
        let hour = 6 * LEAK_SAMPLES;
        let keyed = (0..hour)
            .step_by(8_000 * 10)
            .filter(|&t| b.on_ladder_burst(false, t))
            .count();
        assert!(
            keyed <= 3 + 6,
            "{keyed} NACKs keyed in an hour of failures every 10 s"
        );
    }
}
