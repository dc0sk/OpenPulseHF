use openpulse_audio::loopback::LoopbackBackend;
use openpulse_kiss::{KissConfig, KissServer};
use openpulse_modem::ModemEngine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let port: u16 = std::env::var("KISS_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8100);
    let mode = std::env::var("KISS_MODE").unwrap_or_else(|_| "BPSK250".into());
    let bind_addr = std::env::var("KISS_BIND").unwrap_or_else(|_| "127.0.0.1".into());

    let backend = Box::new(LoopbackBackend::default());
    let mut engine = ModemEngine::new(backend);
    engine.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))?;
    engine.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))?;
    engine.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))?;
    engine.register_plugin(Box::new(psk8_plugin::Psk8Plugin::new()))?;
    engine.enable_csma();

    let config = KissConfig {
        bind_addr: bind_addr.clone(),
        port,
        mode,
        loopback: false,
    };

    tracing::info!("OpenPulse KISS TNC listening on {bind_addr}:{port}");

    KissServer::new(engine, config).run().await?;
    Ok(())
}
