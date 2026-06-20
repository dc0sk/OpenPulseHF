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
    assert_eq!(p.mode_for(SpeedLevel::Sl1), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl2), Some("BPSK31"));
    assert_eq!(p.mode_for(SpeedLevel::Sl3), Some("BPSK63"));
    assert_eq!(p.mode_for(SpeedLevel::Sl4), Some("BPSK250"));
    assert_eq!(p.mode_for(SpeedLevel::Sl5), Some("QPSK250"));
    assert_eq!(p.mode_for(SpeedLevel::Sl6), Some("QPSK500"));
    assert_eq!(p.mode_for(SpeedLevel::Sl7), Some("8PSK500"));
    assert_eq!(p.mode_for(SpeedLevel::Sl8), Some("SCFDMA52-8PSK"));
    // SL9–SL11: higher-order SC-FDMA, all ≤2 kHz (HF-legal).
    assert_eq!(p.mode_for(SpeedLevel::Sl9), Some("SCFDMA52-16QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl10), Some("SCFDMA52-32QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl11), Some("SCFDMA52-64QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl12), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl13), None);
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
    let p = SessionProfile::hpx_ofdm_hf();
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl5), Some(8.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl6), Some(11.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl10), Some(26.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl5), Some(14.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl6), Some(18.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl10), None);
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
