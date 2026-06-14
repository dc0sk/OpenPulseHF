//! HPX session profiles: SpeedLevel-to-mode-string mappings for each bandwidth class.

use crate::rate::SpeedLevel;

/// Profile-entry policy for promoting SC-FDMA QAM rungs into the HPX HF ladder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScfdmaQamHfEntryPolicy {
    /// Minimum per-scenario frame success required for promotion.
    pub min_success_rate: f32,
    /// Deterministic scenario labels that must all meet `min_success_rate`.
    pub required_scenarios: &'static [&'static str],
    /// Frames evaluated per scenario in the deterministic matrix test.
    pub frames_per_scenario: usize,
}

/// Maps each [`SpeedLevel`] to a concrete modulation mode string for a given HPX profile.
///
/// A `None` entry means that speed level is not reachable within the profile (either
/// it's reserved or it's the SL1 chirp fallback, which is handled by the caller on
/// `RateEvent::ChirpFallback`).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionProfile {
    /// Mode strings indexed by SpeedLevel discriminant (1–20); index 0 unused.
    modes: [Option<&'static str>; 21],
    /// Speed level the rate adapter starts at when this profile is activated.
    pub initial_level: SpeedLevel,
    /// Consecutive NACK count that triggers a speed-level decrement.
    pub nack_threshold: u8,
    /// Per-level SNR floor (dB).  Drop below this → immediate step-down.
    snr_floors: [Option<f32>; 21],
    /// Per-level SNR ceiling (dB).  Rise above this → flag upgrade candidate.
    snr_ceilings: [Option<f32>; 21],
    /// If set, ACK-UP at this level requires a prior SNR upgrade candidate.
    ack_up_requires_snr_candidate_at: Option<SpeedLevel>,
}

impl SessionProfile {
    /// Deterministic promotion policy for SC-FDMA QAM modes on HF ladders.
    ///
    /// This policy is validated in
    /// `plugins/scfdma/tests/pilot_channel_estimation.rs` against a Watterson
    /// profile-entry matrix.
    pub const SCFDMA_QAM_HF_ENTRY_POLICY: ScfdmaQamHfEntryPolicy = ScfdmaQamHfEntryPolicy {
        min_success_rate: 0.90,
        required_scenarios: &["good_f1", "good_f2", "moderate_f1"],
        frames_per_scenario: 30,
    };

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

    /// Return the level where ACK-UP promotion is SNR-gated, if the profile
    /// requires that extra admission check.
    pub fn ack_up_requires_snr_candidate_at(&self) -> Option<SpeedLevel> {
        self.ack_up_requires_snr_candidate_at
    }

