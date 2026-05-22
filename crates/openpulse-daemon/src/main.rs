//! `openpulse-server` binary — wraps the modem engine and exposes the NDJSON
//! control port on TCP port 9000 and WebSocket port 9001 (defaults).

use openpulse_audio::LoopbackBackend;
use openpulse_core::audio::AudioBackend;
use openpulse_core::relay::{RelayForwarder, RelayTrustPolicy};
use openpulse_core::trust_store_file::load_trust_store_from_file;
use openpulse_daemon::{
    apply_command_to_engine, check_ptt_watchdog, process_received_bytes, ws, ControlServer,
    RuntimeControlState,
};
use openpulse_modem::ModemEngine;
use openpulse_qsy::session::QsyPolicy;
use openpulse_radio::{NoOpPtt, PttController, RigctldController, RigctldPtt, VoxPtt};
use openpulse_repeater::{CrossBandRepeater, RepeaterConfig};

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use ofdm_plugin::OfdmPlugin;
use psk8_plugin::Psk8Plugin;
use qam64_plugin::Qam64Plugin;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cfg = openpulse_config::load().unwrap_or_default();
    if cfg.station.callsign.trim().eq_ignore_ascii_case("N0CALL") {
        tracing::error!(
            "invalid callsign N0CALL in configuration; set [station].callsign before starting daemon"
        );
        return;
    }

    let mode = cfg.modem.mode.clone();
    let station_id = (
        cfg.station.callsign.clone(),
        cfg.station.grid_square.clone(),
    );
    let initial_qsy_enabled = cfg.qsy.enabled;
    let initial_bandplan_mode = if cfg.qsy.bandplan_awareness_enabled {
        cfg.qsy.bandplan_mode.clone()
    } else {
        "unrestricted".to_string()
    };

    let mut engine = ModemEngine::new(build_audio_backend(&cfg.audio.backend));
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("failed to register BPSK plugin");
    engine
        .register_plugin(Box::new(Fsk4Plugin::new()))
        .expect("failed to register FSK4 plugin");
    engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .expect("failed to register OFDM plugin");
    engine
        .register_plugin(Box::new(Psk8Plugin::new()))
        .expect("failed to register 8PSK plugin");
    engine
        .register_plugin(Box::new(Qam64Plugin::new()))
        .expect("failed to register 64QAM plugin");
    engine
        .register_plugin(Box::new(QpskPlugin::new()))
        .expect("failed to register QPSK plugin");
    engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .expect("failed to register SC-FDMA plugin");

    if cfg.audio.tx_limiter_threshold > 0.0 {
        engine.set_tx_limiter_threshold(cfg.audio.tx_limiter_threshold);
        tracing::info!(
            threshold = cfg.audio.tx_limiter_threshold,
            "TX soft-limiter enabled"
        );
    }

    // Pre-build the cross-band repeater so it is ready when EnableRepeater fires.
    let repeater = {
        let mut rx = ModemEngine::new(build_audio_backend(&cfg.audio.backend));
        for (name, plugin) in [
            (
                "BPSK",
                Box::new(BpskPlugin::new()) as Box<dyn openpulse_core::plugin::ModulationPlugin>,
            ),
            ("QPSK", Box::new(QpskPlugin::new())),
            ("8PSK", Box::new(Psk8Plugin::new())),
        ] {
            if let Err(e) = rx.register_plugin(plugin) {
                tracing::warn!(plugin = name, error = %e, "repeater rx: plugin registration failed");
            }
        }
        let mut tx = ModemEngine::new(build_audio_backend(&cfg.audio.backend));
        for (name, plugin) in [
            (
                "BPSK",
                Box::new(BpskPlugin::new()) as Box<dyn openpulse_core::plugin::ModulationPlugin>,
            ),
            ("QPSK", Box::new(QpskPlugin::new())),
            ("8PSK", Box::new(Psk8Plugin::new())),
        ] {
            if let Err(e) = tx.register_plugin(plugin) {
                tracing::warn!(plugin = name, error = %e, "repeater tx: plugin registration failed");
            }
        }
        let rep_ptt: Box<dyn PttController + Send> = match cfg.radio.rig_b.as_ref() {
            Some(rig_b) => match RigctldPtt::connect(&rig_b.rigctld_addr) {
                Ok(ctrl) => {
                    tracing::info!(addr = %rig_b.rigctld_addr, "repeater PTT connected via rigctld");
                    Box::new(ctrl)
                }
                Err(e) => {
                    tracing::warn!(
                        addr = %rig_b.rigctld_addr,
                        error = %e,
                        "repeater rigctld PTT connect failed; repeater TX will be silent"
                    );
                    Box::new(NoOpPtt::new())
                }
            },
            None => {
                if cfg.repeater.enabled {
                    tracing::warn!(
                        "repeater.enabled = true but [radio.rig_b] is not configured; \
                         repeater PTT will be no-op — add [radio.rig_b] to config.toml"
                    );
                }
                Box::new(NoOpPtt::new())
            }
        };
        let rep_cfg = RepeaterConfig {
            enabled: cfg.repeater.enabled,
            mode: cfg.repeater.mode.clone(),
            tx_hang_ms: cfg.repeater.tx_hang_ms,
            full_duplex: cfg.repeater.full_duplex,
        };
        CrossBandRepeater::new(rep_ptt, rx, tx, rep_cfg)
    };

    let tcp_bind: std::net::SocketAddr = "127.0.0.1:9000".parse().unwrap();
    let ws_bind: std::net::SocketAddr = "127.0.0.1:9001".parse().unwrap();

    let mut handle = ControlServer::spawn(
        tcp_bind,
        &engine,
        mode,
        station_id,
        initial_qsy_enabled,
        initial_bandplan_mode,
        None,
    )
    .await
    .expect("failed to bind TCP control port");

    tracing::info!("openpulse-server TCP control port listening on {tcp_bind}");

    ws::spawn_ws(
        ws_bind,
        ws::WsShared {
            ev_tx: handle.event_tx.clone(),
            cmd_tx: handle.command_tx.clone(),
            active_mode: handle.active_mode.clone(),
            tx_attenuation_db: handle.tx_attenuation_db.clone(),
            qsy_enabled: handle.qsy_enabled.clone(),
            bandplan_mode: handle.bandplan_mode.clone(),
            spectrum_tap: handle.spectrum_tap.clone(),
            station_id: handle.station_id.clone(),
            message_store: handle.message_store.clone(),
        },
        None,
    )
    .await
    .expect("failed to bind WebSocket control port");

    tracing::info!("openpulse-server WebSocket control port listening on {ws_bind}");

    let mut rig_controller = match RigctldController::connect(&cfg.radio.rigctld_addr) {
        Ok(controller) => Some(controller),
        Err(err) => {
            tracing::warn!(
                addr = %cfg.radio.rigctld_addr,
                error = %err,
                "rigctld connect failed; set_freq commands will emit command_error"
            );
            None
        }
    };
    let mut ptt_controller: Option<Box<dyn PttController>> =
        build_ptt_controller(&cfg.modem.ptt_backend, &cfg.radio.rigctld_addr);

    let qsy_policy = match QsyPolicy::from_config(
        cfg.qsy.enabled,
        &cfg.qsy.allow_trustlevels,
        &cfg.qsy.bandplan_mode,
        cfg.qsy.bandplan_awareness_enabled,
        cfg.qsy.enforce_max_channel_width,
        cfg.qsy.enforce_segment_conventions,
    ) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "QSY policy config error; using permissive defaults");
            QsyPolicy::default()
        }
    };

    let relay_forwarder = if cfg.relay.enabled {
        let policy = if cfg.relay.deny_list.is_empty() {
            RelayTrustPolicy::default()
        } else {
            RelayTrustPolicy::deny_relays(cfg.relay.deny_list.iter().map(|s| s.as_str()))
        };
        let ttl_ms = cfg.mesh.store_forward_ttl_s.saturating_mul(1000);
        tracing::info!(
            max_hops = cfg.relay.max_hops,
            deny_count = cfg.relay.deny_list.len(),
            "relay forwarding enabled"
        );
        Some(RelayForwarder::new(ttl_ms, policy))
    } else {
        None
    };

    let mut runtime_state = RuntimeControlState {
        repeater_enabled: cfg.repeater.enabled,
        repeater: Some(repeater),
        qsy_candidate_freqs: cfg.qsy.candidate_freqs_hz.clone(),
        qsy_switchover_offset_s: u32::try_from(cfg.qsy.switchover_offset_s).unwrap_or_else(|_| {
            tracing::warn!(
                value = cfg.qsy.switchover_offset_s,
                "qsy.switchover_offset_s exceeds u32::MAX; clamping to u32::MAX"
            );
            u32::MAX
        }),
        qsy_policy,
        relay_forwarder,
        trust_store: if !cfg.trust.store_path.is_empty() {
            match load_trust_store_from_file(std::path::Path::new(&cfg.trust.store_path)) {
                Ok(store) => {
                    tracing::info!(path = %cfg.trust.store_path, "trust store loaded");
                    store
                }
                Err(e) => {
                    tracing::warn!(
                        path = %cfg.trust.store_path,
                        error = %e,
                        "failed to load trust store; starting with empty store"
                    );
                    Default::default()
                }
            }
        } else {
            Default::default()
        },
        ..RuntimeControlState::default()
    };

    // Execute side-effectful commands against the live modem engine.
    // A 50 ms receive ticker polls the modem for decoded bytes so the QSY
    // responder path can react to incoming RF frames without operator commands.
    let mut rx_ticker = tokio::time::interval(std::time::Duration::from_millis(50));
    loop {
        tokio::select! {
            biased;
            Some(cmd) = handle.commands.recv() => {
                // PTT hardware calls are synchronous; handle them before the async engine dispatch so
                // the borrow of ptt_controller doesn't cross the await point.
                // If the hardware call fails, skip the engine dispatch to avoid emitting a spurious
                // PttChanged event that would tell clients PTT is active when it is not.
                let mut ptt_hard_failed = false;
                match &cmd {
                    openpulse_daemon::Command::PttAssert => {
                        if let Some(ref mut ptt) = ptt_controller {
                            if let Err(e) = ptt.assert_ptt() {
                                tracing::warn!("PTT assert failed: {e}");
                                ptt_hard_failed = true;
                            }
                        }
                    }
                    openpulse_daemon::Command::PttRelease => {
                        if let Some(ref mut ptt) = ptt_controller {
                            if let Err(e) = ptt.release_ptt() {
                                tracing::warn!("PTT release failed: {e}");
                                ptt_hard_failed = true;
                            }
                        }
                    }
                    _ => {}
                }
                if !ptt_hard_failed {
                    apply_command_to_engine(
                        &cmd,
                        &mut engine,
                        &handle.active_mode,
                        &handle.event_tx,
                        rig_controller.as_mut(),
                        &mut runtime_state,
                    )
                    .await;
                }
            }
            _ = rx_ticker.tick() => {
                // Release PTT hardware if the watchdog deadline has elapsed.
                if check_ptt_watchdog(&mut runtime_state, &handle.event_tx) {
                    if let Some(ref mut ptt) = ptt_controller {
                        if let Err(e) = ptt.release_ptt() {
                            tracing::warn!("PTT watchdog release failed: {e}");
                        }
                    }
                }
                let mode = handle.active_mode.lock().await.clone();
                // block_in_place: engine.receive() is synchronous; LoopbackBackend returns
                // immediately. A real audio backend would block until samples are available.
                let bytes = tokio::task::block_in_place(|| {
                    engine.receive(&mode, None).unwrap_or_default()
                });
                if !bytes.is_empty() {
                    process_received_bytes(
                        &bytes,
                        &mut runtime_state,
                        rig_controller.as_mut(),
                        &handle.event_tx,
                        &handle.active_mode,
                        &mut engine,
                    )
                    .await;
                }
                // Refresh live metrics so the periodic metrics task can broadcast real values.
                {
                    let mut m = handle.shared_metrics.lock().await;
                    m.afc_correction_hz = engine.last_afc_offset_hz().unwrap_or(0.0);
                    m.total_rx_bytes += bytes.len() as u64;
                }
            }
        }
    }
}

