use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;

#[test]
fn hpx500_mode_mapping() {
    let p = SessionProfile::hpx500();
    assert_eq!(p.mode_for(SpeedLevel::Sl1), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl2), Some("BPSK31"));
    assert_eq!(p.mode_for(SpeedLevel::Sl3), Some("BPSK63"));
    assert_eq!(p.mode_for(SpeedLevel::Sl4), Some("BPSK250"));
    assert_eq!(p.mode_for(SpeedLevel::Sl5), Some("QPSK250"));
    assert_eq!(p.mode_for(SpeedLevel::Sl6), Some("QPSK500"));
    assert_eq!(p.mode_for(SpeedLevel::Sl7), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl8), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl11), None);
}

#[test]
fn hpx500_initial_level() {
    let p = SessionProfile::hpx500();
    assert_eq!(p.initial_level, SpeedLevel::Sl2);
    assert_eq!(p.nack_threshold, 3);
}

#[test]
fn hpx_pilot_mode_mapping() {
    let p = SessionProfile::hpx_pilot();
    assert_eq!(p.mode_for(SpeedLevel::Sl1), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl2), Some("PILOT-QPSK500"));
    assert_eq!(p.mode_for(SpeedLevel::Sl3), Some("PILOT-8PSK500"));
    assert_eq!(p.mode_for(SpeedLevel::Sl4), Some("PILOT-16QAM500"));
    assert_eq!(p.mode_for(SpeedLevel::Sl5), Some("PILOT-32APSK500"));
    assert_eq!(p.mode_for(SpeedLevel::Sl6), None);
    assert_eq!(p.initial_level, SpeedLevel::Sl2);
    assert_eq!(p.nack_threshold, 3);
}

#[test]
fn hpx_pilot_rrc_is_the_narrowband_sibling() {
    let rect = SessionProfile::hpx_pilot();
    let rrc = SessionProfile::hpx_pilot_rrc();
    assert_eq!(SessionProfile::by_name("hpx_pilot_rrc"), Some(rrc.clone()));
    // Same ladder shape on the -RRC variants.
    assert_eq!(rrc.mode_for(SpeedLevel::Sl2), Some("PILOT-QPSK500-RRC"));
    assert_eq!(rrc.mode_for(SpeedLevel::Sl3), Some("PILOT-8PSK500-RRC"));
    assert_eq!(rrc.mode_for(SpeedLevel::Sl4), Some("PILOT-16QAM500-RRC"));
    assert_eq!(rrc.mode_for(SpeedLevel::Sl5), Some("PILOT-32APSK500-RRC"));
    // Identical control: same levels, initial, thresholds — only the pulse differs.
    assert_eq!(rrc.defined_levels(), rect.defined_levels());
    assert_eq!(rrc.initial_level, rect.initial_level);
    assert_eq!(rrc.nack_threshold, rect.nack_threshold);
    for sl in rrc.defined_levels() {
        assert_eq!(
            rrc.snr_floor_for_level(sl),
            rect.snr_floor_for_level(sl),
            "floor {sl:?}"
        );
        assert_eq!(
            rrc.snr_ceiling_for_level(sl),
            rect.snr_ceiling_for_level(sl),
            "ceiling {sl:?}"
        );
    }
}

#[test]
fn hpx_pilot_fast_is_the_high_throughput_ladder() {
    let base = SessionProfile::hpx_pilot();
    let fast = SessionProfile::hpx_pilot_fast();
    assert_eq!(
        SessionProfile::by_name("hpx_pilot_fast"),
        Some(fast.clone())
    );
    assert_eq!(fast.mode_for(SpeedLevel::Sl2), Some("PILOT-QPSK1000"));
    assert_eq!(fast.mode_for(SpeedLevel::Sl3), Some("PILOT-8PSK1000"));
    assert_eq!(fast.mode_for(SpeedLevel::Sl4), Some("PILOT-16QAM1000"));
    assert_eq!(fast.mode_for(SpeedLevel::Sl5), Some("PILOT-32APSK1000"));
    // Same per-symbol (Es/N0) thresholds and control as the 500-baud ladder.
    assert_eq!(fast.defined_levels(), base.defined_levels());
    assert_eq!(fast.initial_level, base.initial_level);
    for sl in fast.defined_levels() {
        assert_eq!(
            fast.snr_floor_for_level(sl),
            base.snr_floor_for_level(sl),
            "floor {sl:?}"
        );
    }
}

