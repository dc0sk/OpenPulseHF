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
}

#[test]
fn hpx_hf_initial_level() {
    let p = SessionProfile::hpx_hf();
    assert_eq!(p.initial_level, SpeedLevel::Sl2);
    assert_eq!(p.nack_threshold, 3);
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
