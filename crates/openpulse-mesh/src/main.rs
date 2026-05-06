//! `openpulse-mesh` — HPX relay mesh daemon.

use anyhow::Result;
use clap::Parser;
use sha2::{Digest, Sha256};
use tracing::info;

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_mesh::MeshDaemon;
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use qpsk_plugin::QpskPlugin;

use openpulse_core::relay::RelayTrustPolicy;

#[derive(Parser)]
#[command(name = "openpulse-mesh", about = "HPX relay mesh daemon")]
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let cfg = match &cli.config {
        Some(path) => openpulse_config::load_from(path)?,
        None => openpulse_config::load()?,
    };

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

    // Build engine (loopback for now; CpalBackend behind feature flag later).
    let lb = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(lb));
    let _ = engine.register_plugin(Box::new(BpskPlugin::default()));
    let _ = engine.register_plugin(Box::new(Fsk4Plugin::default()));
    let _ = engine.register_plugin(Box::new(QpskPlugin::default()));
    let _ = engine.register_plugin(Box::new(Psk8Plugin::default()));

    // Stable peer ID derived from callsign via SHA-256 (full 32 bytes).
    // TODO: replace with a persisted Ed25519 signing keypair so the peer_id
    // is a real Ed25519 verifying key, matching PeerDescriptor semantics.
    let local_peer_id = peer_id_from_callsign(&cfg.station.callsign);

    // relay_policy string is preserved in config for future trust-level filtering;
    // RelayTrustPolicy currently models only a deny-list — no peers are denied by
    // default. The "strict/balanced/permissive" modes will map to minimum trust
    // levels once RelayTrustPolicy gains that capability.
    let policy = RelayTrustPolicy::deny_relays([] as [&str; 0]);

    let mut daemon = MeshDaemon::new(
        engine,
        &mode,
        local_peer_id,
        max_hops,
        mesh_cfg.beacon_interval_s,
        ttl_ms,
        policy,
        mesh_cfg.peer_cache_capacity,
        mesh_cfg.peer_cache_ttl_s * 1000,
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

/// Derive a stable 32-byte peer ID from a callsign using SHA-256.
///
/// This is a placeholder until proper Ed25519 keypair persistence is implemented.
fn peer_id_from_callsign(callsign: &str) -> [u8; 32] {
    let hash = Sha256::digest(callsign.to_uppercase().as_bytes());
    hash.into()
}