#[test]
fn hpx_pilot_fast_rrc_combines_throughput_and_narrowband() {
    let fast_rrc = SessionProfile::hpx_pilot_fast_rrc();
    assert_eq!(
        SessionProfile::by_name("hpx_pilot_fast_rrc"),
        Some(fast_rrc.clone())
    );
    assert_eq!(
        fast_rrc.mode_for(SpeedLevel::Sl2),
        Some("PILOT-QPSK1000-RRC")
    );
    assert_eq!(
        fast_rrc.mode_for(SpeedLevel::Sl3),
        Some("PILOT-8PSK1000-RRC")
    );
    assert_eq!(
        fast_rrc.mode_for(SpeedLevel::Sl4),
        Some("PILOT-16QAM1000-RRC")
    );
    assert_eq!(
        fast_rrc.mode_for(SpeedLevel::Sl5),
        Some("PILOT-32APSK1000-RRC")
    );
    // Same per-symbol floors as the base pilot ladder.
    let base = SessionProfile::hpx_pilot();
    for sl in fast_rrc.defined_levels() {
        assert_eq!(
            fast_rrc.snr_floor_for_level(sl),
            base.snr_floor_for_level(sl),
            "floor {sl:?}"
        );
    }
}

#[test]
fn hpx_pilot_by_name_and_thresholds() {
    assert_eq!(
        SessionProfile::by_name("hpx_pilot"),
        Some(SessionProfile::hpx_pilot())
    );
    let p = SessionProfile::hpx_pilot();
    assert_eq!(
        p.defined_levels(),
        vec![
            SpeedLevel::Sl2,
            SpeedLevel::Sl3,
            SpeedLevel::Sl4,
            SpeedLevel::Sl5
        ]
    );
    // Monotone, density-ordered SNR thresholds (QPSK < 8PSK < 16QAM < 32APSK).
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl2), Some(6.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl3), Some(12.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl4), Some(17.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl5), Some(23.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl4), Some(23.0));
}

#[test]
fn hpx_wideband_mode_mapping() {
    let p = SessionProfile::hpx_wideband();
    assert_eq!(p.mode_for(SpeedLevel::Sl1), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl7), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl8), Some("QPSK500"));
    assert_eq!(p.mode_for(SpeedLevel::Sl9), Some("QPSK1000"));
    assert_eq!(p.mode_for(SpeedLevel::Sl10), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl11), Some("8PSK1000"));
}

#[test]
fn hpx_wideband_initial_level() {
    let p = SessionProfile::hpx_wideband();
    assert_eq!(p.initial_level, SpeedLevel::Sl8);
    assert_eq!(p.nack_threshold, 3);
}

