use clap::Parser;
use openpulse_ardop::{ArdopConfig, ArdopServer};
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_modem::ModemEngine;

#[cfg(feature = "cpal")]
use openpulse_audio::CpalBackend;

#[derive(Parser)]
#[command(
    name = "openpulse-tnc",
    about = "OpenPulse ARDOP-compatible TNC",
    long_about = "OpenPulse ARDOP-compatible TNC.",
    author,
    version
)]
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

    /// Audio backend: default | cpal | loopback (overrides config file).
    #[arg(long)]
    backend: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut cfg = openpulse_config::load()?;

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
    if let Some(b) = cli.backend {
        cfg.audio.backend = b;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cfg.logging.level)),
        )
        .init();

    let audio: Box<dyn openpulse_core::audio::AudioBackend> = match cfg.audio.backend.as_str() {
        "loopback" => Box::new(LoopbackBackend::default()),
        #[cfg(feature = "cpal")]
        "cpal" | "default" => Box::new(CpalBackend::new()),
        #[cfg(not(feature = "cpal"))]
        "cpal" => {
            tracing::warn!(
                "cpal backend not compiled in (build with --features cpal); using loopback"
            );
            Box::new(LoopbackBackend::default())
        }
        #[cfg(not(feature = "cpal"))]
        "default" => Box::new(LoopbackBackend::default()),
        name => {
            anyhow::bail!("unknown audio backend '{name}' — use 'default', 'cpal', or 'loopback'")
        }
    };

    let mut engine = ModemEngine::new(audio);
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

    if cfg.relay.enabled {
        tracing::info!(
            max_hops = cfg.relay.max_hops,
            "relay forwarding enabled (multi-hop relay not yet wired into ARDOP bridge)"
        );
    }
    if !cfg.trust.store_path.is_empty() {
        tracing::warn!(
            path = %cfg.trust.store_path,
            "trust.store_path is set but trust store loading is not yet implemented"
        );
    }

    ArdopServer::new(engine, config).run().await?;
    Ok(())
}
