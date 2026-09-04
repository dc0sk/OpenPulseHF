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

    // The mesh daemon beacons + relays automatically; refuse to run as the placeholder callsign
    // (matches openpulse-daemon / -tui). §97.119: a station must transmit its own valid call sign.
    // NOTE: this binary cannot reach a sound card (#1251), so "on air" is not currently reachable —
    // the callsign gate is kept because it is the right check for when it is.
    if cfg.station.callsign.trim().eq_ignore_ascii_case("N0CALL") {
        anyhow::bail!(
            "invalid callsign N0CALL in configuration; set [station].callsign before starting the mesh daemon"
        );
    }

    let mode = cli.mode.unwrap_or_else(|| cfg.modem.mode.clone());
    let max_hops = cli.max_hops.unwrap_or(mesh_cfg.max_hops);
    let ttl_ms = mesh_cfg.store_forward_ttl_s * 1000;

    // NO ROUTE TO REAL AUDIO (#1251). This binary has none of the transmit-safety machinery its
    // siblings have — no PTT controller (so `[modem] ptt_backend` is unread and only VOX could key
    // it), no carrier sense, and no `StationIdTimer`, while its beacon carries no callsign field of
    // any kind. `docs/regulatory.md` accordingly lists the repeater and the JS8 beacon as this
    // project's automatically-controlled stations and does NOT list mesh — which is the §97.221 gate
    // the JS8 beacon was deliberately held for and mesh skipped.
    //
    // Rather than hand-write a fourth copy of a keying pattern that has been wrong four times
    // (#1250 ARDOP, #1259 KISS, #1260 repeater, #1262 the daemon itself), the capability to reach a
    // sound card is removed: a capability that does not exist cannot be mis-invoked. Wiring mesh for
    // air starts at the regulatory mapping and the automatic-control sub-band question, not here.
    let audio: Box<dyn openpulse_core::audio::AudioBackend> = match cfg.audio.backend.as_str() {
        "loopback" => Box::new(LoopbackBackend::default()),
        "cpal" => {
            anyhow::bail!(
                "openpulse-mesh cannot use a real audio backend (#1251): it has no PTT controller,                  no carrier sense and no station-ID timer, and its beacon carries no callsign. Use                  `backend = \"loopback\"`, or run the mesh relay inside openpulse-daemon."
            )
        }
        "default" => Box::new(LoopbackBackend::default()),
        name => {
            anyhow::bail!("unknown audio backend '{name}' — use 'default', 'cpal', or 'loopback'")
        }
    };

    let mut engine = ModemEngine::new(audio);
    // Record the operator's identity + declared TX power in the §97 regulatory TX-metadata log (the
    // callsign is already gated to non-N0CALL above). Note the log itself is written only by the
    // daemon — `set_tx_log_path` has one caller workspace-wide — so on this binary this records into
    // the in-memory session log only.
    engine.set_callsign(cfg.station.callsign.clone());
    engine.set_max_power_watts(cfg.station.tx_power_watts);
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
