//! `openpulse-server` binary — wraps the modem engine and exposes the NDJSON
//! control port on TCP port 9000 and WebSocket port 9001 (defaults).

use openpulse_audio::LoopbackBackend;
use openpulse_daemon::{apply_command_to_engine, ws, ControlServer};
use openpulse_modem::ModemEngine;
use openpulse_radio::RigctldController;
use std::collections::HashMap;

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use ofdm_plugin::OfdmPlugin;
use psk8_plugin::Psk8Plugin;
use qam64_plugin::Qam64Plugin;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cfg = openpulse_config::load().unwrap_or_default();
    if cfg.station.callsign.trim().eq_ignore_ascii_case("N0CALL") {
        tracing::error!(
            "invalid callsign N0CALL in configuration; set [station].callsign before starting daemon"
        );
        return;
    }

    let mode = cfg.modem.mode.clone();
    let station_id = (
        cfg.station.callsign.clone(),
        cfg.station.grid_square.clone(),
    );
    let initial_qsy_enabled = cfg.qsy.enabled;
    let initial_bandplan_mode = if cfg.qsy.bandplan_awareness_enabled {
        cfg.qsy.bandplan_mode.clone()
    } else {
        "unrestricted".to_string()
    };

    let audio = Box::new(LoopbackBackend::default());
    let mut engine = ModemEngine::new(audio);
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("failed to register BPSK plugin");
    engine
        .register_plugin(Box::new(Fsk4Plugin::new()))
        .expect("failed to register FSK4 plugin");
    engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .expect("failed to register OFDM plugin");
    engine
        .register_plugin(Box::new(Psk8Plugin::new()))
        .expect("failed to register 8PSK plugin");
    engine
        .register_plugin(Box::new(Qam64Plugin::new()))
        .expect("failed to register 64QAM plugin");
    engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .expect("failed to register QPSK plugin");
    engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .expect("failed to register SC-FDMA plugin");

    let tcp_bind: std::net::SocketAddr = "127.0.0.1:9000".parse().unwrap();
    let ws_bind: std::net::SocketAddr = "127.0.0.1:9001".parse().unwrap();

    let mut handle = ControlServer::spawn(
        tcp_bind,
        &engine,
        mode,
        station_id,
        initial_qsy_enabled,
        initial_bandplan_mode,
        None,
    )
    .await
    .expect("failed to bind TCP control port");

    tracing::info!("openpulse-server TCP control port listening on {tcp_bind}");

    ws::spawn_ws(
        ws_bind,
        ws::WsShared {
            ev_tx: handle.event_tx.clone(),
            cmd_tx: handle.command_tx.clone(),
            active_mode: handle.active_mode.clone(),
            tx_attenuation_db: handle.tx_attenuation_db.clone(),
            qsy_enabled: handle.qsy_enabled.clone(),
            bandplan_mode: handle.bandplan_mode.clone(),
            spectrum_tap: handle.spectrum_tap.clone(),
            station_id: handle.station_id.clone(),
            message_store: handle.message_store.clone(),
        },
        None,
    )
    .await
    .expect("failed to bind WebSocket control port");

    tracing::info!("openpulse-server WebSocket control port listening on {ws_bind}");

    let mut rig_controller = match RigctldController::connect(&cfg.radio.rigctld_addr) {
        Ok(controller) => Some(controller),
        Err(err) => {
            tracing::warn!(
                addr = %cfg.radio.rigctld_addr,
                error = %err,
                "rigctld connect failed; set_freq commands will emit command_error"
            );
            None
        }
    };
    let mut repeater_enabled = cfg.repeater.enabled;
    let mut qsy_decisions: HashMap<String, bool> = HashMap::new();

    // Execute side-effectful commands against the live modem engine.
    while let Some(cmd) = handle.commands.recv().await {
        apply_command_to_engine(
            &cmd,
            &mut engine,
            &handle.active_mode,
            &handle.event_tx,
            rig_controller.as_mut(),
            &mut repeater_enabled,
            &mut qsy_decisions,
        )
        .await;
    }
}