/// Select an audio backend based on the config string.
///
/// `"cpal"` and `"default"` use [`CpalBackend`] when the `cpal` feature is compiled in.
/// All other values (and `"cpal"`/`"default"` without the feature) fall back to
/// [`LoopbackBackend`].  Production builds should be compiled with `--features cpal`.
fn build_audio_backend(backend: &str) -> Box<dyn AudioBackend> {
    #[cfg(feature = "cpal")]
    {
        use openpulse_audio::CpalBackend;
        if matches!(backend, "cpal" | "default") {
            return Box::new(CpalBackend::new());
        }
    }
    if matches!(backend, "cpal" | "default") {
        tracing::warn!(
            backend,
            "cpal audio backend requested but not compiled in (missing --features cpal); using loopback"
        );
    } else if backend != "loopback" {
        tracing::warn!(backend, "unknown audio backend; using loopback");
    }
    Box::new(LoopbackBackend::default())
}

fn build_ptt_controller(backend: &str, rigctld_addr: &str) -> Option<Box<dyn PttController>> {
    match backend {
        "none" => Some(Box::new(NoOpPtt::new())),
        "vox" => Some(Box::new(VoxPtt::new())),
        "rigctld" => match RigctldPtt::connect(rigctld_addr) {
            Ok(ctrl) => Some(Box::new(ctrl)),
            Err(e) => {
                tracing::warn!(
                    addr = %rigctld_addr,
                    error = %e,
                    "rigctld PTT connect failed; PTT commands will be no-ops"
                );
                None
            }
        },
        "rts" | "dtr" => {
            tracing::warn!(
                backend,
                "serial PTT not supported in daemon build (recompile with --features serial); PTT disabled"
            );
            None
        }
        other => {
            tracing::warn!(backend = %other, "unknown PTT backend; PTT disabled");
            None
        }
    }
}