#[test]
fn hpx_hf_mode_mapping() {
    let p = SessionProfile::hpx_hf();
    // SL1 = the MFSK16 non-coherent sub-floor rung (the ChirpFallback deep-fade waveform), one RS block.
    assert_eq!(p.mode_for(SpeedLevel::Sl1), Some("MFSK16"));
    assert_eq!(p.fec_for(SpeedLevel::Sl1), openpulse_core::fec::FecMode::Rs);
    // Finer ladder (research #2): BPSK100 + QPSK250+Rs + SCFDMA26-32QAM + SCFDMA52-64QAM-P4 inserts.
    assert_eq!(p.mode_for(SpeedLevel::Sl2), Some("BPSK31"));
    assert_eq!(p.mode_for(SpeedLevel::Sl3), Some("BPSK63"));
    assert_eq!(p.mode_for(SpeedLevel::Sl4), Some("BPSK100"));
    assert_eq!(p.mode_for(SpeedLevel::Sl5), Some("BPSK250"));
    assert_eq!(p.mode_for(SpeedLevel::Sl6), Some("QPSK250")); // + Rs (MODCOD)
    assert_eq!(p.mode_for(SpeedLevel::Sl7), Some("QPSK250")); // uncoded
    assert_eq!(p.mode_for(SpeedLevel::Sl8), Some("QPSK500"));
    assert_eq!(p.mode_for(SpeedLevel::Sl9), Some("8PSK500"));
    // SL10 stays SC-FDMA (narrowband ~1 kHz fallback); SL11–SL17 re-seated to OFDM (CP rides selective
    // HF fade), all ≤2 kHz (HF-legal). The former P4 dense-pilot rungs were re-indexed out, so the dense
    // ladder is 8 rungs: OFDM52-{8PSK,16QAM,32QAM,64QAM}+SC then 16/32/64QAM at r≈8/9 LDPC.
    assert_eq!(p.mode_for(SpeedLevel::Sl10), Some("SCFDMA26-32QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl11), Some("OFDM52-8PSK"));
    assert_eq!(p.mode_for(SpeedLevel::Sl12), Some("OFDM52-16QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl13), Some("OFDM52-32QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl14), Some("OFDM52-64QAM"));
    // SL15–SL17: the dense OFDM modes at high-rate LDPC (r≈8/9) — MODCOD pairs of SL12–SL14.
    // 64QAM is the densest constellation the plugin has, so above SL14 the only remaining lever on
    // throughput is code rate.
    assert_eq!(p.mode_for(SpeedLevel::Sl15), Some("OFDM52-16QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl16), Some("OFDM52-32QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl17), Some("OFDM52-64QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl18), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl19), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl20), None);
    assert_eq!(
        p.fec_for(SpeedLevel::Sl14),
        openpulse_core::fec::FecMode::SoftConcatenated
    );
    assert_eq!(
        p.fec_for(SpeedLevel::Sl15),
        openpulse_core::fec::FecMode::LdpcHighRate
    );
    assert_eq!(
        p.fec_for(SpeedLevel::Sl17),
        openpulse_core::fec::FecMode::LdpcHighRate
    );
    // SL6/SL7 are the same mode at different FEC — a proper MODCOD rung, not a duplicate.
    assert_eq!(p.fec_for(SpeedLevel::Sl6), openpulse_core::fec::FecMode::Rs);
    assert_eq!(
        p.fec_for(SpeedLevel::Sl7),
        openpulse_core::fec::FecMode::None
    );
}

#[test]
fn hpx_hf_initial_level() {
    let p = SessionProfile::hpx_hf();
    assert_eq!(p.initial_level, SpeedLevel::Sl2);
    assert_eq!(p.nack_threshold, 3);
}

#[test]
fn scfdma_qam_hf_entry_policy_matches_matrix_gate() {
    let policy = SessionProfile::SCFDMA_QAM_HF_ENTRY_POLICY;
    assert_eq!(policy.min_success_rate, 0.90);
    assert_eq!(policy.frames_per_scenario, 30);
    assert_eq!(
        policy.required_scenarios,
        &["good_f1", "good_f2", "moderate_f1"]
    );
}

#[test]
fn hpx_narrowband_mode_mapping() {
    let p = SessionProfile::hpx_narrowband();
    assert_eq!(p.mode_for(SpeedLevel::Sl1), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl7), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl8), Some("QPSK500"));
    assert_eq!(p.mode_for(SpeedLevel::Sl9), Some("QPSK1000"));
    assert_eq!(p.mode_for(SpeedLevel::Sl10), Some("QPSK2000-RRC"));
    assert_eq!(p.mode_for(SpeedLevel::Sl11), Some("8PSK2000-RRC"));
}

#[test]
fn hpx_narrowband_initial_level() {
    let p = SessionProfile::hpx_narrowband();
    assert_eq!(p.initial_level, SpeedLevel::Sl8);
    assert_eq!(p.nack_threshold, 3);
}

#[test]
fn hpx_narrowband_hd_mode_mapping() {
    let p = SessionProfile::hpx_narrowband_hd();
    assert_eq!(p.mode_for(SpeedLevel::Sl1), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl7), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl8), Some("QPSK9600-RRC"));
    assert_eq!(p.mode_for(SpeedLevel::Sl9), Some("8PSK9600-RRC"));
    assert_eq!(p.mode_for(SpeedLevel::Sl10), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl11), None);
}

