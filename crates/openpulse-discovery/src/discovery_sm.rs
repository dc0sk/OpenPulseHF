//! Discovery state machine (plan §4.3), RX-only MVP shape: `INACTIVE → ACTIVATING → DWELLING`.
//!
//! Pure — the daemon feeds [`DiscoveryEvent`]s (the assembled idle predicate, the QSY result, slot
//! boundaries, operator preemption) and executes the returned [`DiscoveryAction`]s (save-home-and-tune,
//! restore-home, decode-the-slot). No TX paths here: the RENDEZVOUS branch and slot TX are later phases.

/// Where discovery is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryState {
    /// Normal operation — not dwelling on the JS8 channel.
    Inactive,
    /// Idle predicate held; saving home frequency and tuning to the JS8 calling frequency.
    Activating,
    /// Parked on the JS8 channel, decoding each slot (RX-only).
    Dwelling,
}

/// Inputs the daemon feeds the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryEvent {
    /// Per-tick idle predicate + clock-skew-ok, with the current time.
    Tick {
        idle: bool,
        clock_ok: bool,
        now_ms: u64,
    },
    /// Result of the QSY to the JS8 calling frequency.
    QsyComplete { ok: bool },
    /// A dwell slot boundary was crossed (from `SlotTracker`).
    SlotElapsed { now_ms: u64 },
    /// An operator command needs the modem (restore home and stand down).
    Preempt,
}

/// Side effects the daemon executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryAction {
    /// Save the current (home) frequency/mode and tune to the JS8 calling frequency.
    SaveHomeAndTune,
    /// Restore the saved home frequency/mode.
    RestoreHome,
    /// Decode the just-completed dwell window and upsert stations.
    DecodeSlot,
    /// The lifecycle state changed (emit a status event).
    StateChanged(DiscoveryState),
}

/// The RX-only discovery state machine.
#[derive(Debug, Clone)]
pub struct DiscoverySm {
    state: DiscoveryState,
    enabled: bool,
    idle_grace_ms: u64,
    /// Maximum dwell before returning home; `0` = until preempted.
    dwell_ms: u64,
    idle_since_ms: Option<u64>,
    dwell_deadline_ms: Option<u64>,
}

impl DiscoverySm {
    /// A machine in `Inactive`. `dwell_ms == 0` dwells until preempted.
    pub fn new(enabled: bool, idle_grace_ms: u64, dwell_ms: u64) -> Self {
        Self {
            state: DiscoveryState::Inactive,
            enabled,
            idle_grace_ms,
            dwell_ms,
            idle_since_ms: None,
            dwell_deadline_ms: None,
        }
    }

    /// Current lifecycle state.
    pub fn state(&self) -> DiscoveryState {
        self.state
    }

    /// Enable/disable discovery. Disabling while active stands the machine down.
    pub fn set_enabled(&mut self, on: bool) -> Vec<DiscoveryAction> {
        self.enabled = on;
        if !on && self.state != DiscoveryState::Inactive {
            return self.stand_down();
        }
        Vec::new()
    }

    /// Feed one event; returns the actions to execute.
    pub fn step(&mut self, ev: DiscoveryEvent) -> Vec<DiscoveryAction> {
        match ev {
            DiscoveryEvent::Preempt => {
                if self.state != DiscoveryState::Inactive {
                    self.stand_down()
                } else {
                    Vec::new()
                }
            }
            DiscoveryEvent::Tick {
                idle,
                clock_ok,
                now_ms,
            } => self.on_tick(idle, clock_ok, now_ms),
            DiscoveryEvent::QsyComplete { ok } => self.on_qsy(ok),
            DiscoveryEvent::SlotElapsed { now_ms } => self.on_slot(now_ms),
        }
    }

    fn on_tick(&mut self, idle: bool, clock_ok: bool, now_ms: u64) -> Vec<DiscoveryAction> {
        // Track how long the idle predicate has continuously held.
        if idle && clock_ok && self.enabled {
            self.idle_since_ms.get_or_insert(now_ms);
        } else {
            self.idle_since_ms = None;
        }

        match self.state {
            DiscoveryState::Inactive => {
                let held = self
                    .idle_since_ms
                    .is_some_and(|since| now_ms.saturating_sub(since) >= self.idle_grace_ms);
                if held {
                    self.state = DiscoveryState::Activating;
                    self.idle_since_ms = None;
                    vec![
                        DiscoveryAction::SaveHomeAndTune,
                        DiscoveryAction::StateChanged(DiscoveryState::Activating),
                    ]
                } else {
                    Vec::new()
                }
            }
            DiscoveryState::Dwelling => {
                // Return home when the dwell budget is spent.
                if self
                    .dwell_deadline_ms
                    .is_some_and(|deadline| now_ms >= deadline)
                {
                    self.stand_down()
                } else {
                    Vec::new()
                }
            }
            DiscoveryState::Activating => Vec::new(),
        }
    }

    fn on_qsy(&mut self, ok: bool) -> Vec<DiscoveryAction> {
        if self.state != DiscoveryState::Activating {
            return Vec::new();
        }
        if ok {
            self.state = DiscoveryState::Dwelling;
            self.dwell_deadline_ms = None; // set on the first slot tick with a real `now`
            vec![DiscoveryAction::StateChanged(DiscoveryState::Dwelling)]
        } else {
            // Tune failed; restore home and give up this attempt.
            self.stand_down()
        }
    }

