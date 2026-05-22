use ofdm_plugin::OfdmPlugin;
use openpulse_core::frame::Frame;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_modem::channel_sim::ChannelSimHarness;
use qpsk_plugin::QpskPlugin;

fn ofdm_cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    }
}

fn qpsk_cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    }
}

fn make_ofdm_harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    h.tx_engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .unwrap();
    h.rx_engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .unwrap();
    h
}

fn make_qpsk_harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    h.tx_engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .unwrap();
    h.rx_engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .unwrap();
    h
}

/// Verify OFDM52 can round-trip a modem FRAME (not raw payload) through the plugin directly.
#[test]
fn ofdm52_plugin_direct_with_modem_frame() {
    let plugin = OfdmPlugin::new();
    let payload: Vec<u8> = (0..32).map(|i| i as u8).collect();
    let frame = Frame::new(0, payload.clone()).unwrap();
    let frame_bytes = frame.encode();
    assert!(
        frame_bytes.starts_with(b"OPLS"),
        "frame must start with magic"
    );
    println!(
        "Frame bytes len={}: magic={:?}",
        frame_bytes.len(),
        &frame_bytes[..4]
    );

    let samples = plugin.modulate(&frame_bytes, &ofdm_cfg("OFDM52")).unwrap();
    println!("OFDM52 samples: {}", samples.len());

    let recovered = plugin.demodulate(&samples, &ofdm_cfg("OFDM52")).unwrap();
    println!(
        "Recovered len={}, starts_with_OPLS={}",
        recovered.len(),
        recovered.get(..4) == Some(b"OPLS")
    );
    println!("Expected: {:?}", &frame_bytes[..4.min(frame_bytes.len())]);
    println!("Got:      {:?}", &recovered[..4.min(recovered.len())]);

    assert_eq!(recovered, frame_bytes, "plugin direct round-trip must work");
}

/// Verify OFDM52 plugin direct loopback without modem frame wrapping.
#[test]
fn ofdm52_plugin_direct_raw() {
    let plugin = OfdmPlugin::new();
    let payload: Vec<u8> = (0..32).map(|i| i as u8).collect();
    let samples = plugin.modulate(&payload, &ofdm_cfg("OFDM52")).unwrap();
    let rx = plugin.demodulate(&samples, &ofdm_cfg("OFDM52")).unwrap();
    assert_eq!(rx, payload);
}

#[test]
fn ofdm52_clean_32b() {
    let mut h = make_ofdm_harness();
    let payload: Vec<u8> = (0..32).map(|i| i as u8).collect();
    h.tx_engine.transmit(&payload, "OFDM52", None).unwrap();
    h.route_clean();
    let rx = h.rx_engine.receive("OFDM52", None).unwrap();
    assert_eq!(rx, payload);
}

#[test]
fn ofdm16_clean_128b() {
    let mut h = make_ofdm_harness();
    let payload: Vec<u8> = (0..128).map(|i| i as u8).collect();
    h.tx_engine.transmit(&payload, "OFDM16", None).unwrap();
    h.route_clean();
    let rx = h.rx_engine.receive("OFDM16", None).unwrap();
    assert_eq!(rx, payload);
}

/// Direct plugin test for OFDM52 with 255-byte payload (RS block size).
/// Regression guard: PAPR clipping must not corrupt RS-encoded zero-padded blocks.
#[test]
fn ofdm52_plugin_direct_255b() {
    let plugin = OfdmPlugin::new();
    let data: Vec<u8> = (0..255u8).collect();
    let samples = plugin.modulate(&data, &ofdm_cfg("OFDM52")).unwrap();
    let recovered = plugin.demodulate(&samples, &ofdm_cfg("OFDM52")).unwrap();
    assert_eq!(recovered, data);
}

/// Engine harness for OFDM52 + RS FEC + clean channel, 128-byte payload.
/// Regression guard: PAPR clipping must not introduce uncorrectable errors in RS blocks.
#[test]
fn ofdm52_rs_clean_128b_engine() {
    let mut h = make_ofdm_harness();
    let payload: Vec<u8> = (0..128).map(|i| i as u8).collect();
    h.tx_engine
        .transmit_with_fec(&payload, "OFDM52", None)
        .unwrap();
    h.route_clean();
    let rx = h.rx_engine.receive_with_fec("OFDM52", None).unwrap();
    assert_eq!(rx, payload);
}

