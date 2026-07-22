//! HPX session profiles: SpeedLevel-to-mode-string mappings for each bandwidth class.

use crate::fec::FecMode;
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
    /// Per-level FEC scheme (MODCOD). `None` = no FEC for that level. Indexed by
    /// SpeedLevel discriminant (1–20); index 0 unused.
    fec_modes: [Option<FecMode>; 21],
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

    /// A stable hash of the **wire-relevant** ladder mapping — the `(level → mode, level → FEC)`
    /// pairs — used to detect when two stations' ladders diverge (e.g. across code versions) so a
    /// `recommended_level` never means different things at the two ends.
    ///
    /// Deliberately EXCLUDES local-only policy (SNR floors/ceilings, `nack_threshold`): two peers
    /// with the same modes+FEC but recalibrated floors still interoperate on air, so a floor tweak
    /// must NOT change the fingerprint. Only a mode/FEC/step change does. FNV-1a over the mapping.
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a offset basis
        let mut mix = |byte: u8| {
            h ^= byte as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3); // FNV prime
        };
        for lvl in self.defined_levels() {
            mix(lvl as u8);
            if let Some(m) = self.mode_for(lvl) {
                for b in m.as_bytes() {
                    mix(*b);
                }
            }
            mix(0xff); // mode/FEC separator
            mix(self.fec_for(lvl) as u8);
            mix(0xfe); // per-level separator
        }
        h
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

    /// Return the FEC scheme for `level` (MODCOD). Defaults to [`FecMode::None`]
    /// for levels without an explicit FEC assignment.
    pub fn fec_for(&self, level: SpeedLevel) -> FecMode {
        self.fec_modes[level as usize].unwrap_or(FecMode::None)
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

    /// Canonical profile names accepted by [`SessionProfile::by_name`], in ladder order.
    pub const PROFILE_NAMES: &'static [&'static str] = &[
        "hpx500",
        "hpx_modcod",
        "hpx_pilot",
        "hpx_pilot_rrc",
        "hpx_pilot_fast",
        "hpx_pilot_fast_rrc",
        "hpx_hf",
        "hpx_ofdm_hf",
        "hpx_wideband",
        "hpx_wideband_hd",
        "hpx_narrowband",
        "hpx_narrowband_hd",
    ];

    /// Construct a profile by name (case-insensitive; `-` and `_` are interchangeable).
    ///
    /// Returns `None` for an unrecognised name; see [`SessionProfile::PROFILE_NAMES`].
    pub fn by_name(name: &str) -> Option<SessionProfile> {
        let key = name.trim().to_ascii_lowercase().replace('-', "_");
        match key.as_str() {
            "hpx500" => Some(Self::hpx500()),
            "hpx_modcod" => Some(Self::hpx_modcod()),
            "hpx_pilot" => Some(Self::hpx_pilot()),
            "hpx_pilot_rrc" => Some(Self::hpx_pilot_rrc()),
            "hpx_pilot_fast" => Some(Self::hpx_pilot_fast()),
            "hpx_pilot_fast_rrc" => Some(Self::hpx_pilot_fast_rrc()),
            "hpx_hf" => Some(Self::hpx_hf()),
            "hpx_ofdm_hf" => Some(Self::hpx_ofdm_hf()),
            "hpx_wideband" => Some(Self::hpx_wideband()),
            "hpx_wideband_hd" => Some(Self::hpx_wideband_hd()),
            "hpx_narrowband" => Some(Self::hpx_narrowband()),
            "hpx_narrowband_hd" => Some(Self::hpx_narrowband_hd()),
            _ => None,
        }
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
            fec_modes: [None; 21],
        }
    }

    /// HPX MODCOD profile: a 500 Hz ladder that adapts **modulation × FEC** together
    /// (DVB-S2 / WiFi-MCS style), interleaving FEC rungs between modulation steps so
    /// the link can trade coding gain for throughput at fine granularity.
    ///
    /// | SL  | Mode     | FEC   | note                         |
    /// |-----|----------|-------|------------------------------|
    /// | SL2 | BPSK250  | LDPC  | most robust (rate-1/2 soft)  |
    /// | SL3 | BPSK250  | RS    | same mod, lighter coding     |
    /// | SL4 | QPSK250  | LDPC  | denser mod, strong coding    |
    /// | SL5 | QPSK250  | RS    | same mod, lighter coding     |
    /// | SL6 | QPSK500  | RS    | fastest mod, coded           |
    /// | SL7 | QPSK500  | none  | peak throughput, uncoded     |
    pub fn hpx_modcod() -> Self {
        let mut modes = [None; 21];
        modes[SpeedLevel::Sl2 as usize] = Some("BPSK250");
        modes[SpeedLevel::Sl3 as usize] = Some("BPSK250");
        modes[SpeedLevel::Sl4 as usize] = Some("QPSK250");
        modes[SpeedLevel::Sl5 as usize] = Some("QPSK250");
        modes[SpeedLevel::Sl6 as usize] = Some("QPSK500");
        modes[SpeedLevel::Sl7 as usize] = Some("QPSK500");

        let mut fec_modes = [None; 21];
        fec_modes[SpeedLevel::Sl2 as usize] = Some(FecMode::Ldpc);
        fec_modes[SpeedLevel::Sl3 as usize] = Some(FecMode::Rs);
        fec_modes[SpeedLevel::Sl4 as usize] = Some(FecMode::Ldpc);
        fec_modes[SpeedLevel::Sl5 as usize] = Some(FecMode::Rs);
        fec_modes[SpeedLevel::Sl6 as usize] = Some(FecMode::Rs);
        fec_modes[SpeedLevel::Sl7 as usize] = Some(FecMode::None);

        let mut snr_floors = [None; 21];
        let mut snr_ceilings = [None; 21];
        for (lvl, (floor, ceil)) in [
            (SpeedLevel::Sl2, (1.0, 6.0)),
            (SpeedLevel::Sl3, (4.0, 9.0)),
            (SpeedLevel::Sl4, (7.0, 12.0)),
            (SpeedLevel::Sl5, (10.0, 15.0)),
            (SpeedLevel::Sl6, (13.0, 18.0)),
            (SpeedLevel::Sl7, (16.0, 21.0)),
        ] {
            snr_floors[lvl as usize] = Some(floor);
            snr_ceilings[lvl as usize] = Some(ceil);
        }

        Self {
            modes,
            initial_level: SpeedLevel::Sl2,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
            ack_up_requires_snr_candidate_at: None,
            fec_modes,
        }
    }

    /// HPX pilot profile: the pilot-framed waveform's adaptive ladder (SL2–SL4).
    ///
    /// Climbs constellation density on the same 500-baud pilot-framed carrier:
    /// QPSK (most robust) → 8PSK → 16QAM (highest throughput). All three recover
    /// the carrier from known in-band pilots, so the ladder stays usable on the
    /// dual-clock / carrier-offset paths where the single-Costas ladders struggle.
    ///
    /// | SL  | Mode           |
    /// |-----|----------------|
    /// | SL2 | PILOT-QPSK500  |
    /// | SL3 | PILOT-8PSK500  |
    /// | SL4 | PILOT-16QAM500 |
    /// | SL5 | PILOT-32APSK500 |
    pub fn hpx_pilot() -> Self {
        let mut modes = [None; 21];
        modes[SpeedLevel::Sl2 as usize] = Some("PILOT-QPSK500");
        modes[SpeedLevel::Sl3 as usize] = Some("PILOT-8PSK500");
        modes[SpeedLevel::Sl4 as usize] = Some("PILOT-16QAM500");
        modes[SpeedLevel::Sl5 as usize] = Some("PILOT-32APSK500");
        let mut snr_floors = [None; 21];
        snr_floors[SpeedLevel::Sl2 as usize] = Some(6.0_f32);
        snr_floors[SpeedLevel::Sl3 as usize] = Some(12.0_f32);
        snr_floors[SpeedLevel::Sl4 as usize] = Some(17.0_f32);
        snr_floors[SpeedLevel::Sl5 as usize] = Some(23.0_f32);
        let mut snr_ceilings = [None; 21];
        snr_ceilings[SpeedLevel::Sl2 as usize] = Some(12.0_f32);
        snr_ceilings[SpeedLevel::Sl3 as usize] = Some(17.0_f32);
        snr_ceilings[SpeedLevel::Sl4 as usize] = Some(23.0_f32);
        // SL5 (32APSK) is the top rung — no ceiling.
        Self {
            modes,
            initial_level: SpeedLevel::Sl2,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
            ack_up_requires_snr_candidate_at: None,
            fec_modes: [None; 21],
        }
    }

    /// HPX pilot profile, narrowband (RRC pulse): same QPSK→8PSK→16QAM→32APSK
    /// ladder as [`hpx_pilot`](Self::hpx_pilot) but on the `-RRC` variants, which
    /// occupy ~half the bandwidth (~(1+α)·baud ≈ 675 Hz). The matched RRC filter
    /// gives the same Eb/N0 as the rectangular pulse, so the per-constellation SNR
    /// thresholds are unchanged; the win is spectral occupancy. RRC samples at a
    /// point (vs the rectangular integrate-and-dump averaging over the symbol), so
    /// prefer [`hpx_pilot`](Self::hpx_pilot) when the link is dual-clock/SRO-heavy.
    pub fn hpx_pilot_rrc() -> Self {
        let mut p = Self::hpx_pilot();
        p.modes[SpeedLevel::Sl2 as usize] = Some("PILOT-QPSK500-RRC");
        p.modes[SpeedLevel::Sl3 as usize] = Some("PILOT-8PSK500-RRC");
        p.modes[SpeedLevel::Sl4 as usize] = Some("PILOT-16QAM500-RRC");
        p.modes[SpeedLevel::Sl5 as usize] = Some("PILOT-32APSK500-RRC");
        p
    }

    /// HPX pilot profile, high-throughput (1000 baud): same constellation ladder
    /// as [`hpx_pilot`](Self::hpx_pilot) on the 1000-baud rungs — 2× the bits/s at
    /// each step (8 samples/symbol at 8 kHz). The SNR thresholds are the same: they
    /// are per-symbol (Es/N0) floors set by the constellation, which the engine
    /// measures from the LLRs after the matched filter — so they don't move with
    /// baud. The cost of the faster rungs is ~2× occupied bandwidth and the wider
    /// noise bandwidth (the channel must actually deliver that Es/N0); prefer
    /// [`hpx_pilot`](Self::hpx_pilot) on bandwidth- or power-limited links.
    pub fn hpx_pilot_fast() -> Self {
        let mut p = Self::hpx_pilot();
        p.modes[SpeedLevel::Sl2 as usize] = Some("PILOT-QPSK1000");
        p.modes[SpeedLevel::Sl3 as usize] = Some("PILOT-8PSK1000");
        p.modes[SpeedLevel::Sl4 as usize] = Some("PILOT-16QAM1000");
        p.modes[SpeedLevel::Sl5 as usize] = Some("PILOT-32APSK1000");
        p
    }

    /// HPX pilot profile, high-throughput **and** narrowband (1000-baud RRC): the
    /// 1000-baud ladder of [`hpx_pilot_fast`](Self::hpx_pilot_fast) on the `-RRC`
    /// variants, so it gets the 2× throughput while keeping the RRC's ~half-band
    /// occupancy (~(1+α)·1000 ≈ 1350 Hz vs the rectangular 1000-baud's wide sinc).
    /// Same per-symbol Es/N0 floors. The combined choice when bandwidth matters but
    /// the link can carry 1000 baud; [`hpx_pilot`](Self::hpx_pilot) stays the
    /// SRO-robust pick (RRC samples at a point — see `hpx_pilot_rrc`).
    pub fn hpx_pilot_fast_rrc() -> Self {
        let mut p = Self::hpx_pilot();
        p.modes[SpeedLevel::Sl2 as usize] = Some("PILOT-QPSK1000-RRC");
        p.modes[SpeedLevel::Sl3 as usize] = Some("PILOT-8PSK1000-RRC");
        p.modes[SpeedLevel::Sl4 as usize] = Some("PILOT-16QAM1000-RRC");
        p.modes[SpeedLevel::Sl5 as usize] = Some("PILOT-32APSK1000-RRC");
        p
    }

    /// HPX HF profile: the full HF-compliant rate ladder (SL2–SL19), 62 bps to 7.7 kbps.
    ///
    /// Every mode here fits within the 2700 Hz HF channel-width limit (SCFDMA52-* is ≈2031 Hz), so the
    /// ladder spans weak-signal BPSK31 all the way to 64QAM SC-FDMA at code rate ≈8/9.  The dense rungs
    /// (SL10–SL19) always run FEC-protected: soft-concatenated up to SL15, then high-rate LDPC for the
    /// top four — see the table in the body for the mode/FEC/rate/floor of every rung and for why LDPC
    /// appears only above SL15.  For SNR-marginal links, `hpx_wideband_hd` provides the narrowband
    /// SCFDMA26-* fallback rungs; for genuinely wider-than-3 kHz channels (FM/UHF/VHF) use
    /// `hpx_wideband`.
    pub fn hpx_hf() -> Self {
        // Finer HF ladder (research #2, docs/dev/research/ladder-granularity.md). Fills the old
        // throughput cliffs and SNR dead-zones with existing (previously unused) modes plus two MODCOD
        // rungs, keeping every rung ≤ ~2 kHz occupied (well within the 2700 Hz HF channel). Pre-release,
        // so the SL re-index carries no ladder-interop concern.
        //
        // **Fade-aware re-seat.** Every rung below is measured to decode on Watterson `moderate_f1`
        // (1 Hz Doppler, 1.0 ms delay — a routine ITU-R moderate HF channel). The previous ladder was
        // not: at their own SNR floors the uncoded rungs SL2–SL5 decoded ~0 % of fading frames, and the
        // coherent single-carrier mid rungs (QPSK250/QPSK500/8PSK500) decoded ~0 % at *any* SNR up to
        // 40 dB. Effective throughput (decode × net bps) at 20 dB used to read 346 (SL6) → 0 → 125 → 0
        // → 395 → 1816: a four-rung dead zone the rung-by-rung adapter had to cross to reach the rungs
        // that work. Those rungs are gone; the ladder is now monotonic **on a fade**, not just on AWGN.
        //
        // | SL | Mode              | FEC | net bps | floor | note                                  |
        // |----|-------------------|-----|---------|-------|---------------------------------------|
        // |  1 | MFSK16            | Rs  |   ~9    |  None | non-coherent sub-floor deep-fade rung |
        // |  2 | BPSK31            | Rs  |     27  |   3   | initial_level                         |
        // |  3 | BPSK63            | Rs  |     54  |   4   |                                       |
        // |  4 | BPSK100           | Rs  |     87  |  4.5  | breaks the 54→219 bps cliff           |
        // |  5 | BPSK250           | Rs  |    219  |   5   |                                       |
        // |  6 | QPSK250-D         | Rs  |    437  |   7   | differential (HF-fade-robust); #923   |
        // |  7 | OFDM52            | SC  |   1264  |  9    | fills the old SL7–SL10 dead zone      |
        // |  8 | OFDM52-8PSK       | SC  |   1895  |  10   | (CP rides selective HF fade)          |
        // |  9 | OFDM52-16QAM      | SC  |   2527  |  12   |                                       |
        // | 10 | OFDM52-32QAM      | SC  |   3159  |  14   |                                       |
        // | 11 | OFDM52-64QAM      | SC  |   3790  |  16   |                                       |
        // | 12 | OFDM52-16QAM      | LHR |   5141  |  18   | LDPC r≈8/9 top-of-ladder rate lever   |
        // | 13 | OFDM52-32QAM      | LHR |   6426  |  19   |                                       |
        // | 14 | OFDM52-64QAM      | LHR |   7710  |  20   | ladder top                            |
        //
        // **Why the BPSK rungs are coded now (they carry `Rs`; v0.15.0 adds an opportunistic
        // per-frame upgrade to `RsStrong` where it costs no extra RS block — see `free_rs_strengthening`).** BPSK is differentially decoded, so
        // it rides the fade rotation — but #923's law applies: *differential needs FEC*, because the
        // symbol a carrier slip costs still has to be corrected. Uncoded on `moderate_f1` at their own
        // floors these rungs decode ~0 % (BPSK63 @4 dB 0.000, BPSK250 @5 dB 0.000); coded they work
        // (BPSK63 @4 dB 0.833, BPSK250 @8 dB 1.00). The floors did not move: they were always
        // fading-appropriate, the rungs just lacked the code to meet them.
        //
        // **Why `Rs`, and why NOT `RsInterleaved` or `RsStrong`** — measured, and the answer depends on
        // payload size in a way that is easy to get wrong:
        //   * `RsInterleaved` is **inert** here (BPSK250 `moderate_f1` @5/8 dB: 0.17/0.58 — identical to
        //     `Rs`). A ≤223-byte payload is ONE RS block, and a single block is position-agnostic, so
        //     there is nothing for the interleaver to spread. Code **strength**, not interleaving, is
        //     the lever (the same finding `plugins/mfsk16/src/robust_ack.rs` records). This is the
        //     opposite of what `docs/mode-fec-ladder.md` §2's "best for HF burst/fading" billing implies.
        //   * `RsStrong` (t=32) is **stronger on a fade** — BPSK31 @3 dB 0.25 → **1.00**, BPSK250 @8 dB
        //     0.58 → 1.00 — and it is genuinely **free for payloads ≤191 B**, because RS(255,223) and
        //     RS(255,191) both emit one 255-byte block. But at **192–223 B it costs 2× the airtime**:
        //     the payload no longer fits one RS(255,191) block while it still fits one RS(255,223).
        //     That window is ordinary traffic — a 200-byte frame doubles BPSK31 from 66 s to 132 s —
        //     and it drops `hpx_hf`'s AWGN goodput from 310 to 199 bps (linksim, 200 B frames), through
        //     the CI goodput floor. `Rs` keeps the whole ladder inside one block up to 223 B.
        //   * So `Rs` is the ladder-wide choice, and the fade gain is the part that matters: uncoded
        //     rungs decode **0.00** at their floors, `Rs` decodes 0.25 (BPSK31 @3 dB) to 1.00 (BPSK63
        //     @7 dB) — dead vs usable-under-ARQ. `RsStrong` remains the right code for a rung whose
        //     frames are known to stay under 191 B; it is not a safe ladder-wide default.
        // The `SCFDMA26-32QAM` narrowband rung was dropped: it decoded 0.00/0.17/0.17 at 8/12/16 dB on
        // `moderate_f1`, so it was not a usable fallback. It still lives in `hpx_wideband_hd`. `OFDM16`
        // is not a rung here — it is the most fade-robust OFDM mode (0.92 @16 dB) and the narrowest
        // (625 Hz), but its ~401 net bps sits *below* SL6, so it has no monotonic slot.
        //
        // Why high-rate LDPC only at the TOP, and not as a swap for `SoftConcatenated` further down.
        // Measured on AWGN (62-byte payload, 90 % frame success, 32 frames/point), `LdpcHighRate`
        // (r≈8/9) costs +4…+8 dB of floor over `SoftConcatenated` (r≈0.437) and returns 2.03× the rate:
        //
        //   mode                SC floor   LHR floor   Δ
        //   SCFDMA26-32QAM         5           11      +6
        //   SCFDMA52-8PSK          5           10      +5
        //   SCFDMA52-16QAM         7           14      +7
        //   SCFDMA52-32QAM         8           15      +7
        //   SCFDMA52-64QAM-P4     15           19      +4
        //   SCFDMA52-64QAM        13           21      +8
        //
        // ~6 dB for 2× the rate is a *worse* trade than climbing one modulation order (8PSK→16QAM buys
        // 1.33× for ~2 dB), so wherever a denser mode still exists, `SoftConcatenated` on it wins. The
        // exception is the top: 64QAM is the densest constellation the plugin has, so above SL15 the
        // only remaining lever is code rate. Hence LHR appears as SL15–SL17 and nowhere below.
        let mut modes = [None; 21];
        // SL1 is the non-coherent MFSK16 sub-floor rung — the actual waveform for the deep-fade
        // ChirpFallback path (3 NACKs at SL2), reached only under sustained failure. Constant-envelope
        // 16-GFSK, ~17 s/frame, one RS block; robust ACK is K=3 union-decoded MFSK16-ACK (REQ-WSIG-01).
        modes[SpeedLevel::Sl1 as usize] = Some("MFSK16");
        modes[SpeedLevel::Sl2 as usize] = Some("BPSK31");
        modes[SpeedLevel::Sl3 as usize] = Some("BPSK63");
        modes[SpeedLevel::Sl4 as usize] = Some("BPSK100");
        modes[SpeedLevel::Sl5 as usize] = Some("BPSK250");
        // SL6 is differential QPSK (`-D`), not coherent QPSK250. Coherent QPSK250+Rs decodes 0% on
        // Watterson moderate_f1 at *every* SNR (issue #923): an absolutely-encoded waveform cannot hold
        // a carrier-phase reference through a 1 Hz Doppler fade, so a cycle slip at a fade null ruins
        // the frame tail. Differential encoding makes the fade rotation cancel symbol-to-symbol (the
        // same immunity BPSK250/SL5 has), recovering the rung from 0.00 → ~0.65 at 20 dB for ~2 dB of
        // AWGN floor (both decode 100% by 4 dB, well under SL6's operating SNR). Differential needs the
        // Rs below to correct the one dibit a slip still costs.
        modes[SpeedLevel::Sl6 as usize] = Some("QPSK250-D");
        // Everything above SL6 is OFDM. The coherent single-carrier rungs that used to sit here
        // (QPSK250 uncoded, QPSK500 uncoded, 8PSK500+Rs) decode ~0 % on `moderate_f1` at *any* SNR up
        // to 40 dB, and none of them is rescuable: FEC does not help (QPSK250+Rs is also 0.00, because
        // the defect is carrier tracking, not errors), and differential does not scale to 8PSK
        // (8PSK500-D measured 0.125 at 40 dB, at a ~4–6 dB AWGN cost — ±22.5° cannot absorb
        // differential's noise doubling). Robustness tracks phase margin, and the margin runs out.
        //
        // OFDM is the mechanism that survives instead: the cyclic prefix rides the delay spread and the
        // per-subcarrier pilots track the fade. Measured on `moderate_f1`, OFDM52 decodes 0.58/0.75/0.83
        // at 8/12/16 dB where 8PSK500 decodes 0.00 at all three. At equal gross rate OFDM also beats
        // SC-FDMA on selective fade (`tests/ofdm_scfdma_bakeoff.rs`: moderate_f1 @20 dB 16QAM OFDM 0.88
        // vs SCFDMA 0.35; moderate_f2 0.93 vs 0.03), which is why the dense rungs are OFDM too.
        modes[SpeedLevel::Sl7 as usize] = Some("OFDM52");
        modes[SpeedLevel::Sl8 as usize] = Some("OFDM52-8PSK");
        modes[SpeedLevel::Sl9 as usize] = Some("OFDM52-16QAM");
        modes[SpeedLevel::Sl10 as usize] = Some("OFDM52-32QAM");
        modes[SpeedLevel::Sl11 as usize] = Some("OFDM52-64QAM");
        // SL12–SL14 re-use SL9–SL11's modes at r≈8/9 LDPC: same modulation, lighter coding — a MODCOD
        // pair. 64QAM is the densest constellation the plugin has, so above SL11 code rate is the only
        // lever left.
        modes[SpeedLevel::Sl12 as usize] = Some("OFDM52-16QAM");
        modes[SpeedLevel::Sl13 as usize] = Some("OFDM52-32QAM");
        modes[SpeedLevel::Sl14 as usize] = Some("OFDM52-64QAM");
        // Per-level FEC (MODCOD). **Every rung is coded** — on a fade there is no such thing as a
        // useful uncoded rung here. SL2–SL5 carry `RsStrong` (t=32): differential BPSK rides the fade
        // rotation but still needs a code to fix the symbols a slip costs (#923's law), and RsStrong is
        // free on the wire for payloads ≤191 B (same 255-byte block as Rs). SL6 = differential
        // QPSK250-D + Rs. SL7+ are OFDM at soft-concatenated FEC (they only ever run FEC-protected; the
        // soft LLRs are per-subcarrier |H|²-weighted, which is where OFDM's fade advantage is realised).
        // Assigned from `tests/snr_floor_calibration.rs::calibrate_fade_aware_ladder`.
        let mut fec_modes = [None; 21];
        fec_modes[SpeedLevel::Sl1 as usize] = Some(FecMode::Rs); // MFSK16 sub-floor: one RS block
        fec_modes[SpeedLevel::Sl2 as usize] = Some(FecMode::Rs);
        fec_modes[SpeedLevel::Sl3 as usize] = Some(FecMode::Rs);
        fec_modes[SpeedLevel::Sl4 as usize] = Some(FecMode::Rs);
        fec_modes[SpeedLevel::Sl5 as usize] = Some(FecMode::Rs);
        fec_modes[SpeedLevel::Sl6 as usize] = Some(FecMode::Rs);
        fec_modes[SpeedLevel::Sl7 as usize] = Some(FecMode::SoftConcatenated);
        fec_modes[SpeedLevel::Sl8 as usize] = Some(FecMode::SoftConcatenated);
        fec_modes[SpeedLevel::Sl9 as usize] = Some(FecMode::SoftConcatenated);
        fec_modes[SpeedLevel::Sl10 as usize] = Some(FecMode::SoftConcatenated);
        fec_modes[SpeedLevel::Sl11 as usize] = Some(FecMode::SoftConcatenated);
        fec_modes[SpeedLevel::Sl12 as usize] = Some(FecMode::LdpcHighRate);
        fec_modes[SpeedLevel::Sl13 as usize] = Some(FecMode::LdpcHighRate);
        fec_modes[SpeedLevel::Sl14 as usize] = Some(FecMode::LdpcHighRate);
        // SNR floors — the SNR/step pairs the fast-downshift jumps to; monotonic across the ladder.
        // SL2–SL5 keep the floors they always had: those were never the problem. Measured on
        // `moderate_f1` **at these exact floors**, the coded rungs now meet them (BPSK31 @3 dB 1.00,
        // BPSK100 @4.5 dB ~1.00, BPSK250 @5 dB 0.58 — against 0.00/0.04/0.00 uncoded). The rungs lacked
        // a code, not headroom, so lowering the floors to the coded AWGN numbers (BPSK250+RsStrong AWGN
        // floor ≈ -1 dB) would only move them somewhere a fade kills them anyway.
        // SL7 (OFDM52) = 10: it clears the 50 % fading target by ~8 dB (measured 0.58/0.75/0.83 at
        // 8/12/16 dB on moderate_f1), and 10 sits between SL6's 7 and SL8's 14. The dense OFDM rungs
        // (SL8–SL14) keep their ≈+8 dB fading margin over OFDM's measured AWGN floors (8PSK 6, 16QAM 8,
        // 32QAM 10, 64QAM 14; 16QAM-LHR 12, 32QAM-LHR 16, 64QAM-LHR 20 — see
        // `ldpc_ladder_rungs::measure_ofdm_floors`). Re-run the sweeps if the DSP changes.
        let mut snr_floors = [None; 21];
        snr_floors[SpeedLevel::Sl2 as usize] = Some(3.0_f32);
        snr_floors[SpeedLevel::Sl3 as usize] = Some(4.0_f32);
        snr_floors[SpeedLevel::Sl4 as usize] = Some(4.5_f32);
        snr_floors[SpeedLevel::Sl5 as usize] = Some(5.0_f32);
        snr_floors[SpeedLevel::Sl6 as usize] = Some(7.0_f32);
        snr_floors[SpeedLevel::Sl7 as usize] = Some(9.0_f32); // OFDM52       +SC
        snr_floors[SpeedLevel::Sl8 as usize] = Some(10.0_f32); // OFDM52-8PSK  +SC
        snr_floors[SpeedLevel::Sl9 as usize] = Some(12.0_f32); // OFDM52-16QAM +SC
        snr_floors[SpeedLevel::Sl10 as usize] = Some(14.0_f32); // OFDM52-32QAM +SC
        snr_floors[SpeedLevel::Sl11 as usize] = Some(16.0_f32); // OFDM52-64QAM +SC
        snr_floors[SpeedLevel::Sl12 as usize] = Some(18.0_f32); // OFDM52-16QAM +LHR
        snr_floors[SpeedLevel::Sl13 as usize] = Some(19.0_f32); // OFDM52-32QAM +LHR
        snr_floors[SpeedLevel::Sl14 as usize] = Some(20.0_f32); // OFDM52-64QAM +LHR (ladder top)
                                                                // Ceilings gate the cautious one-step upshift: a uniform +2 dB hysteresis over the next rung's
                                                                // floor — `ceiling(L) = floor(L+1) + 2` — so every rung dwells the same margin before climbing.
                                                                // Reachability holds (ceiling(L) > floor(L+1)). SL14 is the top rung — no ceiling.
        let mut snr_ceilings = [None; 21];
        snr_ceilings[SpeedLevel::Sl1 as usize] = Some(5.0_f32); // floor(SL2)=3 +2 → climb out of the sub-floor
        snr_ceilings[SpeedLevel::Sl2 as usize] = Some(6.0_f32); // floor(SL3)=4 +2
        snr_ceilings[SpeedLevel::Sl3 as usize] = Some(6.5_f32); // floor(SL4)=4.5 +2
        snr_ceilings[SpeedLevel::Sl4 as usize] = Some(7.0_f32); // floor(SL5)=5 +2
        snr_ceilings[SpeedLevel::Sl5 as usize] = Some(9.0_f32); // floor(SL6)=7 +2
        snr_ceilings[SpeedLevel::Sl6 as usize] = Some(11.0_f32); // floor(SL7)=9 +2
        snr_ceilings[SpeedLevel::Sl7 as usize] = Some(12.0_f32); // floor(SL8)=10 +2
        snr_ceilings[SpeedLevel::Sl8 as usize] = Some(14.0_f32); // floor(SL9)=12 +2
        snr_ceilings[SpeedLevel::Sl9 as usize] = Some(16.0_f32); // floor(SL10)=14 +2
        snr_ceilings[SpeedLevel::Sl10 as usize] = Some(18.0_f32); // floor(SL11)=16 +2
        snr_ceilings[SpeedLevel::Sl11 as usize] = Some(20.0_f32); // floor(SL12)=18 +2
        snr_ceilings[SpeedLevel::Sl12 as usize] = Some(21.0_f32); // floor(SL13)=19 +2
        snr_ceilings[SpeedLevel::Sl13 as usize] = Some(22.0_f32); // floor(SL14)=20 +2
        Self {
            modes,
            initial_level: SpeedLevel::Sl2,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
            // Guard admission to the densest rung (SL14, 64QAM at r≈8/9) behind a prior SNR upgrade
            // candidate, mirroring hpx_wideband_hd.
            ack_up_requires_snr_candidate_at: Some(SpeedLevel::Sl14),
            fec_modes,
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
            fec_modes: [None; 21],
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
            fec_modes: [None; 21],
        }
    }

    /// HPX OFDM HF profile: multi-carrier HF ladder (SL5–SL6), capped at 2031 Hz BW.
    ///
    /// Both modes fit within the 2700 Hz HF channel-width limit.  Channel equalization
    /// (LS estimate + ZF) provides robustness against frequency-selective HF fading that
    /// single-carrier modes cannot achieve without an equalizer.
    ///
    /// | SL  | Mode         | BW        | Gross bps |
    /// |-----|--------------|-----------|-----------|
    /// | SL5 | OFDM16       | ≈ 625 Hz  | ≈ 889     |
    /// | SL6 | OFDM52       | ≈ 2031 Hz | ≈ 2889    |
    /// | SL7 | OFDM52-8PSK  | ≈ 2031 Hz | ≈ 4333    |
    /// | SL8 | OFDM52-16QAM | ≈ 2031 Hz | ≈ 5778    |
    /// | SL9 | OFDM52-32QAM | ≈ 2031 Hz | ≈ 7222    |
    /// | SL10| OFDM52-64QAM | ≈ 2031 Hz | ≈ 8667    |
    ///
    /// The higher-order rungs (SL7+) run FEC-protected (soft); OFDM's per-subcarrier
    /// equalization handles frequency-selective HF fading better than SC-FDMA (no
    /// DFT-despread noise enhancement), making this the high-throughput /
    /// high-reliability HF path.  All rungs fit the 2700 Hz channel.
    pub fn hpx_ofdm_hf() -> Self {
        let mut modes = [None; 21];
        modes[SpeedLevel::Sl5 as usize] = Some("OFDM16");
        modes[SpeedLevel::Sl6 as usize] = Some("OFDM52");
        modes[SpeedLevel::Sl7 as usize] = Some("OFDM52-8PSK");
        modes[SpeedLevel::Sl8 as usize] = Some("OFDM52-16QAM");
        modes[SpeedLevel::Sl9 as usize] = Some("OFDM52-32QAM");
        modes[SpeedLevel::Sl10 as usize] = Some("OFDM52-64QAM");
        // Floors/ceilings are in the units the receiver-led ladder actually reads: the plugin
        // symbol-domain SNR (`ModemEngine::rx_snr_db`), which for OFDM is *conservative* (ZF
        // noise-enhancement on faded subcarriers) and saturates near ~16 dB — it physically cannot
        // report the 20–30 dB the dense rungs run at. This is why the OFDM floors are on a DIFFERENT
        // scale from the single-carrier rungs (which read ~true channel SNR): the two are per-family
        // by physical necessity, not a wart, and cannot be unified — forcing OFDM onto a true-SNR
        // scale would put these floors above anything the estimate can read and stall the SNR climb
        // (the pre-2026-07 bug). The evidence-based climb bridges it; the boundary is pinned by
        // `openpulse-modem/tests/snr_scale_boundary.rs`. Calibrated from measured (plugin-SNR, decode)
        // pairs on moderate_f1 (`ldpc_ladder_rungs`-style sweep): each rung decodes ≥ 0.8 once the
        // plugin reads its floor.
        let mut snr_floors = [None; 21];
        snr_floors[SpeedLevel::Sl5 as usize] = Some(8.0_f32); // OFDM16
        snr_floors[SpeedLevel::Sl6 as usize] = Some(9.0_f32); // OFDM52
        snr_floors[SpeedLevel::Sl7 as usize] = Some(10.0_f32); // OFDM52-8PSK
        snr_floors[SpeedLevel::Sl8 as usize] = Some(12.0_f32); // OFDM52-16QAM
        snr_floors[SpeedLevel::Sl9 as usize] = Some(14.0_f32); // OFDM52-32QAM
        snr_floors[SpeedLevel::Sl10 as usize] = Some(16.0_f32); // OFDM52-64QAM
        let mut snr_ceilings = [None; 21];
        snr_ceilings[SpeedLevel::Sl5 as usize] = Some(11.0_f32); // floor(SL6)=9 +2
        snr_ceilings[SpeedLevel::Sl6 as usize] = Some(12.0_f32); // floor(SL7)=10 +2
        snr_ceilings[SpeedLevel::Sl7 as usize] = Some(14.0_f32); // floor(SL8)=12 +2
        snr_ceilings[SpeedLevel::Sl8 as usize] = Some(16.0_f32); // floor(SL9)=14 +2
        snr_ceilings[SpeedLevel::Sl9 as usize] = Some(18.0_f32); // floor(SL10)=16 +2
                                                                 // SL10 (OFDM52-64QAM) is the ceiling; no upgrade above it.
                                                                 // Every rung carries SoftConcatenated FEC. The unprotected OFDM16/OFDM52 entry rungs failed ~50 %
                                                                 // of moderate_f1 frames (a single faded subcarrier corrupts a byte with no FEC → the ladder stuck
                                                                 // on an unreliable rung); SoftConcatenated's soft LLRs (per-subcarrier |H|²-weighted) take them to
                                                                 // ≥ 0.9. It does NOT hit the padded-RS-block geometry problem plain RS did on OFDM16/OFDM52.
        let mut fec_modes = [None; 21];
        for sl in [
            SpeedLevel::Sl5,
            SpeedLevel::Sl6,
            SpeedLevel::Sl7,
            SpeedLevel::Sl8,
            SpeedLevel::Sl9,
            SpeedLevel::Sl10,
        ] {
            fec_modes[sl as usize] = Some(FecMode::SoftConcatenated);
        }
        Self {
            modes,
            initial_level: SpeedLevel::Sl5,
            nack_threshold: 3,
            snr_floors,
            snr_ceilings,
            // Gate admission to the densest rung behind a prior SNR-upgrade candidate.
            ack_up_requires_snr_candidate_at: Some(SpeedLevel::Sl10),
            fec_modes,
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
            fec_modes: [None; 21],
        }
    }

    /// HPX Wideband HD profile: SC-FDMA crossover ladder (SL12–SL15).
    ///
    /// For VHF/UHF FM, microwave, and satellite links where the 2700 Hz HF bandwidth
    /// ceiling does not apply and SNR margins of 16–40 dB are achievable.
    /// Not suitable for HF ionospheric paths (Watterson fading breaks QAM coherence).
    ///
    /// | SL   | Mode              | Gross bps (8 kHz audio) | Min SNR | Real-audio status |
    /// |------|-------------------|-------------------------|---------|---|
    /// | SL12 | SCFDMA52-16QAM   | ≈ 5778                  | 16 dB   | PASS |
    /// | SL13 | SCFDMA52-32QAM   | ≈ 7222                  | 20 dB   | PASS |
    /// | SL14 | SCFDMA52-64QAM   | ≈ 8667                  | 28 dB   | **marginal — 3/5** |
    /// | SL15 | 64QAM2000-RRC    | ≈ 12000                 | 35 dB   | PASS 3/3 |
    ///
    /// Real-audio status is the dual-card hardware loopback with `soft-concatenated` FEC
    /// (2026-07-22), which is the first time these rungs were measured on a correctly-normalised rig —
    /// an earlier sweep recorded all four as failures while the capture AGC was live, and that was a
    /// property of the rig rather than of the waveforms (`docs/dev/dualcard-loopback.md`).
    ///
    /// **SL14 is the rung to watch.** It decodes 3 of 5 attempts on a clean cable at 71 dB SNR, so it
    /// is at the edge of what a DFT-spread 64QAM waveform holds on a real analog path rather than
    /// comfortably inside it. The ladder reaches it only on an evidence-based climb, and demotes off
    /// it on failure, so a marginal rung costs retries rather than correctness — but do not read the
    /// 28 dB floor as the only thing standing between a link and this rate.
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
            fec_modes: [None; 21],
        }
    }
}
