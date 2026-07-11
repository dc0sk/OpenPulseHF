//! Discovery runtime orchestrator (RX-only MVP): ties the [`DiscoverySm`], [`Js8Clock`],
//! [`SlotTracker`], dwell audio buffer, JS8 [`decode_window`], and [`StationTable`] together.
//!
//! Pure and async-free — the daemon feeds it captured audio + the idle predicate and executes the
//! returned [`DiscoveryOutcome`]s (retune to the JS8 calling frequency, restore home). On each UTC
//! slot boundary while dwelling it decodes the buffered slot, upserts every heard station, and emits
//! `StationHeard`. This keeps the daemon glue (async loop, CAT retune, event plumbing) thin.

use std::collections::VecDeque;

use js8_plugin::beacon::{frame_audio, heartbeat, opulse_hint, BeaconFrame};
use js8_plugin::decoder::{decode_window, DecodeCfg, Js8Decode};
use js8_plugin::grammar::{parse_heartbeat, unpack_compound_frame};
use js8_plugin::submode::{params, Submode};

use crate::discovery_sm::{DiscoveryAction, DiscoveryEvent, DiscoverySm, DiscoveryState};
use crate::hint::{encode_hint, HintPayload, HINT_VERSION};
use crate::hint_assembler::HintAssembler;
use crate::scheduler::{Js8Clock, SlotTracker};
use crate::station::{Observation, OphfHint, StationTable};

/// Beacon-TX policy (plan §8). `RxOnly` transmits nothing; `Beacon`/`Full` opt into heartbeat + hint TX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TxMode {
    /// No transmission at all (the default).
    #[default]
    RxOnly,
    /// Periodic `@HB` heartbeats + `@OPULSE` capability hints.
    Beacon,
    /// Beacon plus (future) directed queries + rendezvous responder.
    Full,
}

/// The JS8 mode label used for the regulatory TX log.
fn js8_mode_label(submode: Submode) -> String {
    format!("JS8-{submode:?}").to_uppercase()
}

/// Audio-offset tolerance (Hz) for bucketing an over's frames — just under one NORMAL tone spacing.
const HINT_FREQ_TOL_HZ: f32 = 6.0;
/// Slots to keep an incomplete over before evicting it (a NORMAL beacon is ~4 frames/slots).
const HINT_MAX_OVER_SLOTS: u64 = 6;

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
    /// Beacon-TX policy (default `RxOnly` — no transmission).
    pub tx_mode: TxMode,
    /// Station callsign for beacons (empty disables TX).
    pub callsign: String,
    /// Station grid for beacons.
    pub grid: String,
    /// Capability hint to advertise; `None` = heartbeat-only.
    pub hint: Option<HintPayload>,
    /// Transmit a beacon every N slots (`N × 15 s` for NORMAL; default 8 ≈ 2 min).
    pub heartbeat_interval_slots: u64,
    /// Send the `@OPULSE` hint on every Nth beacon (0 = never; else heartbeat between).
    pub hint_interval_beacons: u64,
    /// Audio offset (Hz) to transmit beacons at.
    pub tx_offset_hz: f32,
    /// Hard TX-refusal clock-skew bound (ms); JS8's published ±2 s.
    pub max_clock_skew_ms: u64,
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
    /// Transmit this pre-built beacon-frame audio (the daemon does the final DCD check + PTT wrap and
    /// calls `engine.transmit_raw_audio`). Only emitted when `tx_mode != RxOnly` and the clock is in
    /// skew tolerance.
    TransmitBeacon {
        /// Baseband GFSK audio for one 79-symbol JS8 frame.
        audio: Vec<f32>,
        /// Regulatory-log mode label (e.g. `"JS8-NORMAL"`).
        mode: String,
    },
}