#[test]
fn hpx_narrowband_hd_initial_level() {
    let p = SessionProfile::hpx_narrowband_hd();
    assert_eq!(p.initial_level, SpeedLevel::Sl8);
    assert_eq!(p.nack_threshold, 3);
}

#[test]
fn hpx_ofdm_hf_mode_mapping() {
    let p = SessionProfile::hpx_ofdm_hf();
    assert_eq!(p.mode_for(SpeedLevel::Sl1), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl4), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl5), Some("OFDM16"));
    assert_eq!(p.mode_for(SpeedLevel::Sl6), Some("OFDM52"));
    assert_eq!(p.mode_for(SpeedLevel::Sl7), Some("OFDM52-8PSK"));
    assert_eq!(p.mode_for(SpeedLevel::Sl8), Some("OFDM52-16QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl9), Some("OFDM52-32QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl10), Some("OFDM52-64QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl11), None);
}

#[test]
fn hpx_ofdm_hf_initial_level() {
    let p = SessionProfile::hpx_ofdm_hf();
    assert_eq!(p.initial_level, SpeedLevel::Sl5);
    assert_eq!(p.nack_threshold, 3);
}

#[test]
fn hpx_ofdm_hf_snr_thresholds() {
    // Floors are in plugin-SNR units (what the receiver-led ladder reads), calibrated on moderate_f1
    // where that estimate is conservative and saturates ~17 dB — the AWGN-scale numbers never cleared.
    let p = SessionProfile::hpx_ofdm_hf();
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl5), Some(8.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl6), Some(9.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl10), Some(16.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl5), Some(11.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl6), Some(12.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl10), None);
    // Every rung is FEC-protected now (SoftConcatenated) — the unprotected entry rungs failed on fading.
    assert_eq!(
        p.fec_for(SpeedLevel::Sl5),
        openpulse_core::fec::FecMode::SoftConcatenated
    );
}

#[test]
fn hpx_wideband_hd_mode_mapping_uses_crossover_policy() {
    let p = SessionProfile::hpx_wideband_hd();
    // SL9–SL11: narrowband (half-width) HOM fallback rungs.
    assert_eq!(p.mode_for(SpeedLevel::Sl9), Some("SCFDMA26-8PSK"));
    assert_eq!(p.mode_for(SpeedLevel::Sl10), Some("SCFDMA26-16QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl11), Some("SCFDMA26-32QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl12), Some("SCFDMA52-16QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl13), Some("SCFDMA52-32QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl14), Some("SCFDMA52-64QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl15), Some("64QAM2000-RRC"));
}

#[test]
fn hpx_wideband_hd_snr_thresholds_match_policy_intent() {
    let p = SessionProfile::hpx_wideband_hd();
    assert_eq!(p.initial_level, SpeedLevel::Sl12);
    assert_eq!(p.nack_threshold, 2);
    // Narrowband fallback rungs sit below SL12 with lower floors (more robust).
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl9), Some(9.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl10), Some(11.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl11), Some(13.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl11), Some(16.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl12), Some(16.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl13), Some(20.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl14), Some(28.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl15), Some(35.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl12), Some(20.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl13), Some(26.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl14), Some(33.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl15), None);
}

#[test]
fn hpx_wideband_hd_requires_snr_candidate_before_sl15_ack_up() {
    let p = SessionProfile::hpx_wideband_hd();
    assert_eq!(p.ack_up_requires_snr_candidate_at(), Some(SpeedLevel::Sl14));

    let wideband = SessionProfile::hpx_wideband();
    assert_eq!(wideband.ack_up_requires_snr_candidate_at(), None);
}

