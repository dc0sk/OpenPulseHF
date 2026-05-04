use clap::Parser;
use openpulse_ardop::{ArdopConfig, ArdopServer};
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_modem::ModemEngine;

#[derive(Parser)]
#[command(name = "openpulse-tnc", about = "OpenPulse ARDOP-compatible TNC")]
struct Cli {
    /// ARDOP command port (overrides config file).
    #[arg(long)]
    cmd_port: Option<u16>,

    /// ARDOP data port (overrides config file).
    #[arg(long)]
    data_port: Option<u16>,

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
    if let Some(p) = cli.cmd_port {
        cfg.ardop.cmd_port = p;
    }
    if let Some(p) = cli.data_port {
        cfg.ardop.data_port = p;
    }
    if let Some(m) = cli.mode {
        cfg.modem.mode = m;
    }
    if let Some(b) = cli.bind {
        cfg.ardop.bind_addr = b;
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

    let config = ArdopConfig {
        bind_addr: cfg.ardop.bind_addr.clone(),
        command_port: cfg.ardop.cmd_port,
        data_port: cfg.ardop.data_port,
        mode: cfg.modem.mode.clone(),
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
