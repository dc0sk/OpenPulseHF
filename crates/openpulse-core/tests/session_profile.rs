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
fn hpx2300_mode_mapping() {
    let p = SessionProfile::hpx2300();
    assert_eq!(p.mode_for(SpeedLevel::Sl1), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl7), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl8), Some("QPSK500"));
    assert_eq!(p.mode_for(SpeedLevel::Sl9), Some("QPSK1000"));
    assert_eq!(p.mode_for(SpeedLevel::Sl10), None);
    assert_eq!(p.mode_for(SpeedLevel::Sl11), Some("8PSK1000"));
}

#[test]
fn hpx2300_initial_level() {
    let p = SessionProfile::hpx2300();
    assert_eq!(p.initial_level, SpeedLevel::Sl8);
    assert_eq!(p.nack_threshold, 3);
}