/// Direct plugin test for QPSK1000-RRC with 223B payload (reproduces the CRC mismatch bug).
#[test]
fn qpsk1000_rrc_plugin_direct_223b() {
    let plugin = QpskPlugin::new();
    let payload: Vec<u8> = (0..223).map(|i| i as u8).collect();
    let frame = Frame::new(0, payload.clone()).unwrap();
    let frame_bytes = frame.encode();
    println!("Frame bytes len={} (wire for 223B)", frame_bytes.len());

    let samples = plugin
        .modulate(&frame_bytes, &qpsk_cfg("QPSK1000-RRC"))
        .unwrap();
    println!("QPSK1000-RRC 223B samples: {}", samples.len());

    let recovered = plugin
        .demodulate(&samples, &qpsk_cfg("QPSK1000-RRC"))
        .unwrap();
    println!(
        "Recovered len={} (expected {})",
        recovered.len(),
        frame_bytes.len()
    );
    if recovered.len() == frame_bytes.len() {
        for (i, (a, b)) in frame_bytes.iter().zip(recovered.iter()).enumerate() {
            if a != b {
                println!("  First diff at byte {i}: expected {a:#04x} got {b:#04x}");
                break;
            }
        }
    } else {
        println!("  Length mismatch!");
    }

    assert_eq!(
        recovered, frame_bytes,
        "QPSK1000-RRC 223B plugin round-trip must work"
    );
}

/// Direct plugin test for QPSK1000-RRC with 128B payload (baseline — should pass).
#[test]
fn qpsk1000_rrc_plugin_direct_128b() {
    let plugin = QpskPlugin::new();
    let payload: Vec<u8> = (0..128).map(|i| i as u8).collect();
    let frame = Frame::new(0, payload.clone()).unwrap();
    let frame_bytes = frame.encode();
    let samples = plugin
        .modulate(&frame_bytes, &qpsk_cfg("QPSK1000-RRC"))
        .unwrap();
    let recovered = plugin
        .demodulate(&samples, &qpsk_cfg("QPSK1000-RRC"))
        .unwrap();
    assert_eq!(
        recovered, frame_bytes,
        "QPSK1000-RRC 128B plugin round-trip must work"
    );
}

#[test]
fn qpsk1000_rrc_clean_223b() {
    let mut h = make_qpsk_harness();
    let payload: Vec<u8> = (0..223).map(|i| i as u8).collect();
    h.tx_engine
        .transmit(&payload, "QPSK1000-RRC", None)
        .unwrap();
    h.route_clean();
    let rx = h.rx_engine.receive("QPSK1000-RRC", None).unwrap();
    assert_eq!(rx, payload);
}

#[test]
fn qpsk1000_rrc_clean_128b() {
    let mut h = make_qpsk_harness();
    let payload: Vec<u8> = (0..128).map(|i| i as u8).collect();
    h.tx_engine
        .transmit(&payload, "QPSK1000-RRC", None)
        .unwrap();
    h.route_clean();
    let rx = h.rx_engine.receive("QPSK1000-RRC", None).unwrap();
    assert_eq!(rx, payload);
}

/// Direct plugin test for QPSK500-RRC with a 255-byte payload (RS block size).
/// This mimics what the engine sends when RS FEC is active on a 128-byte payload.
#[test]
fn qpsk500_rrc_plugin_direct_255b() {
    let plugin = QpskPlugin::new();
    let data: Vec<u8> = (0..255u8).collect();
    let samples = plugin.modulate(&data, &qpsk_cfg("QPSK500-RRC")).unwrap();
    println!("QPSK500-RRC 255B samples: {}", samples.len());
    let recovered = plugin
        .demodulate(&samples, &qpsk_cfg("QPSK500-RRC"))
        .unwrap();
    println!("Recovered len={} (expected 255)", recovered.len());
    if recovered.len() == data.len() {
        for (i, (a, b)) in data.iter().zip(recovered.iter()).enumerate() {
            if a != b {
                println!("  First diff at byte {i}: expected {a:#04x} got {b:#04x}");
                break;
            }
        }
    } else {
        println!("  Length mismatch: got {} expected 255", recovered.len());
    }
    assert_eq!(recovered, data);
}

/// Engine harness for QPSK500-RRC clean 128B (no FEC) — regression guard.
#[test]
fn qpsk500_rrc_clean_128b() {
    let mut h = make_qpsk_harness();
    let payload: Vec<u8> = (0..128).map(|i| i as u8).collect();
    h.tx_engine.transmit(&payload, "QPSK500-RRC", None).unwrap();
    h.route_clean();
    let rx = h.rx_engine.receive("QPSK500-RRC", None).unwrap();
    assert_eq!(rx, payload);
}
