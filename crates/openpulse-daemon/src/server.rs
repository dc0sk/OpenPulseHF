//! Daemon run loop, extracted from the `openpulse-server` binary so it can be
//! driven in-process — notably to bridge two real daemons through a loopback
//! channel for full-stack validation (the twin-station rig).
//!
//! [`run`] takes an already-built audio backend so a harness can inject a
//! [`openpulse_audio::LoopbackBackend`] whose sample tap it bridges; the
//! `openpulse-server` binary injects the config-selected backend via
//! [`build_audio_backend`]. The control port (TCP 9000 / WS 9001 by default)
//! comes from `[daemon]` in the config, so two daemons just use distinct ports.

use crate::{
    apply_command_to_engine, check_ptt_watchdog, ota_status_event, process_received_bytes, ws,
    ControlServer, RuntimeControlState,
};
use openpulse_audio::LoopbackBackend;
use openpulse_config::OpenpulseConfig;
use openpulse_core::audio::AudioBackend;
use openpulse_core::relay::{RelayForwarder, RelayTrustPolicy};
use openpulse_core::trust_store_file::load_trust_store_from_file;
use openpulse_modem::ModemEngine;
use openpulse_qsy::session::QsyPolicy;
use openpulse_radio::{NoOpPtt, PttController, RigctldController, RigctldPtt, VoxPtt};
use openpulse_repeater::{CrossBandRepeater, RepeaterConfig};

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use ofdm_plugin::OfdmPlugin;
use pilot_plugin::PilotPlugin;
use psk8_plugin::Psk8Plugin;
use qam64_plugin::Qam64Plugin;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

