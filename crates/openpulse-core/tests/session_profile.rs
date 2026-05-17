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
    assert_eq!(p.mode_for(SpeedLevel::Sl8), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl11), None);
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
    assert_eq!(p.mode_for(SpeedLevel::Sl7), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl8), None);
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
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl5), Some(14.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl6), None);
}

#[test]
fn hpx_wideband_hd_mode_mapping_uses_crossover_policy() {
    let p = SessionProfile::hpx_wideband_hd();
    assert_eq!(p.mode_for(SpeedLevel::Sl11), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl12), Some("SCFDMA52-64QAM-P4"));
    assert_eq!(p.mode_for(SpeedLevel::Sl13), Some("SCFDMA52-64QAM"));
    assert_eq!(p.mode_for(SpeedLevel::Sl14), Some("64QAM2000-RRC"));
}

#[test]
fn hpx_wideband_hd_snr_thresholds_match_policy_intent() {
    let p = SessionProfile::hpx_wideband_hd();
    assert_eq!(p.initial_level, SpeedLevel::Sl12);
    assert_eq!(p.nack_threshold, 2);
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl12), Some(22.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl13), Some(24.0));
    assert_eq!(p.snr_floor_for_level(SpeedLevel::Sl14), Some(30.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl12), Some(26.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl13), Some(30.0));
    assert_eq!(p.snr_ceiling_for_level(SpeedLevel::Sl14), None);
}