/// The RX-only discovery runtime.
pub struct DiscoveryRuntime {
    params: DiscoveryParams,
    sm: DiscoverySm,
    clock: Js8Clock,
    slots: SlotTracker,
    table: StationTable,
    /// Cross-slot assembler recognising `@OPULSE` capability beacons.
    assembler: HintAssembler,
    /// Audio accumulated since the last slot boundary (only while dwelling).
    dwell_buf: Vec<f32>,
    /// Frames of the beacon over currently being transmitted, one per slot.
    beacon_queue: VecDeque<BeaconFrame>,
    /// Slots elapsed since the last beacon started (heartbeat cadence).
    slots_since_beacon: u64,
    /// Beacons started so far (hint cadence).
    beacons_sent: u64,
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
            assembler: HintAssembler::new(HINT_FREQ_TOL_HZ, HINT_MAX_OVER_SLOTS),
            dwell_buf: Vec::new(),
            beacon_queue: VecDeque::new(),
            slots_since_beacon: 0,
            beacons_sent: 0,
        }
    }

    /// Current lifecycle state.
    pub fn state(&self) -> DiscoveryState {
        self.sm.state()
    }

    /// Enable/disable discovery at runtime (the operator `EnableDiscovery`/`DisableDiscovery` commands).
    /// Disabling while dwelling stands the machine down.
    pub fn set_enabled(&mut self, on: bool) -> Vec<DiscoveryOutcome> {
        let actions = self.sm.set_enabled(on);
        self.run_actions(actions, 0)
    }

    /// JS8 calling frequency this runtime dwells on (Hz).
    pub fn dial_freq_hz(&self) -> u64 {
        self.params.calling_freq_hz
    }

    /// Set the JS8 calling frequency to dwell on (Hz). The daemon updates this from the operator's
    /// current home band before each activation, so discovery tunes to the band-appropriate calling
    /// channel; it takes effect on the next `Retune` outcome.
    pub fn set_dial_freq_hz(&mut self, hz: u64) {
        self.params.calling_freq_hz = hz;
    }

    /// Current UTC clock drift-bias estimate (ms).
    pub fn drift_bias_ms(&self) -> i64 {
        self.clock.drift_bias_ms()
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

        // On a UTC slot boundary while dwelling: transmit a beacon frame if it's our slot, else decode
        // the slot we just buffered (half-duplex — a TX slot skips RX).
        if self.sm.state() == DiscoveryState::Dwelling {
            if let Some(_completed) = self.slots.advance(self.clock.slot_index(now_ms)) {
                if let Some(tx) = self.maybe_transmit(now_ms) {
                    self.dwell_buf.clear(); // don't decode our own transmission
                    out.push(tx);
                } else {
                    let actions = self.sm.step(DiscoveryEvent::SlotElapsed { now_ms });
                    out.extend(self.run_actions(actions, now_ms));
                }
            }
        }
        out
    }

    /// Decide whether this slot transmits a beacon frame (plan §8): only when opted into TX and the
    /// clock is within skew tolerance. Continues an in-progress over, else starts one on cadence —
    /// every `hint_interval_beacons`-th beacon is an `@OPULSE` hint, the rest are heartbeats.
    fn maybe_transmit(&mut self, now_ms: u64) -> Option<DiscoveryOutcome> {
        if self.params.tx_mode == TxMode::RxOnly || self.params.callsign.trim().is_empty() {
            return None;
        }
        // Hard TX refusal beyond the clock-skew bound (§D5) — degrade to RX-only for this slot.
        if !self.clock.tx_allowed(self.params.max_clock_skew_ms) {
            return None;
        }

        if self.beacon_queue.is_empty() {
            self.slots_since_beacon += 1;
            if self.slots_since_beacon < self.params.heartbeat_interval_slots {
                return None;
            }
            self.slots_since_beacon = 0;
            let use_hint = self.params.hint.is_some()
                && self.params.hint_interval_beacons > 0
                && self
                    .beacons_sent
                    .is_multiple_of(self.params.hint_interval_beacons);
            let frames = if use_hint {
                let text = encode_hint(self.params.hint.as_ref().unwrap(), &self.params.callsign);
                opulse_hint(&self.params.callsign, &self.params.grid, &text)
            } else {
                heartbeat(&self.params.callsign, &self.params.grid)
            };
            self.beacons_sent = self.beacons_sent.wrapping_add(1);
            self.beacon_queue = frames.into();
            let _ = now_ms;
        }

        let frame = self.beacon_queue.pop_front()?;
        let audio = frame_audio(&frame, self.params.tx_offset_hz, self.params.submode);
        Some(DiscoveryOutcome::TransmitBeacon {
            audio,
            mode: js8_mode_label(self.params.submode),
        })
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
        let slot = self.clock.slot_index(now_ms);
        let mut out = Vec::new();
        for d in decode_window(&buf, &sm, &DecodeCfg::default()) {
            // Feed every frame to the cross-slot hint assembler; a completed `@OPULSE` beacon upserts
            // the sender as an OpenPulse peer (its hint) and is reported as heard.
            if let Some(r) = self
                .assembler
                .ingest(&d.payload, d.i3bit, d.base_freq_hz, slot)
            {
                let is_new = self.table.upsert(
                    Observation {
                        callsign: r.callsign.clone(),
                        grid: r.grid.clone(),
                        snr_db: d.snr_db,
                        freq_offset_hz: r.base_freq_hz,
                        dial_freq_hz: self.params.calling_freq_hz,
                        hint: Some(OphfHint::from_payload(HINT_VERSION, &r.hint)),
                    },
                    now_ms,
                );
                out.push(DiscoveryOutcome::StationHeard {
                    callsign: r.callsign,
                    grid: r.grid,
                    is_new,
                });
            }
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
            // Decoder's matched-filter SNR estimate (dB, 2500 Hz ref BW).
            snr_db: d.snr_db,
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
            tx_mode: TxMode::RxOnly,
            callsign: String::new(),
            grid: String::new(),
            hint: None,
            heartbeat_interval_slots: 8,
            hint_interval_beacons: 3,
            tx_offset_hz: 1500.0,
            max_clock_skew_ms: 2000,
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

    fn hex9(s: &str) -> [u8; 9] {
        let mut p = [0u8; 9];
        for (i, b) in p.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
        }
        p
    }

    /// One NORMAL frame of a beacon over, modulated at 1500 Hz (payload9 + its i3bit flag).
    fn beacon_frame(hex: &str, i3bit: u8) -> Vec<f32> {
        let sm = js8_plugin::submode::params(Submode::Normal);
        let info = js8_info_bits(&hex9(hex), i3bit);
        modulate_tones(
            &message_to_tones(&info, CostasKind::Original),
            1500.0,
            &GfskParams::from_submode(&sm),
        )
    }

    #[test]
    fn recognizes_an_opulse_peer_from_a_four_slot_beacon() {
        // The four Huffman-forced frames of `DC0SK: @OPULSE OPHF1 1FAX3AIT` (Qt5 ground truth), each
        // with its transmission flag. A NORMAL over sends one frame per 15 s slot.
        let frames = [
            ("2694fa766ea662ea58", 1u8), // Compound: DC0SK EM73 (First)
            ("531a90d5639ea3f5c8", 0u8), // CompoundDirected: @OPULSE
            ("bfec6491489275029b", 0u8), // Data: "OPHF1 1FAX3A"
            ("b9afffffffffffffff", 2u8), // Data (Last): "IT"
        ];

        let mut rt = DiscoveryRuntime::new(params());
        rt.tick(1000, true);
        rt.qsy_complete(true);
        assert_eq!(rt.state(), DiscoveryState::Dwelling);

        let mut heard = Vec::new();
        let mut t = 1000u64;
        for (hex, i3) in frames {
            rt.push_audio(&beacon_frame(hex, i3));
            rt.tick(t, true); // establish this slot
            t += 15_000;
            heard.extend(rt.tick(t, true)); // cross the slot boundary → decode this frame
        }

        // The sender is cached as an OpenPulse peer with its decoded capabilities.
        let s = rt.stations().get("DC0SK").expect("peer cached");
        let hint = s.hint.expect("carries an @OPULSE hint");
        assert_eq!(hint.caps, 0xB105);
        assert_eq!(hint.pref_channel, Some(42));
        assert_eq!(s.grid.as_deref(), Some("EM73"));
        assert!(
            heard
                .iter()
                .any(|o| matches!(o, DiscoveryOutcome::StationHeard { callsign, .. } if callsign == "DC0SK")),
            "emitted StationHeard for the recognized peer: {heard:?}"
        );
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

    fn dwelling_beacon_runtime(tx_mode: TxMode, hb_slots: u64) -> DiscoveryRuntime {
        let mut p = params();
        p.tx_mode = tx_mode;
        p.callsign = "DC0SK".into();
        p.grid = "JN58".into();
        p.hint = None; // heartbeat-only
        p.heartbeat_interval_slots = hb_slots;
        let mut rt = DiscoveryRuntime::new(p);
        rt.tick(1000, true); // activate
        rt.qsy_complete(true); // dwell
        rt
    }

    #[test]
    fn beacon_mode_transmits_a_heartbeat_on_cadence() {
        let mut rt = dwelling_beacon_runtime(TxMode::Beacon, 2);
        let mut txs = Vec::new();
        let mut t = 1000u64;
        for _ in 0..4 {
            t += 15_000;
            txs.extend(rt.tick(t, true));
        }
        let beacon = txs
            .iter()
            .find(|o| matches!(o, DiscoveryOutcome::TransmitBeacon { .. }))
            .expect("beacon mode transmits on cadence");
        if let DiscoveryOutcome::TransmitBeacon { audio, mode } = beacon {
            assert_eq!(mode, "JS8-NORMAL");
            assert!(!audio.is_empty());
        }
    }

    #[test]
    fn rx_only_never_transmits() {
        let mut rt = dwelling_beacon_runtime(TxMode::RxOnly, 1);
        let mut t = 1000u64;
        for _ in 0..6 {
            t += 15_000;
            let out = rt.tick(t, true);
            assert!(
                !out.iter()
                    .any(|o| matches!(o, DiscoveryOutcome::TransmitBeacon { .. })),
                "rx_only must never transmit"
            );
        }
    }
}
