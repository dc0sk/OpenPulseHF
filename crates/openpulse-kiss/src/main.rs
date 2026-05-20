use clap::Parser;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_core::relay::{RelayForwarder, RelayTrustPolicy};
use openpulse_core::trust_store_file::load_trust_store_from_file;
use openpulse_kiss::{KissConfig, KissServer};
use openpulse_modem::ModemEngine;

#[cfg(feature = "cpal")]
use openpulse_audio::CpalBackend;

#[derive(Parser)]
#[command(
    name = "openpulse-kisstnc",
    about = "OpenPulse KISS TNC",
    long_about = "OpenPulse KISS TNC.",
    author,
    version
)]
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

    /// Audio backend: default | cpal | loopback (overrides config file).
    #[arg(long)]
    backend: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut cfg = openpulse_config::load()?;

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
    engine.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))?;
    engine.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))?;
    engine.register_plugin(Box::new(psk8_plugin::Psk8Plugin::new()))?;
    engine.register_plugin(Box::new(qam64_plugin::Qam64Plugin::new()))?;
    engine.register_plugin(Box::new(scfdma_plugin::ScFdmaPlugin::new()))?;
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

    let trust_store = if !cfg.trust.store_path.is_empty() {
        match load_trust_store_from_file(std::path::Path::new(&cfg.trust.store_path)) {
            Ok(store) => {
                tracing::info!(path = %cfg.trust.store_path, "trust store loaded");
                store
            }
            Err(e) => {
                tracing::warn!(path = %cfg.trust.store_path, error = %e, "failed to load trust store; starting with empty store");
                Default::default()
            }
        }
    } else {
        Default::default()
    };

    let relay_forwarder = if cfg.relay.enabled {
        let policy = if cfg.relay.deny_list.is_empty() {
            RelayTrustPolicy::default()
        } else {
            RelayTrustPolicy::deny_relays(cfg.relay.deny_list.iter().map(|s| s.as_str()))
        };
        let fwd = RelayForwarder::new(300_000, policy);
        tracing::info!(
            max_hops = cfg.relay.max_hops,
            deny_count = cfg.relay.deny_list.len(),
            "relay forwarding enabled"
        );
        Some(fwd)
    } else {
        None
    };

    KissServer::with_trust_and_relay(engine, config, trust_store, relay_forwarder)
        .run()
        .await?;
    Ok(())
}