/// Run the full daemon stack to completion (the loop never returns on success).
///
/// `modem_backend` is the engine's audio backend, injected by the caller: the
/// binary passes the config-selected backend; a harness passes a
/// [`LoopbackBackend`] whose sample tap it bridges to a second daemon. Returns
/// `Err` only on a fatal startup misconfiguration.
pub async fn run(cfg: OpenpulseConfig, modem_backend: Box<dyn AudioBackend>) -> Result<(), String> {
    let mode = cfg.modem.mode.clone();
    let station_id = (
        cfg.station.callsign.clone(),
        cfg.station.grid_square.clone(),
    );
    let initial_qsy_enabled = cfg.qsy.enabled;
    let initial_allow_tuner_on_high_swr = cfg.qsy.allow_integrated_tuner_on_high_swr;
    let initial_bandplan_mode = if cfg.qsy.bandplan_awareness_enabled {
        cfg.qsy.bandplan_mode.clone()
    } else {
        "unrestricted".to_string()
    };

    let mut engine = ModemEngine::new(modem_backend);
    // Pin all audio I/O to a named device when configured (e.g. an snd-aloop PCM
    // for the real-audio twin-station rig). Empty = the backend default device.
    if !cfg.audio.device.is_empty() {
        engine.set_default_device(Some(cfg.audio.device.clone()));
    }

    // Optional GPU acceleration: with `--features gpu` and a compatible adapter, the GPU-capable
    // plugins share one GpuContext; otherwise (or when no adapter is found) they use the CPU path.
    #[cfg(feature = "gpu")]
    let gpu_ctx = {
        let ctx = openpulse_gpu::GpuContext::init();
        match &ctx {
            Some(_) => tracing::info!("GPU acceleration enabled"),
            None => tracing::warn!(
                "GPU acceleration requested but no compatible adapter found; using CPU path"
            ),
        }
        ctx
    };

    // Register a GPU-capable plugin: `with_gpu` when a context is available, else `new`.
    #[cfg(feature = "gpu")]
    macro_rules! register_gpu_plugin {
        ($Plugin:ident, $msg:expr) => {
            engine
                .register_plugin(match &gpu_ctx {
                    Some(c) => Box::new($Plugin::with_gpu(c.clone())),
                    None => Box::new($Plugin::new()),
                })
                .expect($msg)
        };
    }
    #[cfg(not(feature = "gpu"))]
    macro_rules! register_gpu_plugin {
        ($Plugin:ident, $msg:expr) => {
            engine
                .register_plugin(Box::new($Plugin::new()))
                .expect($msg)
        };
    }

    register_gpu_plugin!(BpskPlugin, "failed to register BPSK plugin");
    engine
        .register_plugin(Box::new(Fsk4Plugin::new()))
        .expect("failed to register FSK4 plugin");
    engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .expect("failed to register OFDM plugin");
    register_gpu_plugin!(Psk8Plugin, "failed to register 8PSK plugin");
    register_gpu_plugin!(Qam64Plugin, "failed to register 64QAM plugin");
    register_gpu_plugin!(QpskPlugin, "failed to register QPSK plugin");
    // SC-FDMA uses the CPU path: its small per-frame 256-pt FFTs are measured ~1.2–1.3× slower
    // on the GPU (dispatch+readback overhead exceeds the tiny FFT benefit at HF frame sizes).
    engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .expect("failed to register SC-FDMA plugin");
    engine
        .register_plugin(Box::new(PilotPlugin::new()))
        .expect("failed to register pilot-framed plugin");

    if cfg.audio.tx_limiter_threshold > 0.0 {
        engine.set_tx_limiter_threshold(cfg.audio.tx_limiter_threshold);
        tracing::info!(
            threshold = cfg.audio.tx_limiter_threshold,
            "TX soft-limiter enabled"
        );
    }

    // Receiver-led OTA adaptive rate-stepping (opt-in via [modem] ota_enabled).
    if cfg.modem.ota_enabled {
        let profile_name = if cfg.modem.ota_profile.is_empty() {
            cfg.modem.profile.as_str()
        } else {
            cfg.modem.ota_profile.as_str()
        };
        match openpulse_core::profile::SessionProfile::by_name(profile_name) {
            Some(profile) => {
                engine.start_ota_session(profile);
                let parse = openpulse_core::rate::SpeedLevel::from_name;
                let min = (!cfg.modem.ota_min_level.is_empty())
                    .then(|| parse(&cfg.modem.ota_min_level))
                    .flatten();
                let max = (!cfg.modem.ota_max_level.is_empty())
                    .then(|| parse(&cfg.modem.ota_max_level))
                    .flatten();
                if min.is_some() || max.is_some() {
                    engine.ota_set_level_bounds(min, max);
                }
                if !cfg.modem.ota_lock_level.is_empty() {
                    if let Some(l) = parse(&cfg.modem.ota_lock_level) {
                        engine.ota_lock_level(l);
                    }
                }
                if cfg.modem.ota_min_backlog > 0 {
                    engine.set_min_backlog_for_upgrade(cfg.modem.ota_min_backlog);
                }
                if cfg.modem.ota_upgrade_hold_frames > 0 {
                    engine.set_upgrade_hold_frames(cfg.modem.ota_upgrade_hold_frames);
                }
                tracing::info!(profile = profile_name, "OTA adaptive rate-stepping enabled");
            }
            None => tracing::warn!(
                profile = profile_name,
                "OTA enabled but profile unknown; OTA not started"
            ),
        }
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

    let tcp_bind: std::net::SocketAddr =
        format!("{}:{}", cfg.daemon.tcp_bind_addr, cfg.daemon.tcp_port)
            .parse()
            .map_err(|e| format!("invalid daemon.tcp_bind_addr/tcp_port: {e}"))?;
    let ws_bind: std::net::SocketAddr = format!(
        "{}:{}",
        cfg.daemon.websocket_bind_addr, cfg.daemon.websocket_port
    )
    .parse()
    .map_err(|e| format!("invalid daemon.websocket_bind_addr/websocket_port: {e}"))?;

    let mut handle = ControlServer::spawn(
        tcp_bind,
        &engine,
        crate::ControlServerConfig {
            initial_mode: mode,
            initial_station_id: station_id,
            initial_qsy_enabled,
            initial_bandplan_mode,
            initial_allow_tuner_on_high_swr,
        },
        None,
    )
    .await
    .map_err(|e| format!("failed to bind TCP control port {tcp_bind}: {e}"))?;

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
            allow_tuner_on_high_swr: handle.allow_tuner_on_high_swr.clone(),
            spectrum_tap: handle.spectrum_tap.clone(),
            station_id: handle.station_id.clone(),
            message_store: handle.message_store.clone(),
        },
        None,
    )
    .await
    .map_err(|e| format!("failed to bind WebSocket control port {ws_bind}: {e}"))?;

    tracing::info!("openpulse-server WebSocket control port listening on {ws_bind}");

    // CAT backend selection. "none" runs with no CAT control for a TRX that
    // rigctld/Hamlib does not support — no connection is attempted, the operator
    // tunes manually, and frequency-control commands are rejected. PTT is
    // independent (see build_ptt_controller / [modem] ptt_backend).
    let mut rig_controller = if cfg.radio.cat_backend.eq_ignore_ascii_case("none") {
        tracing::info!("CAT disabled (cat_backend = \"none\"); manual frequency control");
        None
    } else {
        match RigctldController::connect(&cfg.radio.rigctld_addr) {
            Ok(controller) => Some(controller),
            Err(err) => {
                tracing::warn!(
                    addr = %cfg.radio.rigctld_addr,
                    error = %err,
                    "rigctld connect failed; set_freq commands will emit command_error"
                );
                None
            }
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
            return Err(format!(
                "QSY policy config is invalid; refusing to start with permissive defaults — fix [qsy] in config: {e}"
            ));
        }
    };

    let relay_forwarder = if cfg.relay.enabled {
        let policy = if cfg.relay.deny_list.is_empty() {
            RelayTrustPolicy::default()
        } else {
            RelayTrustPolicy::deny_relays(cfg.relay.deny_list.iter().map(|s| s.as_str()))
        };
        let ttl_ms = cfg.relay.store_forward_ttl_s.saturating_mul(1000);
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
        qsy_scan_dwell_ms: cfg.qsy.scan_dwell_ms,
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
    // The receive ticker polls the modem for decoded bytes so the QSY responder path
    // can react to incoming RF frames without operator commands.
    let mut rx_ticker =
        tokio::time::interval(std::time::Duration::from_millis(cfg.daemon.receive_tick_ms));
    // Emit OTA status roughly once per second (when an OTA session is active).
    let ota_status_period = (1000 / cfg.daemon.receive_tick_ms.max(1)).max(1);
    let mut ota_status_tick: u64 = 0;
    // Session id stamped into OTA ACK frames (a hash field; the sender does not
    // gate on it). The callsign keeps it stable and station-meaningful.
    let ota_session_id = if cfg.station.callsign.is_empty() {
        "ota".to_string()
    } else {
        cfg.station.callsign.clone()
    };
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
                    crate::Command::PttAssert => {
                        if let Some(ref mut ptt) = ptt_controller {
                            if let Err(e) = ptt.assert_ptt() {
                                tracing::warn!("PTT assert failed: {e}");
                                ptt_hard_failed = true;
                            }
                        }
                    }
                    crate::Command::PttRelease => {
                        if let Some(ref mut ptt) = ptt_controller {
                            if let Err(e) = ptt.release_ptt() {
                                tracing::warn!("PTT release failed: {e}");
                                ptt_hard_failed = true;
                            }
                        }
                    }
                    _ => {}
                }
                // OTA ISS send with real-radio PTT turnaround: when a session is
                // active, a SendMessage drives the receiver-led OTA send here (where
                // the PTT controller lives) — key PTT for the data frame, release it,
                // then listen for the peer's ACK and adopt its recommendation. Handled
                // here rather than in apply_command_to_engine so PTT is sequenced
                // around the half-duplex turnaround.
                let mut ota_send_handled = false;
                if let crate::Command::SendMessage { body, .. } = &cmd {
                    if engine.ota_active() {
                        ota_send_handled = true;
                        ota_send_with_ptt(
                            &mut engine,
                            &mut ptt_controller,
                            &handle.event_tx,
                            body.as_bytes(),
                        );
                    }
                }
                if !ptt_hard_failed && !ota_send_handled {
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
                // block_in_place: engine receive/transmit are synchronous; LoopbackBackend
                // returns immediately. A real audio backend blocks until samples arrive.
                let decode_start = std::time::Instant::now();
                let bytes = if engine.ota_active() {
                    // Receiver-led OTA: decode without keying, then key PTT only to
                    // answer with the ACK carrying our absolute recommended_level.
                    match tokio::task::block_in_place(|| engine.poll_ota_rx(&ota_session_id, None)) {
                        Ok(Some(res)) => {
                            let mut keyed = true;
                            if let Some(ref mut ptt) = ptt_controller {
                                if let Err(e) = ptt.assert_ptt() {
                                    tracing::warn!("OTA ACK PTT assert failed: {e}");
                                    keyed = false;
                                }
                            }
                            if keyed {
                                let _ = handle
                                    .event_tx
                                    .send(crate::protocol::ControlEvent::PttChanged {
                                        active: true,
                                    });
                                if let Err(e) = tokio::task::block_in_place(|| {
                                    engine.transmit_ack_with_short_fec(&res.ack, None)
                                }) {
                                    tracing::warn!("OTA ACK transmit failed: {e}");
                                }
                                if let Some(ref mut ptt) = ptt_controller {
                                    if let Err(e) = ptt.release_ptt() {
                                        tracing::warn!("OTA ACK PTT release failed: {e}");
                                    }
                                }
                                let _ = handle
                                    .event_tx
                                    .send(crate::protocol::ControlEvent::PttChanged {
                                        active: false,
                                    });
                            }
                            res.payload.unwrap_or_default()
                        }
                        Ok(None) => Vec::new(),
                        Err(e) => {
                            tracing::debug!("OTA RX poll error: {e}");
                            Vec::new()
                        }
                    }
                } else {
                    tokio::task::block_in_place(|| engine.receive(&mode, None).unwrap_or_default())
                };
                let decode_ms = decode_start.elapsed().as_secs_f32() * 1000.0;
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
                    // EWMA of decode latency, sampled only when a frame was actually decoded.
                    if !bytes.is_empty() {
                        m.decode_latency_ms = if m.decode_latency_ms <= 0.0 {
                            decode_ms
                        } else {
                            m.decode_latency_ms * 0.8 + decode_ms * 0.2
                        };
                    }
                }
                // Feed the spectrum/waterfall tap with the engine's most recent audio
                // window (RX capture, or the last TX). Without this the broadcast task
                // FFTs the zero-initialised tap and the panel shows a flat spectrum.
                let audio = engine.last_audio();
                if !audio.is_empty() {
                    *handle.spectrum_tap.write().await = audio.to_vec();
                }
                // Periodic OTA status broadcast (~1 Hz) while a session is active.
                ota_status_tick += 1;
                if engine.ota_active() && ota_status_tick.is_multiple_of(ota_status_period) {
                    let _ = handle.event_tx.send(ota_status_event(&engine));
                }
            }
        }
    }
}

