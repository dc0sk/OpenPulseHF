use clap::Parser;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_kiss::{KissConfig, KissServer};
use openpulse_modem::ModemEngine;

#[derive(Parser)]
#[command(name = "openpulse-kisstnc", about = "OpenPulse KISS TNC")]
struct Cli {
    /// KISS TCP port (overrides config file).
    #[arg(long)]
    port: Option<u16>,

    /// Modulation mode (overrides config file).
    #[arg(long)]
    mode: Option<String>,

    /// Bind address (overrides config file).
    #[arg(long)]
    bind: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut cfg = openpulse_config::load().unwrap_or_default();

    // CLI flags override config file values.
    if let Some(p) = cli.port {
        cfg.kiss.port = p;
    }
    if let Some(m) = cli.mode {
        cfg.modem.mode = m;
    }
    if let Some(b) = cli.bind {
        cfg.kiss.bind_addr = b;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cfg.logging.level)),
        )
        .init();

    let backend = Box::new(LoopbackBackend::default());
    let mut engine = ModemEngine::new(backend);
    engine.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))?;
    engine.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))?;
    engine.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))?;
    engine.register_plugin(Box::new(psk8_plugin::Psk8Plugin::new()))?;
    engine.enable_csma();

    let config = KissConfig {
        bind_addr: cfg.kiss.bind_addr.clone(),
        port: cfg.kiss.port,
        mode: cfg.modem.mode.clone(),
        loopback: false,
    };

    tracing::info!(
        "OpenPulse KISS TNC listening on {}:{}",
        config.bind_addr,
        config.port
    );

    KissServer::new(engine, config).run().await?;
    Ok(())
}
