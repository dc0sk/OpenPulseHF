use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;

#[test]
fn hpx_hf_mode_mapping() {
    let p = SessionProfile::fast();
    // SL1 = the MFSK16 non-coherent sub-floor rung (the ChirpFallback deep-fade waveform), one RS block.
    assert_eq!(p.mode_for(SpeedLevel::Sl1), Some("MFSK16"));
    assert_eq!(p.fec_for(SpeedLevel::Sl1), openpulse_core::fec::FecMode::Rs);
    // Fade-aware ladder: every rung decodes on Watterson moderate_f1. The uncoded rungs are gone —
    // uncoded BPSK decodes ~0 % of fading frames at its own floor (BPSK31 @3 dB: 0.00 uncoded vs 0.25 with Rs), and the coherent single-carrier mid rungs (QPSK250/QPSK500/8PSK500) decode ~0 %
    // at ANY SNR and are not rescuable, so SL7+ is OFDM.
    assert_eq!(p.mode_for(SpeedLevel::Sl2), Some("BPSK31"));
    assert_eq!(p.mode_for(SpeedLevel::Sl3), Some("BPSK63"));
    assert_eq!(p.mode_for(SpeedLevel::Sl4), Some("BPSK100"));
    assert_eq!(p.mode_for(SpeedLevel::Sl5), Some("BPSK250"));
    assert_eq!(p.mode_for(SpeedLevel::Sl6), Some("QPSK250-D")); // differential + Rs; HF-fade-robust (#923)
    assert_eq!(p.mode_for(SpeedLevel::Sl7), Some("OFDM52"));
    assert_eq!(p.mode_for(SpeedLevel::Sl8), Some("OFDM52-8PSK"));
    assert_eq!(p.mode_for(SpeedLevel::Sl9), Some("OFDM52-16QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl10), Some("OFDM52-32QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl11), Some("OFDM52-64QAM"));
    // SL12–SL14: the dense OFDM modes at high-rate LDPC (r≈8/9) — MODCOD pairs of SL9–SL11. 64QAM is
    // the densest constellation the plugin has, so above SL11 the only lever left is code rate.
    assert_eq!(p.mode_for(SpeedLevel::Sl12), Some("OFDM52-16QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl13), Some("OFDM52-32QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl14), Some("OFDM52-64QAM"));
    // The ladder ends at SL14 — three rungs shorter than the pre-fade-aware version, which carried
    // four rungs that could not decode on a fade.
    assert_eq!(p.mode_for(SpeedLevel::Sl15), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl17), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl18), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl19), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl20), None);
    // Every rung is coded: on a fade there is no useful uncoded rung.
    for (lvl, fec) in [
        (SpeedLevel::Sl2, openpulse_core::fec::FecMode::Rs),
        (SpeedLevel::Sl3, openpulse_core::fec::FecMode::Rs),
        (SpeedLevel::Sl4, openpulse_core::fec::FecMode::Rs),
        (SpeedLevel::Sl5, openpulse_core::fec::FecMode::Rs),
        (SpeedLevel::Sl6, openpulse_core::fec::FecMode::Rs),
        (
            SpeedLevel::Sl7,
            openpulse_core::fec::FecMode::SoftConcatenated,
        ),
        (
            SpeedLevel::Sl11,
            openpulse_core::fec::FecMode::SoftConcatenated,
        ),
        (SpeedLevel::Sl12, openpulse_core::fec::FecMode::LdpcHighRate),
        (SpeedLevel::Sl14, openpulse_core::fec::FecMode::LdpcHighRate),
    ] {
        assert_eq!(p.fec_for(lvl), fec, "FEC for {lvl:?}");
    }
    assert_ne!(
        p.fec_for(SpeedLevel::Sl2),
        openpulse_core::fec::FecMode::None,
        "the entry rung must be coded — uncoded BPSK31 decodes 0% of moderate_f1 frames at every SNR"
    );
}

#[test]
fn hpx_hf_initial_level() {
    let p = SessionProfile::fast();
    assert_eq!(p.initial_level, SpeedLevel::Sl2);
    assert_eq!(p.nack_threshold, 3);
}

#[test]
fn by_name_resolves_every_listed_profile() {
    assert_eq!(SessionProfile::PROFILE_NAMES, &["fast", "robust"]);
    for name in SessionProfile::PROFILE_NAMES {
        assert!(
            SessionProfile::by_name(name).is_some(),
            "PROFILE_NAMES entry {name:?} must resolve via by_name"
        );
    }
}

