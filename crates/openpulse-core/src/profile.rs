//! Session profiles: the `hpx_hf` rate ladder and the two ways Release 1 runs it, `fast` and `robust`.

use crate::fec::FecMode;
use crate::rate::SpeedLevel;

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
    /// Highest level this profile may use (`None` = the top mapped level). Local policy: excluded
    /// from [`fingerprint`](Self::fingerprint) so a capped and an uncapped station interoperate.
    max_level: Option<SpeedLevel>,
}

impl SessionProfile {
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

    /// The profile's level cap (`None` = uncapped). Rate controllers keep it apart from operator and
    /// host bounds, which may lower it but never raise it.
    pub fn max_level(&self) -> Option<SpeedLevel> {
        self.max_level
    }

    /// The defined levels at or below the profile's cap, ascending — what a session may use.
    pub fn reachable_levels(&self) -> Vec<SpeedLevel> {
        let cap = self.max_level;
        self.defined_levels()
            .into_iter()
            .filter(|&l| cap.is_none_or(|c| l <= c))
            .collect()
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

    /// Profile names accepted by [`SessionProfile::by_name`].
    pub const PROFILE_NAMES: &'static [&'static str] = &["fast", "robust"];

    /// Construct a profile by name (case-insensitive, surrounding whitespace ignored).
    ///
    /// Returns `None` for an unrecognised name; see [`SessionProfile::PROFILE_NAMES`].
    pub fn by_name(name: &str) -> Option<SessionProfile> {
        match name.trim().to_ascii_lowercase().as_str() {
            "fast" => Some(Self::fast()),
            "robust" => Some(Self::robust()),
            _ => None,
        }
    }

    /// Build a ladder from explicit `(level, mode, fec, snr_floor, snr_ceiling)` rungs, uncapped.
    ///
    /// For test apparatus that needs a ladder shape the shipped profiles do not have (no SL1 rung,
    /// an uncoded wide ladder). Not reachable through [`by_name`](Self::by_name): operators choose
    /// `fast` or `robust`.
    #[allow(clippy::type_complexity)]
    pub fn from_rungs(
        rungs: &[(SpeedLevel, &'static str, FecMode, Option<f32>, Option<f32>)],
        initial_level: SpeedLevel,
        nack_threshold: u8,
    ) -> Self {
        let mut p = Self {
            modes: [None; 21],
            initial_level,
            nack_threshold,
            snr_floors: [None; 21],
            snr_ceilings: [None; 21],
            ack_up_requires_snr_candidate_at: None,
            fec_modes: [None; 21],
            max_level: None,
        };
        for &(level, mode, fec, floor, ceiling) in rungs {
            let i = level as usize;
            p.modes[i] = Some(mode);
            p.fec_modes[i] = Some(fec);
            p.snr_floors[i] = floor;
            p.snr_ceilings[i] = ceiling;
        }
        p
    }

    /// `robust`: the [`fast`](Self::fast) ladder capped at SL6 — MFSK16, BPSK31–250 and QPSK250-D,
    /// all coded, single-carrier, ≤ 500 Hz occupied. For poor conditions, narrow filters and small or
    /// non-linear PAs. Same rungs as `fast`, so the two interoperate (same ladder fingerprint).
    pub fn robust() -> Self {
        Self {
            max_level: Some(SpeedLevel::Sl6),
            ..Self::fast()
        }
    }

    /// `fast`: the full `hpx_hf` rate ladder (SL1–SL14), ~9 bps to 7.7 kbps, up to ≈2031 Hz occupied.
    ///
    /// Every mode here fits within the 2700 Hz HF channel-width limit (SCFDMA52-* is ≈2031 Hz), so the
    /// ladder spans weak-signal BPSK31 all the way to 64QAM SC-FDMA at code rate ≈8/9.  The dense rungs
    /// (SL10–SL19) always run FEC-protected: soft-concatenated up to SL15, then high-rate LDPC for the
    /// top four — see the table in the body for the mode/FEC/rate/floor of every rung and for why LDPC
    /// appears only above SL15.  For poor conditions or limited gear use
    /// [`robust`](Self::robust), the same ladder capped at SL6.
    pub fn fast() -> Self {
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
        // `moderate_f1`, so it was not a usable fallback. `OFDM16`
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
            // candidate.
            ack_up_requires_snr_candidate_at: Some(SpeedLevel::Sl14),
            fec_modes,
            max_level: None,
        }
    }
}
