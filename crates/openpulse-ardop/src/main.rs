use openpulse_ardop::{ArdopConfig, ArdopServer};
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_modem::ModemEngine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let command_port: u16 = std::env::var("ARDOP_CMD_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8515);
    let data_port: u16 = std::env::var("ARDOP_DATA_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8516);
    let mode = std::env::var("ARDOP_MODE").unwrap_or_else(|_| "BPSK250".into());
    let bind_addr = std::env::var("ARDOP_BIND").unwrap_or_else(|_| "127.0.0.1".into());

    let backend = Box::new(LoopbackBackend::default());
    let mut engine = ModemEngine::new(backend);
    engine.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))?;
    engine.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))?;
    engine.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))?;
    engine.register_plugin(Box::new(psk8_plugin::Psk8Plugin::new()))?;

    let config = ArdopConfig {
        bind_addr,
        command_port,
        data_port,
        mode,
        loopback: false,
    };

    tracing::info!(
        "OpenPulse TNC listening on {}:{} (cmd) / {}:{} (data)",
        config.bind_addr,
        config.command_port,
        config.bind_addr,
        config.data_port,
    );

    ArdopServer::new(engine, config).run().await?;
    Ok(())
}
