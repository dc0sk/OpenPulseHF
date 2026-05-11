//! `openpulse-server` binary — wraps the modem engine and exposes the NDJSON
//! control port on TCP port 9000 and WebSocket port 9001 (defaults).

use openpulse_audio::LoopbackBackend;
use openpulse_daemon::{ws, ControlServer};
use openpulse_modem::ModemEngine;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cfg = openpulse_config::load().unwrap_or_default();
    let mode = cfg.modem.mode.clone();

    let audio = Box::new(LoopbackBackend::default());
    let engine = ModemEngine::new(audio);

    let tcp_bind: std::net::SocketAddr = "127.0.0.1:9000".parse().unwrap();
    let ws_bind: std::net::SocketAddr = "127.0.0.1:9001".parse().unwrap();

    let mut handle = ControlServer::spawn(tcp_bind, &engine, mode, None)
        .await
        .expect("failed to bind TCP control port");

    tracing::info!("openpulse-server TCP control port listening on {tcp_bind}");

    ws::spawn_ws(
        ws_bind,
        &engine,
        ws::WsShared {
            ev_tx: handle.event_tx.clone(),
            cmd_tx: handle.command_tx.clone(),
            active_mode: handle.active_mode.clone(),
            tx_attenuation_db: handle.tx_attenuation_db.clone(),
            spectrum_tap: handle.spectrum_tap.clone(),
        },
        None,
    )
    .await
    .expect("failed to bind WebSocket control port");

    tracing::info!("openpulse-server WebSocket control port listening on {ws_bind}");

    // Drain command channel; in a real deployment additional handlers would
    // act on each command (set frequency on rig, toggle repeater, etc.).
    while let Some(cmd) = handle.commands.recv().await {
        tracing::info!(?cmd, "control command received");
    }
}
