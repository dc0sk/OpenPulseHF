//! `openpulse-mesh` — HPX relay mesh daemon.

use anyhow::Result;
use clap::Parser;
use tracing::info;

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_mesh::MeshDaemon;
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use qpsk_plugin::QpskPlugin;

#[cfg(feature = "cpal")]
use openpulse_audio::CpalBackend;

use openpulse_core::relay::RelayTrustPolicy;
use openpulse_mesh::trust_filter_from_policy;

#[derive(Parser)]
#[command(
    name = "openpulse-mesh",
    about = "HPX relay mesh daemon",
    long_about = "HPX relay mesh daemon.",
    author,
    version
)]
struct Cli {
    /// Override config file path.
    #[arg(long)]
    config: Option<std::path::PathBuf>,

    /// Override modulation mode.
    #[arg(long)]
    mode: Option<String>,

    /// Override max relay hops.
    #[arg(long)]
    max_hops: Option<u8>,

    /// Audio backend: default | cpal | loopback (overrides config file).
    #[arg(long)]
    backend: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut cfg = match &cli.config {
        Some(path) => openpulse_config::load_from(path)?,
        None => openpulse_config::load()?,
    };

    if let Some(b) = cli.backend {
        cfg.audio.backend = b;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| cfg.logging.level.as_str().into()),
        )
        .init();

    let mesh_cfg = cfg.mesh.clone();
    if !mesh_cfg.enabled {
        info!("mesh is disabled in config; set [mesh] enabled = true to start");
        return Ok(());
    }

    let mode = cli.mode.unwrap_or_else(|| cfg.modem.mode.clone());
    let max_hops = cli.max_hops.unwrap_or(mesh_cfg.max_hops);
    let ttl_ms = mesh_cfg.store_forward_ttl_s * 1000;

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
    let _ = engine.register_plugin(Box::new(BpskPlugin::default()));
    let _ = engine.register_plugin(Box::new(Fsk4Plugin::default()));
    let _ = engine.register_plugin(Box::new(QpskPlugin::default()));
    let _ = engine.register_plugin(Box::new(Psk8Plugin::default()));

    // Load or generate a persistent Ed25519 signing key seed.
    // peer_id is the 32-byte Ed25519 verifying key derived from that seed.
    let seed = openpulse_config::load_or_generate_identity()?;
    let local_peer_id = ed25519_dalek::SigningKey::from_bytes(&seed)
        .verifying_key()
        .to_bytes();

    let trust_filter = trust_filter_from_policy(&mesh_cfg.relay_policy);
    let policy = RelayTrustPolicy::with_trust_filter([] as [&str; 0], trust_filter);

    let mut daemon = MeshDaemon::new(
        engine,
        &mode,
        local_peer_id,
        max_hops,
        mesh_cfg.beacon_interval_s,
        ttl_ms,
        policy,
        mesh_cfg.peer_cache_capacity,
        mesh_cfg.peer_cache_ttl_s.saturating_mul(1000),
        seed,
        cfg.station.callsign.clone(),
    );

    info!(
        callsign = %cfg.station.callsign,
        mode = %mode,
        max_hops = max_hops,
        relay_policy = %mesh_cfg.relay_policy,
        "openpulse-mesh started"
    );

    loop {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let events = daemon.step(now_ms);
        for event in events {
            info!(?event, "mesh event");
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