#[test]
fn by_name_resolves_every_listed_profile() {
    for name in SessionProfile::PROFILE_NAMES {
        assert!(
            SessionProfile::by_name(name).is_some(),
            "PROFILE_NAMES entry {name:?} must resolve via by_name"
        );
    }
}

#[test]
fn by_name_matches_constructors() {
    assert_eq!(
        SessionProfile::by_name("hpx500"),
        Some(SessionProfile::hpx500())
    );
    assert_eq!(
        SessionProfile::by_name("hpx_hf"),
        Some(SessionProfile::hpx_hf())
    );
    assert_eq!(
        SessionProfile::by_name("hpx_ofdm_hf"),
        Some(SessionProfile::hpx_ofdm_hf())
    );
}

#[test]
fn by_name_normalises_case_and_separators() {
    let canonical = SessionProfile::by_name("hpx_ofdm_hf");
    assert!(canonical.is_some());
    assert_eq!(SessionProfile::by_name("HPX-OFDM-HF"), canonical);
    assert_eq!(SessionProfile::by_name("  Hpx_Ofdm-Hf  "), canonical);
}

#[test]
fn by_name_ofdm_hf_exposes_the_hom_ladder() {
    // The OFDM higher-order ladder (PR #407) must be reachable by name.
    let p = SessionProfile::by_name("hpx_ofdm_hf").expect("ofdm-hf resolves");
    assert_eq!(p.initial_level, SpeedLevel::Sl5);
    assert_eq!(p.mode_for(SpeedLevel::Sl8), Some("OFDM52-16QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl10), Some("OFDM52-64QAM"));
}

#[test]
fn by_name_rejects_unknown() {
    assert_eq!(SessionProfile::by_name("nope"), None);
    assert_eq!(SessionProfile::by_name(""), None);
}

// ── Ladder fingerprint (backward-compat guard) ────────────────────────────────

#[test]
fn fingerprint_is_deterministic_and_distinguishes_profiles() {
    let a = SessionProfile::hpx_hf();
    // Same definition → identical fingerprint (stable across builds/instances).
    assert_eq!(a.fingerprint(), SessionProfile::hpx_hf().fingerprint());
    // Different ladders → different fingerprints.
    assert_ne!(
        SessionProfile::hpx_hf().fingerprint(),
        SessionProfile::hpx500().fingerprint()
    );
    assert_ne!(
        SessionProfile::hpx500().fingerprint(),
        SessionProfile::hpx_modcod().fingerprint()
    );
}

#[test]
fn fingerprint_tracks_mode_and_fec_mapping() {
    // The fingerprint reads only the (level → mode, level → FEC) mapping (see `fingerprint()`), so
    // two profiles with different modes/FEC differ, and the value is a stable u64 (non-zero for a
    // populated ladder) suitable for advertising in the handshake.
    let hf = SessionProfile::hpx_hf();
    assert_ne!(
        hf.fingerprint(),
        0,
        "a populated ladder has a non-trivial fingerprint"
    );
    // hpx_hf and hpx_ofdm_hf share some level numbers but map them to different modes → distinct.
    assert_ne!(
        hf.fingerprint(),
        SessionProfile::by_name("hpx_ofdm_hf")
            .unwrap()
            .fingerprint()
    );
}

/// The ladder is only meaningful if climbing it always costs SNR and always buys throughput. Floors
/// must strictly increase, and each ceiling must be exactly `floor(next) + 2` (the uniform upshift
/// hysteresis normalised in PR #680).
#[test]
fn hpx_hf_floors_are_monotonic_and_ceilings_follow_the_hysteresis_rule() {
    let p = SessionProfile::hpx_hf();
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
    assert_eq!(top, SpeedLevel::Sl17);
    assert!(
        p.snr_ceiling_for_level(top).is_none(),
        "the top rung has no ceiling to climb past"
    );
}
