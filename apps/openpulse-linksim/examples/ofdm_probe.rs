//! TEMPORARY probe: why does 213B zeros OFDM52+Rs fail through the free-strengthening seam?

use openpulse_core::fec::{free_rs_strengthening, FecCodec, FecMode};

fn main() {
    let payload = vec![0u8; 213];
    println!(
        "upgrade decision for len {} => {:?}",
        payload.len() + openpulse_core::frame::Frame::WIRE_OVERHEAD,
        free_rs_strengthening(
            FecMode::Rs,
            payload.len() + openpulse_core::frame::Frame::WIRE_OVERHEAD
        )
    );
    // Raw codec check: encode strong, decode via new-then-strong fallback.
    let framed = vec![0u8; 223];
    let strong_wire = FecCodec::strong().encode(&framed);
    println!(
        "strong wire len {} | new().decode -> {:?} | strong().decode -> {:?}",
        strong_wire.len(),
        FecCodec::new().decode(&strong_wire).map(|v| v.len()),
        FecCodec::strong().decode(&strong_wire).map(|v| v.len()),
    );

    let mut h = openpulse_modem::channel_sim::ChannelSimHarness::new();
    let e = &mut h.tx_engine;
    e.set_cessb_enabled(true);
    e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new())).ok();
    h.rx_engine
        .register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .ok();
    h.tx_engine
        .transmit_with_fec_mode(&payload, "OFDM52", FecMode::Rs, None)
        .unwrap();
    h.route_clean();
    match h.rx_engine.receive_with_fec_mode("OFDM52", FecMode::Rs, None) {
        Ok(rx) => println!(
            "decode OK len {} prefix-matches {}",
            rx.len(),
            rx.len() >= 213 && rx[..213] == payload[..]
        ),
        Err(err) => println!("decode ERR: {err}"),
    }
    // Control: explicit strong both ends.
    let mut h2 = openpulse_modem::channel_sim::ChannelSimHarness::new();
    for eng in [&mut h2.tx_engine, &mut h2.rx_engine] {
        eng.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new())).ok();
        eng.set_cessb_enabled(true);
    }
    h2.tx_engine
        .transmit_with_fec_mode(&payload, "OFDM52", FecMode::RsStrong, None)
        .unwrap();
    h2.route_clean();
    match h2.rx_engine.receive_with_fec_mode("OFDM52", FecMode::RsStrong, None) {
        Ok(rx) => println!("strong/strong OK len {}", rx.len()),
        Err(err) => println!("strong/strong ERR: {err}"),
    }
}
