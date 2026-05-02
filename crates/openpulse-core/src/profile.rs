//! HPX session profiles: SpeedLevel-to-mode-string mappings for each bandwidth class.

use crate::rate::SpeedLevel;

/// Maps each [`SpeedLevel`] to a concrete modulation mode string for a given HPX profile.
///
/// A `None` entry means that speed level is not reachable within the profile (either
/// it's reserved or it's the SL1 chirp fallback, which is handled by the caller on
/// `RateEvent::ChirpFallback`).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionProfile {
    /// Mode strings indexed by SpeedLevel discriminant (1–11); index 0 unused.
    modes: [Option<&'static str>; 12],
    /// Speed level the rate adapter starts at when this profile is activated.
    pub initial_level: SpeedLevel,
    /// Consecutive NACK count that triggers a speed-level decrement.
    pub nack_threshold: u8,
}

impl SessionProfile {
    /// Return the mode string for the given speed level, or `None` if the level
    /// is not mapped in this profile.
    pub fn mode_for(&self, level: SpeedLevel) -> Option<&'static str> {
        self.modes[level as usize]
    }

    /// HPX500 profile: 500 Hz class, BPSK/QPSK rate ladder (SL2–SL6).
    ///
    /// | SL  | Mode     |
    /// |-----|----------|
    /// | SL1 | — (chirp fallback) |
    /// | SL2 | BPSK31   |
    /// | SL3 | BPSK63   |
    /// | SL4 | BPSK250  |
    /// | SL5 | QPSK250  |
    /// | SL6 | QPSK500  |
    /// | SL7–SL11 | — (reserved / HPX2300) |
    pub fn hpx500() -> Self {
        let mut modes = [None; 12];
        modes[SpeedLevel::Sl2 as usize] = Some("BPSK31");
        modes[SpeedLevel::Sl3 as usize] = Some("BPSK63");
        modes[SpeedLevel::Sl4 as usize] = Some("BPSK250");
        modes[SpeedLevel::Sl5 as usize] = Some("QPSK250");
        modes[SpeedLevel::Sl6 as usize] = Some("QPSK500");
        Self {
            modes,
            initial_level: SpeedLevel::Sl2,
            nack_threshold: 3,
        }
    }

    /// HPX2300 profile: 2300 Hz class, single-carrier QPSK/8PSK ladder (SL8–SL11).
    ///
    /// Single-carrier over OFDM: lower PAPR, no cyclic prefix overhead, simpler AFC.
    /// See docs/architecture.md for the full design rationale.
    ///
    /// | SL  | Mode      |
    /// |-----|-----------|
    /// | SL1–SL7 | — (chirp fallback / HPX500) |
    /// | SL8 | QPSK500   |
    /// | SL9 | QPSK1000  |
    /// | SL10 | — (reserved) |
    /// | SL11 | 8PSK1000  |
    pub fn hpx2300() -> Self {
        let mut modes = [None; 12];
        modes[SpeedLevel::Sl8 as usize] = Some("QPSK500");
        modes[SpeedLevel::Sl9 as usize] = Some("QPSK1000");
        modes[SpeedLevel::Sl11 as usize] = Some("8PSK1000");
        Self {
            modes,
            initial_level: SpeedLevel::Sl8,
            nack_threshold: 3,
        }
    }
}
