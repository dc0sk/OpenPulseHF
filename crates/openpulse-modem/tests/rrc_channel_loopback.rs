use openpulse_channel::{awgn::AwgnChannel, AwgnConfig};
use openpulse_modem::channel_sim::ChannelSimHarness;
use qpsk_plugin::QpskPlugin;

fn make_qpsk_harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    h.tx_engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .expect("tx QPSK registration");
    h.rx_engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .expect("rx QPSK registration");
    h
}

/// QPSK500-RRC clean loopback.
#[test]
fn qpsk500_rrc_clean_loopback() {
    let mut h = make_qpsk_harness();
    let payload = b"RRC matched filter test payload";
    h.tx_engine.transmit(payload, "QPSK500-RRC", None).unwrap();
    h.route_clean();
    let rx = h.rx_engine.receive("QPSK500-RRC", None).unwrap();
    assert_eq!(&rx[..payload.len()], payload);
}

/// QPSK500-RRC through AWGN at 20 dB SNR (seed 42).
#[test]
fn qpsk500_rrc_awgn_20db() {
    let mut h = make_qpsk_harness();
    let payload = b"RRC AWGN 20 dB test payload";
    let mut channel = AwgnChannel::new(AwgnConfig::new(20.0, Some(42))).unwrap();
    h.tx_engine.transmit(payload, "QPSK500-RRC", None).unwrap();
    h.route(&mut channel);
    let rx = h.rx_engine.receive("QPSK500-RRC", None).unwrap();
    assert_eq!(&rx[..payload.len()], payload);
}
