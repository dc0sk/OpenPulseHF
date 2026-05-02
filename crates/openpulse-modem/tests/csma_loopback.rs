use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::error::ModemError;
use openpulse_modem::ModemEngine;

fn make_engine() -> ModemEngine {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register BPSK plugin");
    engine
}

fn make_engine_with_backend(backend: LoopbackBackend) -> ModemEngine {
    let mut engine = ModemEngine::new(Box::new(backend));
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register BPSK plugin");
    engine
}

/// After receiving a signal, DCD should report the channel as busy.
#[test]
fn dcd_detects_energy_from_received_signal() {
    let mut engine = make_engine();
    engine.transmit(b"signal", "BPSK250", None).unwrap();
    let _ = engine.receive("BPSK250", None).unwrap();
    assert!(
        engine.is_channel_busy(),
        "DCD must be busy after receiving a signal"
    );
}

/// When CSMA is enabled and DCD is busy, transmit must return ChannelBusy.
#[test]
fn csma_blocks_transmit_when_dcd_busy() {
    let mut engine = make_engine();

    // Transmit and receive WITHOUT CSMA so DCD fires; then enable CSMA.
    engine.transmit(b"remote", "BPSK250", None).unwrap();
    let _ = engine.receive("BPSK250", None).unwrap();
    assert!(engine.is_channel_busy());

    engine.enable_csma();

    let result = engine.transmit(b"our payload", "BPSK250", None);
    assert!(
        matches!(result, Err(ModemError::ChannelBusy)),
        "CSMA must block transmit on a busy channel, got: {result:?}"
    );
}

/// When CSMA is disabled, transmit must proceed even with DCD busy.
#[test]
fn csma_disabled_ignores_dcd() {
    let mut engine = make_engine();
    // CSMA is off by default.
    engine.transmit(b"remote", "BPSK250", None).unwrap();
    let _ = engine.receive("BPSK250", None).unwrap();
    assert!(engine.is_channel_busy());

    let result = engine.transmit(b"proceed", "BPSK250", None);
    assert!(result.is_ok(), "disabled CSMA must not block transmit");
}

/// Two engines sharing the same loopback buffer: station A transmits, station B
/// reads from the same buffer, DCD fires on B, and B's CSMA defers the next TX.
#[test]
fn two_station_scenario_second_defers_on_dcd() {
    let shared_backend = LoopbackBackend::new();
    let backend_b = shared_backend.clone_shared();

    let mut station_a = make_engine_with_backend(shared_backend);
    let mut station_b = make_engine_with_backend(backend_b);
    station_b.enable_csma();

    // Station A transmits — samples land in the shared buffer.
    station_a
        .transmit(b"station A data", "BPSK250", None)
        .unwrap();

    // Station B reads from the shared buffer — DCD detects A's carrier.
    let _ = station_b.receive("BPSK250", None).unwrap();
    assert!(
        station_b.is_channel_busy(),
        "station B DCD must detect station A's carrier"
    );

    // Station B attempts to transmit — CSMA defers.
    let result = station_b.transmit(b"station B data", "BPSK250", None);
    assert!(
        matches!(result, Err(ModemError::ChannelBusy)),
        "station B must defer while channel is busy"
    );
}