    fn on_slot(&mut self, now_ms: u64) -> Vec<DiscoveryAction> {
        if self.state != DiscoveryState::Dwelling {
            return Vec::new();
        }
        // Arm the dwell deadline on the first slot boundary (anchors the budget to real time).
        if self.dwell_ms > 0 && self.dwell_deadline_ms.is_none() {
            self.dwell_deadline_ms = Some(now_ms + self.dwell_ms);
        }
        let mut actions = vec![DiscoveryAction::DecodeSlot];
        if self
            .dwell_deadline_ms
            .is_some_and(|deadline| now_ms >= deadline)
        {
            actions.extend(self.stand_down());
        }
        actions
    }

    fn stand_down(&mut self) -> Vec<DiscoveryAction> {
        self.state = DiscoveryState::Inactive;
        self.idle_since_ms = None;
        self.dwell_deadline_ms = None;
        vec![
            DiscoveryAction::RestoreHome,
            DiscoveryAction::StateChanged(DiscoveryState::Inactive),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::DiscoveryAction::*;
    use super::DiscoveryState::*;
    use super::*;

    fn tick(idle: bool, now: u64) -> DiscoveryEvent {
        DiscoveryEvent::Tick {
            idle,
            clock_ok: true,
            now_ms: now,
        }
    }

    #[test]
    fn activates_only_after_idle_holds_for_the_grace_period() {
        let mut sm = DiscoverySm::new(true, 5000, 60_000);
        assert!(sm.step(tick(true, 1000)).is_empty()); // idle starts
        assert!(sm.step(tick(true, 3000)).is_empty()); // 2 s held < 5 s
        let a = sm.step(tick(true, 6000)); // 5 s held → activate
        assert_eq!(a, vec![SaveHomeAndTune, StateChanged(Activating)]);
        assert_eq!(sm.state(), Activating);
    }

    #[test]
    fn losing_idle_resets_the_grace_timer() {
        let mut sm = DiscoverySm::new(true, 5000, 0);
        sm.step(tick(true, 1000));
        assert!(sm.step(tick(false, 3000)).is_empty()); // busy → reset
        assert!(sm.step(tick(true, 4000)).is_empty()); // idle restarts at 4000
        assert!(sm.step(tick(true, 8000)).is_empty()); // only 4 s held
        assert_eq!(
            sm.step(tick(true, 9500)),
            vec![SaveHomeAndTune, StateChanged(Activating)]
        );
    }

    #[test]
    fn qsy_success_dwells_failure_restores() {
        let mut ok = DiscoverySm::new(true, 0, 0);
        ok.step(tick(true, 0));
        assert_eq!(ok.state(), Activating);
        assert_eq!(
            ok.step(DiscoveryEvent::QsyComplete { ok: true }),
            vec![StateChanged(Dwelling)]
        );

        let mut bad = DiscoverySm::new(true, 0, 0);
        bad.step(tick(true, 0));
        assert_eq!(
            bad.step(DiscoveryEvent::QsyComplete { ok: false }),
            vec![RestoreHome, StateChanged(Inactive)]
        );
        assert_eq!(bad.state(), Inactive);
    }

    #[test]
    fn dwelling_decodes_each_slot_then_returns_home_at_the_budget() {
        let mut sm = DiscoverySm::new(true, 0, 30_000);
        sm.step(tick(true, 0));
        sm.step(DiscoveryEvent::QsyComplete { ok: true });
        // First slot arms the 30 s budget and decodes.
        assert_eq!(
            sm.step(DiscoveryEvent::SlotElapsed { now_ms: 15_000 }),
            vec![DecodeSlot]
        );
        assert_eq!(
            sm.step(DiscoveryEvent::SlotElapsed { now_ms: 30_000 }),
            vec![DecodeSlot]
        );
        // Budget (15_000 + 30_000 = 45_000) reached → decode + stand down.
        let a = sm.step(DiscoveryEvent::SlotElapsed { now_ms: 45_000 });
        assert_eq!(a, vec![DecodeSlot, RestoreHome, StateChanged(Inactive)]);
        assert_eq!(sm.state(), Inactive);
    }

    #[test]
    fn preempt_and_disable_stand_down_from_dwell() {
        let mut sm = DiscoverySm::new(true, 0, 0);
        sm.step(tick(true, 0));
        sm.step(DiscoveryEvent::QsyComplete { ok: true });
        assert_eq!(sm.state(), Dwelling);
        assert_eq!(
            sm.step(DiscoveryEvent::Preempt),
            vec![RestoreHome, StateChanged(Inactive)]
        );

        // Disabling mid-dwell also stands down.
        let mut sm2 = DiscoverySm::new(true, 0, 0);
        sm2.step(tick(true, 0));
        sm2.step(DiscoveryEvent::QsyComplete { ok: true });
        assert_eq!(
            sm2.set_enabled(false),
            vec![RestoreHome, StateChanged(Inactive)]
        );
    }

    #[test]
    fn disabled_or_bad_clock_never_activates() {
        let mut off = DiscoverySm::new(false, 0, 0);
        assert!(off.step(tick(true, 0)).is_empty());
        assert!(off.step(tick(true, 10_000)).is_empty());
        assert_eq!(off.state(), Inactive);

        let mut skewed = DiscoverySm::new(true, 0, 0);
        let ev = DiscoveryEvent::Tick {
            idle: true,
            clock_ok: false,
            now_ms: 10_000,
        };
        assert!(skewed.step(ev).is_empty());
        assert_eq!(skewed.state(), Inactive);
    }
}