/// Receiver-led OTA send with the real-radio half-duplex PTT turnaround.
///
/// For each of up to `1 + MAX_RETRIES` attempts: key PTT, transmit the data frame
/// at the current OTA mode+FEC, **release PTT**, then listen for the peer's FSK4
/// ACK (PTT down) and adopt its absolute `recommended_level` — which steps the
/// rate ladder. Splitting the transmit from the ACK listen (vs the bundled
/// `transmit_arq_ota`) is what lets PTT be keyed only for the TX, so the radio can
/// hear the ACK. PTT is a no-op on the twin rig (NoOpPtt); on a real rig this is
/// the correct turnaround. The long phases run under `block_in_place` so the
/// blocking turnaround does not stall the daemon's async runtime.
fn ota_send_with_ptt(
    engine: &mut ModemEngine,
    ptt_controller: &mut Option<Box<dyn PttController>>,
    event_tx: &std::sync::Arc<tokio::sync::broadcast::Sender<crate::protocol::ControlEvent>>,
    body: &[u8],
) {
    use crate::protocol::ControlEvent;
    use openpulse_core::ack::AckType;
    const MAX_RETRIES: usize = 3;
    const ACK_TIMEOUT_MS: u64 = 4000;

    for _ in 0..=MAX_RETRIES {
        let Some(mode) = engine.ota_tx_mode().map(|m| m.to_owned()) else {
            return; // no OTA session
        };
        let fec = engine.ota_tx_fec();

        // Key PTT for the data frame.
        if let Some(ptt) = ptt_controller.as_mut() {
            if let Err(e) = ptt.assert_ptt() {
                tracing::warn!("OTA send PTT assert failed: {e}");
                return;
            }
        }
        let _ = event_tx.send(ControlEvent::PttChanged { active: true });
        let tx =
            tokio::task::block_in_place(|| engine.transmit_with_fec_mode(body, &mode, fec, None));
        // Release PTT before listening (half-duplex turnaround).
        if let Some(ptt) = ptt_controller.as_mut() {
            if let Err(e) = ptt.release_ptt() {
                tracing::warn!("OTA send PTT release failed: {e}");
            }
        }
        let _ = event_tx.send(ControlEvent::PttChanged { active: false });

        if let Err(e) = tx {
            tracing::warn!(error = %e, "OTA data transmit failed");
            continue;
        }

        // Listen for the ACK with PTT down; adopt the peer's recommended level.
        match tokio::task::block_in_place(|| {
            engine.receive_ack_with_short_fec_within(None, ACK_TIMEOUT_MS)
        }) {
            Ok(ack) => {
                engine.apply_ota_ack(&ack);
                let _ = event_tx.send(ota_status_event(engine));
                if ack.ack_type != AckType::Nack {
                    return;
                }
            }
            Err(e) => tracing::debug!(error = %e, "OTA ACK not received within window"),
        }
    }
}

/// Select an audio backend based on the config string.
///
/// `"cpal"` and `"default"` use [`CpalBackend`] when the `cpal` feature is compiled in.
/// All other values (and `"cpal"`/`"default"` without the feature) fall back to
/// [`LoopbackBackend`].  Production builds should be compiled with `--features cpal`.
pub fn build_audio_backend(backend: &str) -> Box<dyn AudioBackend> {
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