#[test]
fn by_name_matches_constructors_and_ignores_case() {
    assert_eq!(
        SessionProfile::by_name("fast"),
        Some(SessionProfile::fast())
    );
    assert_eq!(
        SessionProfile::by_name("robust"),
        Some(SessionProfile::robust())
    );
    assert_eq!(
        SessionProfile::by_name("  Robust "),
        Some(SessionProfile::robust())
    );
    assert_eq!(
        SessionProfile::by_name("FAST"),
        Some(SessionProfile::fast())
    );
}

/// Decision 18: the old names are gone with no alias, so a pre-rename config fails loudly instead
/// of silently picking a ladder.
#[test]
fn by_name_rejects_unknown_and_the_retired_names() {
    for name in [
        "nope",
        "",
        "hpx_hf",
        "hpx500",
        "hpx_modcod",
        "hpx_ofdm_hf",
        "hpx_wideband",
        "hpx_wideband_hd",
        "hpx_narrowband",
        "hpx_narrowband_hd",
        "hpx_pilot",
    ] {
        assert_eq!(
            SessionProfile::by_name(name),
            None,
            "{name:?} must not resolve"
        );
    }
}

#[test]
fn robust_is_the_fast_ladder_capped_at_sl6() {
    let (fast, robust) = (SessionProfile::fast(), SessionProfile::robust());
    assert_eq!(fast.max_level(), None);
    assert_eq!(robust.max_level(), Some(SpeedLevel::Sl6));
    for l in fast.defined_levels() {
        assert_eq!(robust.mode_for(l), fast.mode_for(l), "{l:?}");
        assert_eq!(robust.fec_for(l), fast.fec_for(l), "{l:?}");
    }
    assert_eq!(robust.initial_level, fast.initial_level);
    // Every rung `robust` can reach is single-carrier — no OFDM.
    for l in robust
        .defined_levels()
        .into_iter()
        .filter(|&l| l <= SpeedLevel::Sl6)
    {
        let mode = robust.mode_for(l).expect("mapped");
        assert!(!mode.starts_with("OFDM"), "{l:?} = {mode} under the cap");
    }
}

// ── Ladder fingerprint (backward-compat guard) ────────────────────────────────

#[test]
fn fingerprint_is_deterministic_and_non_trivial() {
    let a = SessionProfile::fast();
    assert_eq!(a.fingerprint(), SessionProfile::fast().fingerprint());
    assert_ne!(
        a.fingerprint(),
        0,
        "a populated ladder has a non-trivial fingerprint"
    );
}

/// The cap is local policy: a `fast` and a `robust` station advertise the same ladder, so the
/// handshake's compatibility guard keeps adaptive OTA between them.
#[test]
fn the_cap_is_not_part_of_the_fingerprint() {
    assert_eq!(
        SessionProfile::fast().fingerprint(),
        SessionProfile::robust().fingerprint()
    );
}

/// The ladder is only meaningful if climbing it always costs SNR and always buys throughput. Floors
/// must strictly increase, and each ceiling must be exactly `floor(next) + 2` (the uniform upshift
/// hysteresis normalised in PR #680).
#[test]
fn hpx_hf_floors_are_monotonic_and_ceilings_follow_the_hysteresis_rule() {
    let p = SessionProfile::fast();
    let rungs: Vec<SpeedLevel> = p
        .defined_levels()
        .into_iter()
        .filter(|l| p.mode_for(*l).is_some())
        .collect();

    for pair in rungs.windows(2) {
        let (lo, hi) = (pair[0], pair[1]);
        let f_hi = p.snr_floor_for_level(hi).expect("floor");
        // SL1 (the bottom MFSK16 sub-floor rung) intentionally has no floor — it is never abandoned
        // downward (reached only via ChirpFallback / a sub-3 dB fast-downshift). The floor-ordering
        // invariant applies only where the lower rung has a floor; the ceiling rule holds for every pair.
        if let Some(f_lo) = p.snr_floor_for_level(lo) {
            assert!(
                f_hi > f_lo,
                "floor({hi:?}) = {f_hi} must exceed floor({lo:?}) = {f_lo}"
            );
        }
        let ceiling = p.snr_ceiling_for_level(lo).expect("ceiling");
        assert!(
            (ceiling - (f_hi + 2.0)).abs() < 1e-6,
            "ceiling({lo:?}) = {ceiling} must be floor({hi:?}) + 2 = {}",
            f_hi + 2.0
        );
    }

    let top = *rungs.last().expect("rungs");
    assert_eq!(top, SpeedLevel::Sl14);
    assert!(
        p.snr_ceiling_for_level(top).is_none(),
        "the top rung has no ceiling to climb past"
    );
}
