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
    /// Per-level SNR floor (dB).  Drop below this → immediate step-down.
    snr_floors: [Option<f32>; 12],
    /// Per-level SNR ceiling (dB).  Rise above this → flag upgrade candidate.
    snr_ceilings: [Option<f32>; 12],
}

impl SessionProfile {
    /// Return the mode string for the given speed level, or `None` if the level
    /// is not mapped in this profile.
    pub fn mode_for(&self, level: SpeedLevel) -> Option<&'static str> {
        self.modes[level as usize]
    }

    /// Return the SNR floor (dB) for `level`, or `None` if no threshold is defined.
    ///
    /// When measured SNR drops below this, the rate adapter steps down immediately
    /// without waiting for a NACK.
    pub fn snr_floor_for_level(&self, level: SpeedLevel) -> Option<f32> {
        self.snr_floors[level as usize]
    }

    /// Return the SNR ceiling (dB) for `level`, or `None` if no threshold is defined.
    ///
    /// When measured SNR exceeds this, the rate adapter sets an upgrade-candidate flag.
    pub fn snr_ceiling_for_level(&self, level: SpeedLevel) -> Option<f32> {
        self.snr_ceilings[level as usize]
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
        // SNR floors: 3 dB headroom above Eb/N₀ required for 10⁻³ BER.
        let mut snr_floors = [None; 12];
        snr_floors[SpeedLevel::Sl2 as usize] = Some(3.0_f32);
        snr_floors[SpeedLevel::Sl3 as usize] = Some(4.0_f32);
        snr_floors[SpeedLevel::Sl4 as usize] = Some(5.0_f32);
        snr_floors[SpeedLevel::Sl5 as usize] = Some(9.0_f32);
        snr_floors[SpeedLevel::Sl6 as usize] = Some(11.0_f32);
        let mut snr_ceilings = [None; 12];
        snr_ceilings[SpeedLevel::Sl2 as usize] = Some(8.0_f32);
        snr_ceilings[SpeedLevel::Sl3 as usize] = Some(9.0_f32);
        snr_ceilings[SpeedLevel::Sl4 as usize] = Some(11.0_f32);
        snr_ceilings[SpeedLevel::Sl5 as usize] = Some(14.0_f32);
        snr_ceilings[SpeedLevel::Sl6 as usize] = Some(18.0_f32);
        Self {
            modes,
            initial_level: SpeedLevel::Sl2,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
        }
    }

    /// HPX HF profile: HF-compliant rate ladder (SL2–SL7), capped at 8PSK500 (≈2000 Hz BW).
    ///
    /// Every mode in this profile fits within the 2700 Hz HF channel-width limit.
    /// Use this profile for on-air HF operation.  For FM/satellite/UHF links with wider
    /// channels use [`SessionProfile::hpx_wideband`].
    ///
    /// | SL  | Mode     |
    /// |-----|----------|
    /// | SL2 | BPSK31   |
    /// | SL3 | BPSK63   |
    /// | SL4 | BPSK250  |
    /// | SL5 | QPSK250  |
    /// | SL6 | QPSK500  |
    /// | SL7 | 8PSK500  |
    pub fn hpx_hf() -> Self {
        let mut modes = [None; 12];
        modes[SpeedLevel::Sl2 as usize] = Some("BPSK31");
        modes[SpeedLevel::Sl3 as usize] = Some("BPSK63");
        modes[SpeedLevel::Sl4 as usize] = Some("BPSK250");
        modes[SpeedLevel::Sl5 as usize] = Some("QPSK250");
        modes[SpeedLevel::Sl6 as usize] = Some("QPSK500");
        modes[SpeedLevel::Sl7 as usize] = Some("8PSK500");
        let mut snr_floors = [None; 12];
        snr_floors[SpeedLevel::Sl2 as usize] = Some(3.0_f32);
        snr_floors[SpeedLevel::Sl3 as usize] = Some(4.0_f32);
        snr_floors[SpeedLevel::Sl4 as usize] = Some(5.0_f32);
        snr_floors[SpeedLevel::Sl5 as usize] = Some(9.0_f32);
        snr_floors[SpeedLevel::Sl6 as usize] = Some(11.0_f32);
        snr_floors[SpeedLevel::Sl7 as usize] = Some(14.0_f32);
        let mut snr_ceilings = [None; 12];
        snr_ceilings[SpeedLevel::Sl2 as usize] = Some(8.0_f32);
        snr_ceilings[SpeedLevel::Sl3 as usize] = Some(9.0_f32);
        snr_ceilings[SpeedLevel::Sl4 as usize] = Some(11.0_f32);
        snr_ceilings[SpeedLevel::Sl5 as usize] = Some(14.0_f32);
        snr_ceilings[SpeedLevel::Sl6 as usize] = Some(18.0_f32);
        snr_ceilings[SpeedLevel::Sl7 as usize] = Some(22.0_f32);
        Self {
            modes,
            initial_level: SpeedLevel::Sl2,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
        }
    }

    /// HPX Wideband profile: wideband class, single-carrier QPSK/8PSK ladder (SL8–SL11).
    ///
    /// Single-carrier over OFDM: lower PAPR, no cyclic prefix overhead, simpler AFC.
    /// See docs/architecture.md for the full design rationale.
    ///
    /// **Bandwidth note**: SL9 (QPSK1000) and SL11 (8PSK1000) exceed the 2700 Hz HF
    /// channel-width limit.  Use this profile on FM, satellite, and UHF/VHF links only.
    /// For HF operation use [`SessionProfile::hpx_hf`].
    ///
    /// | SL  | Mode      |
    /// |-----|-----------|
    /// | SL1–SL7 | — (chirp fallback / HPX500) |
    /// | SL8 | QPSK500   |
    /// | SL9 | QPSK1000  |
    /// | SL10 | — (reserved) |
    /// | SL11 | 8PSK1000  |
    pub fn hpx_wideband() -> Self {
        let mut modes = [None; 12];
        modes[SpeedLevel::Sl8 as usize] = Some("QPSK500");
        modes[SpeedLevel::Sl9 as usize] = Some("QPSK1000");
        modes[SpeedLevel::Sl11 as usize] = Some("8PSK1000");
        let mut snr_floors = [None; 12];
        snr_floors[SpeedLevel::Sl8 as usize] = Some(11.0_f32);
        snr_floors[SpeedLevel::Sl9 as usize] = Some(14.0_f32);
        snr_floors[SpeedLevel::Sl11 as usize] = Some(18.0_f32);
        let mut snr_ceilings = [None; 12];
        snr_ceilings[SpeedLevel::Sl8 as usize] = Some(18.0_f32);
        snr_ceilings[SpeedLevel::Sl9 as usize] = Some(22.0_f32);
        // SL11 is the ceiling; no upgrade above it.
        Self {
            modes,
            initial_level: SpeedLevel::Sl8,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
        }
    }
}
