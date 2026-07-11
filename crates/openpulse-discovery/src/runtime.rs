//! Discovery runtime orchestrator (RX-only MVP): ties the [`DiscoverySm`], [`Js8Clock`],
//! [`SlotTracker`], dwell audio buffer, JS8 [`decode_window`], and [`StationTable`] together.
//!
//! Pure and async-free — the daemon feeds it captured audio + the idle predicate and executes the
//! returned [`DiscoveryOutcome`]s (retune to the JS8 calling frequency, restore home). On each UTC
//! slot boundary while dwelling it decodes the buffered slot, upserts every heard station, and emits
//! `StationHeard`. This keeps the daemon glue (async loop, CAT retune, event plumbing) thin.

use js8_plugin::decoder::{decode_window, DecodeCfg, Js8Decode};
use js8_plugin::grammar::{parse_heartbeat, unpack_compound_frame};
use js8_plugin::submode::{params, Submode};

use crate::discovery_sm::{DiscoveryAction, DiscoveryEvent, DiscoverySm, DiscoveryState};
use crate::scheduler::{Js8Clock, SlotTracker};
use crate::station::{Observation, StationTable};

/// Runtime parameters resolved from `[discovery]` config (plan §8) plus the resolved dwell frequency.
#[derive(Debug, Clone)]
pub struct DiscoveryParams {
    /// Discovery master switch.
    pub enabled: bool,
    /// Idle predicate hold time before activating (ms).
    pub idle_grace_ms: u64,
    /// Maximum dwell before returning home (ms; 0 = until preempted).
    pub dwell_ms: u64,
    /// Station-table TTL (ms).
    pub station_ttl_ms: u64,
    /// Calling submode (MVP: NORMAL).
    pub submode: Submode,
    /// JS8 calling frequency for the current band (Hz).
    pub calling_freq_hz: u64,
}

/// Side effects the daemon executes / events it forwards.
#[derive(Debug, Clone, PartialEq)]
pub enum DiscoveryOutcome {
    /// Save the home frequency/mode and tune to this JS8 calling frequency.
    Retune { dial_freq_hz: u64 },
    /// Restore the saved home frequency/mode.
    RestoreHome,
    /// Lifecycle state changed.
    StateChanged(DiscoveryState),
    /// A station was heard (new or updated) this slot.
    StationHeard {
        /// Sender callsign.
        callsign: String,
        /// Grid if advertised.
        grid: Option<String>,
        /// Whether this was the first time we heard it.
        is_new: bool,
    },
}

/// The RX-only discovery runtime.
pub struct DiscoveryRuntime {
    params: DiscoveryParams,
    sm: DiscoverySm,
    clock: Js8Clock,
    slots: SlotTracker,
    table: StationTable,
    /// Audio accumulated since the last slot boundary (only while dwelling).
    dwell_buf: Vec<f32>,
}

impl DiscoveryRuntime {
    /// Build a runtime for `params`.
    pub fn new(params: DiscoveryParams) -> Self {
        let sm = DiscoverySm::new(params.enabled, params.idle_grace_ms, params.dwell_ms);
        let clock = Js8Clock::new(params.submode);
        Self {
            params,
            sm,
            clock,
            slots: SlotTracker::new(),
            table: StationTable::new(),
            dwell_buf: Vec::new(),
        }
    }

    /// Current lifecycle state.
    pub fn state(&self) -> DiscoveryState {
        self.sm.state()
    }

    /// The discovered-station table (the panel's source of truth).
    pub fn stations(&self) -> &StationTable {
        &self.table
    }

    /// Clock, for drift-bias updates from decode `dt`s.
    pub fn clock_mut(&mut self) -> &mut Js8Clock {
        &mut self.clock
    }

    /// Append captured audio (buffered only while dwelling).
    pub fn push_audio(&mut self, samples: &[f32]) {
        if self.sm.state() == DiscoveryState::Dwelling {
            self.dwell_buf.extend_from_slice(samples);
        }
    }

    /// Report the result of a requested retune.
    pub fn qsy_complete(&mut self, ok: bool) -> Vec<DiscoveryOutcome> {
        let actions = self.sm.step(DiscoveryEvent::QsyComplete { ok });
        self.run_actions(actions, 0)
    }

    /// An operator command needs the modem — stand down.
    pub fn preempt(&mut self) -> Vec<DiscoveryOutcome> {
        let actions = self.sm.step(DiscoveryEvent::Preempt);
        self.run_actions(actions, 0)
    }

    /// One scheduler tick: feed the idle predicate + clock state, advance slots, and (while dwelling)
    /// decode a completed slot. `now_ms` is UTC epoch millis.
    pub fn tick(&mut self, now_ms: u64, idle: bool) -> Vec<DiscoveryOutcome> {
        let clock_ok = self.clock.tx_allowed(u64::MAX); // RX gate is always open; TX skew is Phase E
        let actions = self.sm.step(DiscoveryEvent::Tick {
            idle,
            clock_ok,
            now_ms,
        });
        let mut out = self.run_actions(actions, now_ms);

        // On a UTC slot boundary while dwelling, decode the slot we just buffered.
        if self.sm.state() == DiscoveryState::Dwelling {
            if let Some(_completed) = self.slots.advance(self.clock.slot_index(now_ms)) {
                let actions = self.sm.step(DiscoveryEvent::SlotElapsed { now_ms });
                out.extend(self.run_actions(actions, now_ms));
            }
        }
        out
    }

