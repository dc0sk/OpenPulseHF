//! `openpulse-server` binary — wraps the modem engine and exposes the NDJSON
//! control port on TCP port 9000 (default).

use openpulse_audio::LoopbackBackend;
use openpulse_daemon::ControlServer;
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

    let bind: std::net::SocketAddr = "127.0.0.1:9000".parse().unwrap();
    let mut handle = ControlServer::spawn(bind, &engine, mode, None)
        .await
        .expect("failed to bind control port");

    tracing::info!("openpulse-server listening on {bind}");

    // Drain command channel; in a real deployment additional handlers would
    // act on each command (set frequency on rig, toggle repeater, etc.).
    while let Some(cmd) = handle.commands.recv().await {
        tracing::info!(?cmd, "control command received");
    }
}