    /// Return all speed levels that have a mode string defined in this profile, in
    /// ascending order.  Useful for building profile-driven recommendation tables
    /// without hard-coding a fixed level range.
    pub fn defined_levels(&self) -> Vec<SpeedLevel> {
        use SpeedLevel::*;
        [
            Sl1, Sl2, Sl3, Sl4, Sl5, Sl6, Sl7, Sl8, Sl9, Sl10, Sl11, Sl12, Sl13, Sl14, Sl15, Sl16,
            Sl17, Sl18, Sl19, Sl20,
        ]
        .into_iter()
        .filter(|&l| self.modes[l as usize].is_some())
        .collect()
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
        let mut modes = [None; 21];
        modes[SpeedLevel::Sl2 as usize] = Some("BPSK31");
        modes[SpeedLevel::Sl3 as usize] = Some("BPSK63");
        modes[SpeedLevel::Sl4 as usize] = Some("BPSK250");
        modes[SpeedLevel::Sl5 as usize] = Some("QPSK250");
        modes[SpeedLevel::Sl6 as usize] = Some("QPSK500");
        // SNR floors: 3 dB headroom above Eb/N₀ required for 10⁻³ BER.
        let mut snr_floors = [None; 21];
        snr_floors[SpeedLevel::Sl2 as usize] = Some(3.0_f32);
        snr_floors[SpeedLevel::Sl3 as usize] = Some(4.0_f32);
        snr_floors[SpeedLevel::Sl4 as usize] = Some(5.0_f32);
        snr_floors[SpeedLevel::Sl5 as usize] = Some(9.0_f32);
        snr_floors[SpeedLevel::Sl6 as usize] = Some(11.0_f32);
        let mut snr_ceilings = [None; 21];
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
            ack_up_requires_snr_candidate_at: None,
        }
    }

    /// HPX HF profile: HF-compliant rate ladder (SL2–SL8), capped at SCFDMA52-8PSK (≈2031 Hz BW).
    ///
    /// Every mode in this profile fits within the 2700 Hz HF channel-width limit.
    /// Use this profile for on-air HF operation.  For FM/satellite/UHF links with wider
    /// channels use [`SessionProfile::hpx_wideband`].
    ///
    /// | SL  | Mode            |
    /// |-----|-----------------|
    /// | SL2 | BPSK31          |
    /// | SL3 | BPSK63          |
    /// | SL4 | BPSK250         |
    /// | SL5 | QPSK250         |
    /// | SL6 | QPSK500         |
    /// | SL7 | 8PSK500         |
    /// | SL8 | SCFDMA52-8PSK   |
    /// | SL12+ | — (reserved until `SCFDMA_QAM_HF_ENTRY_POLICY` is satisfied) |
    pub fn hpx_hf() -> Self {
        let mut modes = [None; 21];
        modes[SpeedLevel::Sl2 as usize] = Some("BPSK31");
        modes[SpeedLevel::Sl3 as usize] = Some("BPSK63");
        modes[SpeedLevel::Sl4 as usize] = Some("BPSK250");
        modes[SpeedLevel::Sl5 as usize] = Some("QPSK250");
        modes[SpeedLevel::Sl6 as usize] = Some("QPSK500");
        modes[SpeedLevel::Sl7 as usize] = Some("8PSK500");
        modes[SpeedLevel::Sl8 as usize] = Some("SCFDMA52-8PSK");
        let mut snr_floors = [None; 21];
        snr_floors[SpeedLevel::Sl2 as usize] = Some(3.0_f32);
        snr_floors[SpeedLevel::Sl3 as usize] = Some(4.0_f32);
        snr_floors[SpeedLevel::Sl4 as usize] = Some(5.0_f32);
        snr_floors[SpeedLevel::Sl5 as usize] = Some(9.0_f32);
        snr_floors[SpeedLevel::Sl6 as usize] = Some(11.0_f32);
        snr_floors[SpeedLevel::Sl7 as usize] = Some(14.0_f32);
        snr_floors[SpeedLevel::Sl8 as usize] = Some(16.0_f32);
        let mut snr_ceilings = [None; 21];
        snr_ceilings[SpeedLevel::Sl2 as usize] = Some(8.0_f32);
        snr_ceilings[SpeedLevel::Sl3 as usize] = Some(9.0_f32);
        snr_ceilings[SpeedLevel::Sl4 as usize] = Some(11.0_f32);
        snr_ceilings[SpeedLevel::Sl5 as usize] = Some(14.0_f32);
        snr_ceilings[SpeedLevel::Sl6 as usize] = Some(18.0_f32);
        snr_ceilings[SpeedLevel::Sl7 as usize] = Some(20.0_f32);
        // SL8 is the ceiling of hpx_hf; no upgrade above it.
        Self {
            modes,
            initial_level: SpeedLevel::Sl2,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
            ack_up_requires_snr_candidate_at: None,
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
        let mut modes = [None; 21];
        modes[SpeedLevel::Sl8 as usize] = Some("QPSK500");
        modes[SpeedLevel::Sl9 as usize] = Some("QPSK1000");
        modes[SpeedLevel::Sl11 as usize] = Some("8PSK1000");
        let mut snr_floors = [None; 21];
        snr_floors[SpeedLevel::Sl8 as usize] = Some(11.0_f32);
        snr_floors[SpeedLevel::Sl9 as usize] = Some(14.0_f32);
        snr_floors[SpeedLevel::Sl11 as usize] = Some(18.0_f32);
        let mut snr_ceilings = [None; 21];
        snr_ceilings[SpeedLevel::Sl8 as usize] = Some(18.0_f32);
        snr_ceilings[SpeedLevel::Sl9 as usize] = Some(22.0_f32);
        // SL11 is the ceiling; no upgrade above it.
        Self {
            modes,
            initial_level: SpeedLevel::Sl8,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
            ack_up_requires_snr_candidate_at: None,
        }
    }

    /// HPX Narrowband profile: 12.5 kHz PMR/LMR channel at 8 kHz audio (standard tier).
    ///
    /// All modes fit within a 12.5 kHz channelised plan.  Requires only an 8 kHz audio
    /// path — suitable for standard PMR/LMR radios.
    ///
    /// | SL  | Mode           |
    /// |-----|----------------|
    /// | SL1–SL7 | — (fall-through to HF/wideband rungs) |
    /// | SL8  | QPSK500        |
    /// | SL9  | QPSK1000       |
    /// | SL10 | QPSK2000-RRC   |
    /// | SL11 | 8PSK2000-RRC   |
    pub fn hpx_narrowband() -> Self {
        let mut modes = [None; 21];
        modes[SpeedLevel::Sl8 as usize] = Some("QPSK500");
        modes[SpeedLevel::Sl9 as usize] = Some("QPSK1000");
        modes[SpeedLevel::Sl10 as usize] = Some("QPSK2000-RRC");
        modes[SpeedLevel::Sl11 as usize] = Some("8PSK2000-RRC");
        let mut snr_floors = [None; 21];
        snr_floors[SpeedLevel::Sl8 as usize] = Some(11.0_f32);
        snr_floors[SpeedLevel::Sl9 as usize] = Some(14.0_f32);
        snr_floors[SpeedLevel::Sl10 as usize] = Some(17.0_f32);
        snr_floors[SpeedLevel::Sl11 as usize] = Some(20.0_f32);
        let mut snr_ceilings = [None; 21];
        snr_ceilings[SpeedLevel::Sl8 as usize] = Some(18.0_f32);
        snr_ceilings[SpeedLevel::Sl9 as usize] = Some(21.0_f32);
        snr_ceilings[SpeedLevel::Sl10 as usize] = Some(24.0_f32);
        // SL11 is the ceiling; no upgrade above it.
        Self {
            modes,
            initial_level: SpeedLevel::Sl8,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
            ack_up_requires_snr_candidate_at: None,
        }
    }

    /// HPX OFDM HF profile: multi-carrier HF ladder (SL5–SL6), capped at 2031 Hz BW.
    ///
    /// Both modes fit within the 2700 Hz HF channel-width limit.  Channel equalization
    /// (LS estimate + ZF) provides robustness against frequency-selective HF fading that
    /// single-carrier modes cannot achieve without an equalizer.
    ///
    /// | SL  | Mode    | BW       | Gross bps |
    /// |-----|---------|----------|-----------|
    /// | SL5 | OFDM16  | ≈ 625 Hz | ≈ 889     |
    /// | SL6 | OFDM52  | ≈ 2031 Hz| ≈ 2889    |
    pub fn hpx_ofdm_hf() -> Self {
        let mut modes = [None; 21];
        modes[SpeedLevel::Sl5 as usize] = Some("OFDM16");
        modes[SpeedLevel::Sl6 as usize] = Some("OFDM52");
        let mut snr_floors = [None; 21];
        snr_floors[SpeedLevel::Sl5 as usize] = Some(8.0_f32);
        snr_floors[SpeedLevel::Sl6 as usize] = Some(11.0_f32);
        let mut snr_ceilings = [None; 21];
        snr_ceilings[SpeedLevel::Sl5 as usize] = Some(14.0_f32);
        // SL6 is the ceiling; no upgrade above it.
        Self {
            modes,
            initial_level: SpeedLevel::Sl5,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
            ack_up_requires_snr_candidate_at: None,
        }
    }

    /// HPX Narrowband HD profile: 12.5 kHz channel at 48 kHz audio (fills the channel).
    ///
    /// Occupies the full 12.5 kHz channel at 9600 baud (α=0.35 RRC ≈ 13 kHz BW).
    /// **Requires a 48 kHz audio path** — not available on standard PMR/LMR radios.
    ///
    /// | SL  | Mode           |
    /// |-----|----------------|
    /// | SL1–SL7 | — (fall-through to narrowband rungs) |
    /// | SL8  | QPSK9600-RRC   |
    /// | SL9  | 8PSK9600-RRC   |
    pub fn hpx_narrowband_hd() -> Self {
        let mut modes = [None; 21];
        modes[SpeedLevel::Sl8 as usize] = Some("QPSK9600-RRC");
        modes[SpeedLevel::Sl9 as usize] = Some("8PSK9600-RRC");
        let mut snr_floors = [None; 21];
        snr_floors[SpeedLevel::Sl8 as usize] = Some(17.0_f32);
        snr_floors[SpeedLevel::Sl9 as usize] = Some(20.0_f32);
        let mut snr_ceilings = [None; 21];
        snr_ceilings[SpeedLevel::Sl8 as usize] = Some(24.0_f32);
        // SL9 is the ceiling; no upgrade above it.
        Self {
            modes,
            initial_level: SpeedLevel::Sl8,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
            ack_up_requires_snr_candidate_at: None,
        }
    }

    /// HPX Wideband HD profile: SC-FDMA crossover ladder (SL12–SL15).
    ///
    /// For VHF/UHF FM, microwave, and satellite links where the 2700 Hz HF bandwidth
    /// ceiling does not apply and SNR margins of 16–40 dB are achievable.
    /// Not suitable for HF ionospheric paths (Watterson fading breaks QAM coherence).
    ///
    /// | SL   | Mode              | Gross bps (8 kHz audio) | Min SNR |
    /// |------|-------------------|-------------------------|---------|
    /// | SL12 | SCFDMA52-16QAM   | ≈ 5778                  | 16 dB   |
    /// | SL13 | SCFDMA52-32QAM   | ≈ 7222                  | 20 dB   |
    /// | SL14 | SCFDMA52-64QAM   | ≈ 8667                  | 28 dB   |
    /// | SL15 | 64QAM2000-RRC    | ≈ 12000                 | 35 dB   |
    pub fn hpx_wideband_hd() -> Self {
        let mut modes = [None; 21];
        // SL9–SL11: half-width SCFDMA26 higher-order rungs — the robust graceful-
        // degradation path. Same constellations as the wide SL12+ modes but ~half the
        // occupied bandwidth (~+3 dB per-subcarrier SNR), so an adaptive session drops
        // here when the link cannot sustain the full-width modes. Hardware-validated
        // with soft-concatenated FEC (the session FEC these dense modes run under).
        modes[SpeedLevel::Sl9 as usize] = Some("SCFDMA26-8PSK");
        modes[SpeedLevel::Sl10 as usize] = Some("SCFDMA26-16QAM");
        modes[SpeedLevel::Sl11 as usize] = Some("SCFDMA26-32QAM");
        modes[SpeedLevel::Sl12 as usize] = Some("SCFDMA52-16QAM");
        modes[SpeedLevel::Sl13 as usize] = Some("SCFDMA52-32QAM");
        modes[SpeedLevel::Sl14 as usize] = Some("SCFDMA52-64QAM");
        modes[SpeedLevel::Sl15 as usize] = Some("64QAM2000-RRC");
        let mut snr_floors = [None; 21];
        snr_floors[SpeedLevel::Sl9 as usize] = Some(9.0_f32);
        snr_floors[SpeedLevel::Sl10 as usize] = Some(11.0_f32);
        snr_floors[SpeedLevel::Sl11 as usize] = Some(13.0_f32);
        snr_floors[SpeedLevel::Sl12 as usize] = Some(16.0_f32);
        snr_floors[SpeedLevel::Sl13 as usize] = Some(20.0_f32);
        snr_floors[SpeedLevel::Sl14 as usize] = Some(28.0_f32);
        snr_floors[SpeedLevel::Sl15 as usize] = Some(35.0_f32);
        let mut snr_ceilings = [None; 21];
        snr_ceilings[SpeedLevel::Sl9 as usize] = Some(12.0_f32);
        snr_ceilings[SpeedLevel::Sl10 as usize] = Some(14.0_f32);
        snr_ceilings[SpeedLevel::Sl11 as usize] = Some(16.0_f32);
        snr_ceilings[SpeedLevel::Sl12 as usize] = Some(20.0_f32);
        snr_ceilings[SpeedLevel::Sl13 as usize] = Some(26.0_f32);
        snr_ceilings[SpeedLevel::Sl14 as usize] = Some(33.0_f32);
        // SL15 is the ceiling; no upgrade above it.
        Self {
            modes,
            initial_level: SpeedLevel::Sl12,
            nack_threshold: 2,
            snr_floors,
            snr_ceilings,
            ack_up_requires_snr_candidate_at: Some(SpeedLevel::Sl14),
        }
    }
}