    /// Translate SM actions into outcomes, performing the decode for `DecodeSlot`.
    fn run_actions(&mut self, actions: Vec<DiscoveryAction>, now_ms: u64) -> Vec<DiscoveryOutcome> {
        let mut out = Vec::new();
        for action in actions {
            match action {
                DiscoveryAction::SaveHomeAndTune => out.push(DiscoveryOutcome::Retune {
                    dial_freq_hz: self.params.calling_freq_hz,
                }),
                DiscoveryAction::RestoreHome => {
                    self.dwell_buf.clear();
                    out.push(DiscoveryOutcome::RestoreHome);
                }
                DiscoveryAction::StateChanged(s) => out.push(DiscoveryOutcome::StateChanged(s)),
                DiscoveryAction::DecodeSlot => out.extend(self.decode_slot(now_ms)),
            }
        }
        out
    }

    /// Decode the buffered slot, upsert every heard station, and emit `StationHeard`.
    fn decode_slot(&mut self, now_ms: u64) -> Vec<DiscoveryOutcome> {
        let buf = std::mem::take(&mut self.dwell_buf);
        let sm = params(self.params.submode);
        // Only decode once a full slot's audio is present.
        if buf.len() < sm.samples_per_period() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for d in decode_window(&buf, &sm, &DecodeCfg::default()) {
            if let Some(o) = self.ingest_decode(&d, now_ms) {
                out.push(o);
            }
        }
        self.table.sweep(now_ms, self.params.station_ttl_ms);
        out
    }

    /// Turn one JS8 decode into a station upsert (heartbeats carry callsign + grid).
    fn ingest_decode(&mut self, d: &Js8Decode, now_ms: u64) -> Option<DiscoveryOutcome> {
        let hb = unpack_compound_frame(&d.payload)
            .as_ref()
            .and_then(parse_heartbeat)?;
        let obs = Observation {
            callsign: hb.callsign.clone(),
            grid: (!hb.grid.is_empty()).then_some(hb.grid.clone()),
            // Sync-score proxy for SNR (0..21 → −21..0 dB) until a true estimate is wired; monotone.
            snr_db: d.sync_score - 21.0,
            freq_offset_hz: d.base_freq_hz,
            dial_freq_hz: self.params.calling_freq_hz,
            hint: None, // @OPULSE hint marking needs varicode free-text decode (a later unit)
        };
        let is_new = self.table.upsert(obs, now_ms);
        Some(DiscoveryOutcome::StationHeard {
            callsign: hb.callsign,
            grid: (!hb.grid.is_empty()).then_some(hb.grid),
            is_new,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use js8_plugin::costas::CostasKind;
    use js8_plugin::message::js8_info_bits;
    use js8_plugin::modulate::{modulate_tones, GfskParams};
    use js8_plugin::tones::message_to_tones;

    fn params() -> DiscoveryParams {
        DiscoveryParams {
            enabled: true,
            idle_grace_ms: 0,
            dwell_ms: 0,
            station_ttl_ms: 3_600_000,
            submode: Submode::Normal,
            calling_freq_hz: 14_078_000,
        }
    }

    /// A NORMAL slot of audio with one heartbeat (KN4CRD EM73) at 1500 Hz.
    fn heartbeat_slot() -> Vec<f32> {
        // Payload from the C-2 upstream ground-truth vector.
        let payload: [u8; 9] = [0x0a, 0x2f, 0xb3, 0xa3, 0xee, 0x2e, 0xe2, 0xea, 0x58];
        let info = js8_info_bits(&payload, 0);
        let sm = js8_plugin::submode::params(Submode::Normal);
        modulate_tones(
            &message_to_tones(&info, CostasKind::Original),
            1500.0,
            &GfskParams::from_submode(&sm),
        )
    }

    #[test]
    fn activates_dwells_hears_and_caches_a_station() {
        let mut rt = DiscoveryRuntime::new(params());
        // Idle → activate (idle_grace 0).
        let a = rt.tick(1000, true);
        assert!(a.contains(&DiscoveryOutcome::Retune {
            dial_freq_hz: 14_078_000
        }));
        assert_eq!(rt.state(), DiscoveryState::Activating);
        // Retune ok → dwell.
        rt.qsy_complete(true);
        assert_eq!(rt.state(), DiscoveryState::Dwelling);

        // Buffer a slot of audio, then cross a slot boundary → decode + cache.
        rt.push_audio(&heartbeat_slot());
        // First tick establishes the slot; the next slot advances the tracker.
        rt.tick(1000, true);
        let out = rt.tick(16_000, true); // next UTC slot
        assert!(
            out.iter().any(|o| matches!(o, DiscoveryOutcome::StationHeard { callsign, .. } if callsign == "KN4CRD")),
            "heard KN4CRD: {out:?}"
        );
        let s = rt.stations().get("KN4CRD").expect("station cached");
        assert_eq!(s.grid.as_deref(), Some("EM73"));
    }

    #[test]
    fn disabled_runtime_never_retunes() {
        let mut rt = DiscoveryRuntime::new(DiscoveryParams {
            enabled: false,
            ..params()
        });
        assert!(rt.tick(1000, true).is_empty());
        assert!(rt.tick(60_000, true).is_empty());
        assert_eq!(rt.state(), DiscoveryState::Inactive);
    }

    #[test]
    fn preempt_restores_home_and_clears_the_buffer() {
        let mut rt = DiscoveryRuntime::new(params());
        rt.tick(1000, true);
        rt.qsy_complete(true);
        rt.push_audio(&heartbeat_slot());
        let out = rt.preempt();
        assert!(out.contains(&DiscoveryOutcome::RestoreHome));
        assert_eq!(rt.state(), DiscoveryState::Inactive);
    }
}
